use std::fs;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use include_dir::include_dir;
use walkdir::WalkDir;

const DEFAULT_WORKFLOW_TOML: &str = r#"name = "commit_flow"

[defaults]
engine = "codex"
mock = true

[agents.commit]
engine = "codex"
model = "gpt-5"
prompt = ".codex-flow/prompts/workflows/git-commit-workflow.md"

[workflow]
description = "从 git diff 生成提交信息"

  [[workflow.steps]]
  agent = "commit"
  # Mock 模式下无需 input/output，真实模式可选用以下字段：
  # [workflow.steps.input]
  # template = "..."
  # [workflow.steps.output]
  # kind = "stdout" | "file"
  # path = "..."
"#;

static EMBEDDED_PROMPTS: include_dir::Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/templates/prompts");

pub fn init_scaffold(target_dir: &Path, templates_dir: Option<&Path>, force: bool) -> Result<()> {
    let root = target_dir.join(".codex-flow");
    let prompts_dst = root.join("prompts");
    if !root.exists() {
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create {}", root.display()))?;
    }

    // Ensure runtime directories exist even before the first run for clearer structure.
    let runtime_root = root.join("runtime");
    for sub in ["debug", "logs", "memory"] {
        let dir = runtime_root.join(sub);
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    }

    fs::create_dir_all(&prompts_dst)
        .with_context(|| format!("failed to create {}", prompts_dst.display()))?;
    if let Some(path) = templates_dir {
        copy_dir(path, &prompts_dst, force)?;
    } else {
        copy_embedded_templates(&prompts_dst, force)?;
    }

    // Create a sample single-workflow file under .codex-flow/workflows/
    let workflows_dir = root.join("workflows");
    fs::create_dir_all(&workflows_dir)
        .with_context(|| format!("failed to create {}", workflows_dir.display()))?;
    let workflow_file = workflows_dir.join("commit.workflow.toml");
    if !workflow_file.exists() || force {
        fs::write(&workflow_file, DEFAULT_WORKFLOW_TOML)
            .with_context(|| format!("failed to write {}", workflow_file.display()))?;
    }
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path, force: bool) -> Result<()> {
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let rel = match entry.path().strip_prefix(src) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let target_path = dst.join(rel);
        if entry.path().is_dir() {
            fs::create_dir_all(&target_path)
                .with_context(|| format!("failed to create dir {}", target_path.display()))?;
        } else {
            if target_path.exists() && !force {
                // Skip existing file when not forced
                continue;
            }
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create dir {}", parent.display()))?;
            }
            let data = fs::read(entry.path())
                .with_context(|| format!("failed to read {}", entry.path().display()))?;
            fs::write(&target_path, data)
                .with_context(|| format!("failed to write {}", target_path.display()))?;
        }
    }
    Ok(())
}

fn copy_embedded_templates(dst: &Path, force: bool) -> Result<()> {
    for file in EMBEDDED_PROMPTS.files() {
        let target_path = dst.join(file.path());
        if target_path.exists() && !force {
            continue;
        }
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }
        fs::write(&target_path, file.contents())
            .with_context(|| format!("failed to write {}", target_path.display()))?;
    }
    Ok(())
}
