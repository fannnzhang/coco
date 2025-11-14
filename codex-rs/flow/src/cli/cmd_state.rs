use std::fs;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use walkdir::WalkDir;

use crate::cli::args::StateArgs;
use crate::cli::args::StateCommand;
use crate::cli::args::StatePruneArgs;
use crate::runtime::init as runtime_init;

pub fn run(args: StateArgs) -> Result<()> {
    match args.command {
        StateCommand::Prune(prune) => prune_state(prune),
    }
}

fn prune_state(args: StatePruneArgs) -> Result<()> {
    if args.days == 0 {
        bail!("--days must be greater than 0");
    }
    let runtime_root = runtime_init::ensure_runtime_tree()?;
    let state_root = runtime_root.join("state");
    let now = SystemTime::now();
    let cutoff = now
        .checked_sub(Duration::from_secs(args.days.saturating_mul(86_400)))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut stats = PruneStats::default();
    for entry in WalkDir::new(&state_root) {
        let entry = entry.with_context(|| format!("failed to walk {}", state_root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !name.ends_with(".resume.json") {
            continue;
        }
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to read metadata for {}", entry.path().display()))?;
        let len = metadata.len();
        stats.total_files += 1;
        stats.total_bytes += len;

        let stale = metadata
            .modified()
            .map(|mtime| mtime <= cutoff)
            .unwrap_or(true);
        if stale {
            fs::remove_file(entry.path())
                .with_context(|| format!("failed to remove {}", entry.path().display()))?;
            stats.removed_files += 1;
            stats.reclaimed_bytes += len;
        }
    }

    runtime_init::refresh_state_readme()?;
    print_summary(&state_root, args.days, &stats);
    Ok(())
}

fn print_summary(state_root: &Path, days: u64, stats: &PruneStats) {
    let remaining_bytes = stats.total_bytes.saturating_sub(stats.reclaimed_bytes);
    println!(
        "[state] scanned {} file(s) ({}) under {}",
        stats.total_files,
        format_bytes(stats.total_bytes),
        state_root.display()
    );
    println!(
        "[state] removed {} file(s) older than {} day(s); reclaimed {} (remaining {})",
        stats.removed_files,
        days,
        format_bytes(stats.reclaimed_bytes),
        format_bytes(remaining_bytes)
    );
}

#[derive(Default)]
struct PruneStats {
    total_files: u64,
    total_bytes: u64,
    removed_files: u64,
    reclaimed_bytes: u64,
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}
