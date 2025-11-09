use chrono::DateTime;
use chrono::Duration as ChronoDuration;
use chrono::Utc;
use filetime::FileTime;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::Duration;
use std::time::SystemTime;
use tracing::warn;

use crate::token_data::PlanType;
use crate::token_data::TokenData;
use codex_keyring_store::DefaultKeyringStore;
use codex_keyring_store::KeyringStore;

/// Determine where Codex should store CLI auth credentials.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthCredentialsStoreMode {
    #[default]
    /// Persist credentials in CODEX_HOME/auth.json.
    File,
    /// Persist credentials in the keyring. Fail if unavailable.
    Keyring,
    /// Use keyring when available; otherwise, fall back to a file in CODEX_HOME.
    Auto,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct AccountState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_issue: Option<AccountIssue>,
}

impl AccountState {
    pub fn record_issue(&mut self, issue: AccountIssue) {
        self.last_issue = Some(issue);
    }

    pub fn current_issue(&self, now: DateTime<Utc>) -> Option<&AccountIssue> {
        match self.last_issue.as_ref() {
            Some(AccountIssue::UsageLimit(status)) if status.is_active(now) => {
                self.last_issue.as_ref()
            }
            Some(AccountIssue::UsageLimit(_)) => None,
            other => other,
        }
    }

    pub fn current_usage_limit(&self, now: DateTime<Utc>) -> Option<&UsageLimitStatus> {
        match self.current_issue(now) {
            Some(AccountIssue::UsageLimit(status)) => Some(status),
            _ => None,
        }
    }

    pub fn is_available(&self, now: DateTime<Utc>) -> bool {
        self.current_usage_limit(now).is_none()
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AccountIssue {
    UsageLimit(UsageLimitStatus),
    UnexpectedResponse(UnexpectedResponseStatus),
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct UsageLimitStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plan_type: Option<PlanType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
    pub recorded_at: DateTime<Utc>,
}

impl UsageLimitStatus {
    pub fn next_retry_at(&self) -> DateTime<Utc> {
        self.resets_at
            .unwrap_or_else(|| self.recorded_at + ChronoDuration::hours(5))
    }

    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.next_retry_at() > now
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct UnexpectedResponseStatus {
    pub recorded_at: DateTime<Utc>,
    pub status: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
}

/// Expected structure for $CODEX_HOME/auth.json.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AuthDotJson {
    #[serde(rename = "OPENAI_API_KEY")]
    pub openai_api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenData>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<DateTime<Utc>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_state: Option<AccountState>,
}

impl AuthDotJson {
    pub fn is_available(&self, now: DateTime<Utc>) -> bool {
        self.account_state
            .as_ref()
            .is_none_or(|state| state.is_available(now))
    }

    pub fn current_usage_limit(&self, now: DateTime<Utc>) -> Option<&UsageLimitStatus> {
        self.account_state
            .as_ref()
            .and_then(|state| state.current_usage_limit(now))
    }
}

enum CandidateOutcome {
    Available(AuthDotJson),
    UsageLimited {
        auth: AuthDotJson,
        limit: UsageLimitStatus,
    },
}

pub(super) fn get_auth_file(codex_home: &Path) -> PathBuf {
    codex_home.join("auth.json")
}

pub(super) fn delete_file_if_exists(codex_home: &Path) -> std::io::Result<bool> {
    let auth_file = get_auth_file(codex_home);
    match std::fs::remove_file(&auth_file) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

pub(super) trait AuthStorageBackend: Debug + Send + Sync {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>>;
    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()>;
    fn delete(&self) -> std::io::Result<bool>;
    fn invalidate_active_account(&self) -> std::io::Result<Option<PathBuf>> {
        Ok(None)
    }
}

#[derive(Clone, Debug)]
pub(super) struct FileAuthStorage {
    codex_home: PathBuf,
    active_auth_file: Arc<Mutex<Option<PathBuf>>>,
}

impl FileAuthStorage {
    pub(super) fn new(codex_home: PathBuf) -> Self {
        Self {
            codex_home,
            active_auth_file: Arc::new(Mutex::new(None)),
        }
    }

    fn lock_active_auth_file(&self) -> MutexGuard<'_, Option<PathBuf>> {
        match self.active_auth_file.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn clear_active_if_matches(&self, path: &Path) {
        let mut guard = self.lock_active_auth_file();
        if guard.as_ref().is_some_and(|current| current == path) {
            guard.take();
        }
    }

    fn set_active_path(&self, path: PathBuf) {
        let mut guard = self.lock_active_auth_file();
        *guard = Some(path);
    }

    fn accounts_dir(&self) -> PathBuf {
        self.codex_home.join("auth")
    }

    fn write_json(&self, path: &Path, auth: &AuthDotJson) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json_data = serde_json::to_string_pretty(auth)?;
        let mut options = OpenOptions::new();
        options.truncate(true).write(true).create(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(json_data.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    fn infer_account_file(&self, auth: &AuthDotJson) -> Option<PathBuf> {
        let email = auth.tokens.as_ref()?.id_token.email.as_ref()?;
        Some(self.accounts_dir().join(format!("{email}.json")))
    }

    fn write_fallback_auth(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        let fallback = get_auth_file(&self.codex_home);
        self.write_json(&fallback, auth)?;
        self.mark_file_used(&fallback);
        self.set_active_path(fallback);
        Ok(())
    }

    fn candidate_paths(&self) -> std::io::Result<Vec<PathBuf>> {
        let mut candidates: Vec<(u128, PathBuf)> = Vec::new();
        match std::fs::read_dir(self.accounts_dir()) {
            Ok(iter) => {
                for entry in iter {
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(err) => {
                            warn!("failed to read auth directory entry: {err}");
                            continue;
                        }
                    };

                    let path = entry.path();
                    let file_type = match entry.file_type() {
                        Ok(file_type) => file_type,
                        Err(err) => {
                            warn!(
                                "failed to inspect auth directory entry for {}: {err}",
                                path.display()
                            );
                            continue;
                        }
                    };
                    if !file_type.is_file() || !is_email_auth_candidate(&path) {
                        continue;
                    }

                    let metadata = match entry.metadata() {
                        Ok(metadata) => metadata,
                        Err(err) => {
                            warn!("failed to read metadata for {}: {err}", path.display());
                            continue;
                        }
                    };
                    let modified = modified_millis(&metadata);
                    candidates.push((modified, path));
                }
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }

        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(candidates.into_iter().map(|(_, path)| path).collect())
    }

    fn mark_file_used(&self, path: &Path) {
        if let Err(err) = filetime::set_file_mtime(path, FileTime::now()) {
            warn!(
                "failed to update auth file timestamp for {}: {err}",
                path.display()
            );
        }
    }

    fn evaluate_candidate(
        &self,
        path: &Path,
        now: DateTime<Utc>,
    ) -> std::io::Result<Option<CandidateOutcome>> {
        match self.try_read_auth_json(path) {
            Ok(auth) => {
                if let Some(limit) = auth.current_usage_limit(now).cloned() {
                    return Ok(Some(CandidateOutcome::UsageLimited { auth, limit }));
                }
                Ok(Some(CandidateOutcome::Available(auth)))
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                self.clear_active_if_matches(path);
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    /// Attempt to read and refresh the `auth.json` file in the given `CODEX_HOME` directory.
    /// Returns the full AuthDotJson structure after refreshing if necessary.
    pub(super) fn try_read_auth_json(&self, auth_file: &Path) -> std::io::Result<AuthDotJson> {
        let mut file = File::open(auth_file)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let auth_dot_json: AuthDotJson = serde_json::from_str(&contents)?;

        Ok(auth_dot_json)
    }
}

fn is_email_auth_candidate(path: &Path) -> bool {
    if path.file_name() == Some(OsStr::new("auth.json")) {
        return false;
    }
    if path.extension().and_then(OsStr::to_str) != Some("json") {
        return false;
    }
    path.file_stem()
        .and_then(OsStr::to_str)
        .is_some_and(|stem| stem.contains('@'))
}

fn modified_millis(metadata: &std::fs::Metadata) -> u128 {
    match metadata.modified() {
        Ok(time) => match time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => duration.as_millis(),
            Err(_) => Duration::ZERO.as_millis(),
        },
        Err(_) => Duration::ZERO.as_millis(),
    }
}

impl AuthStorageBackend for FileAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        let now = Utc::now();

        let mut ordered_paths: Vec<PathBuf> = Vec::new();
        if let Some(active) = self.lock_active_auth_file().clone() {
            if is_email_auth_candidate(&active) {
                ordered_paths.push(active);
            }
        }
        for path in self.candidate_paths()? {
            if !ordered_paths.iter().any(|existing| existing == &path) {
                ordered_paths.push(path);
            }
        }

        let mut blocked: Option<(DateTime<Utc>, PathBuf, AuthDotJson)> = None;

        for path in ordered_paths {
            let outcome = match self.evaluate_candidate(&path, now)? {
                Some(outcome) => outcome,
                None => continue,
            };

            match outcome {
                CandidateOutcome::Available(auth) => {
                    self.set_active_path(path.clone());
                    self.mark_file_used(&path);
                    return Ok(Some(auth));
                }
                CandidateOutcome::UsageLimited { auth, limit } => {
                    self.clear_active_if_matches(&path);
                    let retry_at = limit.next_retry_at();
                    if blocked
                        .as_ref()
                        .is_none_or(|(best_retry, _, _)| retry_at < *best_retry)
                    {
                        blocked = Some((retry_at, path.clone(), auth));
                    }
                }
            }
        }

        if let Some((_, path, auth)) = blocked {
            self.set_active_path(path);
            return Ok(Some(auth));
        }

        let fallback = get_auth_file(&self.codex_home);
        match std::fs::metadata(&fallback) {
            Ok(metadata) if metadata.is_file() => match self.try_read_auth_json(&fallback) {
                Ok(auth) => {
                    self.set_active_path(fallback.clone());
                    self.mark_file_used(&fallback);
                    Ok(Some(auth))
                }
                Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
                Err(err) => Err(err),
            },
            Ok(_) => Ok(None),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn save(&self, auth_dot_json: &AuthDotJson) -> std::io::Result<()> {
        let current_active = {
            let guard = self.lock_active_auth_file();
            guard.clone()
        };
        let active_is_fallback = current_active
            .as_deref()
            .is_some_and(|path| path.file_name() == Some(OsStr::new("auth.json")));

        if !active_is_fallback {
            if let Some(path) = self.infer_account_file(auth_dot_json) {
                self.write_json(&path, auth_dot_json)?;
                self.mark_file_used(&path);
                self.set_active_path(path);
                return Ok(());
            }
        }

        if let Some(path) = current_active {
            if active_is_fallback {
                return self.write_fallback_auth(auth_dot_json);
            }

            self.write_json(&path, auth_dot_json)?;
            self.mark_file_used(&path);
            self.set_active_path(path);
            return Ok(());
        }

        self.write_fallback_auth(auth_dot_json)
    }

    fn delete(&self) -> std::io::Result<bool> {
        let removed_active = {
            let mut guard = self.lock_active_auth_file();
            let active = guard.take();
            match active {
                Some(path) => match std::fs::remove_file(&path) {
                    Ok(()) => true,
                    Err(err) if err.kind() == ErrorKind::NotFound => false,
                    Err(err) => return Err(err),
                },
                None => false,
            }
        };

        let removed_fallback = delete_file_if_exists(&self.codex_home)?;
        Ok(removed_active || removed_fallback)
    }

    fn invalidate_active_account(&self) -> std::io::Result<Option<PathBuf>> {
        let active = {
            let guard = self.lock_active_auth_file();
            guard.clone()
        };

        let Some(path) = active else {
            return Ok(None);
        };

        if path.file_name() == Some(OsStr::new("auth.json")) {
            return Ok(None);
        }

        let original_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| std::io::Error::other("active account file missing file name"))?;

        let parent = match path.parent() {
            Some(parent) => parent.to_path_buf(),
            None => return Ok(None),
        };

        let mut invalid_path = parent.join(format!("invalid-{original_name}"));
        if invalid_path.exists() {
            let timestamp = Utc::now().format("%Y%m%d%H%M%S");
            invalid_path = parent.join(format!("invalid-{timestamp}-{original_name}"));
        }

        match std::fs::rename(&path, &invalid_path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {
                self.clear_active_if_matches(&path);
                return Ok(None);
            }
            Err(err) => return Err(err),
        }

        self.clear_active_if_matches(&path);
        if let Err(err) = delete_file_if_exists(&self.codex_home) {
            warn!(
                "failed to remove stale auth.json after invalidating account {}: {err}",
                path.display()
            );
        }

        Ok(Some(invalid_path))
    }
}

const KEYRING_SERVICE: &str = "Codex Auth";

// turns codex_home path into a stable, short key string
fn compute_store_key(codex_home: &Path) -> std::io::Result<String> {
    let canonical = codex_home
        .canonicalize()
        .unwrap_or_else(|_| codex_home.to_path_buf());
    let path_str = canonical.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let digest = hasher.finalize();
    let hex = format!("{digest:x}");
    let truncated = hex.get(..16).unwrap_or(&hex);
    Ok(format!("cli|{truncated}"))
}

#[derive(Clone, Debug)]
struct KeyringAuthStorage {
    codex_home: PathBuf,
    keyring_store: Arc<dyn KeyringStore>,
}

impl KeyringAuthStorage {
    fn new(codex_home: PathBuf, keyring_store: Arc<dyn KeyringStore>) -> Self {
        Self {
            codex_home,
            keyring_store,
        }
    }

    fn load_from_keyring(&self, key: &str) -> std::io::Result<Option<AuthDotJson>> {
        match self.keyring_store.load(KEYRING_SERVICE, key) {
            Ok(Some(serialized)) => serde_json::from_str(&serialized).map(Some).map_err(|err| {
                std::io::Error::other(format!(
                    "failed to deserialize CLI auth from keyring: {err}"
                ))
            }),
            Ok(None) => Ok(None),
            Err(error) => Err(std::io::Error::other(format!(
                "failed to load CLI auth from keyring: {}",
                error.message()
            ))),
        }
    }

    fn save_to_keyring(&self, key: &str, value: &str) -> std::io::Result<()> {
        match self.keyring_store.save(KEYRING_SERVICE, key, value) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = format!(
                    "failed to write OAuth tokens to keyring: {}",
                    error.message()
                );
                warn!("{message}");
                Err(std::io::Error::other(message))
            }
        }
    }
}

impl AuthStorageBackend for KeyringAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        let key = compute_store_key(&self.codex_home)?;
        self.load_from_keyring(&key)
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        let key = compute_store_key(&self.codex_home)?;
        // Simpler error mapping per style: prefer method reference over closure
        let serialized = serde_json::to_string(auth).map_err(std::io::Error::other)?;
        self.save_to_keyring(&key, &serialized)?;
        if let Err(err) = delete_file_if_exists(&self.codex_home) {
            warn!("failed to remove CLI auth fallback file: {err}");
        }
        Ok(())
    }

    fn delete(&self) -> std::io::Result<bool> {
        let key = compute_store_key(&self.codex_home)?;
        let keyring_removed = self
            .keyring_store
            .delete(KEYRING_SERVICE, &key)
            .map_err(|err| {
                std::io::Error::other(format!("failed to delete auth from keyring: {err}"))
            })?;
        let file_removed = delete_file_if_exists(&self.codex_home)?;
        Ok(keyring_removed || file_removed)
    }
}

#[derive(Clone, Debug)]
struct AutoAuthStorage {
    keyring_storage: Arc<KeyringAuthStorage>,
    file_storage: Arc<FileAuthStorage>,
}

impl AutoAuthStorage {
    fn new(codex_home: PathBuf, keyring_store: Arc<dyn KeyringStore>) -> Self {
        Self {
            keyring_storage: Arc::new(KeyringAuthStorage::new(codex_home.clone(), keyring_store)),
            file_storage: Arc::new(FileAuthStorage::new(codex_home)),
        }
    }
}

impl AuthStorageBackend for AutoAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        match self.keyring_storage.load() {
            Ok(Some(auth)) => Ok(Some(auth)),
            Ok(None) => self.file_storage.load(),
            Err(err) => {
                warn!("failed to load CLI auth from keyring, falling back to file storage: {err}");
                self.file_storage.load()
            }
        }
    }

    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()> {
        match self.keyring_storage.save(auth) {
            Ok(()) => Ok(()),
            Err(err) => {
                warn!("failed to save auth to keyring, falling back to file storage: {err}");
                self.file_storage.write_fallback_auth(auth)
            }
        }
    }

    fn delete(&self) -> std::io::Result<bool> {
        // Keyring storage will delete from disk as well
        self.keyring_storage.delete()
    }
}

pub(super) fn create_auth_storage(
    codex_home: PathBuf,
    mode: AuthCredentialsStoreMode,
) -> Arc<dyn AuthStorageBackend> {
    let keyring_store: Arc<dyn KeyringStore> = Arc::new(DefaultKeyringStore);
    create_auth_storage_with_keyring_store(codex_home, mode, keyring_store)
}

fn create_auth_storage_with_keyring_store(
    codex_home: PathBuf,
    mode: AuthCredentialsStoreMode,
    keyring_store: Arc<dyn KeyringStore>,
) -> Arc<dyn AuthStorageBackend> {
    match mode {
        AuthCredentialsStoreMode::File => Arc::new(FileAuthStorage::new(codex_home)),
        AuthCredentialsStoreMode::Keyring => {
            Arc::new(KeyringAuthStorage::new(codex_home, keyring_store))
        }
        AuthCredentialsStoreMode::Auto => Arc::new(AutoAuthStorage::new(codex_home, keyring_store)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_data::IdTokenInfo;
    use anyhow::Context;
    use base64::Engine;
    use filetime::FileTime;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use tempfile::tempdir;

    use codex_keyring_store::tests::MockKeyringStore;
    use keyring::Error as KeyringError;

    #[tokio::test]
    async fn file_storage_load_returns_auth_dot_json() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
        let auth_dot_json = AuthDotJson {
            openai_api_key: Some("test-key".to_string()),
            tokens: None,
            last_refresh: Some(Utc::now()),
            account_state: None,
        };

        storage
            .save(&auth_dot_json)
            .context("failed to save auth file")?;

        let loaded = storage.load().context("failed to load auth file")?;
        assert_eq!(Some(auth_dot_json), loaded);
        Ok(())
    }

    #[tokio::test]
    async fn file_storage_save_persists_auth_dot_json() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
        let auth_dot_json = AuthDotJson {
            openai_api_key: Some("test-key".to_string()),
            tokens: None,
            last_refresh: Some(Utc::now()),
            account_state: None,
        };

        let file = get_auth_file(codex_home.path());
        storage
            .save(&auth_dot_json)
            .context("failed to save auth file")?;

        let same_auth_dot_json = storage
            .try_read_auth_json(&file)
            .context("failed to read auth file after save")?;
        assert_eq!(auth_dot_json, same_auth_dot_json);
        Ok(())
    }

    #[test]
    fn file_storage_invalidate_active_account_marks_file_invalid() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
        let auth_dot_json = auth_with_prefix("alice");

        <FileAuthStorage as AuthStorageBackend>::save(&storage, &auth_dot_json)?;

        let auth_dir = codex_home.path().join("auth");
        let original_path = auth_dir.join("alice@example.com.json");
        assert!(
            original_path.exists(),
            "expected active account file to exist before invalidation"
        );
        let current_path = get_auth_file(codex_home.path());
        assert!(
            !current_path.exists(),
            "fallback auth.json should not exist before invalidation"
        );

        let invalid_path =
            <FileAuthStorage as AuthStorageBackend>::invalidate_active_account(&storage)?
                .expect("expected account file to be invalidated");

        assert!(
            !original_path.exists(),
            "original account file should be renamed after invalidation"
        );
        assert!(
            invalid_path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with("invalid-")),
            "renamed account file should be prefixed with invalid-"
        );
        assert!(
            !current_path.exists(),
            "fallback auth.json should remain absent after invalidation"
        );
        Ok(())
    }

    #[test]
    fn file_storage_delete_removes_auth_file() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let auth_dot_json = AuthDotJson {
            openai_api_key: Some("sk-test-key".to_string()),
            tokens: None,
            last_refresh: None,
            account_state: None,
        };
        let storage = create_auth_storage(dir.path().to_path_buf(), AuthCredentialsStoreMode::File);
        storage.save(&auth_dot_json)?;
        assert!(dir.path().join("auth.json").exists());
        let storage = FileAuthStorage::new(dir.path().to_path_buf());
        let removed = storage.delete()?;
        assert!(removed);
        assert!(!dir.path().join("auth.json").exists());
        Ok(())
    }

    #[test]
    fn file_storage_load_rotates_between_oldest_email_files() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let alice_auth = auth_with_prefix("alice");
        let bob_auth = auth_with_prefix("bob");
        let auth_dir = codex_home.path().join("auth");
        std::fs::create_dir_all(&auth_dir)?;
        let alice_path = auth_dir.join("alice@example.com.json");
        let bob_path = auth_dir.join("bob@example.com.json");
        std::fs::write(
            &alice_path,
            serde_json::to_string_pretty(&alice_auth).context("serialize alice auth")?,
        )?;
        std::fs::write(
            &bob_path,
            serde_json::to_string_pretty(&bob_auth).context("serialize bob auth")?,
        )?;
        filetime::set_file_mtime(&alice_path, FileTime::from_unix_time(1, 0))?;
        filetime::set_file_mtime(&bob_path, FileTime::from_unix_time(10, 0))?;

        let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
        let first = <FileAuthStorage as AuthStorageBackend>::load(&storage)?
            .expect("should load alice auth first");
        assert_eq!(first, alice_auth);

        let alice_mtime = std::fs::metadata(&alice_path)?.modified()?;
        let bob_mtime_before = std::fs::metadata(&bob_path)?.modified()?;
        assert!(
            alice_mtime > bob_mtime_before,
            "alice mtime should update after use"
        );

        let storage_second = FileAuthStorage::new(codex_home.path().to_path_buf());
        let second = <FileAuthStorage as AuthStorageBackend>::load(&storage_second)?
            .expect("should load bob auth second");
        assert_eq!(second, bob_auth);
        let current_path = codex_home.path().join("auth.json");
        assert!(
            !current_path.exists(),
            "fallback auth.json should remain untouched when rotating accounts"
        );
        Ok(())
    }

    #[test]
    fn file_storage_load_skips_usage_limited_accounts() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mut limited_auth = auth_with_prefix("alice");
        let mut limited_state = AccountState::default();
        limited_state.record_issue(AccountIssue::UsageLimit(UsageLimitStatus {
            plan_type: None,
            resets_at: Some(Utc::now() + chrono::Duration::hours(1)),
            recorded_at: Utc::now(),
        }));
        limited_auth.account_state = Some(limited_state);
        let available_auth = auth_with_prefix("bob");

        let auth_dir = codex_home.path().join("auth");
        std::fs::create_dir_all(&auth_dir)?;
        let limited_path = auth_dir.join("alice@example.com.json");
        let available_path = auth_dir.join("bob@example.com.json");
        std::fs::write(
            &limited_path,
            serde_json::to_string_pretty(&limited_auth).context("serialize limited auth")?,
        )?;
        std::fs::write(
            &available_path,
            serde_json::to_string_pretty(&available_auth).context("serialize available auth")?,
        )?;
        filetime::set_file_mtime(&limited_path, FileTime::from_unix_time(1, 0))?;
        filetime::set_file_mtime(&available_path, FileTime::from_unix_time(5, 0))?;

        let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
        let loaded = storage
            .load()
            .context("load should skip limited account")?
            .expect("available auth should load");
        assert_eq!(loaded, available_auth);
        let current_path = codex_home.path().join("auth.json");
        assert!(
            !current_path.exists(),
            "fallback auth.json should remain untouched when limited accounts are skipped"
        );
        Ok(())
    }

    #[test]
    fn file_storage_load_returns_limited_when_only_blocked_accounts() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mut limited_auth = auth_with_prefix("alice");
        let mut limited_state = AccountState::default();
        limited_state.record_issue(AccountIssue::UsageLimit(UsageLimitStatus {
            plan_type: None,
            resets_at: Some(Utc::now() + chrono::Duration::hours(1)),
            recorded_at: Utc::now(),
        }));
        limited_auth.account_state = Some(limited_state);
        let auth_dir = codex_home.path().join("auth");
        std::fs::create_dir_all(&auth_dir)?;
        let limited_path = auth_dir.join("alice@example.com.json");
        std::fs::write(
            &limited_path,
            serde_json::to_string_pretty(&limited_auth).context("serialize limited auth")?,
        )?;

        let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
        let loaded = storage
            .load()
            .context("load should fall back to limited auth when all blocked")?
            .expect("limited auth should still be returned");
        assert_eq!(loaded, limited_auth);
        let current_path = codex_home.path().join("auth.json");
        assert!(
            !current_path.exists(),
            "fallback auth.json should not mirror limited account data"
        );
        Ok(())
    }

    #[test]
    fn file_storage_save_writes_to_active_email_file() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let alice_auth = auth_with_prefix("alice");
        let auth_dir = codex_home.path().join("auth");
        std::fs::create_dir_all(&auth_dir)?;
        let alice_path = auth_dir.join("alice@example.com.json");
        std::fs::write(
            &alice_path,
            serde_json::to_string_pretty(&alice_auth).context("serialize alice auth")?,
        )?;

        let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
        let loaded = <FileAuthStorage as AuthStorageBackend>::load(&storage)?
            .expect("should load alice auth");
        assert_eq!(loaded, alice_auth);

        let mut updated = loaded;
        updated.openai_api_key = Some("alice-updated".to_string());
        <FileAuthStorage as AuthStorageBackend>::save(&storage, &updated)?;

        let saved = storage
            .try_read_auth_json(&alice_path)
            .context("read updated alice auth")?;
        assert_eq!(saved, updated);
        let current_path = codex_home.path().join("auth.json");
        assert!(
            !current_path.exists(),
            "saving multi-account credentials should not touch fallback auth.json"
        );
        Ok(())
    }

    #[test]
    fn file_storage_save_prefers_inferred_email_file() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let alice_auth = auth_with_prefix("alice");
        let bob_auth = auth_with_prefix("bob");
        let auth_dir = codex_home.path().join("auth");
        std::fs::create_dir_all(&auth_dir)?;
        let alice_path = auth_dir.join("alice@example.com.json");
        let bob_path = auth_dir.join("bob@example.com.json");
        std::fs::write(
            &alice_path,
            serde_json::to_string_pretty(&alice_auth).context("serialize alice auth")?,
        )?;

        let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
        let loaded = <FileAuthStorage as AuthStorageBackend>::load(&storage)?
            .expect("should load alice auth first");
        assert_eq!(loaded, alice_auth);

        <FileAuthStorage as AuthStorageBackend>::save(&storage, &bob_auth)?;

        let saved_bob = storage
            .try_read_auth_json(&bob_path)
            .context("read bob auth after save")?;
        assert_eq!(saved_bob, bob_auth);
        let saved_alice = storage
            .try_read_auth_json(&alice_path)
            .context("read alice auth remains unchanged")?;
        assert_eq!(saved_alice, alice_auth);
        let fallback = codex_home.path().join("auth.json");
        assert!(
            !fallback.exists(),
            "saving inferred account should not touch fallback auth.json"
        );
        Ok(())
    }

    #[test]
    fn file_storage_fallback_save_does_not_recreate_removed_account() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let alice_auth = auth_with_prefix("alice");
        let fallback_auth = auth_with_prefix("charlie");
        let auth_dir = codex_home.path().join("auth");
        std::fs::create_dir_all(&auth_dir)?;
        let alice_path = auth_dir.join("alice@example.com.json");
        std::fs::write(
            &alice_path,
            serde_json::to_string_pretty(&alice_auth).context("serialize alice auth")?,
        )?;

        let storage = FileAuthStorage::new(codex_home.path().to_path_buf());
        let loaded = <FileAuthStorage as AuthStorageBackend>::load(&storage)?
            .expect("should load alice auth first");
        assert_eq!(loaded, alice_auth);

        std::fs::remove_file(&alice_path)?;
        std::fs::remove_dir_all(&auth_dir)?;

        let fallback_path = codex_home.path().join("auth.json");
        std::fs::write(
            &fallback_path,
            serde_json::to_string_pretty(&fallback_auth).context("serialize fallback auth")?,
        )?;

        let fallback_loaded = <FileAuthStorage as AuthStorageBackend>::load(&storage)?
            .expect("should load fallback auth");
        assert_eq!(fallback_loaded, fallback_auth);

        let mut updated = fallback_loaded;
        updated.openai_api_key = Some("charlie-updated".to_string());
        <FileAuthStorage as AuthStorageBackend>::save(&storage, &updated)?;

        let hydrated = storage
            .try_read_auth_json(&fallback_path)
            .context("read fallback auth after save")?;
        assert_eq!(hydrated, updated);
        assert!(
            !alice_path.exists(),
            "saving fallback credentials should not recreate removed account file"
        );
        Ok(())
    }

    fn seed_keyring_and_fallback_auth_file_for_delete<F>(
        mock_keyring: &MockKeyringStore,
        codex_home: &Path,
        compute_key: F,
    ) -> anyhow::Result<(String, PathBuf)>
    where
        F: FnOnce() -> std::io::Result<String>,
    {
        let key = compute_key()?;
        mock_keyring.save(KEYRING_SERVICE, &key, "{}")?;
        let auth_file = get_auth_file(codex_home);
        std::fs::write(&auth_file, "stale")?;
        Ok((key, auth_file))
    }

    fn seed_keyring_with_auth<F>(
        mock_keyring: &MockKeyringStore,
        compute_key: F,
        auth: &AuthDotJson,
    ) -> anyhow::Result<()>
    where
        F: FnOnce() -> std::io::Result<String>,
    {
        let key = compute_key()?;
        let serialized = serde_json::to_string(auth)?;
        mock_keyring.save(KEYRING_SERVICE, &key, &serialized)?;
        Ok(())
    }

    fn assert_keyring_saved_auth_and_removed_fallback(
        mock_keyring: &MockKeyringStore,
        key: &str,
        codex_home: &Path,
        expected: &AuthDotJson,
    ) {
        let saved_value = mock_keyring
            .saved_value(key)
            .expect("keyring entry should exist");
        let expected_serialized = serde_json::to_string(expected).expect("serialize expected auth");
        assert_eq!(saved_value, expected_serialized);
        let auth_file = get_auth_file(codex_home);
        assert!(
            !auth_file.exists(),
            "fallback auth.json should be removed after keyring save"
        );
    }

    fn id_token_with_prefix(prefix: &str) -> IdTokenInfo {
        #[derive(Serialize)]
        struct Header {
            alg: &'static str,
            typ: &'static str,
        }

        let header = Header {
            alg: "none",
            typ: "JWT",
        };
        let payload = json!({
            "email": format!("{prefix}@example.com"),
            "https://api.openai.com/auth": {
                "chatgpt_account_id": format!("{prefix}-account"),
            },
        });
        let encode = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let header_b64 = encode(&serde_json::to_vec(&header).expect("serialize header"));
        let payload_b64 = encode(&serde_json::to_vec(&payload).expect("serialize payload"));
        let signature_b64 = encode(b"sig");
        let fake_jwt = format!("{header_b64}.{payload_b64}.{signature_b64}");

        crate::token_data::parse_id_token(&fake_jwt).expect("fake JWT should parse")
    }

    fn auth_with_prefix(prefix: &str) -> AuthDotJson {
        AuthDotJson {
            openai_api_key: Some(format!("{prefix}-api-key")),
            tokens: Some(TokenData {
                id_token: id_token_with_prefix(prefix),
                access_token: format!("{prefix}-access"),
                refresh_token: format!("{prefix}-refresh"),
                account_id: Some(format!("{prefix}-account-id")),
            }),
            last_refresh: None,
            account_state: None,
        }
    }

    #[test]
    fn keyring_auth_storage_load_returns_deserialized_auth() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mock_keyring = MockKeyringStore::default();
        let storage = KeyringAuthStorage::new(
            codex_home.path().to_path_buf(),
            Arc::new(mock_keyring.clone()),
        );
        let expected = AuthDotJson {
            openai_api_key: Some("sk-test".to_string()),
            tokens: None,
            last_refresh: None,
            account_state: None,
        };
        seed_keyring_with_auth(
            &mock_keyring,
            || compute_store_key(codex_home.path()),
            &expected,
        )?;

        let loaded = storage.load()?;
        assert_eq!(Some(expected), loaded);
        Ok(())
    }

    #[test]
    fn keyring_auth_storage_compute_store_key_for_home_directory() -> anyhow::Result<()> {
        let codex_home = PathBuf::from("~/.codex");

        let key = compute_store_key(codex_home.as_path())?;

        assert_eq!(key, "cli|940db7b1d0e4eb40");
        Ok(())
    }

    #[test]
    fn keyring_auth_storage_save_persists_and_removes_fallback_file() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mock_keyring = MockKeyringStore::default();
        let storage = KeyringAuthStorage::new(
            codex_home.path().to_path_buf(),
            Arc::new(mock_keyring.clone()),
        );
        let auth_file = get_auth_file(codex_home.path());
        std::fs::write(&auth_file, "stale")?;
        let auth = AuthDotJson {
            openai_api_key: None,
            tokens: Some(TokenData {
                id_token: Default::default(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                account_id: Some("account".to_string()),
            }),
            last_refresh: Some(Utc::now()),
            account_state: None,
        };

        storage.save(&auth)?;

        let key = compute_store_key(codex_home.path())?;
        assert_keyring_saved_auth_and_removed_fallback(
            &mock_keyring,
            &key,
            codex_home.path(),
            &auth,
        );
        Ok(())
    }

    #[test]
    fn keyring_auth_storage_delete_removes_keyring_and_file() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mock_keyring = MockKeyringStore::default();
        let storage = KeyringAuthStorage::new(
            codex_home.path().to_path_buf(),
            Arc::new(mock_keyring.clone()),
        );
        let (key, auth_file) = seed_keyring_and_fallback_auth_file_for_delete(
            &mock_keyring,
            codex_home.path(),
            || compute_store_key(codex_home.path()),
        )?;

        let removed = storage.delete()?;

        assert!(removed, "delete should report removal");
        assert!(
            !mock_keyring.contains(&key),
            "keyring entry should be removed"
        );
        assert!(
            !auth_file.exists(),
            "fallback auth.json should be removed after keyring delete"
        );
        Ok(())
    }

    #[test]
    fn auto_auth_storage_load_prefers_keyring_value() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mock_keyring = MockKeyringStore::default();
        let storage = AutoAuthStorage::new(
            codex_home.path().to_path_buf(),
            Arc::new(mock_keyring.clone()),
        );
        let keyring_auth = auth_with_prefix("keyring");
        seed_keyring_with_auth(
            &mock_keyring,
            || compute_store_key(codex_home.path()),
            &keyring_auth,
        )?;

        let file_auth = auth_with_prefix("file");
        storage.file_storage.save(&file_auth)?;

        let loaded = storage.load()?;
        assert_eq!(loaded, Some(keyring_auth));
        Ok(())
    }

    #[test]
    fn auto_auth_storage_load_uses_file_when_keyring_empty() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mock_keyring = MockKeyringStore::default();
        let storage = AutoAuthStorage::new(codex_home.path().to_path_buf(), Arc::new(mock_keyring));

        let expected = auth_with_prefix("file-only");
        storage.file_storage.save(&expected)?;

        let loaded = storage.load()?;
        assert_eq!(loaded, Some(expected));
        Ok(())
    }

    #[test]
    fn auto_auth_storage_load_falls_back_when_keyring_errors() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mock_keyring = MockKeyringStore::default();
        let storage = AutoAuthStorage::new(
            codex_home.path().to_path_buf(),
            Arc::new(mock_keyring.clone()),
        );
        let key = compute_store_key(codex_home.path())?;
        mock_keyring.set_error(&key, KeyringError::Invalid("error".into(), "load".into()));

        let expected = auth_with_prefix("fallback");
        storage.file_storage.save(&expected)?;

        let loaded = storage.load()?;
        assert_eq!(loaded, Some(expected));
        Ok(())
    }

    #[test]
    fn auto_auth_storage_save_prefers_keyring() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mock_keyring = MockKeyringStore::default();
        let storage = AutoAuthStorage::new(
            codex_home.path().to_path_buf(),
            Arc::new(mock_keyring.clone()),
        );
        let key = compute_store_key(codex_home.path())?;

        let stale = auth_with_prefix("stale");
        storage.file_storage.save(&stale)?;

        let expected = auth_with_prefix("to-save");
        storage.save(&expected)?;

        assert_keyring_saved_auth_and_removed_fallback(
            &mock_keyring,
            &key,
            codex_home.path(),
            &expected,
        );
        Ok(())
    }

    #[test]
    fn auto_auth_storage_save_falls_back_when_keyring_errors() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mock_keyring = MockKeyringStore::default();
        let storage = AutoAuthStorage::new(
            codex_home.path().to_path_buf(),
            Arc::new(mock_keyring.clone()),
        );
        let key = compute_store_key(codex_home.path())?;
        mock_keyring.set_error(&key, KeyringError::Invalid("error".into(), "save".into()));

        let auth = auth_with_prefix("fallback");
        storage.save(&auth)?;

        let auth_file = get_auth_file(codex_home.path());
        assert!(
            auth_file.exists(),
            "fallback auth.json should be created when keyring save fails"
        );
        let saved = storage
            .file_storage
            .load()?
            .context("fallback auth should exist")?;
        assert_eq!(saved, auth);
        assert!(
            mock_keyring.saved_value(&key).is_none(),
            "keyring should not contain value when save fails"
        );
        Ok(())
    }

    #[test]
    fn auto_auth_storage_delete_removes_keyring_and_file() -> anyhow::Result<()> {
        let codex_home = tempdir()?;
        let mock_keyring = MockKeyringStore::default();
        let storage = AutoAuthStorage::new(
            codex_home.path().to_path_buf(),
            Arc::new(mock_keyring.clone()),
        );
        let (key, auth_file) = seed_keyring_and_fallback_auth_file_for_delete(
            &mock_keyring,
            codex_home.path(),
            || compute_store_key(codex_home.path()),
        )?;

        let removed = storage.delete()?;

        assert!(removed, "delete should report removal");
        assert!(
            !mock_keyring.contains(&key),
            "keyring entry should be removed"
        );
        assert!(
            !auth_file.exists(),
            "fallback auth.json should be removed after delete"
        );
        Ok(())
    }
}
