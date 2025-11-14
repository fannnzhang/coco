use std::path::PathBuf;

use clap::ArgAction;
use clap::Args;
use clap::Parser;
use clap::Subcommand;

#[derive(Parser, Debug)]
#[command(
    name = "codex-flow",
    version,
    about = "Lightweight agent workflow runner (mock-first)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Init(InitArgs),
    Run(RunArgs),
    Resume(ResumeArgs),
    State(StateArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Target directory to place .codex-flow (default: current dir)
    #[arg(long)]
    pub dir: Option<PathBuf>,

    /// Force overwrite existing files
    #[arg(long)]
    pub force: bool,

    /// Templates source directory (default: embedded prompts bundled in the binary)
    #[arg(long, value_name = "DIR")]
    pub templates_dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Path to workflow TOML file
    pub file: PathBuf,

    /// Force mock execution (overrides defaults.mock)
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "no_mock")]
    pub mock: bool,

    /// Disable mock execution (overrides defaults.mock)
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "mock")]
    pub no_mock: bool,

    /// Verbose logs
    #[arg(long)]
    pub verbose: bool,

    /// Custom run identifier used for resume state files
    #[arg(long, value_name = "RUN_ID")]
    pub run_id: Option<String>,

    /// Resume from an existing state file instead of starting from step-0
    #[arg(long, value_name = "STATE_PATH")]
    pub resume_from: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ResumeArgs {
    /// Path to workflow TOML file
    pub file: PathBuf,

    /// Run identifier captured during the original execution
    #[arg(long, value_name = "RUN_ID")]
    pub run_id: String,

    /// Force mock execution when resuming
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "no_mock")]
    pub mock: bool,

    /// Disable mock execution even if defaults.mock is true
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "mock")]
    pub no_mock: bool,

    /// Legacy alias for --mock retained for compatibility
    #[arg(long, action = ArgAction::SetTrue, hide = true)]
    pub mock_only: bool,

    /// Verbose logs
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Args, Debug)]
pub struct StateArgs {
    #[command(subcommand)]
    pub command: StateCommand,
}

#[derive(Subcommand, Debug)]
pub enum StateCommand {
    Prune(StatePruneArgs),
}

#[derive(Args, Debug)]
pub struct StatePruneArgs {
    /// Delete resume files older than this many days
    #[arg(long, value_name = "DAYS")]
    pub days: u64,
}
