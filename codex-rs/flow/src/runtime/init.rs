use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;

const STATE_README_TEMPLATE: &str = include_str!("../../templates/runtime/README-state.md");

pub fn ensure_runtime_tree() -> Result<PathBuf> {
    ensure_runtime_tree_at(Path::new(".codex-flow"))
}

pub fn ensure_runtime_tree_at(flow_root: &Path) -> Result<PathBuf> {
    if !flow_root.exists() {
        fs::create_dir_all(flow_root)
            .with_context(|| format!("failed to create {}", flow_root.display()))?;
    }
    let runtime_root = flow_root.join("runtime");
    fs::create_dir_all(&runtime_root)
        .with_context(|| format!("failed to create {}", runtime_root.display()))?;
    for dir in ["debug", "logs", "memory", "state"] {
        let path = runtime_root.join(dir);
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
    }
    write_state_readme(&runtime_root.join("state"))?;
    Ok(runtime_root)
}

pub fn warn_if_state_missing() {
    warn_if_state_missing_at(Path::new(".codex-flow"));
}

pub fn warn_if_state_missing_at(flow_root: &Path) {
    let state_dir = flow_root.join("runtime").join("state");
    if !state_dir.exists() {
        eprintln!(
            "warning: missing runtime state directory {}; run `codex-flow init` to create it",
            state_dir.display()
        );
    }
}

pub fn refresh_state_readme() -> Result<()> {
    let runtime_root = ensure_runtime_tree()?;
    write_state_readme_force(&runtime_root.join("state"))
}

fn write_state_readme(state_dir: &Path) -> Result<()> {
    let readme_path = state_dir.join("README-state.md");
    if readme_path.exists() {
        return Ok(());
    }
    write_state_readme_force(state_dir)
}

fn write_state_readme_force(state_dir: &Path) -> Result<()> {
    let readme_path = state_dir.join("README-state.md");
    fs::write(&readme_path, STATE_README_TEMPLATE)
        .with_context(|| format!("failed to write {}", readme_path.display()))?;
    Ok(())
}
