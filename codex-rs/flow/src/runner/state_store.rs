use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use crate::runner::migrations;
use crate::runtime::state_store as runtime_state;

pub const WORKFLOW_STATE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceMode {
    Mock,
    Real,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
}

impl Default for TokenUsage {
    fn default() -> Self {
        Self {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            total_cost: 0.0,
        }
    }
}

impl TokenUsage {
    pub fn add_assign(&mut self, other: &TokenUsage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.total_tokens += other.total_tokens;
        self.total_cost += other.total_cost;
    }

    pub fn is_zero(&self) -> bool {
        self.prompt_tokens == 0
            && self.completion_tokens == 0
            && self.total_tokens == 0
            && self.total_cost == 0.0
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepState {
    pub index: usize,
    pub status: StepStatus,
    pub memory_path: String,
    pub debug_log: Option<String>,
    #[serde(default)]
    pub needs_real: bool,
    #[serde(default)]
    pub token_delta: Option<TokenUsage>,
}

impl StepState {
    pub fn ensure_needs_real(&mut self) {
        if self.debug_log.is_none() {
            self.needs_real = true;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRunState {
    pub schema_version: u32,
    pub workflow_name: String,
    pub run_id: String,
    pub resume_pointer: usize,
    #[serde(default)]
    pub steps: Vec<StepState>,
    #[serde(default)]
    pub token_usage: Option<TokenUsage>,
}

pub struct WorkflowStateStore {
    path: PathBuf,
    mode: PersistenceMode,
    state: WorkflowRunState,
}

impl WorkflowStateStore {
    pub fn load_or_init(workflow_name: &str, run_id: &str, mode: PersistenceMode) -> Result<Self> {
        let path = runtime_state::state_file_path(workflow_name, run_id)?;
        let (state, needs_persist) = if path.exists() {
            match read_state(&path) {
                Ok((mut loaded, migrated)) => {
                    let mut dirty = migrated;
                    if loaded.workflow_name.is_empty() {
                        loaded.workflow_name = workflow_name.to_string();
                        dirty = true;
                    }
                    if loaded.run_id.is_empty() {
                        loaded.run_id = run_id.to_string();
                        dirty = true;
                    }
                    (loaded, dirty)
                }
                Err(err) => {
                    let backup = backup_corrupt_file(&path)?;
                    if let Some(backup_path) = backup {
                        eprintln!(
                            "workflow state corrupted at {}; moved to {}: {err}; starting fresh",
                            path.display(),
                            backup_path.display()
                        );
                    } else {
                        eprintln!(
                            "workflow state corrupted at {}: {err}; starting fresh",
                            path.display()
                        );
                    }
                    (WorkflowRunState::new(workflow_name, run_id), false)
                }
            }
        } else {
            (WorkflowRunState::new(workflow_name, run_id), false)
        };

        let store = Self { path, mode, state };
        if needs_persist {
            store.persist()?;
        }
        Ok(store)
    }

    pub fn state(&self) -> &WorkflowRunState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut WorkflowRunState {
        &mut self.state
    }

    pub fn record_step(&mut self, mut step: StepState) -> Result<()> {
        step.needs_real = matches!(self.mode, PersistenceMode::Mock);
        step.ensure_needs_real();
        if matches!(step.status, StepStatus::Completed) {
            self.state.resume_pointer = step.index.saturating_add(1);
        }
        if let Some(existing) = self
            .state
            .steps
            .iter_mut()
            .find(|existing| existing.index == step.index)
        {
            *existing = step;
        } else {
            self.state.steps.push(step);
            self.state.steps.sort_by_key(|s| s.index);
        }
        self.persist()
    }

    pub fn record_interruption(&mut self, resume_pointer: usize) -> Result<()> {
        self.state.resume_pointer = resume_pointer;
        self.persist()
    }

    pub fn update_token_usage(&mut self, usage: TokenUsage) -> Result<()> {
        self.state.token_usage = Some(usage);
        self.persist()
    }

    pub fn append_token_usage(&mut self, delta: &TokenUsage) -> Result<()> {
        if delta.is_zero() {
            return Ok(());
        }
        let mut total = self.state.token_usage.clone().unwrap_or_default();
        total.add_assign(delta);
        self.update_token_usage(total)
    }

    pub fn mark_step_needs_real(&mut self, index: usize) -> Result<()> {
        let mut updated = false;
        if let Some(step) = self.state.steps.iter_mut().find(|step| step.index == index)
            && !step.needs_real
        {
            step.needs_real = true;
            updated = true;
        }
        if updated { self.persist() } else { Ok(()) }
    }

    fn persist(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| {
                format!("failed to create workflow state dir {}", dir.display())
            })?;
        }
        let json = serde_json::to_string_pretty(&self.state)? + "\n";
        let tmp_name = format!(
            "{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("state.resume.json")
        );
        let tmp_path = self.path.with_file_name(tmp_name);
        fs::write(&tmp_path, json.as_bytes()).with_context(|| {
            format!("failed to write workflow state tmp {}", tmp_path.display())
        })?;
        fs::rename(&tmp_path, &self.path).with_context(|| {
            format!(
                "failed to atomically persist workflow state {}",
                self.path.display()
            )
        })?;
        Ok(())
    }
}

impl WorkflowRunState {
    fn new(workflow_name: &str, run_id: &str) -> Self {
        Self {
            schema_version: WORKFLOW_STATE_SCHEMA_VERSION,
            workflow_name: workflow_name.to_string(),
            run_id: run_id.to_string(),
            resume_pointer: 0,
            steps: Vec::new(),
            token_usage: None,
        }
    }

    pub fn first_needs_real_before(&self, before: usize) -> Option<usize> {
        self.steps
            .iter()
            .filter(|step| step.index < before && step.needs_real)
            .map(|step| step.index)
            .min()
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        let (state, _) = read_state(path)?;
        Ok(state)
    }
}

impl WorkflowStateStore {
    pub fn flush(&self) -> Result<()> {
        self.persist()
    }
}

fn read_state(path: &Path) -> Result<(WorkflowRunState, bool)> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read workflow state {}", path.display()))?;
    let (value, migrated) = migrations::upgrade(&raw)
        .with_context(|| format!("failed to migrate workflow state {}", path.display()))?;
    let mut state: WorkflowRunState = serde_json::from_value(value)
        .with_context(|| format!("failed to parse workflow state {}", path.display()))?;
    state.schema_version = WORKFLOW_STATE_SCHEMA_VERSION;
    Ok((state, migrated))
}

fn backup_corrupt_file(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.resume.json");
    let backup_name = format!("{file_name}.corrupt-{timestamp}");
    let backup_path = path.with_file_name(backup_name);
    fs::rename(path, &backup_path)
        .with_context(|| format!("failed to move corrupt workflow state {}", path.display()))?;
    Ok(Some(backup_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use tempfile::tempdir;

    struct DirGuard {
        prev: PathBuf,
    }

    impl DirGuard {
        fn enter(path: &Path) -> Self {
            let prev = env::current_dir().expect("cwd");
            env::set_current_dir(path).expect("chdir");
            Self { prev }
        }
    }

    impl Drop for DirGuard {
        fn drop(&mut self) {
            env::set_current_dir(&self.prev).expect("restore cwd");
        }
    }

    #[test]
    fn persists_resume_pointer() {
        let tmp = tempdir().expect("tempdir");
        let runtime_root = tmp.path().join(".codex-flow").join("runtime");
        let _guard = DirGuard::enter(tmp.path());
        let mut store =
            WorkflowStateStore::load_or_init("workflow", "run-1", PersistenceMode::Mock)
                .expect("load store");

        let step = StepState {
            index: 0,
            status: StepStatus::Completed,
            memory_path: runtime_root
                .join("debug")
                .join("memory.json")
                .display()
                .to_string(),
            debug_log: Some(
                runtime_root
                    .join("logs")
                    .join("log.json")
                    .display()
                    .to_string(),
            ),
            needs_real: false,
            token_delta: None,
        };
        store.record_step(step).expect("record step");

        let state = store.state().clone();
        assert_eq!(state.resume_pointer, 1);
        assert_eq!(state.steps.len(), 1);
        assert!(state.steps[0].memory_path.ends_with("memory.json"));

        // Re-load from disk to verify persistence
        let reloaded = WorkflowStateStore::load_or_init("workflow", "run-1", PersistenceMode::Mock)
            .expect("reload store");
        assert_eq!(reloaded.state().resume_pointer, 1);
        assert!(
            reloaded.state().steps[0]
                .memory_path
                .ends_with("memory.json")
        );
    }

    #[test]
    fn applies_migrations() {
        let tmp = tempdir().expect("tempdir");
        let _guard = DirGuard::enter(tmp.path());

        let legacy_path =
            runtime_state::state_file_path("workflow", "legacy").expect("legacy path");
        let legacy_state = r#"{
            "schema_version": 1,
            "workflow_name": "workflow",
            "run_id": "legacy",
            "resume_pointer": 1,
            "steps": [
                {
                    "index": 0,
                    "status": "completed",
                    "memory_path": "memory",
                    "debug_log": "log",
                    "needs_real": false,
                    "token_delta": {
                        "prompt_tokens": 10,
                        "completion_tokens": 5,
                        "total_tokens": 15,
                        "total_cost": 0.25
                    }
                }
            ]
        }"#;
        fs::write(&legacy_path, legacy_state).expect("write legacy");

        let store = WorkflowStateStore::load_or_init("workflow", "legacy", PersistenceMode::Mock)
            .expect("load migrated");
        assert_eq!(store.state().schema_version, WORKFLOW_STATE_SCHEMA_VERSION);
        let usage = store
            .state()
            .token_usage
            .clone()
            .expect("token usage populated");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
        assert!((usage.total_cost - 0.25).abs() < f64::EPSILON);

        let rewritten = fs::read_to_string(&legacy_path).expect("read rewritten");
        assert!(rewritten.contains("\"schema_version\": 2"));

        let future_path =
            runtime_state::state_file_path("workflow", "future").expect("future path");
        fs::write(&future_path, "{\"schema_version\": 99}").expect("write future");
        let store = WorkflowStateStore::load_or_init("workflow", "future", PersistenceMode::Mock)
            .expect("load fresh state");
        assert_eq!(store.state().resume_pointer, 0);

        let dir = future_path.parent().expect("parent dir");
        let backups: Vec<_> = fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.contains("future.resume.json.corrupt")
                    .then_some(entry.path())
            })
            .collect();
        assert!(!backups.is_empty());
    }
}
