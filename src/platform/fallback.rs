use std::path::{Path, PathBuf};

use crate::catalog::{CatalogItem, LaunchAction};
use crate::commands::SystemCommand;

use super::{ClipboardSnapshot, InputSource, PlatformError, Result};

pub fn discover_applications() -> Result<Vec<CatalogItem>> {
    Err(unsupported("application discovery"))
}

pub fn launch(action: &LaunchAction) -> Result<()> {
    match action {
        LaunchAction::CopyText { .. } => Err(PlatformError::UnsupportedAction {
            action: "copy text",
        }),
        LaunchAction::SystemCommand { .. } => Err(PlatformError::UnsupportedAction {
            action: "system command",
        }),
        LaunchAction::EnterSearchMode { .. } => Err(PlatformError::UnsupportedAction {
            action: "enter search mode",
        }),
        LaunchAction::ManageQuickLinks => Err(PlatformError::UnsupportedAction {
            action: "manage quick links",
        }),
        LaunchAction::RefreshCatalog => Err(PlatformError::UnsupportedAction {
            action: "refresh catalog",
        }),
        LaunchAction::Quit => Err(PlatformError::UnsupportedAction { action: "quit" }),
        LaunchAction::OpenApplication { .. }
        | LaunchAction::OpenPath { .. }
        | LaunchAction::OpenUrl { .. } => Err(unsupported("launching catalog items")),
    }
}

pub fn reveal_in_file_manager(_path: &Path) -> Result<()> {
    Err(unsupported("revealing items in the file manager"))
}

pub fn execute_system_command(_command: &SystemCommand) -> Result<()> {
    Err(unsupported("executing system commands"))
}

pub fn search_files(_query: &str, _limit: usize) -> Result<Vec<super::FileSearchResult>> {
    Err(unsupported("searching files"))
}

pub fn file_icon_png(_path: &Path) -> Option<Vec<u8>> {
    None
}

pub fn show_quick_look(_path: &Path) -> Result<()> {
    Err(unsupported("showing Quick Look"))
}

pub fn close_quick_look() -> Result<()> {
    Ok(())
}

pub fn quick_look_is_open() -> Result<bool> {
    Ok(false)
}

pub fn clipboard_change_count() -> Result<i64> {
    Err(unsupported("reading the clipboard change count"))
}

pub fn read_clipboard_text_if_changed(_previous: i64) -> Result<ClipboardSnapshot> {
    Err(unsupported("reading clipboard text"))
}

pub fn copy_text(_text: &str) -> Result<i64> {
    Err(unsupported("copying text to the clipboard"))
}

pub fn launch_at_login_enabled() -> bool {
    false
}

pub fn set_launch_at_login(_enabled: bool) -> Result<()> {
    Err(unsupported("launch at login"))
}

pub fn available_input_sources() -> Result<Vec<InputSource>> {
    Err(unsupported("listing keyboard input sources"))
}

pub fn current_input_source_identifier() -> Result<String> {
    Err(unsupported("reading the current keyboard input source"))
}

pub fn select_input_source(_identifier: &str) -> Result<()> {
    Err(unsupported("selecting a keyboard input source"))
}

fn unsupported(operation: &'static str) -> PlatformError {
    PlatformError::UnsupportedPlatform {
        operation,
        platform: std::env::consts::OS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_reports_unsupported_operations() {
        assert!(matches!(
            discover_applications(),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(matches!(
            set_launch_at_login(true),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(matches!(
            available_input_sources(),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(matches!(
            current_input_source_identifier(),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(matches!(
            select_input_source("com.example.input-source"),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(matches!(
            reveal_in_file_manager(Path::new("/Applications/Example.app")),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(matches!(
            execute_system_command(&SystemCommand::OpenSystemSettings),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(matches!(
            search_files("example", 10),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert_eq!(file_icon_png(Path::new("/tmp/example")), None);
        assert!(matches!(
            show_quick_look(Path::new("/tmp/example")),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(close_quick_look().is_ok());
        assert!(matches!(
            clipboard_change_count(),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(matches!(
            read_clipboard_text_if_changed(0),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(matches!(
            copy_text("example"),
            Err(PlatformError::UnsupportedPlatform { .. })
        ));
        assert!(!launch_at_login_enabled());
    }
}
