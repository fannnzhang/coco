use std::fs::File;
use std::fs::{self};
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::io::{self};
use std::path::Path;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;
use std::thread;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use codex_exec::exec_events::ThreadEvent;
use codex_exec::exec_events::ThreadItemDetails;

use crate::config::AgentSpec;
use crate::config::FlowConfig;
use crate::config::StepSpec;
use crate::human_renderer::HumanEventRenderer;
use codex_protocol::config_types::ReasoningEffort;
use codex_protocol::config_types::ReasoningSummary;
use metrics::token_ledger::UsageRecorder;

#[derive(Debug, Clone)]
pub struct ResolvedStep {
    pub engine: String,
    pub model: String,
    pub profile: Option<String>,
    pub prompt_path: String,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub reasoning_summary: Option<ReasoningSummary>,
}

pub fn resolve_step(base: &AgentSpec, step: &StepSpec) -> ResolvedStep {
    let engine = step
        .engine
        .as_deref()
        .or(base.engine.as_deref())
        .unwrap_or("codex");
    let model = step
        .model
        .as_deref()
        .or(base.model.as_deref())
        .unwrap_or("gpt-5");
    let prompt_path = step.prompt.as_deref().unwrap_or(&base.prompt);
    let profile = base.profile.clone();
    let reasoning_effort = step.reasoning_effort.or(base.reasoning_effort);
    let reasoning_summary = step.reasoning_summary.or(base.reasoning_summary);
    ResolvedStep {
        engine: engine.to_string(),
        model: model.to_string(),
        profile,
        prompt_path: prompt_path.to_string(),
        reasoning_effort,
        reasoning_summary,
    }
}

pub mod metrics;

pub struct EngineContext<'a> {
    pub cfg: &'a FlowConfig,
    pub resolved: &'a ResolvedStep,
    pub memory_path: &'a Path,
    // Path to write the agent's final message (Markdown) via `codex exec -o`
    pub result_path: &'a Path,
    pub renderer: &'a mut HumanEventRenderer,
}

pub trait Engine {
    fn name(&self) -> &'static str;
    fn run(
        &mut self,
        ctx: EngineContext<'_>,
        metrics: Option<&mut dyn UsageRecorder>,
    ) -> Result<()>;
}

pub struct CodexEngine;

impl CodexEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodexEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for CodexEngine {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn run(
        &mut self,
        ctx: EngineContext<'_>,
        metrics: Option<&mut dyn UsageRecorder>,
    ) -> Result<()> {
        run_codex(ctx, metrics)
    }
}

pub struct MockEngine {
    delay: Duration,
}

impl MockEngine {
    pub fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

impl Default for MockEngine {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(150),
        }
    }
}

impl Engine for MockEngine {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn run(
        &mut self,
        ctx: EngineContext<'_>,
        metrics: Option<&mut dyn UsageRecorder>,
    ) -> Result<()> {
        replay_mock(ctx, self.delay, metrics)
    }
}

fn run_codex(ctx: EngineContext<'_>, mut metrics: Option<&mut dyn UsageRecorder>) -> Result<()> {
    let prompt = fs::read_to_string(&ctx.resolved.prompt_path).with_context(|| {
        format!(
            "failed to read prompt template {}",
            ctx.resolved.prompt_path
        )
    })?;

    let (bin, preset_args) = ctx
        .cfg
        .engines
        .codex
        .as_ref()
        .map(|detail| {
            (
                detail.bin.clone().unwrap_or_else(|| "cocos".to_string()),
                detail.args.clone(),
            )
        })
        .unwrap_or_else(|| ("cocos".to_string(), Vec::new()));

    let mut cmd = Command::new(bin);
    if !preset_args.is_empty() {
        cmd.args(&preset_args);
    }
    if !preset_args.iter().any(|arg| arg == "exec") {
        cmd.arg("exec");
    }

    if let Some(effort) = ctx.resolved.reasoning_effort {
        cmd.arg("--config");
        cmd.arg(format!("model_reasoning_effort=\"{effort}\""));
    }

    if let Some(summary) = ctx.resolved.reasoning_summary {
        cmd.arg("--config");
        cmd.arg(format!("reasoning_summary=\"{summary}\""));
    }

    if let Some(profile) = &ctx.resolved.profile {
        cmd.arg("--profile");
        cmd.arg(profile);
    } else {
        cmd.arg("--model");
        cmd.arg(&ctx.resolved.model);
    }

    if !preset_args.iter().any(|arg| arg == "--json") {
        cmd.arg("--json");
    }

    // Ensure the final agent message is captured to Markdown for memory reuse.
    // This mirrors the debug JSON stream but writes a clean summary.
    cmd.arg("--output-last-message");
    cmd.arg(ctx.result_path);

    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn codex exec")?;
    {
        let mut stdin = child
            .stdin
            .take()
            .context("failed to open codex exec stdin handle")?;
        stdin
            .write_all(prompt.as_bytes())
            .context("failed to write prompt to codex exec stdin")?;
    }
    let stdout = child
        .stdout
        .take()
        .context("failed to open codex exec stdout handle")?;
    let stderr = child
        .stderr
        .take()
        .context("failed to open codex exec stderr handle")?;

    let mut log_writer = BufWriter::new(
        File::create(ctx.memory_path)
            .with_context(|| format!("failed to create step log {}", ctx.memory_path.display()))?,
    );

    let stderr_handle = thread::spawn(move || -> io::Result<String> {
        let mut reader = BufReader::new(stderr);
        let mut collected = String::new();
        loop {
            let mut line = String::new();
            let len = reader.read_line(&mut line)?;
            if len == 0 {
                break;
            }
            io::stderr().flush().ok();
            collected.push_str(&line);
        }
        Ok(collected)
    });

    let mut reader = BufReader::new(stdout);

    loop {
        let mut line = String::new();
        let len = reader
            .read_line(&mut line)
            .context("failed to read codex exec stdout")?;
        if len == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.starts_with('{') {
            ctx.renderer.log_plain_line(trimmed);
            continue;
        }
        writeln!(log_writer, "{trimmed}")
            .with_context(|| format!("failed to write step log {}", ctx.memory_path.display()))?;
        log_writer
            .flush()
            .with_context(|| format!("failed to flush step log {}", ctx.memory_path.display()))?;
        let event: ThreadEvent = serde_json::from_str(trimmed)
            .with_context(|| format!("failed to parse codex exec event: {trimmed}"))?;
        ctx.renderer.render_event(&event);
        if let Some(sink) = metrics.as_deref_mut()
            && let ThreadEvent::TurnCompleted(turn) = &event
        {
            sink.record_turn_usage(&turn.usage);
        }
    }

    log_writer
        .flush()
        .with_context(|| format!("failed to flush step log {}", ctx.memory_path.display()))?;

    let status = child
        .wait()
        .context("failed to wait on codex exec process")?;

    let stderr_output = stderr_handle
        .join()
        .map_err(|_| anyhow!("failed to join codex exec stderr reader"))?
        .map_err(|err| anyhow!("failed to read codex exec stderr: {err}"))?;

    if !stderr_output.is_empty() {
        writeln!(log_writer, "STDERR: {}", stderr_output.trim_end())
            .with_context(|| format!("failed to write step log {}", ctx.memory_path.display()))?;
        log_writer
            .flush()
            .with_context(|| format!("failed to flush step log {}", ctx.memory_path.display()))?;
    }

    if !status.success() {
        bail!("codex exec exited with {}", display_exit(status));
    }

    Ok(())
}

fn replay_mock(
    ctx: EngineContext<'_>,
    delay: Duration,
    mut metrics: Option<&mut dyn UsageRecorder>,
) -> Result<()> {
    let file = File::open(ctx.memory_path).with_context(|| {
        format!(
            "failed to open mock memory log {}",
            ctx.memory_path.display()
        )
    })?;
    let reader = BufReader::new(file);

    let mut emitted_any = false;
    let mut last_agent_message: Option<String> = None;
    for line in reader.lines() {
        let line = line.with_context(|| {
            format!(
                "failed to read mock memory log {}",
                ctx.memory_path.display()
            )
        })?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if emitted_any {
            thread::sleep(delay);
        }
        let event: ThreadEvent = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "failed to parse mock memory event from {}: {trimmed}",
                ctx.memory_path.display()
            )
        })?;
        // Track the latest agent message to mirror `codex exec -o` behavior in mock mode.
        match &event {
            ThreadEvent::ItemStarted(e) => {
                if let ThreadItemDetails::AgentMessage(msg) = &e.item.details {
                    last_agent_message = Some(msg.text.clone());
                }
            }
            ThreadEvent::ItemUpdated(e) => {
                if let ThreadItemDetails::AgentMessage(msg) = &e.item.details {
                    last_agent_message = Some(msg.text.clone());
                }
            }
            ThreadEvent::ItemCompleted(e) => {
                if let ThreadItemDetails::AgentMessage(msg) = &e.item.details {
                    last_agent_message = Some(msg.text.clone());
                }
            }
            _ => {}
        }
        ctx.renderer.render_event(&event);
        if let Some(sink) = metrics.as_deref_mut()
            && let ThreadEvent::TurnCompleted(turn) = &event
        {
            sink.record_turn_usage(&turn.usage);
        }
        emitted_any = true;
    }

    if !emitted_any {
        bail!(
            "mock memory log {} does not contain any JSON events",
            ctx.memory_path.display()
        );
    }

    // Write the final agent message to the desired result path if available.
    if let Some(text) = last_agent_message {
        if let Some(parent) = ctx.result_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to ensure memory dir {}", parent.display()))?;
        }
        fs::write(ctx.result_path, format!("{text}\n")).with_context(|| {
            format!("failed to write agent result {}", ctx.result_path.display())
        })?;
    }

    Ok(())
}

fn display_exit(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        format!("code {code}")
    } else {
        "signal".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentSpec;
    use crate::config::StepSpec;

    fn agent_spec(
        reasoning_effort: Option<ReasoningEffort>,
        reasoning_summary: Option<ReasoningSummary>,
    ) -> AgentSpec {
        AgentSpec {
            engine: Some("codex".to_string()),
            model: Some("gpt-5".to_string()),
            profile: None,
            prompt: "prompt.md".to_string(),
            reasoning_effort,
            reasoning_summary,
        }
    }

    fn step_spec(
        reasoning_effort: Option<ReasoningEffort>,
        reasoning_summary: Option<ReasoningSummary>,
    ) -> StepSpec {
        StepSpec {
            agent: "commit".to_string(),
            reasoning_effort,
            reasoning_summary,
            ..StepSpec::default()
        }
    }

    #[test]
    fn resolve_step_inherits_agent_reasoning_effort() {
        let agent = agent_spec(Some(ReasoningEffort::Low), None);
        let step = step_spec(None, None);

        let resolved = resolve_step(&agent, &step);

        assert_eq!(resolved.reasoning_effort, Some(ReasoningEffort::Low));
    }

    #[test]
    fn resolve_step_prefers_step_reasoning_effort() {
        let agent = agent_spec(Some(ReasoningEffort::Low), None);
        let step = step_spec(Some(ReasoningEffort::High), None);

        let resolved = resolve_step(&agent, &step);

        assert_eq!(resolved.reasoning_effort, Some(ReasoningEffort::High));
    }

    #[test]
    fn resolve_step_inherits_agent_reasoning_summary() {
        let agent = agent_spec(None, Some(ReasoningSummary::Concise));
        let step = step_spec(None, None);

        let resolved = resolve_step(&agent, &step);

        assert_eq!(resolved.reasoning_summary, Some(ReasoningSummary::Concise));
    }

    #[test]
    fn resolve_step_prefers_step_reasoning_summary() {
        let agent = agent_spec(None, Some(ReasoningSummary::Detailed));
        let step = step_spec(None, Some(ReasoningSummary::None));

        let resolved = resolve_step(&agent, &step);

        assert_eq!(resolved.reasoning_summary, Some(ReasoningSummary::None));
    }
}
