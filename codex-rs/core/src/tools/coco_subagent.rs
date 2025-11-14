use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_channel::Receiver;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::user_input::UserInput;
use shlex::split;
use shlex::try_join;
use tokio::time;
use tokio_util::sync::CancellationToken;

use crate::codex::TurnContext;
use crate::codex_delegate::run_codex_conversation_one_shot;
use crate::exec::ExecParams;
use crate::exec::ExecToolCallOutput;
use crate::exec::StreamOutput;
use crate::function_tool::FunctionCallError;
use crate::protocol::Event;
use crate::protocol::EventMsg;
use crate::protocol::ExecCommandOutputDeltaEvent;
use crate::protocol::ExecOutputStream;
use crate::tools::context::ToolOutput;
use crate::tools::events::ToolEmitter;
use crate::tools::events::ToolEventCtx;
use crate::tools::events::ToolEventFailure;
use crate::tools::events::ToolEventStage;
use crate::tools::format_exec_output_for_model;

const COCO_BINARY_BASENAMES: &[&str] = &["coco", "coco.exe", "cocos", "cocos.exe"];
const COCO_TRUNCATION_NOTICE: &str = "[... coco exec output truncated ...]";
const MAX_COCO_CAPTURED_LINES: usize = 200;
const COCO_SUB_AGENT_LABEL: &str = "coco";

pub(crate) async fn maybe_run_coco_command(
    exec_params: &ExecParams,
    session: &Arc<crate::codex::Session>,
    turn: &Arc<TurnContext>,
    call_id: &str,
    is_user_shell_command: bool,
) -> Result<Option<ToolOutput>, FunctionCallError> {
    let Some(invocation) = CocoInvocation::parse(&exec_params.command) else {
        return Ok(None);
    };

    if invocation.prompt().trim().is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "coco command requires a prompt argument.".to_string(),
        ));
    }

    let output = run_coco_command(
        &invocation,
        exec_params,
        session,
        turn,
        call_id,
        is_user_shell_command,
    )
    .await?;

    Ok(Some(output))
}

#[derive(Debug)]
struct CocoInvocation {
    prompt: String,
}

impl CocoInvocation {
    fn parse(command: &[String]) -> Option<Self> {
        let tokens = parse_coco_tokens(command)?;
        let prompt = if tokens.len() <= 1 {
            String::new()
        } else {
            tokens[1..].join(" ")
        };
        Some(Self { prompt })
    }

    fn prompt(&self) -> &str {
        &self.prompt
    }
}

async fn run_coco_command(
    invocation: &CocoInvocation,
    exec_params: &ExecParams,
    session: &Arc<crate::codex::Session>,
    turn: &Arc<TurnContext>,
    call_id: &str,
    is_user_shell_command: bool,
) -> Result<ToolOutput, FunctionCallError> {
    let emitter = ToolEmitter::shell(
        exec_params.command.clone(),
        exec_params.cwd.clone(),
        is_user_shell_command,
    );
    let begin_ctx = ToolEventCtx::new(session.as_ref(), turn.as_ref(), call_id, None);
    emitter.begin(begin_ctx).await;

    let started_at = Instant::now();
    let outcome = match execute_coco_subagent(invocation, exec_params, session, turn, call_id).await
    {
        Ok(outcome) => outcome,
        Err(CocoError::Execution { message, log }) => {
            let mut combined = message.clone();
            if !log.is_empty() {
                combined.push('\n');
                combined.push_str(&log.join("\n"));
            }
            let event_ctx = ToolEventCtx::new(session.as_ref(), turn.as_ref(), call_id, None);
            emitter
                .emit(
                    event_ctx,
                    ToolEventStage::Failure(ToolEventFailure::Message(message)),
                )
                .await;
            return Err(FunctionCallError::RespondToModel(combined));
        }
    };

    let duration = started_at.elapsed();
    let log_text = outcome.log.join("\n");
    let final_message = outcome.final_message.clone().unwrap_or_else(|| {
        "coco sub-agent finished without returning an agent message.".to_string()
    });

    let event_output = ExecToolCallOutput {
        exit_code: outcome.exit_code,
        stdout: StreamOutput::new(log_text.clone()),
        stderr: StreamOutput::new(String::new()),
        aggregated_output: StreamOutput::new(log_text),
        duration,
        timed_out: false,
    };
    let event_ctx = ToolEventCtx::new(session.as_ref(), turn.as_ref(), call_id, None);
    emitter
        .emit(event_ctx, ToolEventStage::Success(event_output))
        .await;

    let model_output = ExecToolCallOutput {
        exit_code: outcome.exit_code,
        stdout: StreamOutput::new(final_message.clone()),
        stderr: StreamOutput::new(String::new()),
        aggregated_output: StreamOutput::new(final_message.clone()),
        duration,
        timed_out: false,
    };
    let content = format_exec_output_for_model(&model_output);

    Ok(ToolOutput::Function {
        content,
        content_items: None,
        success: Some(outcome.exit_code == 0),
    })
}

#[derive(Debug)]
struct CocoRunOutcome {
    final_message: Option<String>,
    log: Vec<String>,
    exit_code: i32,
}

#[derive(Debug)]
enum CocoError {
    Execution { message: String, log: Vec<String> },
}

#[derive(Debug, Default)]
struct CocoEventCollector {
    lines: Vec<String>,
    pending_agent: Option<String>,
    last_agent_message: Option<String>,
}

impl CocoEventCollector {
    fn push_line(&mut self, line: impl Into<String>) -> Option<String> {
        let line = line.into();
        if line.is_empty() {
            return None;
        }
        self.lines.push(line.clone());
        Some(line)
    }

    fn push_agent_delta(&mut self, delta: &str) {
        let entry = self.pending_agent.get_or_insert_with(String::new);
        entry.push_str(delta);
    }

    fn commit_agent_message(&mut self, message: &str) -> Option<String> {
        self.pending_agent = None;
        let trimmed = message.trim_end();
        if trimmed.is_empty() {
            return None;
        }
        if self
            .last_agent_message
            .as_deref()
            .is_some_and(|existing| existing == trimmed)
        {
            return None;
        }
        self.last_agent_message = Some(trimmed.to_string());
        self.push_line(format!("assistant: {trimmed}"))
    }

    fn finalize_pending_agent(&mut self) -> Option<String> {
        if let Some(buffer) = self.pending_agent.take() {
            let trimmed = buffer.trim_end();
            if trimmed.is_empty() {
                return None;
            }
            self.last_agent_message = Some(trimmed.to_string());
            return self.push_line(format!("assistant: {trimmed}"));
        }
        None
    }

    fn append_exec_output(&mut self, output: &str) -> Vec<String> {
        let mut appended = Vec::new();
        let mut count = 0usize;
        for line in output.lines() {
            if line.is_empty() {
                continue;
            }
            if count >= MAX_COCO_CAPTURED_LINES {
                let notice = COCO_TRUNCATION_NOTICE.to_string();
                self.lines.push(notice.clone());
                appended.push(notice);
                return appended;
            }
            let formatted = format!("  {line}");
            self.lines.push(formatted.clone());
            appended.push(formatted);
            count += 1;
        }
        appended
    }

    fn last_agent_message(&self) -> Option<&String> {
        self.last_agent_message.as_ref()
    }

    fn into_lines(self) -> Vec<String> {
        self.lines
    }
}

fn parse_coco_tokens(command: &[String]) -> Option<Vec<String>> {
    if command.is_empty() {
        return None;
    }
    if is_coco_program(&command[0]) {
        return Some(command.to_vec());
    }
    if command.len() >= 3 && is_shell_wrapper(&command[0]) && command[1] == "-lc"
        && let Some(tokens) = split(&command[2])
            && !tokens.is_empty() && is_coco_program(&tokens[0]) {
                return Some(tokens);
            }
    None
}

fn is_coco_program(cmd: &str) -> bool {
    let name = command_basename(cmd);
    COCO_BINARY_BASENAMES
        .iter()
        .any(|candidate| candidate == &name)
}

fn command_basename(cmd: &str) -> &str {
    Path::new(cmd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(cmd)
}

fn is_shell_wrapper(cmd: &str) -> bool {
    matches!(command_basename(cmd), "bash" | "zsh" | "sh")
}

fn join_command(tokens: &[String]) -> String {
    try_join(tokens.iter().map(String::as_str)).unwrap_or_else(|_| tokens.join(" "))
}

fn format_duration_compact(duration: Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    if secs == 0 {
        format!("{millis}ms")
    } else {
        format!("{secs}.{millis:03}s")
    }
}

async fn emit_coco_stdout_line(
    session: &Arc<crate::codex::Session>,
    turn: &Arc<TurnContext>,
    call_id: &str,
    line: &str,
) {
    if line.is_empty() {
        return;
    }
    let mut chunk = line.to_string();
    chunk.push('\n');
    let event = ExecCommandOutputDeltaEvent {
        call_id: call_id.to_string(),
        stream: ExecOutputStream::Stdout,
        chunk: chunk.into_bytes(),
    };
    session
        .send_event(turn.as_ref(), EventMsg::ExecCommandOutputDelta(event))
        .await;
}

async fn execute_coco_subagent(
    invocation: &CocoInvocation,
    exec_params: &ExecParams,
    session: &Arc<crate::codex::Session>,
    turn: &Arc<TurnContext>,
    call_id: &str,
) -> Result<CocoRunOutcome, CocoError> {
    let mut sub_agent_config = turn.client.config().as_ref().clone();
    sub_agent_config.cwd = exec_params.cwd.clone();

    let inputs = vec![UserInput::Text {
        text: invocation.prompt().to_string(),
    }];

    let cancel_token = CancellationToken::new();
    let io = run_codex_conversation_one_shot(
        sub_agent_config,
        Arc::clone(&session.services.auth_manager),
        inputs,
        Arc::clone(session),
        Arc::clone(turn),
        cancel_token.clone(),
        None,
        SubAgentSource::Other(COCO_SUB_AGENT_LABEL.to_string()),
    )
    .await
    .map_err(|e| CocoError::Execution {
        message: format!("failed to start coco sub-agent: {e:#}"),
        log: Vec::new(),
    })?;

    let receiver = io.rx_event;
    let collect_future = collect_coco_events(receiver, session, turn, call_id);
    let outcome = if let Some(timeout_ms) = exec_params.timeout_ms {
        match time::timeout(Duration::from_millis(timeout_ms), collect_future).await {
            Ok(result) => result,
            Err(_) => {
                cancel_token.cancel();
                return Err(CocoError::Execution {
                    message: format!("coco sub-agent timed out after {timeout_ms} ms"),
                    log: Vec::new(),
                });
            }
        }
    } else {
        collect_future.await
    }?;

    Ok(outcome)
}

async fn collect_coco_events(
    rx: Receiver<Event>,
    session: &Arc<crate::codex::Session>,
    turn: &Arc<TurnContext>,
    call_id: &str,
) -> Result<CocoRunOutcome, CocoError> {
    let mut collector = CocoEventCollector::default();
    let mut task_started_logged = false;
    let mut success = false;
    let mut failure_message: Option<String> = None;

    while let Ok(event) = rx.recv().await {
        match event.msg {
            EventMsg::AgentMessage(ev) => {
                if let Some(line) = collector.commit_agent_message(&ev.message) {
                    emit_coco_stdout_line(session, turn, call_id, &line).await;
                }
            }
            EventMsg::AgentMessageDelta(ev) => {
                collector.push_agent_delta(&ev.delta);
            }
            EventMsg::AgentReasoningRawContent(ev) => {
                let trimmed = ev.text.trim_end();
                if !trimmed.is_empty()
                    && let Some(line) = collector.push_line(format!("thinking: {trimmed}")) {
                        emit_coco_stdout_line(session, turn, call_id, &line).await;
                    }
            }
            EventMsg::AgentReasoningRawContentDelta(ev) => {
                let trimmed = ev.delta.trim_end();
                if !trimmed.is_empty()
                    && let Some(line) = collector.push_line(format!("thinking: {trimmed}")) {
                        emit_coco_stdout_line(session, turn, call_id, &line).await;
                    }
            }
            EventMsg::TaskStarted(_) => {
                if !task_started_logged {
                    if let Some(line) = collector.push_line("sub-agent task started") {
                        emit_coco_stdout_line(session, turn, call_id, &line).await;
                    }
                    task_started_logged = true;
                }
            }
            EventMsg::ExecCommandBegin(ev) => {
                if let Some(line) = collector.push_line(format!(
                    "exec: {} (cwd {})",
                    join_command(&ev.command),
                    ev.cwd.display()
                )) {
                    emit_coco_stdout_line(session, turn, call_id, &line).await;
                }
            }
            EventMsg::ExecCommandEnd(ev) => {
                if let Some(line) = collector.push_line(format!(
                    "exec exited {} in {}",
                    ev.exit_code,
                    format_duration_compact(ev.duration)
                )) {
                    emit_coco_stdout_line(session, turn, call_id, &line).await;
                }
                for line in collector.append_exec_output(&ev.aggregated_output) {
                    emit_coco_stdout_line(session, turn, call_id, &line).await;
                }
            }
            EventMsg::Warning(ev) => {
                let trimmed = ev.message.trim_end();
                if !trimmed.is_empty()
                    && let Some(line) = collector.push_line(format!("warning: {trimmed}")) {
                        emit_coco_stdout_line(session, turn, call_id, &line).await;
                    }
            }
            EventMsg::Error(ev) => {
                let trimmed = ev.message.trim_end().to_string();
                if !trimmed.is_empty()
                    && let Some(line) = collector.push_line(format!("error: {trimmed}")) {
                        emit_coco_stdout_line(session, turn, call_id, &line).await;
                    }
                failure_message = Some(trimmed);
                break;
            }
            EventMsg::TaskComplete(ev) => {
                if let Some(line) = collector.finalize_pending_agent() {
                    emit_coco_stdout_line(session, turn, call_id, &line).await;
                }
                if let Some(last) = ev.last_agent_message.as_deref()
                    && let Some(line) = collector.commit_agent_message(last) {
                        emit_coco_stdout_line(session, turn, call_id, &line).await;
                    }
                success = true;
                break;
            }
            EventMsg::TurnAborted(ev) => {
                if let Some(line) = collector.finalize_pending_agent() {
                    emit_coco_stdout_line(session, turn, call_id, &line).await;
                }
                let reason = match ev.reason {
                    TurnAbortReason::Interrupted => "interrupted",
                    TurnAbortReason::Replaced => "replaced",
                    TurnAbortReason::ReviewEnded => "review ended",
                };
                let message = format!("sub-agent aborted ({reason})");
                if let Some(line) = collector.push_line(&message) {
                    emit_coco_stdout_line(session, turn, call_id, &line).await;
                }
                failure_message = Some(message);
                break;
            }
            _ => {}
        }
    }

    if let Some(line) = collector.finalize_pending_agent() {
        emit_coco_stdout_line(session, turn, call_id, &line).await;
    }
    let final_message = collector.last_agent_message().cloned();
    let lines = collector.into_lines();

    if success {
        if final_message.is_none() {
            return Err(CocoError::Execution {
                message: "coco sub-agent finished without returning an agent message.".to_string(),
                log: lines,
            });
        }
        return Ok(CocoRunOutcome {
            final_message,
            log: lines,
            exit_code: 0,
        });
    }

    let message = failure_message.unwrap_or_else(|| {
        "coco sub-agent ended unexpectedly without producing output.".to_string()
    });
    Err(CocoError::Execution {
        message,
        log: lines,
    })
}
