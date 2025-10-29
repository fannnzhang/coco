use std::borrow::Cow;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use codex_apply_patch::ApplyPatchAction;
use codex_apply_patch::MaybeApplyPatchVerified;
use similar::TextDiff;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum LegacyEditError {
    #[error("{0}")]
    Message(String),
}

impl LegacyEditError {
    fn new(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

#[derive(Debug)]
enum LegacyEditCommand {
    WriteFile {
        path: String,
        content: String,
    },
    DeleteFile {
        path: String,
    },
    Replace {
        path: String,
        old: String,
        new: String,
        expected_replacements: Option<usize>,
    },
}

pub(crate) fn maybe_build_apply_patch_action(
    command: &[String],
    cwd: &Path,
) -> Result<Option<ApplyPatchAction>, LegacyEditError> {
    let Some(command_name) = command.first().map(|s| s.as_str()) else {
        return Ok(None);
    };

    let edit_command = match command_name {
        "create" | "write_file" => {
            if command.len() != 3 {
                return Err(LegacyEditError::new(format!(
                    "{command_name} expects exactly 2 arguments: path and content."
                )));
            }
            LegacyEditCommand::WriteFile {
                path: command[1].clone(),
                content: command[2].clone(),
            }
        }
        "delete" | "delete_file" => {
            if command.len() != 2 {
                return Err(LegacyEditError::new(format!(
                    "{command_name} expects exactly 1 argument: path."
                )));
            }
            LegacyEditCommand::DeleteFile {
                path: command[1].clone(),
            }
        }
        "replace" => {
            if command.len() != 4 && command.len() != 5 {
                return Err(LegacyEditError::new(
                    "replace expects arguments: path, old_string, new_string, [expected_replacements]",
                ));
            }
            let expected_replacements = if command.len() == 5 {
                Some(parse_expected_replacements(&command[4])?)
            } else {
                None
            };
            LegacyEditCommand::Replace {
                path: command[1].clone(),
                old: command[2].clone(),
                new: command[3].clone(),
                expected_replacements,
            }
        }
        _ => return Ok(None),
    };

    let action = build_action(edit_command, cwd)?;
    Ok(Some(action))
}

fn build_action(
    edit_command: LegacyEditCommand,
    cwd: &Path,
) -> Result<ApplyPatchAction, LegacyEditError> {
    match edit_command {
        LegacyEditCommand::WriteFile { path, content } => prepare_write_file(&path, content, cwd),
        LegacyEditCommand::DeleteFile { path } => prepare_delete_file(&path, cwd),
        LegacyEditCommand::Replace {
            path,
            old,
            new,
            expected_replacements,
        } => prepare_replace(&path, &old, &new, expected_replacements, cwd),
    }
}

pub(crate) fn build_write_file_action(
    path: &str,
    content: &str,
    cwd: &Path,
) -> Result<ApplyPatchAction, LegacyEditError> {
    build_action(
        LegacyEditCommand::WriteFile {
            path: path.to_string(),
            content: content.to_string(),
        },
        cwd,
    )
}

pub(crate) fn build_delete_file_action(
    path: &str,
    cwd: &Path,
) -> Result<ApplyPatchAction, LegacyEditError> {
    build_action(
        LegacyEditCommand::DeleteFile {
            path: path.to_string(),
        },
        cwd,
    )
}

pub(crate) fn build_replace_action(
    path: &str,
    old: &str,
    new: &str,
    expected_replacements: Option<usize>,
    cwd: &Path,
) -> Result<ApplyPatchAction, LegacyEditError> {
    build_action(
        LegacyEditCommand::Replace {
            path: path.to_string(),
            old: old.to_string(),
            new: new.to_string(),
            expected_replacements,
        },
        cwd,
    )
}

fn prepare_write_file(
    path: &str,
    content: String,
    cwd: &Path,
) -> Result<ApplyPatchAction, LegacyEditError> {
    let absolute_path = resolve_path(path, cwd);
    let (current_content, existed) = match fs::read_to_string(&absolute_path) {
        Ok(content) => (content, true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
        Err(err) => {
            return Err(LegacyEditError::new(format!(
                "write_file failed: unable to read {} ({err}).",
                absolute_path.display()
            )));
        }
    };

    if current_content == content {
        return Err(LegacyEditError::new(format!(
            "write_file skipped: new content for {} is identical to the existing content.",
            absolute_path.display()
        )));
    }

    let patch = if !existed {
        build_add_file_patch(&absolute_path, cwd, &content)
    } else {
        build_update_patch(&absolute_path, cwd, &current_content, &content)?
    };

    parse_patch(patch, cwd)
}

fn prepare_delete_file(path: &str, cwd: &Path) -> Result<ApplyPatchAction, LegacyEditError> {
    let absolute_path = resolve_path(path, cwd);
    if !absolute_path.exists() {
        return Err(LegacyEditError::new(format!(
            "delete failed: {} does not exist.",
            absolute_path.display()
        )));
    }
    let patch_path = path_for_patch(&absolute_path, cwd);
    let patch = format!("*** Begin Patch\n*** Delete File: {patch_path}\n*** End Patch");
    parse_patch(patch, cwd)
}

fn prepare_replace(
    path: &str,
    old: &str,
    new: &str,
    expected_replacements: Option<usize>,
    cwd: &Path,
) -> Result<ApplyPatchAction, LegacyEditError> {
    let absolute_path = resolve_path(path, cwd);
    let current_content = fs::read_to_string(&absolute_path).map_err(|err| {
        LegacyEditError::new(format!(
            "replace failed: unable to read {} ({err}).",
            absolute_path.display()
        ))
    })?;

    if old.is_empty() {
        return Err(LegacyEditError::new(
            "replace failed: old_string must not be empty. Use write_file to create a new file.",
        ));
    }

    let occurrences = current_content.match_indices(old).count();
    if occurrences == 0 {
        return Err(LegacyEditError::new(format!(
            "replace failed: did not find old_string in {}.",
            absolute_path.display()
        )));
    }

    let expected = expected_replacements.unwrap_or(1);
    if occurrences != expected {
        return Err(LegacyEditError::new(format!(
            "replace failed: expected {expected} occurrence(s) but found {occurrences} in {}.",
            absolute_path.display()
        )));
    }

    if old == new {
        return Err(LegacyEditError::new(
            "replace skipped: old_string and new_string are identical.",
        ));
    }

    let new_content = current_content.replacen(old, new, expected);
    if new_content == current_content {
        return Err(LegacyEditError::new(
            "replace skipped: no changes were produced.",
        ));
    }

    let patch = build_update_patch(&absolute_path, cwd, &current_content, &new_content)?;
    parse_patch(patch, cwd)
}

fn build_add_file_patch(path: &Path, cwd: &Path, content: &str) -> String {
    let patch_path = path_for_patch(path, cwd);
    let mut patch = String::new();
    patch.push_str("*** Begin Patch\n");
    patch.push_str(&format!("*** Add File: {patch_path}\n"));
    if !content.is_empty() {
        append_added_lines(&mut patch, content);
    }
    patch.push_str("*** End Patch");
    patch
}

fn build_update_patch(
    path: &Path,
    cwd: &Path,
    old_content: &str,
    new_content: &str,
) -> Result<String, LegacyEditError> {
    let patch_path = path_for_patch(path, cwd);
    let diff = TextDiff::from_lines(old_content, new_content);
    let diff_text = diff.unified_diff().context_radius(3).to_string();
    let mut unified = normalize_unified_diff(&diff_text);
    if unified.trim().is_empty() {
        return Err(LegacyEditError::new("generated diff is empty."));
    }
    if !unified.ends_with('\n') {
        unified.push('\n');
    }
    let patch = format!("*** Begin Patch\n*** Update File: {patch_path}\n{unified}*** End Patch");
    Ok(patch)
}

fn normalize_unified_diff(diff: &str) -> String {
    diff.split_inclusive('\n')
        .map(|line| {
            let (body, newline) = line
                .strip_suffix('\n')
                .map_or((line, ""), |stripped| (stripped, "\n"));
            let normalized = if let Some(rest) = body.strip_prefix("@@") {
                if rest.trim_start().starts_with('-') {
                    "@@"
                } else {
                    body
                }
            } else {
                body
            };
            format!("{normalized}{newline}")
        })
        .collect()
}

fn append_added_lines(dst: &mut String, content: &str) {
    for line in content.split_inclusive('\n') {
        if let Some(stripped) = line.strip_suffix('\n') {
            dst.push('+');
            dst.push_str(stripped);
            dst.push('\n');
        } else {
            dst.push('+');
            dst.push_str(line);
            dst.push('\n');
        }
    }
    if !content.ends_with('\n') || content.is_empty() {
        dst.push_str("*** End of File\n");
    }
}

fn parse_expected_replacements(value: &str) -> Result<usize, LegacyEditError> {
    let parsed = value.parse::<usize>().map_err(|_| {
        LegacyEditError::new(format!(
            "replace failed: expected_replacements must be a positive integer, got {value}."
        ))
    })?;
    if parsed == 0 {
        return Err(LegacyEditError::new(
            "replace failed: expected_replacements must be greater than zero.",
        ));
    }
    Ok(parsed)
}

fn parse_patch(patch: String, cwd: &Path) -> Result<ApplyPatchAction, LegacyEditError> {
    let argv = vec!["apply_patch".to_string(), patch];
    match codex_apply_patch::maybe_parse_apply_patch_verified(&argv, cwd) {
        MaybeApplyPatchVerified::Body(action) => Ok(action),
        MaybeApplyPatchVerified::CorrectnessError(err) => Err(LegacyEditError::new(format!(
            "failed to verify generated patch: {err}"
        ))),
        MaybeApplyPatchVerified::ShellParseError(err) => Err(LegacyEditError::new(format!(
            "failed to parse generated patch: {err:?}"
        ))),
        MaybeApplyPatchVerified::NotApplyPatch => Err(LegacyEditError::new(
            "failed to recognize generated patch as apply_patch.",
        )),
    }
}

fn resolve_path(path: &str, cwd: &Path) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate
    } else {
        cwd.join(candidate)
    }
}

fn path_for_patch(path: &Path, cwd: &Path) -> String {
    let relative = if let Ok(rel) = path.strip_prefix(cwd) {
        Cow::Owned(rel.to_path_buf())
    } else {
        Cow::Borrowed(path)
    };
    relative.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_apply_patch::ApplyPatchFileChange;
    use std::fs;
    use tempfile::tempdir;

    fn command(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn write_file_creates_new_file() {
        let tmp = tempdir().unwrap();
        let cwd = tmp.path();
        let args = command(&["write_file", "hello.txt", "hi there\n"]);
        let action = maybe_build_apply_patch_action(&args, cwd)
            .unwrap()
            .expect("write_file action");
        let changes = action.changes();
        let file_path = cwd.join("hello.txt");
        match changes.get(&file_path) {
            Some(ApplyPatchFileChange::Add { content }) => {
                assert_eq!(content, "hi there\n");
            }
            other => panic!("expected Add change, got {other:?}"),
        }
    }

    #[test]
    fn delete_file_requires_existing_file() {
        let tmp = tempdir().unwrap();
        let cwd = tmp.path();
        let args = command(&["delete", "missing.txt"]);
        let err = maybe_build_apply_patch_action(&args, cwd)
            .expect_err("delete should fail for missing file");
        assert!(
            err.to_string().contains("does not exist"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn replace_updates_content() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("note.md");
        fs::write(&file, "hello world\n").unwrap();
        let args = command(&["replace", "note.md", "world", "codex"]);
        let action = maybe_build_apply_patch_action(&args, tmp.path())
            .unwrap()
            .expect("replace action");
        match action.changes().get(&file) {
            Some(ApplyPatchFileChange::Update { unified_diff, .. }) => {
                assert!(
                    unified_diff.contains("-hello world"),
                    "diff missing removal"
                );
                assert!(
                    unified_diff.contains("+hello codex"),
                    "diff missing addition"
                );
            }
            other => panic!("expected Update change, got {other:?}"),
        }
    }
}
