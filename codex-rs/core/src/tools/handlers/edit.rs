use crate::apply_patch;
use crate::apply_patch::InternalApplyPatchInvocation;
use crate::apply_patch::convert_apply_patch_to_protocol;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::events::ToolEmitter;
use crate::tools::events::ToolEventCtx;
use crate::tools::handlers::legacy_edit;
use crate::tools::orchestrator::ToolOrchestrator;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use crate::tools::runtimes::apply_patch::ApplyPatchRequest;
use crate::tools::runtimes::apply_patch::ApplyPatchRuntime;
use crate::tools::sandboxing::ToolCtx;
use async_trait::async_trait;
use codex_apply_patch::ApplyPatchAction;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct EditHandler;

#[derive(Debug, Deserialize)]
struct WriteFileToolArgs {
    file_path: String,
    content: String,
    #[serde(flatten)]
    _extra: HashMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
struct ReplaceToolArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    _instruction: Option<String>,
    #[serde(default)]
    expected_replacements: Option<usize>,
    #[serde(flatten)]
    _extra: HashMap<String, JsonValue>,
}

#[derive(Debug, Deserialize)]
struct DeleteFileToolArgs {
    file_path: String,
    #[serde(flatten)]
    _extra: HashMap<String, JsonValue>,
}

#[async_trait]
impl ToolHandler for EditHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            tracker,
            call_id,
            tool_name,
            payload,
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "edit tool invoked with non-function payload".to_string(),
                ));
            }
        };

        let cwd = turn.cwd.clone();
        let action = match tool_name.as_str() {
            "write_file" => {
                let params: WriteFileToolArgs =
                    serde_json::from_str(&arguments).map_err(|err| {
                        FunctionCallError::RespondToModel(format!(
                            "write_file arguments could not be parsed as JSON: {err}"
                        ))
                    })?;
                build_write_file_action(&params.file_path, &params.content, &cwd)?
            }
            "replace" => {
                let params: ReplaceToolArgs = serde_json::from_str(&arguments).map_err(|err| {
                    FunctionCallError::RespondToModel(format!(
                        "replace arguments could not be parsed as JSON: {err}"
                    ))
                })?;
                build_replace_action(
                    &params.file_path,
                    &params.old_string,
                    &params.new_string,
                    params.expected_replacements,
                    &cwd,
                )?
            }
            "delete" => {
                let params: DeleteFileToolArgs =
                    serde_json::from_str(&arguments).map_err(|err| {
                        FunctionCallError::RespondToModel(format!(
                            "delete arguments could not be parsed as JSON: {err}"
                        ))
                    })?;
                build_delete_action(&params.file_path, &cwd)?
            }
            other => {
                return Err(FunctionCallError::Fatal(format!(
                    "unsupported edit tool {other}"
                )));
            }
        };

        Self::execute_apply_patch_action(&tool_name, action, &session, &turn, &tracker, &call_id)
            .await
    }
}

fn build_write_file_action(
    file_path: &str,
    content: &str,
    cwd: &Path,
) -> Result<ApplyPatchAction, FunctionCallError> {
    legacy_edit::build_write_file_action(file_path, content, cwd)
        .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))
}

fn build_replace_action(
    file_path: &str,
    old: &str,
    new: &str,
    expected_replacements: Option<usize>,
    cwd: &Path,
) -> Result<ApplyPatchAction, FunctionCallError> {
    legacy_edit::build_replace_action(file_path, old, new, expected_replacements, cwd)
        .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))
}

fn build_delete_action(file_path: &str, cwd: &Path) -> Result<ApplyPatchAction, FunctionCallError> {
    legacy_edit::build_delete_file_action(file_path, cwd)
        .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))
}

impl EditHandler {
    async fn execute_apply_patch_action(
        tool_name: &str,
        action: ApplyPatchAction,
        session: &Arc<crate::codex::Session>,
        turn: &Arc<crate::codex::TurnContext>,
        tracker: &crate::tools::context::SharedTurnDiffTracker,
        call_id: &str,
    ) -> Result<ToolOutput, FunctionCallError> {
        match apply_patch::apply_patch(session.as_ref(), turn.as_ref(), call_id, action).await {
            InternalApplyPatchInvocation::Output(item) => {
                let content = item?;
                Ok(ToolOutput::Function {
                    content,
                    success: Some(true),
                })
            }
            InternalApplyPatchInvocation::DelegateToExec(apply) => {
                let emitter = ToolEmitter::apply_patch(
                    convert_apply_patch_to_protocol(&apply.action),
                    !apply.user_explicitly_approved_this_action,
                );
                let event_ctx =
                    ToolEventCtx::new(session.as_ref(), turn.as_ref(), call_id, Some(&tracker));
                emitter.begin(event_ctx).await;

                let req = ApplyPatchRequest {
                    patch: apply.action.patch.clone(),
                    cwd: apply.action.cwd.clone(),
                    timeout_ms: None,
                    user_explicitly_approved: apply.user_explicitly_approved_this_action,
                    codex_exe: turn.codex_linux_sandbox_exe.clone(),
                };

                let mut orchestrator = ToolOrchestrator::new();
                let mut runtime = ApplyPatchRuntime::new();
                let tool_ctx = ToolCtx {
                    session: session.as_ref(),
                    turn: turn.as_ref(),
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                };
                let out = orchestrator
                    .run(&mut runtime, &req, &tool_ctx, turn, turn.approval_policy)
                    .await;
                let event_ctx =
                    ToolEventCtx::new(session.as_ref(), turn.as_ref(), call_id, Some(tracker));
                let content = emitter.finish(event_ctx, out).await?;
                Ok(ToolOutput::Function {
                    content,
                    success: Some(true),
                })
            }
        }
    }
}
