use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::UsageRecord;
use crate::clipboard_history::ClipboardHistory;
use crate::quick_link::QuickLink;
use crate::shortcut::ShortcutBinding;

pub const STORE_SCHEMA_VERSION: u32 = 2;
const STORE_FILE_NAME: &str = "state.json";
static UNIQUE_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchEngine {
    #[default]
    #[serde(rename = "google")]
    Google,
    #[serde(rename = "duckduckgo")]
    DuckDuckGo,
}

impl fmt::Display for SearchEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Google => "Google",
            Self::DuckDuckGo => "DuckDuckGo",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub shortcut: ShortcutBinding,
    /// Input-source identifier selected when the launcher search field gains focus.
    #[serde(default)]
    pub preferred_input_source: Option<String>,
    /// Clipboard contents are recorded only after the user explicitly opts in.
    #[serde(default)]
    pub clipboard_history_enabled: bool,
    /// Update checks are enabled by default, including for existing settings files.
    #[serde(default = "default_update_checks_enabled")]
    pub update_checks_enabled: bool,
    /// Search engine used by the dynamic web-search fallback.
    #[serde(default)]
    pub search_engine: SearchEngine,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            shortcut: ShortcutBinding::default(),
            preferred_input_source: None,
            clipboard_history_enabled: false,
            update_checks_enabled: default_update_checks_enabled(),
            search_engine: SearchEngine::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreData {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub usage: Vec<UsageRecord>,
    #[serde(default)]
    pub settings: AppSettings,
    #[serde(default)]
    pub quick_links: Vec<QuickLink>,
    #[serde(default = "default_next_quick_link_id")]
    pub next_quick_link_id: u64,
    #[serde(default)]
    pub clipboard_history: ClipboardHistory,
    #[serde(default = "default_next_clipboard_entry_id")]
    pub next_clipboard_entry_id: u64,
    /// Whether the required anonymous-usage disclosure was acknowledged.
    #[serde(default)]
    pub telemetry_disclosure_acknowledged: bool,
    /// Opaque random ID used only for anonymous usage statistics.
    #[serde(default)]
    pub telemetry_installation_id: Option<String>,
    /// UTC day when the last daily activity event was scheduled.
    #[serde(default)]
    pub telemetry_last_active_day: Option<u64>,
}

impl Default for StoreData {
    fn default() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            usage: Vec::new(),
            settings: AppSettings::default(),
            quick_links: Vec::new(),
            next_quick_link_id: default_next_quick_link_id(),
            clipboard_history: ClipboardHistory::default(),
            next_clipboard_entry_id: default_next_clipboard_entry_id(),
            telemetry_disclosure_acknowledged: false,
            telemetry_installation_id: None,
            telemetry_last_active_day: None,
        }
    }
}

impl StoreData {
    pub fn from_usage(usage: impl IntoIterator<Item = UsageRecord>) -> Self {
        let mut data = Self::default();
        for record in usage {
            data.upsert_usage(record);
        }
        data
    }

    pub fn usage_for(&self, item_id: &str) -> Option<&UsageRecord> {
        self.usage.iter().find(|record| record.item_id == item_id)
    }

    pub fn usage_for_mut(&mut self, item_id: &str) -> &mut UsageRecord {
        if let Some(index) = self
            .usage
            .iter()
            .position(|record| record.item_id == item_id)
        {
            return &mut self.usage[index];
        }

        self.usage.push(UsageRecord::new(item_id));
        let inserted_index = self.usage.len() - 1;
        &mut self.usage[inserted_index]
    }

    pub fn upsert_usage(&mut self, record: UsageRecord) {
        if let Some(existing) = self
            .usage
            .iter_mut()
            .find(|existing| existing.item_id == record.item_id)
        {
            *existing = record;
        } else {
            self.usage.push(record);
        }
    }

    pub fn allocate_quick_link_id(&mut self) -> u64 {
        let id = self.next_quick_link_id.max(1);
        self.next_quick_link_id = id.saturating_add(1);
        id
    }

    pub fn allocate_clipboard_entry_id(&mut self) -> u64 {
        let id = self.next_clipboard_entry_id.max(1);
        self.next_clipboard_entry_id = id.saturating_add(1);
        id
    }

    fn normalize(&mut self) {
        let records = std::mem::take(&mut self.usage);
        for record in records {
            self.upsert_usage(record);
        }

        let mut seen_quick_links = std::collections::HashSet::new();
        self.quick_links
            .retain(|quick_link| seen_quick_links.insert(quick_link.id()));
        let next_after_existing = self
            .quick_links
            .iter()
            .map(QuickLink::id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.next_quick_link_id = self.next_quick_link_id.max(next_after_existing).max(1);
        let next_after_clipboard_history = self
            .clipboard_history
            .entries()
            .iter()
            .map(crate::clipboard_history::ClipboardEntry::id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.next_clipboard_entry_id = self
            .next_clipboard_entry_id
            .max(next_after_clipboard_history)
            .max(1);
    }
}

fn current_schema_version() -> u32 {
    STORE_SCHEMA_VERSION
}

const fn default_next_quick_link_id() -> u64 {
    1
}

const fn default_next_clipboard_entry_id() -> u64 {
    1
}

const fn default_update_checks_enabled() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadOutcome {
    pub data: StoreData,
    /// Contains the preserved original when malformed JSON was recovered.
    pub recovered_corrupt_file: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("the operating system did not provide an application data directory")]
    DataDirectoryUnavailable,
    #[error("invalid store path: {0}")]
    InvalidPath(PathBuf),
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not serialize application data: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("store schema version {found} is not supported (expected {expected})")]
    UnsupportedSchema { found: u32, expected: u32 },
}

#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    /// Locates DuckGooKey's per-user local application data directory.
    pub fn open_default() -> Result<Self, StoreError> {
        let directories = ProjectDirs::from("com", "DuckGoo", "DuckGooKey")
            .ok_or(StoreError::DataDirectoryUnavailable)?;
        Ok(Self::at(directories.data_local_dir().join(STORE_FILE_NAME)))
    }

    /// Creates a store handle for a caller-selected file without performing I/O.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<LoadOutcome, StoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadOutcome {
                    data: StoreData::default(),
                    recovered_corrupt_file: None,
                });
            }
            Err(source) => return Err(self.io_error(source)),
        };

        let mut data = match serde_json::from_slice::<StoreData>(&bytes) {
            Ok(data) => data,
            Err(parse_error) => {
                let corrupt_path = self.quarantine_corrupt_file()?;
                tracing::warn!(
                    store = %self.path.display(),
                    preserved_at = %corrupt_path.display(),
                    error = %parse_error,
                    "Recovered from malformed DuckGooKey application data"
                );
                return Ok(LoadOutcome {
                    data: StoreData::default(),
                    recovered_corrupt_file: Some(corrupt_path),
                });
            }
        };

        match data.schema_version {
            1 => data.schema_version = STORE_SCHEMA_VERSION,
            STORE_SCHEMA_VERSION => {}
            found => {
                return Err(StoreError::UnsupportedSchema {
                    found,
                    expected: STORE_SCHEMA_VERSION,
                });
            }
        }
        data.normalize();

        Ok(LoadOutcome {
            data,
            recovered_corrupt_file: None,
        })
    }

    pub fn save(&self, data: &StoreData) -> Result<(), StoreError> {
        if data.schema_version != STORE_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedSchema {
                found: data.schema_version,
                expected: STORE_SCHEMA_VERSION,
            });
        }

        let parent = self.parent_directory()?;
        fs::create_dir_all(parent).map_err(|source| StoreError::Io {
            path: parent.to_owned(),
            source,
        })?;

        let mut encoded = serde_json::to_vec_pretty(data).map_err(StoreError::Serialize)?;
        encoded.push(b'\n');

        let (file, temp_path) = create_unique_file(&self.path, "tmp")?;
        let mut pending = PendingFile::new(temp_path);
        write_and_sync(file, &encoded).map_err(|source| StoreError::Io {
            path: pending.path.clone(),
            source,
        })?;
        replace_file(&pending.path, &self.path).map_err(|source| self.io_error(source))?;
        pending.committed = true;
        sync_parent_directory(parent).map_err(|source| StoreError::Io {
            path: parent.to_owned(),
            source,
        })?;
        Ok(())
    }

    pub fn load_usage(&self) -> Result<Vec<UsageRecord>, StoreError> {
        Ok(self.load()?.data.usage)
    }

    pub fn save_usage(&self, usage: &[UsageRecord]) -> Result<(), StoreError> {
        let mut data = self.load()?.data;
        data.usage.clear();
        for record in usage.iter().cloned() {
            data.upsert_usage(record);
        }
        self.save(&data)
    }

    fn quarantine_corrupt_file(&self) -> Result<PathBuf, StoreError> {
        let quarantine_path = unique_sibling_path(&self.path, "corrupt")?;
        fs::rename(&self.path, &quarantine_path).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })?;
        Ok(quarantine_path)
    }

    fn parent_directory(&self) -> Result<&Path, StoreError> {
        self.path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .or_else(|| self.path.file_name().map(|_| Path::new(".")))
            .ok_or_else(|| StoreError::InvalidPath(self.path.clone()))
    }

    fn io_error(&self, source: io::Error) -> StoreError {
        StoreError::Io {
            path: self.path.clone(),
            source,
        }
    }
}

fn create_unique_file(target: &Path, purpose: &str) -> Result<(File, PathBuf), StoreError> {
    for _ in 0..128 {
        let path = unique_sibling_path(target, purpose)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(StoreError::Io { path, source }),
        }
    }

    Err(StoreError::Io {
        path: target.to_owned(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique temporary file",
        ),
    })
}

fn unique_sibling_path(target: &Path, purpose: &str) -> Result<PathBuf, StoreError> {
    let file_name = target
        .file_name()
        .ok_or_else(|| StoreError::InvalidPath(target.to_owned()))?;
    let counter = UNIQUE_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut unique_name = file_name.to_os_string();
    unique_name.push(format!(
        ".{purpose}-{}-{timestamp}-{counter}",
        std::process::id()
    ));
    Ok(target.with_file_name(unique_name))
}

fn write_and_sync(mut file: File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    if !destination.exists() {
        return fs::rename(source, destination);
    }

    // Windows rename does not replace an existing destination. Preserve the
    // previous complete file until the new complete file is in place.
    let backup = unique_sibling_path(destination, "previous")
        .map_err(|error| io::Error::other(error.to_string()))?;
    fs::rename(destination, &backup)?;
    match fs::rename(source, destination) {
        Ok(()) => {
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(backup, destination);
            Err(error)
        }
    }
}

#[cfg(unix)]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

struct PendingFile {
    path: PathBuf,
    committed: bool,
}

impl PendingFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_store_loads_empty_data() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::at(directory.path().join("state.json"));

        let outcome = store.load().unwrap();

        assert_eq!(outcome.data, StoreData::default());
        assert_eq!(outcome.recovered_corrupt_file, None);
        assert!(!store.path().exists());
    }

    #[test]
    fn save_is_replaceable_and_round_trips_usage() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::at(directory.path().join("nested/state.json"));
        let mut first = UsageRecord::new("first");
        first.record_launch(123);
        store.save_usage(&[first]).unwrap();

        let mut replacement = UsageRecord::new("replacement");
        replacement.pinned = true;
        store.save_usage(&[replacement.clone()]).unwrap();

        assert_eq!(store.load_usage().unwrap(), vec![replacement]);
        let entries = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(entries, ["state.json"]);
    }

    #[test]
    fn saving_usage_preserves_settings() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::at(directory.path().join("state.json"));
        let mut data = StoreData::default();
        data.settings.shortcut = ShortcutBinding::new(
            crate::shortcut::ShortcutModifiers::COMMAND,
            crate::shortcut::ShortcutKey::K,
        )
        .unwrap();
        data.settings.preferred_input_source = Some("com.apple.keylayout.ABC".to_owned());
        store.save(&data).unwrap();

        store
            .save_usage(&[UsageRecord::new("application:test")])
            .unwrap();

        let loaded = store.load().unwrap().data;
        assert_eq!(loaded.settings, data.settings);
        assert_eq!(loaded.usage[0].item_id, "application:test");
    }

    #[test]
    fn malformed_json_is_preserved_and_recovered_safely() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::at(directory.path().join("state.json"));
        let corrupt_bytes = b"{ definitely not valid json";
        fs::write(store.path(), corrupt_bytes).unwrap();

        let outcome = store.load().unwrap();

        assert_eq!(outcome.data, StoreData::default());
        let preserved = outcome.recovered_corrupt_file.unwrap();
        assert_eq!(fs::read(&preserved).unwrap(), corrupt_bytes);
        assert!(!store.path().exists());
    }

    #[test]
    fn unsupported_schema_is_not_treated_as_corruption() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::at(directory.path().join("state.json"));
        fs::write(store.path(), br#"{"schema_version":999,"usage":[]}"#).unwrap();

        assert!(matches!(
            store.load(),
            Err(StoreError::UnsupportedSchema {
                found: 999,
                expected: STORE_SCHEMA_VERSION
            })
        ));
        assert!(store.path().exists());
    }

    #[test]
    fn duplicate_usage_is_normalized_to_one_record_per_item() {
        let data = StoreData::from_usage([
            UsageRecord::new("same"),
            UsageRecord {
                item_id: "same".to_owned(),
                launch_count: 2,
                last_launched_at_ms: Some(10),
                pinned: true,
            },
        ]);

        assert_eq!(data.usage.len(), 1);
        assert_eq!(data.usage[0].launch_count, 2);
        assert!(data.usage[0].pinned);
    }

    #[test]
    fn legacy_store_without_settings_uses_the_default_shortcut() {
        let data: StoreData = serde_json::from_str(r#"{"schema_version":1,"usage":[]}"#).unwrap();

        assert_eq!(data.settings, AppSettings::default());
        assert_eq!(data.settings.shortcut, ShortcutBinding::default());
        assert_eq!(data.settings.preferred_input_source, None);
        assert!(data.settings.update_checks_enabled);
        assert!(!data.telemetry_disclosure_acknowledged);
        assert_eq!(data.settings.search_engine, SearchEngine::Google);
    }

    #[test]
    fn legacy_settings_without_preferred_input_source_remain_compatible() {
        let data: StoreData = serde_json::from_str(
            r#"{"schema_version":1,"usage":[],"settings":{"shortcut":{"modifiers":{"command":false,"option":true,"control":false,"shift":false},"key":"space"}}}"#,
        )
        .unwrap();

        assert_eq!(data.settings.shortcut, ShortcutBinding::default());
        assert_eq!(data.settings.preferred_input_source, None);
        assert!(data.settings.update_checks_enabled);
        assert!(!data.telemetry_disclosure_acknowledged);
        assert_eq!(data.settings.search_engine, SearchEngine::Google);
    }

    #[test]
    fn legacy_telemetry_opt_out_field_is_ignored_after_deserialization() {
        let data: StoreData = serde_json::from_str(
            r#"{"schema_version":2,"settings":{"anonymous_usage_stats_enabled":false}}"#,
        )
        .unwrap();

        assert!(!data.telemetry_disclosure_acknowledged);
    }

    #[test]
    fn preferred_input_source_round_trips_without_a_schema_change() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::at(directory.path().join("state.json"));
        let mut data = StoreData::default();
        data.settings.preferred_input_source = Some("com.apple.keylayout.ABC".to_owned());
        data.settings.search_engine = SearchEngine::DuckDuckGo;

        store.save(&data).unwrap();
        let loaded = store.load().unwrap().data;

        assert_eq!(loaded.schema_version, STORE_SCHEMA_VERSION);
        assert_eq!(
            loaded.settings.preferred_input_source.as_deref(),
            Some("com.apple.keylayout.ABC")
        );
        assert_eq!(loaded.settings.search_engine, SearchEngine::DuckDuckGo);
    }

    #[test]
    fn schema_v1_loads_as_v2_with_new_feature_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::at(directory.path().join("state.json"));
        fs::write(
            store.path(),
            r#"{
                "schema_version": 1,
                "usage": [{
                    "item_id": "application:test",
                    "launch_count": 3,
                    "last_launched_at_ms": 42,
                    "pinned": true
                }],
                "settings": {
                    "preferred_input_source": "com.apple.keylayout.ABC"
                }
            }"#,
        )
        .unwrap();

        let outcome = store.load().unwrap();
        let data = outcome.data;

        assert_eq!(outcome.recovered_corrupt_file, None);
        assert_eq!(data.schema_version, STORE_SCHEMA_VERSION);
        assert_eq!(data.usage.len(), 1);
        assert_eq!(data.usage[0].item_id, "application:test");
        assert_eq!(data.usage[0].launch_count, 3);
        assert!(data.usage[0].pinned);
        assert_eq!(data.settings.shortcut, ShortcutBinding::default());
        assert_eq!(
            data.settings.preferred_input_source.as_deref(),
            Some("com.apple.keylayout.ABC")
        );
        assert!(!data.settings.clipboard_history_enabled);
        assert!(data.quick_links.is_empty());
        assert_eq!(data.next_quick_link_id, 1);
        assert!(data.clipboard_history.is_empty());
        assert_eq!(data.next_clipboard_entry_id, 1);
    }

    #[test]
    fn quick_links_and_clipboard_round_trip_with_normalized_identifiers() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::at(directory.path().join("state.json"));
        let mut data = StoreData::default();
        data.settings.clipboard_history_enabled = true;
        data.quick_links = vec![
            QuickLink::new(7, "Rust", "HTTPS://WWW.RUST-LANG.ORG/learn").unwrap(),
            QuickLink::new(7, "Duplicate ID", "https://example.com/duplicate").unwrap(),
            QuickLink::new(41, "DuckDuckGo", "https://duckduckgo.com").unwrap(),
        ];
        data.next_quick_link_id = 2;
        data.clipboard_history
            .capture(9, 100, "older clipboard value")
            .unwrap();
        data.clipboard_history
            .capture(15, 200, "newer clipboard value")
            .unwrap();
        data.next_clipboard_entry_id = 3;

        store.save(&data).unwrap();
        let mut restored = store.load().unwrap().data;

        assert!(restored.settings.clipboard_history_enabled);
        assert_eq!(restored.quick_links.len(), 2);
        assert_eq!(restored.quick_links[0].id(), 7);
        assert_eq!(restored.quick_links[0].title(), "Rust");
        assert_eq!(
            restored.quick_links[0].url(),
            "https://www.rust-lang.org/learn"
        );
        assert_eq!(restored.quick_links[1].id(), 41);
        assert_eq!(restored.next_quick_link_id, 42);
        assert_eq!(
            restored
                .clipboard_history
                .entries()
                .iter()
                .map(crate::clipboard_history::ClipboardEntry::id)
                .collect::<Vec<_>>(),
            vec![15, 9]
        );
        assert_eq!(restored.next_clipboard_entry_id, 16);
        assert_eq!(restored.allocate_quick_link_id(), 42);
        assert_eq!(restored.allocate_clipboard_entry_id(), 16);
        assert_eq!(restored.next_quick_link_id, 43);
        assert_eq!(restored.next_clipboard_entry_id, 17);
    }

    #[cfg(unix)]
    #[test]
    fn state_file_is_created_and_replaced_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let store = Store::at(directory.path().join("state.json"));
        store.save(&StoreData::default()).unwrap();

        let mode = fs::metadata(store.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        fs::set_permissions(store.path(), fs::Permissions::from_mode(0o644)).unwrap();
        let mut replacement = StoreData::default();
        replacement.settings.clipboard_history_enabled = true;
        store.save(&replacement).unwrap();

        let mode = fs::metadata(store.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
