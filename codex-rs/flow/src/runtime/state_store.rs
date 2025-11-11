use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;

const RUNTIME_STATE_ENV: &str = "CODEX_FLOW_RUNTIME_DIR";

pub fn state_file_path(workflow_name: &str, run_id: &str) -> Result<PathBuf> {
    let dir = ensure_workflow_state_dir(workflow_name)?;
    Ok(dir.join(format!("{run_id}.resume.json")))
}

pub fn ensure_workflow_state_dir(workflow_name: &str) -> Result<PathBuf> {
    let dir = state_root().join(workflow_name);
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "failed to create workflow state directory {}",
            dir.display()
        )
    })?;
    Ok(dir)
}

pub fn state_root() -> PathBuf {
    runtime_root().join("state")
}

pub fn runtime_root() -> PathBuf {
    if let Ok(path) = std::env::var(RUNTIME_STATE_ENV) {
        PathBuf::from(path)
    } else {
        PathBuf::from(".codex-flow").join("runtime")
    }
}
