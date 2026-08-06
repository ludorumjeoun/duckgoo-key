use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[cfg(not(target_os = "macos"))]
mod fallback;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
pub use fallback::{
    available_input_sources, clipboard_change_count, close_quick_look, copy_text,
    current_input_source_identifier, discover_applications, execute_system_command, file_icon_png,
    launch, launch_at_login_enabled, quick_look_is_open, read_clipboard_text_if_changed,
    reveal_in_file_manager, search_files, select_input_source, set_launch_at_login,
    show_quick_look,
};
#[cfg(target_os = "macos")]
pub use macos::{
    available_input_sources, clipboard_change_count, close_quick_look, copy_text,
    current_input_source_identifier, discover_applications, execute_system_command, file_icon_png,
    launch, launch_at_login_enabled, quick_look_is_open, read_clipboard_text_if_changed,
    reveal_in_file_manager, search_files, select_input_source, set_launch_at_login,
    show_quick_look,
};

pub type Result<T> = std::result::Result<T, PlatformError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileSearchMatchKind {
    DirectPath,
    FuzzyPath,
    FileName,
    Content,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSearchResult {
    pub path: PathBuf,
    pub match_kind: FileSearchMatchKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSource {
    pub identifier: String,
    pub localized_name: String,
}

/// A point-in-time view of the general clipboard.
///
/// `text` is `None` when the clipboard has not changed since the caller's
/// previous change count, or when the new clipboard value is not plain text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardSnapshot {
    /// A stable checkpoint the caller can supply on its next poll. If the
    /// clipboard changes during a read, this remains the caller's previous
    /// checkpoint so the new value is retried instead of being skipped.
    pub change_count: i64,
    pub text: Option<String>,
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("could not determine the current user's home directory")]
    HomeDirectoryUnavailable,
    #[error("could not determine the current executable: {0}")]
    CurrentExecutable(#[source] io::Error),
    #[error("the executable path is not valid UTF-8: {0}")]
    InvalidExecutablePath(PathBuf),
    #[error("I/O error while {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not serialize the launch-at-login property list: {0}")]
    PlistSerialization(#[source] plist::Error),
    #[error("{executable} exited unsuccessfully with status {status:?}")]
    CommandFailed {
        executable: &'static str,
        status: Option<i32>,
    },
    #[error("{executable} did not finish within {timeout_ms} ms")]
    CommandTimedOut {
        executable: &'static str,
        timeout_ms: u64,
    },
    #[error("{action} must be handled by the launcher instead of the generic platform launch path")]
    UnsupportedAction { action: &'static str },
    #[error("{operation} is not supported on {platform}")]
    UnsupportedPlatform {
        operation: &'static str,
        platform: &'static str,
    },
    #[error("macOS did not return the enabled keyboard input-source list")]
    InputSourceListUnavailable,
    #[error("macOS did not return the current keyboard input source")]
    CurrentInputSourceUnavailable,
    #[error("input source property `{property}` is missing")]
    InputSourcePropertyMissing { property: &'static str },
    #[error("input source property `{property}` has an unexpected Core Foundation type")]
    InputSourcePropertyTypeMismatch { property: &'static str },
    #[error("input source property `{property}` is empty")]
    InputSourcePropertyEmpty { property: &'static str },
    #[error("input source `{identifier}` is not enabled and selectable")]
    InputSourceUnavailable { identifier: String },
    #[error("macOS could not select input source `{identifier}` (OSStatus {status})")]
    InputSourceSelectionFailed { identifier: String, status: i32 },
    #[error("the clipboard rejected the {operation} operation")]
    ClipboardOperationRejected { operation: &'static str },
}
