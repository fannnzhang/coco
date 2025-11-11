use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use chrono::Utc;
use clap::Parser;

use crate::config;
use crate::runner::PersistenceMode;
use crate::runner::RunOptions;
use crate::runner::StatePersistence;
use crate::runner::StepStatus;
use crate::runner::WorkflowRunState;
use crate::runner::WorkflowStateStore;
use crate::runner::planner::ResumePlanner;
use crate::runner::{self};
use crate::runtime::config as runtime_config;
use crate::runtime::init as runtime_init;
use crate::runtime::state_store as runtime_state;
use crate::scaffold;

pub mod args;
mod cmd_state;
mod output;

use args::Cli;
use args::Command;
use args::InitArgs;
use args::ResumeArgs;
use args::RunArgs;
use output::print_completion_summary;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    dispatch(cli)
}

fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => cmd_init(args),
        Command::Run(args) => cmd_run(args),
        Command::Resume(args) => cmd_resume(args),
        Command::State(args) => cmd_state::run(args),
    }
}

fn cmd_init(args: InitArgs) -> Result<()> {
    let dir = args
        .dir
        .clone()
        .unwrap_or(std::env::current_dir().context("failed to read current dir")?);
    let templates = args.templates_dir.as_deref();
    scaffold::init_scaffold(&dir, templates, args.force)
}

fn cmd_run(args: RunArgs) -> Result<()> {
    runtime_init::ensure_runtime_tree()?;
    let (cfg, workflow_name, defaults_mock) = load_workflow(&args.file)?;
    let workflow = cfg
        .workflows
        .get(&workflow_name)
        .with_context(|| format!("workflow `{workflow_name}` not found"))?;
    let mock = resolve_mock_flag(&args, defaults_mock);
    let (run_id, was_generated) = derive_run_id(args.run_id.clone())?;
    let resume_disabled = runtime_config::resume_disabled();
    if resume_disabled && args.resume_from.is_some() {
        bail!(
            "--resume-from cannot be used while {} is set",
            runtime_config::RESUME_DISABLED_ENV
        );
    }
    let mode = if mock {
        PersistenceMode::Mock
    } else {
        PersistenceMode::Real
    };
    let persistence = if resume_disabled {
        None
    } else {
        let mut store = WorkflowStateStore::load_or_init(&workflow_name, &run_id, mode)?;
        let mut start_index = 0usize;
        if let Some(state_path) = &args.resume_from {
            let resume_state = WorkflowRunState::load_from_path(state_path).with_context(|| {
                format!("failed to load resume state from {}", state_path.display())
            })?;
            ensure_resume_source_matches(&resume_state, &workflow_name)?;
            ensure_resume_bounds(&resume_state, workflow, &workflow_name)?;
            let pointer = resume_state.resume_pointer.min(workflow.steps.len());
            hydrate_store_from_source(&mut store, &resume_state, pointer)?;
            start_index = compute_resume_start(&resume_state, pointer);
        }
        Some(StatePersistence::with_start(
            run_id.clone(),
            start_index,
            store,
        ))
    };

    let summary = runner::run_workflow(
        &cfg,
        &workflow_name,
        RunOptions {
            mock,
            verbose: args.verbose,
        },
        persistence,
    )?;

    if was_generated {
        eprintln!("info: generated run-id {run_id}");
    }
    if resume_disabled {
        eprintln!(
            "info: {} is set; workflow state persistence skipped",
            runtime_config::RESUME_DISABLED_ENV
        );
    }
    print_completion_summary("run", Some(&run_id), &summary, args.verbose);
    Ok(())
}

fn cmd_resume(args: ResumeArgs) -> Result<()> {
    runtime_init::ensure_runtime_tree()?;
    if runtime_config::resume_disabled() {
        bail!(
            "codex-flow resume is disabled while {} is set",
            runtime_config::RESUME_DISABLED_ENV
        );
    }

    let (cfg, workflow_name, defaults_mock) = load_workflow(&args.file)?;
    validate_run_id(&args.run_id)?;
    let workflow = cfg
        .workflows
        .get(&workflow_name)
        .with_context(|| format!("workflow `{workflow_name}` not found"))?;
    let mock = resolve_resume_mock_flag(&args, defaults_mock);
    let mode = if mock {
        PersistenceMode::Mock
    } else {
        PersistenceMode::Real
    };

    let state_path = runtime_state::state_file_path(&workflow_name, &args.run_id)?;
    if !state_path.exists() {
        bail!(
            "resume state not found at {}. Run `codex-flow run` with --run-id {} first",
            state_path.display(),
            args.run_id
        );
    }

    let mut store = WorkflowStateStore::load_or_init(&workflow_name, &args.run_id, mode)?;
    ensure_resume_bounds(store.state(), workflow, &workflow_name)?;
    let planner = ResumePlanner::new(workflow);
    let plan = planner.plan(store.state());
    if plan.remaining_steps == 0 {
        println!(
            "Workflow `{}` run `{}` already completed; 0 steps executed.",
            workflow_name, args.run_id
        );
        return Ok(());
    }

    let mut start_index = plan.next_step;
    if !mock {
        let missing = mark_missing_debug_logs(&mut store, plan.next_step)?;
        for idx in missing {
            eprintln!(
                "step-{} debug log missing; marking needs_real=true and rerunning with real engine",
                idx + 1
            );
        }
        if let Some(idx) = store.state().first_needs_real_before(plan.next_step) {
            start_index = start_index.min(idx);
        }
    }

    let persistence = StatePersistence::with_start(args.run_id.clone(), start_index, store);
    let summary = runner::run_workflow(
        &cfg,
        &workflow_name,
        RunOptions {
            mock,
            verbose: args.verbose,
        },
        Some(persistence),
    )?;

    print_completion_summary("resume", Some(&args.run_id), &summary, args.verbose);
    Ok(())
}

fn load_workflow(path: &Path) -> Result<(config::FlowConfig, String, Option<bool>)> {
    if let Ok(file) = config::WorkflowFile::load(path) {
        let name = file.name.clone().unwrap_or_else(|| "main".to_string());
        let defaults = file.defaults.mock;
        Ok((file.into_flow_config(), name, defaults))
    } else {
        let cfg = config::FlowConfig::load(path)?;
        let name = cfg
            .workflows
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "main".to_string());
        let defaults = cfg.defaults.mock;
        Ok((cfg, name, defaults))
    }
}

fn resolve_mock_flag(args: &RunArgs, default: Option<bool>) -> bool {
    if args.mock {
        true
    } else if args.no_mock {
        false
    } else {
        default.unwrap_or(false)
    }
}

fn derive_run_id(input: Option<String>) -> Result<(String, bool)> {
    if let Some(value) = input {
        validate_run_id(&value)?;
        Ok((value, false))
    } else {
        Ok((default_run_id(), true))
    }
}

fn validate_run_id(id: &str) -> Result<()> {
    if id.is_empty() {
        bail!("run-id must not be empty");
    }
    if id.len() > 64 {
        bail!("run-id must be at most 64 characters");
    }
    if id
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
    {
        bail!("run-id may only contain alphanumeric characters, '-', '_', or '.'");
    }
    if id.contains('/') || id.contains('\\') {
        bail!("run-id must not contain path separators");
    }
    Ok(())
}

fn default_run_id() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

fn resolve_resume_mock_flag(args: &ResumeArgs, default: Option<bool>) -> bool {
    if args.mock || args.mock_only {
        true
    } else if args.no_mock {
        false
    } else {
        default.unwrap_or(true)
    }
}

fn ensure_resume_source_matches(state: &WorkflowRunState, workflow_name: &str) -> Result<()> {
    if state.workflow_name.is_empty() || state.workflow_name == workflow_name {
        Ok(())
    } else {
        bail!(
            "resume state belongs to workflow `{}` but `{workflow_name}` was requested",
            state.workflow_name
        );
    }
}

fn ensure_resume_bounds(
    state: &WorkflowRunState,
    workflow: &config::WorkflowSpec,
    workflow_name: &str,
) -> Result<()> {
    let total = workflow.steps.len();
    if state.resume_pointer > total {
        bail!(
            "resume pointer {} exceeds workflow `{}` step count {}",
            state.resume_pointer,
            workflow_name,
            total
        );
    }
    if let Some(step) = state.steps.iter().find(|step| step.index >= total) {
        bail!(
            "resume state references step-{} but workflow `{}` only has {} step(s)",
            step.index + 1,
            workflow_name,
            total
        );
    }
    Ok(())
}

fn hydrate_store_from_source(
    store: &mut WorkflowStateStore,
    source: &WorkflowRunState,
    pointer: usize,
) -> Result<()> {
    let state = store.state_mut();
    state.resume_pointer = pointer;
    state.steps = source.steps.clone();
    state.token_usage = source.token_usage.clone();
    store.flush()
}

fn compute_resume_start(state: &WorkflowRunState, pointer: usize) -> usize {
    state.first_needs_real_before(pointer).unwrap_or(pointer)
}

fn mark_missing_debug_logs(store: &mut WorkflowStateStore, before: usize) -> Result<Vec<usize>> {
    let missing: Vec<usize> = store
        .state()
        .steps
        .iter()
        .filter(|step| step.index < before)
        .filter(|step| matches!(step.status, StepStatus::Completed))
        .filter(|step| {
            !step
                .debug_log
                .as_deref()
                .map(debug_log_exists)
                .unwrap_or(false)
        })
        .map(|step| step.index)
        .collect();
    for idx in &missing {
        store.mark_step_needs_real(*idx)?;
    }
    Ok(missing)
}

fn debug_log_exists(path: &str) -> bool {
    Path::new(path).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_run_ids() {
        assert!(validate_run_id("").is_err());
        assert!(validate_run_id("../../etc/passwd").is_err());
        assert!(validate_run_id("spaces not allowed").is_err());
        assert!(validate_run_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn accepts_valid_run_ids() {
        assert!(validate_run_id("test123").is_ok());
        assert!(validate_run_id("2025-11-11T01").is_ok());
        assert!(validate_run_id("alpha_beta.gamma-123").is_ok());
    }
}
