use std::fs;

use anyhow::Result;
use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn flow_init_creates_scaffold() -> Result<()> {
    let temp = TempDir::new()?;
    let target = temp.path().join("workspace");
    fs::create_dir_all(&target)?;

    Command::cargo_bin("codex")?
        .arg("flow")
        .arg("init")
        .arg("--dir")
        .arg(&target)
        .arg("--force")
        .assert()
        .success();

    assert!(target.join(".codex-flow").is_dir());
    assert!(
        target
            .join(".codex-flow/workflows/commit.workflow.toml")
            .is_file()
    );

    Ok(())
}
