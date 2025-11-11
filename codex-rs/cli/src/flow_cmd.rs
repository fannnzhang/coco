use std::path::PathBuf;

use anyhow::Result;
use clap::ArgAction;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use codex_flow::config;
use codex_flow::runner;
use codex_flow::runner::RunSummary;
use codex_flow::scaffold;

#[derive(Debug, Parser)]
pub struct FlowCli {
    #[command(subcommand)]
    command: FlowCommand,
}

impl FlowCli {
    pub fn run(self) -> Result<()> {
        match self.command {
            FlowCommand::Init(args) => handle_init(args),
            FlowCommand::Run(args) => handle_run(args),
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum FlowCommand {
    /// Initialize a .codex-flow workspace with sample prompts and workflows.
    Init(FlowInitArgs),

    /// Execute a workflow definition.
    Run(FlowRunArgs),
}

#[derive(Debug, Args)]
pub struct FlowInitArgs {
    /// Target directory to place .codex-flow (defaults to current directory).
    #[arg(long)]
    dir: Option<PathBuf>,

    /// Force overwriting existing files.
    #[arg(long)]
    force: bool,

    /// Templates source directory (default: embedded prompts bundled in the binary).
    #[arg(long, value_name = "DIR")]
    templates_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct FlowRunArgs {
    /// Path to workflow TOML file.
    #[arg(value_name = "FILE")]
    file: PathBuf,

    /// Force mock execution (overrides defaults.mock).
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "no_mock")]
    mock: bool,

    /// Disable mock execution (overrides defaults.mock).
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "mock")]
    no_mock: bool,

    /// Enable verbose logs.
    #[arg(long)]
    verbose: bool,
}

fn handle_init(args: FlowInitArgs) -> Result<()> {
    let dir = args.dir.unwrap_or(std::env::current_dir()?);
    let templates = args.templates_dir.as_deref();
    scaffold::init_scaffold(&dir, templates, args.force)
}

fn handle_run(args: FlowRunArgs) -> Result<()> {
    let verbose = args.verbose;
    let mock_override = if args.mock {
        Some(true)
    } else if args.no_mock {
        Some(false)
    } else {
        None
    };

    if let Ok(wf) = config::WorkflowFile::load(&args.file) {
        let mock = mock_override.unwrap_or_else(|| wf.defaults.mock.unwrap_or(true));
        runner::run_workflow_file(&wf, runner::RunOptions { mock, verbose }, None);
    } else {
        let cfg = config::FlowConfig::load(&args.file)?;
        let mock = mock_override.unwrap_or_else(|| cfg.defaults.mock.unwrap_or(true));
        let name = cfg
            .workflows
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "main".to_string());
        runner::run_workflow(&cfg, &name, runner::RunOptions { mock, verbose }, None);
    }

    Ok(())
}
