use serde::{Deserialize, Serialize};

use crate::catalog::{CatalogItem, LaunchAction};

/// A bounded set of operating-system commands exposed by the launcher.
///
/// Keeping these commands typed prevents user-visible labels from becoming
/// executable input. The platform layer is responsible for mapping each
/// variant to a native operation without invoking a shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemCommand {
    OpenSystemSettings,
    Sleep,
    ToggleAppearance,
    EmptyTrash,
    LogOut,
    Restart,
    ShutDown,
}

impl SystemCommand {
    pub const ALL: [Self; 7] = [
        Self::OpenSystemSettings,
        Self::Sleep,
        Self::ToggleAppearance,
        Self::EmptyTrash,
        Self::LogOut,
        Self::Restart,
        Self::ShutDown,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::OpenSystemSettings => "open-system-settings",
            Self::Sleep => "sleep",
            Self::ToggleAppearance => "toggle-appearance",
            Self::EmptyTrash => "empty-trash",
            Self::LogOut => "log-out",
            Self::Restart => "restart",
            Self::ShutDown => "shut-down",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::OpenSystemSettings => "Open System Settings",
            Self::Sleep => "Sleep",
            Self::ToggleAppearance => "Toggle System Appearance",
            Self::EmptyTrash => "Empty Trash",
            Self::LogOut => "Log Out",
            Self::Restart => "Restart",
            Self::ShutDown => "Shut Down",
        }
    }

    pub const fn subtitle(self) -> &'static str {
        match self {
            Self::OpenSystemSettings => "Open macOS System Settings",
            Self::Sleep => "Put this Mac to sleep",
            Self::ToggleAppearance => "Switch between light and dark appearance",
            Self::EmptyTrash => "Permanently delete every item in the Trash",
            Self::LogOut => "Log out the current macOS user",
            Self::Restart => "Restart this Mac",
            Self::ShutDown => "Shut down this Mac",
        }
    }

    pub const fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::OpenSystemSettings => &["preferences", "settings", "macos"],
            Self::Sleep => &["suspend", "standby", "power"],
            Self::ToggleAppearance => &["dark", "light", "theme", "mode"],
            Self::EmptyTrash => &["bin", "delete", "files", "recycle"],
            Self::LogOut => &["logout", "sign out", "session", "user"],
            Self::Restart => &["reboot", "power"],
            Self::ShutDown => &["shutdown", "power off", "turn off"],
        }
    }

    /// Whether activating this command must first show an explicit confirmation.
    pub const fn requires_confirmation(self) -> bool {
        matches!(
            self,
            Self::EmptyTrash | Self::LogOut | Self::Restart | Self::ShutDown
        )
    }

    pub const fn confirmation_prompt(self) -> Option<&'static str> {
        match self {
            Self::EmptyTrash => Some("Permanently delete every item in the Trash?"),
            Self::LogOut => Some("Log out now and close all open applications?"),
            Self::Restart => Some("Restart this Mac now?"),
            Self::ShutDown => Some("Shut down this Mac now?"),
            Self::OpenSystemSettings | Self::Sleep | Self::ToggleAppearance => None,
        }
    }

    pub fn catalog_item(self) -> CatalogItem {
        CatalogItem {
            id: format!("system:{}", self.id()),
            title: self.title().to_owned(),
            subtitle: Some(self.subtitle().to_owned()),
            icon_path: None,
            keywords: self
                .keywords()
                .iter()
                .map(|keyword| (*keyword).to_owned())
                .collect(),
            action: LaunchAction::SystemCommand { command: self },
            pinnable: true,
        }
    }
}

pub fn system_command_items() -> Vec<CatalogItem> {
    SystemCommand::ALL
        .into_iter()
        .map(SystemCommand::catalog_item)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn command_metadata_has_unique_stable_identifiers() {
        let ids = SystemCommand::ALL
            .into_iter()
            .map(SystemCommand::id)
            .collect::<HashSet<_>>();

        assert_eq!(ids.len(), SystemCommand::ALL.len());
        assert!(ids.iter().all(|id| !id.is_empty()));
    }

    #[test]
    fn destructive_or_session_ending_commands_require_confirmation() {
        for command in [
            SystemCommand::EmptyTrash,
            SystemCommand::LogOut,
            SystemCommand::Restart,
            SystemCommand::ShutDown,
        ] {
            assert!(command.requires_confirmation());
            assert!(command.confirmation_prompt().is_some());
        }

        for command in [
            SystemCommand::OpenSystemSettings,
            SystemCommand::Sleep,
            SystemCommand::ToggleAppearance,
        ] {
            assert!(!command.requires_confirmation());
            assert_eq!(command.confirmation_prompt(), None);
        }
    }

    #[test]
    fn catalog_items_preserve_the_typed_action_and_can_be_pinned() {
        let items = system_command_items();

        assert_eq!(items.len(), SystemCommand::ALL.len());
        for (item, command) in items.iter().zip(SystemCommand::ALL) {
            assert_eq!(item.id, format!("system:{}", command.id()));
            assert_eq!(item.title, command.title());
            assert_eq!(item.subtitle.as_deref(), Some(command.subtitle()));
            assert!(item.pinnable);
            assert_eq!(item.action, LaunchAction::SystemCommand { command });
        }
    }

    #[test]
    fn serde_names_are_human_readable_and_stable() {
        assert_eq!(
            serde_json::to_string(&SystemCommand::ToggleAppearance).unwrap(),
            r#""toggle_appearance""#
        );
        assert_eq!(
            serde_json::from_str::<SystemCommand>(r#""shut_down""#).unwrap(),
            SystemCommand::ShutDown
        );
    }
}
