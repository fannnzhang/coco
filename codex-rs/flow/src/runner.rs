use std::fs::{self};
use std::path::Path;
use std::path::PathBuf;

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
use crate::engine::resolve_step;
use crate::human_renderer::HumanEventRenderer;

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
        let paths = create_step_paths(idx, step, agent_id)?;
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
        )?;
    }
    Ok(())
}

pub fn run_workflow_file(file: &WorkflowFile, opts: RunOptions) -> Result<()> {
    let name = file.name.clone().unwrap_or_else(|| "main".to_string());
    let cfg = file.clone().into_flow_config();
    run_workflow(&cfg, &name, opts)
}

fn run_step(
    cfg: &FlowConfig,
    step: &ResolvedStep,
    opts: RunOptions,
    step_index: usize,
    original_step: &StepSpec,
    agent_id: &str,
    memory_path: &Path,
    result_path: &Path,
    human_log_path: &Path,
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
            eprintln!("       log={}", memory_path.display());
            eprintln!("       result={}", result_path.display());
        }
    }

    let mut renderer = HumanEventRenderer::with_log_path(human_log_path)?;
    match step.engine.as_str() {
        "codex" => {
            if opts.mock {
                let mut engine = MockEngine::default();
                engine.run(EngineContext {
                    cfg,
                    resolved: step,
                    memory_path,
                    result_path,
                    renderer: &mut renderer,
                })?;
            } else {
                let mut engine = CodexEngine::new();
                engine.run(EngineContext {
                    cfg,
                    resolved: step,
                    memory_path,
                    result_path,
                    renderer: &mut renderer,
                })?;
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
        "codex" => {
            if let Some(path) = output_path {
                format!(
                    "cat \"{prompt}\" | codex exec --model {model} -o \"{out}\"",
                    prompt = step.prompt_path,
                    model = step.model,
                    out = path.display()
                )
            } else {
                format!(
                    "cat \"{prompt}\" | codex exec --model {model}",
                    prompt = step.prompt_path,
                    model = step.model
                )
            }
        }
        "codemachine" => format!(
            "codemachine run --agent-model {model} --prompt-file \"{prompt}\"",
            model = step.model,
            prompt = step.prompt_path
        ),
        other => format!("echo 'Unsupported engine: {other}'"),
    }
}

struct StepPaths {
    memory: PathBuf,
    human_log: PathBuf,
    result_md: PathBuf,
}

fn create_step_paths(step_index: usize, _step: &StepSpec, agent_id: &str) -> Result<StepPaths> {
    let slug = sanitize_label(agent_id);
    let stem = format!("{:02}-{slug}-agent", step_index + 1, slug = slug);

    // JSON event logs now go under .codex-flow/debug to avoid polluting memory context
    let memory_dir = Path::new(".codex-flow").join("debug");
    fs::create_dir_all(&memory_dir)
        .with_context(|| format!("failed to create debug dir {}", memory_dir.display()))?;

    let logs_dir = Path::new(".codex-flow").join("logs");
    fs::create_dir_all(&logs_dir)
        .with_context(|| format!("failed to create logs dir {}", logs_dir.display()))?;

    let memory_md_dir = Path::new(".codex-flow").join("memory");
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
