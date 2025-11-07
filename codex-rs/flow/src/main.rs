use std::path::PathBuf;

use anyhow::Result;
use clap::ArgAction;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use codex_flow::config;
use codex_flow::runner;
use codex_flow::scaffold;

#[derive(Parser, Debug)]
#[command(
    name = "codex-flow",
    version,
    about = "Lightweight agent workflow runner (mock-first)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init(InitArgs),
    Run(RunArgs),
}

#[derive(Args, Debug)]
struct InitArgs {
    /// Target directory to place .codex-flow (default: current dir)
    #[arg(long)]
    dir: Option<PathBuf>,

    /// Force overwrite existing files
    #[arg(long)]
    force: bool,

    /// Templates source directory (default: embedded prompts bundled in the binary)
    #[arg(long, value_name = "DIR")]
    templates_dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Path to workflow TOML file
    file: PathBuf,

    /// Force mock execution (overrides defaults.mock)
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "no_mock")]
    mock: bool,

    /// Disable mock execution (overrides defaults.mock)
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "mock")]
    no_mock: bool,

    /// Verbose logs
    #[arg(long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init(args) => {
            let dir = args.dir.unwrap_or(std::env::current_dir()?);
            let templates = args.templates_dir.as_deref();
            scaffold::init_scaffold(&dir, templates, args.force)?;
        }
        Commands::Run(args) => {
            let verbose = args.verbose;
            let mock_override = if args.mock {
                Some(true)
            } else if args.no_mock {
                Some(false)
            } else {
                None
            };
            // Prefer single-workflow schema; fall back to multi-workflow.
            if let Ok(wf) = config::WorkflowFile::load(&args.file) {
                let mock = mock_override.unwrap_or_else(|| wf.defaults.mock.unwrap_or(true));
                runner::run_workflow_file(&wf, runner::RunOptions { mock, verbose })?;
            } else {
                let cfg = config::FlowConfig::load(&args.file)?;
                let mock = mock_override.unwrap_or_else(|| cfg.defaults.mock.unwrap_or(true));
                let name = cfg
                    .workflows
                    .keys()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| "main".to_string());
                runner::run_workflow(&cfg, &name, runner::RunOptions { mock, verbose })?;
            }
        }
    }
    Ok(())
}
