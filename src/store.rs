use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::UsageRecord;

pub const STORE_SCHEMA_VERSION: u32 = 1;
const STORE_FILE_NAME: &str = "state.json";
static UNIQUE_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreData {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub usage: Vec<UsageRecord>,
}

impl Default for StoreData {
    fn default() -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            usage: Vec::new(),
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

    fn normalize(&mut self) {
        let records = std::mem::take(&mut self.usage);
        for record in records {
            self.upsert_usage(record);
        }
    }
}

fn current_schema_version() -> u32 {
    STORE_SCHEMA_VERSION
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

        if data.schema_version != STORE_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedSchema {
                found: data.schema_version,
                expected: STORE_SCHEMA_VERSION,
            });
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
        self.save(&StoreData::from_usage(usage.iter().cloned()))
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
        match OpenOptions::new().write(true).create_new(true).open(&path) {
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
}
