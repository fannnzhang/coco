use std::fs::{self};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;

use crate::config::FlowConfig;
use crate::config::StepSpec;
use crate::config::WorkflowFile;
use crate::engine::CodexEngine;
use crate::engine::Engine;
use crate::engine::EngineContext;
use crate::engine::MockEngine;
use crate::engine::ResolvedStep;
use crate::engine::metrics::token_ledger::StepHandle;
use crate::engine::metrics::token_ledger::TokenLedger;
use crate::engine::metrics::token_ledger::UsageRecorder;
use crate::engine::resolve_step;
use crate::human_renderer::HumanEventRenderer;
use crate::runtime::init as runtime_init;

pub mod migrations;
pub mod planner;
pub mod state_store;

pub use state_store::PersistenceMode;
pub use state_store::StepState;
pub use state_store::StepStatus;
pub use state_store::TokenUsage;
pub use state_store::WorkflowRunState;
pub use state_store::WorkflowStateStore;

#[derive(Debug)]
pub struct RunSummary {
    pub executed_steps: usize,
    pub skipped_steps: usize,
    pub resume_pointer: usize,
    pub run_id: Option<String>,
    pub token_usage: Option<TokenUsage>,
}

pub struct StatePersistence {
    pub run_id: String,
    pub start_index: usize,
    pub store: WorkflowStateStore,
}

impl StatePersistence {
    pub fn with_start(run_id: String, start_index: usize, store: WorkflowStateStore) -> Self {
        Self {
            run_id,
            start_index,
            store,
        }
    }
}

#[derive(Clone, Copy)]
pub struct RunOptions {
    pub mock: bool,
    pub verbose: bool,
}

pub fn run_workflow(
    cfg: &FlowConfig,
    name: &str,
    opts: RunOptions,
    persistence: Option<StatePersistence>,
) -> Result<RunSummary> {
    runtime_init::ensure_runtime_tree()?;
    let Some(wf) = cfg.workflows.get(name) else {
        bail!("workflow not found: {name}");
    };
    if opts.verbose {
        eprintln!("Running workflow {name} (mock={})", opts.mock);
    }

    let (mut state_store, mut resume_cursor, run_id) = if let Some(p) = persistence {
        (Some(p.store), p.start_index, Some(p.run_id))
    } else {
        (None, 0, None)
    };
    let initial_pointer = resume_cursor;
    let interrupt_flag = install_interrupt_handler();
    interrupt_flag.store(false, Ordering::SeqCst);

    let mut executed_steps = 0usize;
    let mut ledger = if state_store.is_some() || opts.verbose {
        Some(TokenLedger::new())
    } else {
        None
    };

    for (idx, step) in wf.steps.iter().enumerate() {
        if interrupt_flag.load(Ordering::SeqCst) {
            if let Some(store) = state_store.as_mut() {
                store.record_interruption(store.state().resume_pointer)?;
            }
            bail!("workflow interrupted (SIGINT)");
        }
        if idx < resume_cursor {
            if opts.verbose {
                eprintln!(
                    "Skipping step-{} (resume pointer at {})",
                    idx + 1,
                    resume_cursor
                );
            }
            continue;
        }
        let agent_id = &step.agent;
        let Some(agent) = cfg.agents.get(agent_id) else {
            bail!("agent not found: {agent_id}");
        };
        let resolved = resolve_step(agent, step);
        let paths = create_step_paths(idx, step, agent_id)?;
        let memory_path_str = paths.result_md.display().to_string();
        let debug_log_str = paths.memory.display().to_string();
        let mut step_handle = ledger.as_mut().map(|ledger| ledger.step(&resolved.model));
        let run_result = {
            let usage_recorder = step_handle
                .as_mut()
                .map(|handle| handle as &mut dyn UsageRecorder);
            run_step(
                cfg,
                &resolved,
                opts,
                idx,
                step,
                agent_id,
                paths.memory.as_path(),
                paths.result_md.as_path(),
                paths.human_log.as_path(),
                usage_recorder,
            )
        };
        let token_delta = step_handle.and_then(StepHandle::finish);
        match run_result {
            Ok(()) => {
                if let Some(store) = state_store.as_mut() {
                    store.record_step(StepState {
                        index: idx,
                        status: StepStatus::Completed,
                        memory_path: memory_path_str.clone(),
                        debug_log: Some(debug_log_str.clone()),
                        needs_real: false,
                        token_delta: token_delta.clone(),
                    })?;
                    resume_cursor = store.state().resume_pointer;
                }
                executed_steps += 1;
            }
            Err(err) => {
                if let Some(store) = state_store.as_mut() {
                    store.record_step(StepState {
                        index: idx,
                        status: StepStatus::Failed,
                        memory_path: memory_path_str,
                        debug_log: Some(debug_log_str),
                        needs_real: false,
                        token_delta,
                    })?;
                }
                return Err(err);
            }
        }
    }
    let resume_pointer = state_store
        .as_ref()
        .map(|store| store.state().resume_pointer)
        .unwrap_or(resume_cursor);
    let ledger_total = ledger
        .as_ref()
        .and_then(|ledger| ledger.total_usage().cloned());
    if let (Some(store), Some(delta)) = (state_store.as_mut(), ledger_total.as_ref()) {
        store.append_token_usage(delta)?;
    }
    Ok(RunSummary {
        executed_steps,
        skipped_steps: initial_pointer.min(wf.steps.len()),
        resume_pointer,
        run_id,
        token_usage: ledger_total,
    })
}

pub fn run_workflow_file(
    file: &WorkflowFile,
    opts: RunOptions,
    persistence: Option<StatePersistence>,
) -> Result<RunSummary> {
    let name = file.name.clone().unwrap_or_else(|| "main".to_string());
    let cfg = file.clone().into_flow_config();
    run_workflow(&cfg, &name, opts, persistence)
}

#[allow(clippy::too_many_arguments)]
fn run_step<'a>(
    cfg: &FlowConfig,
    step: &'a ResolvedStep,
    opts: RunOptions,
    step_index: usize,
    original_step: &StepSpec,
    agent_id: &str,
    memory_path: &'a Path,
    result_path: &'a Path,
    human_log_path: &'a Path,
    mut usage_recorder: Option<&'a mut dyn UsageRecorder>,
) -> Result<()> {
    let step_label = original_step
        .description
        .as_deref()
        .filter(|desc| !desc.trim().is_empty())
        .unwrap_or(agent_id);

    if opts.verbose {
        let mode = if opts.mock { "mock" } else { "real" };
        eprintln!(
            "[{mode}] step-{} ({}) -> {agent_id}",
            step_index + 1,
            step_label
        );
        if opts.mock {
            eprintln!("       replay={}", memory_path.display());
            eprintln!(
                "       command={}",
                build_shell_command(step, Some(result_path))
            );
        } else {
            eprintln!(
                "       engine={} model={} prompt={}",
                step.engine, step.model, step.prompt_path
            );
            if let Some(effort) = step.reasoning_effort {
                eprintln!("       reasoning_effort={effort}");
            }
            if let Some(summary) = step.reasoning_summary {
                eprintln!("       reasoning_summary={summary}");
            }
            eprintln!("       log={}", memory_path.display());
            eprintln!("       result={}", result_path.display());
        }
    }

    let mut renderer = HumanEventRenderer::with_log_path(human_log_path)?;
    match step.engine.as_str() {
        "codex" => {
            if opts.mock {
                let mut engine = MockEngine::default();
                engine.run(
                    EngineContext {
                        cfg,
                        resolved: step,
                        memory_path,
                        result_path,
                        renderer: &mut renderer,
                    },
                    usage_recorder.take(),
                )?;
            } else {
                let mut engine = CodexEngine::new();
                engine.run(
                    EngineContext {
                        cfg,
                        resolved: step,
                        memory_path,
                        result_path,
                        renderer: &mut renderer,
                    },
                    usage_recorder.take(),
                )?;
            }
        }
        "codemachine" => {
            let cmd = build_shell_command(step, Some(result_path));
            eprintln!("codemachine execution not yet implemented, command: {cmd}");
        }
        other => bail!("Unsupported engine: {other}"),
    }
    Ok(())
}

fn build_shell_command(step: &ResolvedStep, output_path: Option<&Path>) -> String {
    match step.engine.as_str() {
        "codex" => build_codex_command(step, output_path),
        "codemachine" => format!(
            "codemachine run --agent-model {model} --prompt-file \"{prompt}\"",
            model = step.model,
            prompt = step.prompt_path
        ),
        other => format!("echo 'Unsupported engine: {other}'"),
    }
}

fn build_codex_command(step: &ResolvedStep, output_path: Option<&Path>) -> String {
    let mut cmd = format!(
        "cat \"{prompt}\" | codex exec --model {model}",
        prompt = step.prompt_path,
        model = step.model
    );
    if let Some(effort) = step.reasoning_effort {
        cmd.push_str(&format!(
            " --config model_reasoning_effort=\\\"{effort}\\\""
        ));
    }
    if let Some(summary) = step.reasoning_summary {
        cmd.push_str(&format!(" --config reasoning_summary=\\\"{summary}\\\""));
    }
    if let Some(path) = output_path {
        cmd.push_str(&format!(" -o \"{}\"", path.display()));
    }
    cmd
}

struct StepPaths {
    memory: PathBuf,
    human_log: PathBuf,
    result_md: PathBuf,
}

fn create_step_paths(step_index: usize, _step: &StepSpec, agent_id: &str) -> Result<StepPaths> {
    let slug = sanitize_label(agent_id);
    let stem = format!("{:02}-{slug}-agent", step_index + 1, slug = slug);

    // All runtime artifacts live under .codex-flow/runtime to keep the workspace tidy
    let runtime_root = Path::new(".codex-flow").join("runtime");
    fs::create_dir_all(&runtime_root)
        .with_context(|| format!("failed to create runtime dir {}", runtime_root.display()))?;

    let memory_dir = runtime_root.join("debug");
    fs::create_dir_all(&memory_dir)
        .with_context(|| format!("failed to create debug dir {}", memory_dir.display()))?;

    let logs_dir = runtime_root.join("logs");
    fs::create_dir_all(&logs_dir)
        .with_context(|| format!("failed to create logs dir {}", logs_dir.display()))?;

    let memory_md_dir = runtime_root.join("memory");
    fs::create_dir_all(&memory_md_dir)
        .with_context(|| format!("failed to create memory dir {}", memory_md_dir.display()))?;

    Ok(StepPaths {
        memory: memory_dir.join(format!("{stem}.json")),
        human_log: logs_dir.join(format!("{stem}.log")),
        result_md: memory_md_dir.join(format!("{stem}-result.md")),
    })
}

fn sanitize_label(label: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if (ch.is_ascii_whitespace() || matches!(ch, '-' | '_' | '.' | '/'))
            && !last_was_dash
            && !slug.is_empty()
        {
            slug.push('-');
            last_was_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "step".to_string()
    } else {
        trimmed.to_string()
    }
}

fn install_interrupt_handler() -> Arc<AtomicBool> {
    static INTERRUPT_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    INTERRUPT_FLAG
        .get_or_init(|| {
            let flag = Arc::new(AtomicBool::new(false));
            let handler_flag = flag.clone();
            // Ignore handler installation errors; another handler may already be set in tests.
            let _ = ctrlc::set_handler(move || {
                handler_flag.store(true, Ordering::SeqCst);
            });
            flag
        })
        .clone()
}
