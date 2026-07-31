use crate::catalog::{CatalogItem, LaunchAction};

use super::{PlatformError, Result};

pub fn discover_applications() -> Result<Vec<CatalogItem>> {
    Err(unsupported("application discovery"))
}

pub fn launch(action: &LaunchAction) -> Result<()> {
    match action {
        LaunchAction::RefreshCatalog => Err(PlatformError::UnsupportedAction {
            action: "refresh catalog",
        }),
        LaunchAction::Quit => Err(PlatformError::UnsupportedAction { action: "quit" }),
        LaunchAction::OpenApplication { .. } | LaunchAction::OpenPath { .. } => {
            Err(unsupported("launching catalog items"))
        }
    }
}

pub fn launch_at_login_enabled() -> bool {
    false
}

pub fn set_launch_at_login(_enabled: bool) -> Result<()> {
    Err(unsupported("launch at login"))
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
        assert!(!launch_at_login_enabled());
    }
}
