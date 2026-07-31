use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::UserDirs;
use plist::{Dictionary, Value};
use walkdir::WalkDir;

use crate::START_HIDDEN_ARGUMENT;
use crate::catalog::{CatalogItem, LaunchAction};

use super::{PlatformError, Result};

const OPEN_EXECUTABLE: &str = "/usr/bin/open";
const LAUNCH_AGENT_LABEL: &str = "com.duckgoo.key";
const LAUNCH_AGENT_FILE_NAME: &str = "com.duckgoo.key.plist";
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn discover_applications() -> Result<Vec<CatalogItem>> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(user_directories) = UserDirs::new() {
        roots.push(user_directories.home_dir().join("Applications"));
    }

    Ok(discover_in_roots(&roots))
}

pub fn launch(action: &LaunchAction) -> Result<()> {
    let target = match action {
        LaunchAction::OpenApplication { path } | LaunchAction::OpenPath { path } => path,
        LaunchAction::RefreshCatalog => {
            return Err(PlatformError::UnsupportedAction {
                action: "refresh catalog",
            });
        }
        LaunchAction::Quit => {
            return Err(PlatformError::UnsupportedAction { action: "quit" });
        }
    };

    let status = Command::new(OPEN_EXECUTABLE)
        .arg(target)
        .status()
        .map_err(|source| PlatformError::Io {
            operation: "launching an item",
            path: target.clone(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(PlatformError::CommandFailed {
            executable: OPEN_EXECUTABLE,
            status: status.code(),
        })
    }
}

pub fn launch_at_login_enabled() -> bool {
    let Some(path) = launch_agent_path() else {
        return false;
    };
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    launch_at_login_enabled_at(&path, &executable)
}

pub fn set_launch_at_login(enabled: bool) -> Result<()> {
    let path = launch_agent_path().ok_or(PlatformError::HomeDirectoryUnavailable)?;
    if enabled {
        let executable = std::env::current_exe().map_err(PlatformError::CurrentExecutable)?;
        write_launch_agent(&path, &executable)
    } else {
        remove_launch_agent(&path)
    }
}

fn discover_in_roots(roots: &[PathBuf]) -> Vec<CatalogItem> {
    let mut applications = Vec::new();
    let mut seen_paths = HashSet::new();

    for root in roots.iter().filter(|root| root.is_dir()) {
        let mut entries = WalkDir::new(root).follow_links(false).into_iter();
        while let Some(entry) = entries.next() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(
                        root = %root.display(),
                        error = %error,
                        "Could not inspect part of an application directory"
                    );
                    continue;
                }
            };
            let path = entry.path();
            if !is_application_bundle(path) {
                continue;
            }

            if entry.file_type().is_dir() {
                entries.skip_current_dir();
            }
            if !path.is_dir() || !seen_paths.insert(path.to_owned()) {
                continue;
            }

            applications.push(catalog_item_from_bundle(path));
        }
    }

    applications.sort_by(|left, right| {
        left.title
            .to_lowercase()
            .cmp(&right.title.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    applications
}

fn is_application_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

fn catalog_item_from_bundle(bundle_path: &Path) -> CatalogItem {
    let fallback_title = bundle_path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Application")
        .to_owned();
    let info_path = bundle_path.join("Contents/Info.plist");
    let dictionary = match Value::from_file(&info_path) {
        Ok(Value::Dictionary(dictionary)) => Some(dictionary),
        Ok(_) => {
            tracing::debug!(
                path = %info_path.display(),
                "Application Info.plist was not a dictionary"
            );
            None
        }
        Err(error) => {
            tracing::debug!(
                path = %info_path.display(),
                error = %error,
                "Using the bundle name because Info.plist could not be read"
            );
            None
        }
    };

    let display_name = dictionary
        .as_ref()
        .and_then(|dictionary| nonempty_string(dictionary, "CFBundleDisplayName"))
        .or_else(|| {
            dictionary
                .as_ref()
                .and_then(|dictionary| nonempty_string(dictionary, "CFBundleName"))
        })
        .unwrap_or_else(|| fallback_title.clone());

    let mut item = CatalogItem::application(bundle_path, display_name);
    if let Some(dictionary) = dictionary {
        for key in ["CFBundleName", "CFBundleIdentifier"] {
            if let Some(keyword) = nonempty_string(&dictionary, key)
                && !item
                    .keywords
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&keyword))
                && !item.title.eq_ignore_ascii_case(&keyword)
            {
                item.keywords.push(keyword);
            }
        }
    }
    item
}

fn nonempty_string(dictionary: &Dictionary, key: &str) -> Option<String> {
    dictionary
        .get(key)
        .and_then(Value::as_string)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn launch_agent_path() -> Option<PathBuf> {
    UserDirs::new().map(|directories| {
        directories
            .home_dir()
            .join("Library/LaunchAgents")
            .join(LAUNCH_AGENT_FILE_NAME)
    })
}

fn launch_at_login_enabled_at(path: &Path, executable: &Path) -> bool {
    let Ok(Value::Dictionary(dictionary)) = Value::from_file(path) else {
        return false;
    };

    let label_matches = dictionary
        .get("Label")
        .and_then(Value::as_string)
        .is_some_and(|label| label == LAUNCH_AGENT_LABEL);
    let run_at_load = dictionary
        .get("RunAtLoad")
        .and_then(Value::as_boolean)
        .unwrap_or(false);
    let executable_matches = dictionary
        .get("ProgramArguments")
        .and_then(Value::as_array)
        .and_then(|arguments| arguments.first())
        .and_then(Value::as_string)
        .is_some_and(|configured| Path::new(configured) == executable);
    let starts_hidden = dictionary
        .get("ProgramArguments")
        .and_then(Value::as_array)
        .and_then(|arguments| arguments.get(1))
        .and_then(Value::as_string)
        .is_some_and(|argument| argument == START_HIDDEN_ARGUMENT);

    label_matches && run_at_load && executable_matches && starts_hidden
}

fn write_launch_agent(path: &Path, executable: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| PlatformError::Io {
            operation: "locating the LaunchAgents directory",
            path: path.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
        })?;
    fs::create_dir_all(parent).map_err(|source| PlatformError::Io {
        operation: "creating the LaunchAgents directory",
        path: parent.to_owned(),
        source,
    })?;

    let mut dictionary = Dictionary::new();
    dictionary.insert(
        "Label".to_owned(),
        Value::String(LAUNCH_AGENT_LABEL.to_owned()),
    );
    dictionary.insert(
        "ProgramArguments".to_owned(),
        Value::Array(vec![
            Value::String(
                executable
                    .to_str()
                    .ok_or_else(|| PlatformError::InvalidExecutablePath(executable.to_owned()))?
                    .to_owned(),
            ),
            Value::String(START_HIDDEN_ARGUMENT.to_owned()),
        ]),
    );
    dictionary.insert("RunAtLoad".to_owned(), Value::Boolean(true));

    let mut encoded = Vec::new();
    Value::Dictionary(dictionary)
        .to_writer_xml(&mut encoded)
        .map_err(PlatformError::PlistSerialization)?;

    let (mut file, temp_path) = create_temporary_file(path)?;
    if let Err(source) = file.write_all(&encoded).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temp_path);
        return Err(PlatformError::Io {
            operation: "writing the launch-at-login property list",
            path: temp_path,
            source,
        });
    }
    drop(file);

    if let Err(source) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(PlatformError::Io {
            operation: "installing the launch-at-login property list",
            path: path.to_owned(),
            source,
        });
    }
    sync_directory(parent).map_err(|source| PlatformError::Io {
        operation: "synchronizing the LaunchAgents directory",
        path: parent.to_owned(),
        source,
    })
}

fn remove_launch_agent(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(PlatformError::Io {
                operation: "removing the launch-at-login property list",
                path: path.to_owned(),
                source,
            });
        }
    }

    if let Some(parent) = path.parent() {
        sync_directory(parent).map_err(|source| PlatformError::Io {
            operation: "synchronizing the LaunchAgents directory",
            path: parent.to_owned(),
            source,
        })?;
    }
    Ok(())
}

fn create_temporary_file(target: &Path) -> Result<(File, PathBuf)> {
    let file_name = target.file_name().ok_or_else(|| PlatformError::Io {
        operation: "allocating a launch-at-login temporary file",
        path: target.to_owned(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"),
    })?;

    for _ in 0..128 {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let mut name = file_name.to_os_string();
        name.push(format!(".tmp-{}-{timestamp}-{counter}", std::process::id()));
        let path = target.with_file_name(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(PlatformError::Io {
                    operation: "creating a launch-at-login temporary file",
                    path,
                    source,
                });
            }
        }
    }

    Err(PlatformError::Io {
        operation: "creating a launch-at-login temporary file",
        path: target.to_owned(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique temporary file",
        ),
    })
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_bundle(path: &Path, display_name: Option<&str>, bundle_name: &str) {
        fs::create_dir_all(path.join("Contents")).unwrap();
        let mut dictionary = Dictionary::new();
        if let Some(display_name) = display_name {
            dictionary.insert(
                "CFBundleDisplayName".to_owned(),
                Value::String(display_name.to_owned()),
            );
        }
        dictionary.insert(
            "CFBundleName".to_owned(),
            Value::String(bundle_name.to_owned()),
        );
        dictionary.insert(
            "CFBundleIdentifier".to_owned(),
            Value::String(format!("test.{bundle_name}")),
        );
        Value::Dictionary(dictionary)
            .to_file_xml(path.join("Contents/Info.plist"))
            .unwrap();
    }

    #[test]
    fn discovery_reads_plist_and_never_descends_into_a_bundle() {
        let directory = tempfile::tempdir().unwrap();
        let applications = directory.path().join("Applications");
        let outer = applications.join("Outer.app");
        let nested = outer.join("Contents/Helpers/Nested.app");
        let utility = applications.join("Utilities/Utility.app");
        write_bundle(&outer, Some("Displayed Outer"), "Outer");
        write_bundle(&nested, Some("Must Not Appear"), "Nested");
        write_bundle(&utility, None, "Named Utility");

        let items = discover_in_roots(&[applications]);

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|item| item.title == "Displayed Outer"));
        assert!(items.iter().any(|item| item.title == "Named Utility"));
        assert!(!items.iter().any(|item| item.title == "Must Not Appear"));
    }

    #[test]
    fn launch_agent_is_valid_xml_with_required_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(LAUNCH_AGENT_FILE_NAME);
        let executable = Path::new("/Applications/DuckGooKey.app/Contents/MacOS/DuckGooKey");

        write_launch_agent(&path, executable).unwrap();

        assert!(launch_at_login_enabled_at(&path, executable));
        let Value::Dictionary(dictionary) = Value::from_file(&path).unwrap() else {
            panic!("launch agent must be a dictionary");
        };
        assert_eq!(
            dictionary.get("Label").and_then(Value::as_string),
            Some(LAUNCH_AGENT_LABEL)
        );
        assert_eq!(
            dictionary.get("RunAtLoad").and_then(Value::as_boolean),
            Some(true)
        );
        assert_eq!(
            dictionary
                .get("ProgramArguments")
                .and_then(Value::as_array)
                .and_then(|arguments| arguments.first())
                .and_then(Value::as_string),
            Some(executable.to_str().unwrap())
        );
        assert_eq!(
            dictionary
                .get("ProgramArguments")
                .and_then(Value::as_array)
                .and_then(|arguments| arguments.get(1))
                .and_then(Value::as_string),
            Some(START_HIDDEN_ARGUMENT)
        );
    }

    #[test]
    fn disabling_login_launch_removes_only_the_exact_agent_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(LAUNCH_AGENT_FILE_NAME);
        let neighbor = directory.path().join("keep-me.plist");
        fs::write(&path, b"agent").unwrap();
        fs::write(&neighbor, b"neighbor").unwrap();

        remove_launch_agent(&path).unwrap();

        assert!(!path.exists());
        assert_eq!(fs::read(neighbor).unwrap(), b"neighbor");
        remove_launch_agent(&path).unwrap();
    }

    #[test]
    fn internal_actions_are_not_sent_to_open() {
        assert!(matches!(
            launch(&LaunchAction::RefreshCatalog),
            Err(PlatformError::UnsupportedAction { .. })
        ));
        assert!(matches!(
            launch(&LaunchAction::Quit),
            Err(PlatformError::UnsupportedAction { .. })
        ));
    }
}
