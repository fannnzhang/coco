use async_trait::async_trait;
use codex_protocol::models::ShellToolCallParams;
use std::sync::Arc;

use super::legacy_edit;
use crate::apply_patch;
use crate::apply_patch::InternalApplyPatchInvocation;
use crate::apply_patch::convert_apply_patch_to_protocol;
use crate::codex::TurnContext;
use crate::exec::ExecParams;
use crate::exec_env::create_env;
use crate::function_tool::FunctionCallError;
use crate::tools::coco_subagent;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::events::ToolEmitter;
use crate::tools::events::ToolEventCtx;
use crate::tools::orchestrator::ToolOrchestrator;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use crate::tools::runtimes::apply_patch::ApplyPatchRequest;
use crate::tools::runtimes::apply_patch::ApplyPatchRuntime;
use crate::tools::runtimes::shell::ShellRequest;
use crate::tools::runtimes::shell::ShellRuntime;
use crate::tools::sandboxing::ToolCtx;
use codex_apply_patch::ApplyPatchAction;

pub struct ShellHandler;

impl ShellHandler {
    fn to_exec_params(params: ShellToolCallParams, turn_context: &TurnContext) -> ExecParams {
        ExecParams {
            command: params.command,
            cwd: turn_context.resolve_path(params.workdir.clone()),
            timeout_ms: params.timeout_ms,
            env: create_env(&turn_context.shell_environment_policy),
            with_escalated_permissions: params.with_escalated_permissions,
            justification: params.justification,
            arg0: None,
        }
    }
}

#[async_trait]
impl ToolHandler for ShellHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(
            payload,
            ToolPayload::Function { .. } | ToolPayload::LocalShell { .. }
        )
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

        match payload {
            ToolPayload::Function { arguments } => {
                let params: ShellToolCallParams =
                    serde_json::from_str(&arguments).map_err(|e| {
                        FunctionCallError::RespondToModel(format!(
                            "failed to parse function arguments: {e:?}"
                        ))
                    })?;
                let exec_params = Self::to_exec_params(params, turn.as_ref());
                Self::run_exec_like(
                    tool_name.as_str(),
                    exec_params,
                    session,
                    turn,
                    tracker,
                    call_id,
                    false,
                )
                .await
            }
            ToolPayload::LocalShell { params } => {
                let exec_params = Self::to_exec_params(params, turn.as_ref());
                Self::run_exec_like(
                    tool_name.as_str(),
                    exec_params,
                    session,
                    turn,
                    tracker,
                    call_id,
                    true,
                )
                .await
            }
            _ => Err(FunctionCallError::RespondToModel(format!(
                "unsupported payload for shell handler: {tool_name}"
            ))),
        }
    }
}

impl ShellHandler {
    async fn run_exec_like(
        tool_name: &str,
        exec_params: ExecParams,
        session: Arc<crate::codex::Session>,
        turn: Arc<TurnContext>,
        tracker: crate::tools::context::SharedTurnDiffTracker,
        call_id: String,
        is_user_shell_command: bool,
    ) -> Result<ToolOutput, FunctionCallError> {
        // Approval policy guard for explicit escalation in non-OnRequest modes.
        if exec_params.with_escalated_permissions.unwrap_or(false)
            && !matches!(
                turn.approval_policy,
                codex_protocol::protocol::AskForApproval::OnRequest
            )
        {
            return Err(FunctionCallError::RespondToModel(format!(
                "approval policy is {policy:?}; reject command — you should not ask for escalated permissions if the approval policy is {policy:?}",
                policy = turn.approval_policy
            )));
        }

        if let Some(output) = coco_subagent::maybe_run_coco_command(
            &exec_params,
            &session,
            &turn,
            &call_id,
            is_user_shell_command,
        )
        .await?
        {
            return Ok(output);
        }

        // Intercept apply_patch if present.
        match codex_apply_patch::maybe_parse_apply_patch_verified(
            &exec_params.command,
            &exec_params.cwd,
        ) {
            codex_apply_patch::MaybeApplyPatchVerified::Body(changes) => {
                return Self::execute_apply_patch_action(
                    tool_name,
                    changes,
                    exec_params.timeout_ms,
                    &session,
                    &turn,
                    &tracker,
                    &call_id,
                )
                .await;
            }
            codex_apply_patch::MaybeApplyPatchVerified::CorrectnessError(parse_error) => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "apply_patch verification failed: {parse_error}"
                )));
            }
            codex_apply_patch::MaybeApplyPatchVerified::ShellParseError(error) => {
                tracing::trace!("Failed to parse shell command, {error:?}");
                // Fall through to regular shell execution.
            }
            codex_apply_patch::MaybeApplyPatchVerified::NotApplyPatch => {
                // Fall through to regular shell execution.
            }
        }

        match legacy_edit::maybe_build_apply_patch_action(&exec_params.command, &exec_params.cwd) {
            Ok(Some(action)) => {
                return Self::execute_apply_patch_action(
                    tool_name,
                    action,
                    exec_params.timeout_ms,
                    &session,
                    &turn,
                    &tracker,
                    &call_id,
                )
                .await;
            }
            Ok(None) => { /* proceed with shell */ }
            Err(err) => {
                return Err(FunctionCallError::RespondToModel(err.to_string()));
            }
        }

        // Regular shell execution path.
        let emitter = ToolEmitter::shell(
            exec_params.command.clone(),
            exec_params.cwd.clone(),
            is_user_shell_command,
        );
        let event_ctx = ToolEventCtx::new(session.as_ref(), turn.as_ref(), &call_id, None);
        emitter.begin(event_ctx).await;

        let req = ShellRequest {
            command: exec_params.command.clone(),
            cwd: exec_params.cwd.clone(),
            timeout_ms: exec_params.timeout_ms,
            env: exec_params.env.clone(),
            with_escalated_permissions: exec_params.with_escalated_permissions,
            justification: exec_params.justification.clone(),
        };
        let mut orchestrator = ToolOrchestrator::new();
        let mut runtime = ShellRuntime::new();
        let tool_ctx = ToolCtx {
            session: session.as_ref(),
            turn: turn.as_ref(),
            call_id: call_id.clone(),
            tool_name: tool_name.to_string(),
        };
        let out = orchestrator
            .run(&mut runtime, &req, &tool_ctx, &turn, turn.approval_policy)
            .await;
        let event_ctx = ToolEventCtx::new(session.as_ref(), turn.as_ref(), &call_id, None);
        let content = emitter.finish(event_ctx, out).await?;
        Ok(ToolOutput::Function {
            content,
            content_items: None,
            success: Some(true),
        })
    }

    async fn execute_apply_patch_action(
        tool_name: &str,
        action: ApplyPatchAction,
        timeout_ms: Option<u64>,
        session: &Arc<crate::codex::Session>,
        turn: &Arc<TurnContext>,
        tracker: &crate::tools::context::SharedTurnDiffTracker,
        call_id: &str,
    ) -> Result<ToolOutput, FunctionCallError> {
        match apply_patch::apply_patch(session.as_ref(), turn.as_ref(), call_id, action).await {
            InternalApplyPatchInvocation::Output(item) => {
                let content = item?;
                Ok(ToolOutput::Function {
                    content,
                    content_items: None,
                    success: Some(true),
                })
            }
            InternalApplyPatchInvocation::DelegateToExec(apply) => {
                let emitter = ToolEmitter::apply_patch(
                    convert_apply_patch_to_protocol(&apply.action),
                    !apply.user_explicitly_approved_this_action,
                );
                let event_ctx =
                    ToolEventCtx::new(session.as_ref(), turn.as_ref(), call_id, Some(tracker));
                emitter.begin(event_ctx).await;

                let req = ApplyPatchRequest {
                    patch: apply.action.patch.clone(),
                    cwd: apply.action.cwd.clone(),
                    timeout_ms,
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
                    content_items: None,
                    success: Some(true),
                })
            }
        }
    }
}
