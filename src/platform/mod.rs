use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[cfg(not(target_os = "macos"))]
mod fallback;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
pub use fallback::{discover_applications, launch, launch_at_login_enabled, set_launch_at_login};
#[cfg(target_os = "macos")]
pub use macos::{discover_applications, launch, launch_at_login_enabled, set_launch_at_login};

pub type Result<T> = std::result::Result<T, PlatformError>;

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
    #[error("{action} is an application action and cannot be launched by the platform")]
    UnsupportedAction { action: &'static str },
    #[error("{operation} is not supported on {platform}")]
    UnsupportedPlatform {
        operation: &'static str,
        platform: &'static str,
    },
}
