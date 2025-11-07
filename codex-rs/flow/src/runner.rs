use std::collections::HashMap;
use std::fs::File;
use std::fs::{self};
use std::io::BufRead;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::io::{self};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Stdio;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use codex_exec::exec_events::CommandExecutionItem;
use codex_exec::exec_events::CommandExecutionStatus;
use codex_exec::exec_events::ThreadEvent;
use codex_exec::exec_events::ThreadItemDetails;

use crate::config::AgentSpec;
use crate::config::FlowConfig;
use crate::config::StepSpec;
use crate::config::WorkflowFile;

#[derive(Clone, Copy)]
pub struct RunOptions {
    pub mock: bool,
    pub verbose: bool,
}

pub fn run_workflow(cfg: &FlowConfig, name: &str, opts: RunOptions) -> Result<()> {
    let Some(wf) = cfg.workflows.get(name) else {
        bail!("workflow not found: {name}");
    };
    if opts.verbose {
        eprintln!("Running workflow {name} (mock={})", opts.mock);
    }
    for (idx, step) in wf.steps.iter().enumerate() {
        let agent_id = &step.agent;
        let Some(agent) = cfg.agents.get(agent_id) else {
            bail!("agent not found: {agent_id}");
        };
        let resolved = resolve_step(agent, step);
        let cmd = build_shell_command(&resolved);
        let step_display = step
            .description
            .as_deref()
            .filter(|desc| !desc.trim().is_empty())
            .unwrap_or(agent_id);
        if opts.mock {
            if opts.verbose {
                eprintln!(
                    "[mock] {name} step-{} ({}) -> {agent_id}",
                    idx + 1,
                    step_display
                );
            }
            println!("{cmd}");
        } else {
            run_real_step(cfg, &resolved, opts, idx, step, agent_id)?;
        }
    }
    Ok(())
}

pub fn run_workflow_file(file: &WorkflowFile, opts: RunOptions) -> Result<()> {
    let name = file.name.clone().unwrap_or_else(|| "main".to_string());
    let cfg = file.clone().into_flow_config();
    run_workflow(&cfg, &name, opts)
}

fn run_real_step(
    cfg: &FlowConfig,
    step: &ResolvedStep,
    opts: RunOptions,
    step_index: usize,
    original_step: &StepSpec,
    agent_id: &str,
) -> Result<()> {
    let memory_path = create_memory_log_path(step_index, original_step, agent_id)?;
    if opts.verbose {
        let step_label = original_step
            .description
            .as_deref()
            .filter(|desc| !desc.trim().is_empty())
            .unwrap_or(agent_id);
        eprintln!(
            "[real] step-{} ({}) -> {agent_id}",
            step_index + 1,
            step_label
        );
        eprintln!(
            "       engine={} model={} prompt={}",
            step.engine, step.model, step.prompt_path
        );
        eprintln!("       log={}", memory_path.display());
    }
    match step.engine.as_str() {
        "codex" => {
            let status = execute_codex_step(cfg, step, &memory_path)?;
            if !status.success() {
                bail!("codex exec exited with {}", display_exit(&status));
            }
        }
        "codemachine" => {
            let cmd = build_shell_command(step);
            eprintln!("codemachine execution not yet implemented, command: {cmd}");
        }
        other => {
            bail!("Unsupported engine in real mode: {other}");
        }
    }
    Ok(())
}

fn build_shell_command(step: &ResolvedStep) -> String {
    match step.engine.as_str() {
        "codex" => format!(
            "cat \"{prompt}\" | codex exec --model {model}",
            prompt = step.prompt_path,
            model = step.model
        ),
        "codemachine" => format!(
            "codemachine run --agent-model {model} --prompt-file \"{prompt}\"",
            model = step.model,
            prompt = step.prompt_path
        ),
        other => format!("echo 'Unsupported engine: {other}'"),
    }
}

fn resolve_step(base: &AgentSpec, step: &StepSpec) -> ResolvedStep {
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
    ResolvedStep {
        engine: engine.to_string(),
        model: model.to_string(),
        prompt_path: prompt_path.to_string(),
    }
}

#[derive(Debug, Clone)]
struct ResolvedStep {
    engine: String,
    model: String,
    prompt_path: String,
}

fn create_memory_log_path(step_index: usize, step: &StepSpec, agent_id: &str) -> Result<PathBuf> {
    let memory_dir = Path::new(".codex-flow").join("memory");
    fs::create_dir_all(&memory_dir)
        .with_context(|| format!("failed to create memory dir {}", memory_dir.display()))?;
    let label = step
        .description
        .as_deref()
        .map(str::trim)
        .filter(|desc| !desc.is_empty())
        .unwrap_or(agent_id);
    let slug = sanitize_label(label);
    let file_name = format!("{:02}-{slug}.json", step_index + 1, slug = slug);
    Ok(memory_dir.join(file_name))
}

fn sanitize_label(label: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if ch.is_ascii_whitespace() || matches!(ch, '-' | '_' | '.' | '/') {
            if !last_was_dash && !slug.is_empty() {
                slug.push('-');
                last_was_dash = true;
            }
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "step".to_string()
    } else {
        trimmed.to_string()
    }
}

fn execute_codex_step(
    cfg: &FlowConfig,
    step: &ResolvedStep,
    memory_path: &Path,
) -> Result<ExitStatus> {
    let prompt = fs::read_to_string(&step.prompt_path)
        .with_context(|| format!("failed to read prompt template {}", step.prompt_path))?;

    let (bin, preset_args) = cfg
        .engines
        .codex
        .as_ref()
        .map(|detail| {
            (
                detail.bin.clone().unwrap_or_else(|| "codex".to_string()),
                detail.args.clone(),
            )
        })
        .unwrap_or_else(|| ("codex".to_string(), Vec::new()));

    let mut cmd = Command::new(bin);
    if !preset_args.is_empty() {
        cmd.args(&preset_args);
    }
    if !preset_args.iter().any(|arg| arg == "exec") {
        cmd.arg("exec");
    }
    cmd.arg("--model");
    cmd.arg(&step.model);

    if !preset_args.iter().any(|arg| arg == "--json") {
        cmd.arg("--json");
    }

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
        File::create(memory_path)
            .with_context(|| format!("failed to create step log {}", memory_path.display()))?,
    );

    let stderr_handle = std::thread::spawn(move || -> std::io::Result<String> {
        let mut reader = BufReader::new(stderr);
        let mut collected = String::new();
        loop {
            let mut line = String::new();
            let len = reader.read_line(&mut line)?;
            if len == 0 {
                break;
            }
            eprint!("{line}");
            io::stderr().flush().ok();
            collected.push_str(&line);
        }
        Ok(collected)
    });

    let mut reader = BufReader::new(stdout);
    let mut command_outputs: HashMap<String, String> = HashMap::new();

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
            println!("{trimmed}");
            continue;
        }
        writeln!(log_writer, "{trimmed}")
            .with_context(|| format!("failed to write step log {}", memory_path.display()))?;
        log_writer
            .flush()
            .with_context(|| format!("failed to flush step log {}", memory_path.display()))?;
        let event: ThreadEvent = serde_json::from_str(trimmed)
            .with_context(|| format!("failed to parse codex exec event: {trimmed}"))?;
        render_event(&event, &mut command_outputs);
    }

    log_writer
        .flush()
        .with_context(|| format!("failed to flush step log {}", memory_path.display()))?;

    let status = child
        .wait()
        .context("failed to wait on codex exec process")?;

    let stderr_output = stderr_handle
        .join()
        .map_err(|_| anyhow!("failed to join codex exec stderr reader"))?
        .map_err(|err| anyhow!("failed to read codex exec stderr: {err}"))?;

    drop(stderr_output);
    Ok(status)
}

fn render_event(event: &ThreadEvent, command_outputs: &mut HashMap<String, String>) {
    match event {
        ThreadEvent::ThreadStarted(ev) => {
            println!("Thread started: {}", ev.thread_id);
        }
        ThreadEvent::TurnStarted(_) => {
            println!("Turn started");
        }
        ThreadEvent::TurnCompleted(ev) => {
            println!(
                "Turn completed (tokens: in={}, cached={}, out={})",
                ev.usage.input_tokens, ev.usage.cached_input_tokens, ev.usage.output_tokens
            );
        }
        ThreadEvent::TurnFailed(ev) => {
            eprintln!("Turn failed: {}", ev.error.message);
        }
        ThreadEvent::ItemStarted(ev) => {
            if let ThreadItemDetails::CommandExecution(cmd) = &ev.item.details {
                println!("$ {}", cmd.command);
                command_outputs.insert(ev.item.id.clone(), String::new());
            }
        }
        ThreadEvent::ItemUpdated(ev) => {
            if let ThreadItemDetails::CommandExecution(cmd) = &ev.item.details {
                render_command_delta(command_outputs, &ev.item.id, &cmd.aggregated_output);
            }
        }
        ThreadEvent::ItemCompleted(ev) => match &ev.item.details {
            ThreadItemDetails::AgentMessage(msg) => {
                println!("{}", msg.text.trim());
            }
            ThreadItemDetails::Reasoning(reason) => {
                println!("Reasoning: {}", reason.text.trim());
            }
            ThreadItemDetails::CommandExecution(cmd) => {
                render_command_delta(command_outputs, &ev.item.id, &cmd.aggregated_output);
                println!(
                    "[{}] {} (exit: {})",
                    format_command_status(cmd),
                    cmd.command,
                    cmd.exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                );
                command_outputs.remove(&ev.item.id);
            }
            ThreadItemDetails::FileChange(change) => {
                println!("File changes: {} change(s)", change.changes.len());
            }
            ThreadItemDetails::McpToolCall(call) => {
                println!(
                    "MCP tool '{}' on server '{}' -> {:?}",
                    call.tool, call.server, call.status
                );
            }
            ThreadItemDetails::WebSearch(search) => {
                println!("Web search completed: {}", search.query);
            }
            ThreadItemDetails::TodoList(list) => {
                println!("Todo list updated ({} item(s))", list.items.len());
            }
            ThreadItemDetails::Error(err) => {
                eprintln!("Error: {}", err.message);
            }
        },
        ThreadEvent::Error(err) => {
            eprintln!("Stream error: {}", err.message);
        }
    }
}

fn render_command_delta(
    command_outputs: &mut HashMap<String, String>,
    item_id: &str,
    aggregated_output: &str,
) {
    let entry = command_outputs.entry(item_id.to_string()).or_default();
    if aggregated_output.len() >= entry.len() {
        let delta = &aggregated_output[entry.len()..];
        if !delta.is_empty() {
            print!("{delta}");
            if !delta.ends_with('\n') {
                println!();
            }
            io::stdout().flush().ok();
        }
    } else if !aggregated_output.is_empty() {
        println!("{aggregated_output}");
        io::stdout().flush().ok();
    }
    *entry = aggregated_output.to_string();
}

fn format_command_status(cmd: &CommandExecutionItem) -> &'static str {
    match cmd.status {
        CommandExecutionStatus::InProgress => "in-progress",
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
    }
}

fn display_exit(status: &ExitStatus) -> String {
    if let Some(code) = status.code() {
        format!("code {code}")
    } else {
        "signal".to_string()
    }
}
