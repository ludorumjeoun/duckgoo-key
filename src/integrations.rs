use global_hotkey::hotkey::{Code, HotKey, Modifiers as GlobalHotKeyModifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use image::ImageFormat;
use tracing::warn;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::shortcut::ShortcutBinding;

const SHOW_MENU_ID: &str = "duckgookey.show";
const LAUNCH_AT_LOGIN_MENU_ID: &str = "duckgookey.launch-at-login";
const QUIT_MENU_ID: &str = "duckgookey.quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationEvent {
    ToggleLauncher,
    ToggleQuickLook,
    ShowLauncher,
    SetLaunchAtLogin(bool),
    Quit,
}

pub struct DesktopIntegrations {
    hotkey_manager: Option<GlobalHotKeyManager>,
    launcher_hotkey: Option<HotKey>,
    quick_look_hotkey: Option<HotKey>,
    quick_look_hotkey_unavailable: bool,
    tray_icon: Option<TrayIcon>,
    show_menu_id: MenuId,
    launch_at_login_menu_id: MenuId,
    launch_at_login_item: Option<CheckMenuItem>,
    quit_menu_id: MenuId,
    warnings: Vec<String>,
}

impl DesktopIntegrations {
    pub fn initialize(launch_at_login: bool, shortcut: ShortcutBinding) -> Self {
        let requested_hotkey = shortcut.to_hotkey();
        let mut warnings = Vec::new();

        let (hotkey_manager, launcher_hotkey) = match GlobalHotKeyManager::new() {
            Ok(manager) => match manager.register(requested_hotkey) {
                Ok(()) => (Some(manager), Some(requested_hotkey)),
                Err(error) => {
                    let message = format!("Could not register the {shortcut} shortcut: {error}");
                    warn!("{message}");
                    warnings.push(message);
                    (Some(manager), None)
                }
            },
            Err(error) => {
                let message = format!("Could not initialize global shortcuts: {error}");
                warn!("{message}");
                warnings.push(message);
                (None, None)
            }
        };

        let show_menu_id = MenuId::new(SHOW_MENU_ID);
        let launch_at_login_menu_id = MenuId::new(LAUNCH_AT_LOGIN_MENU_ID);
        let quit_menu_id = MenuId::new(QUIT_MENU_ID);
        let (tray_icon, launch_at_login_item) = match create_tray_icon(
            &show_menu_id,
            &launch_at_login_menu_id,
            &quit_menu_id,
            launch_at_login,
        ) {
            Ok((tray_icon, item)) => (Some(tray_icon), Some(item)),
            Err(error) => {
                let message = format!("Could not create the menu bar icon: {error}");
                warn!("{message}");
                warnings.push(message);
                (None, None)
            }
        };

        Self {
            hotkey_manager,
            launcher_hotkey,
            quick_look_hotkey: None,
            quick_look_hotkey_unavailable: false,
            tray_icon,
            show_menu_id,
            launch_at_login_menu_id,
            launch_at_login_item,
            quit_menu_id,
            warnings,
        }
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn is_shortcut_active(&self, shortcut: ShortcutBinding) -> bool {
        self.launcher_hotkey == Some(shortcut.to_hotkey())
    }

    /// Replaces the active shortcut while preserving the previous registration
    /// whenever the operating system rejects the requested combination.
    pub fn change_shortcut(&mut self, shortcut: ShortcutBinding) -> Result<(), String> {
        let Some(manager) = self.hotkey_manager.as_ref() else {
            return Err("global shortcut manager is unavailable".to_owned());
        };
        let requested = shortcut.to_hotkey();
        if self.launcher_hotkey == Some(requested) {
            return Ok(());
        }

        let previous = self.launcher_hotkey;
        if let Some(previous) = previous {
            manager
                .unregister(previous)
                .map_err(|error| format!("could not release the previous shortcut: {error}"))?;
        }

        match manager.register(requested) {
            Ok(()) => {
                self.launcher_hotkey = Some(requested);
                Ok(())
            }
            Err(error) => {
                self.launcher_hotkey = None;
                if let Some(previous) = previous {
                    match manager.register(previous) {
                        Ok(()) => {
                            self.launcher_hotkey = Some(previous);
                            Err(format!("{error}; the previous shortcut was restored"))
                        }
                        Err(rollback_error) => Err(format!(
                            "{error}; the previous shortcut could not be restored: {rollback_error}"
                        )),
                    }
                } else {
                    Err(error.to_string())
                }
            }
        }
    }

    /// Registers Command-Y only while the launcher can act on it. This lets the
    /// system consume the shortcut before Iced's focused text input can insert
    /// a literal `y`.
    pub fn set_quick_look_hotkey_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if enabled {
            if self.quick_look_hotkey.is_some() || self.quick_look_hotkey_unavailable {
                return Ok(());
            }

            let Some(manager) = self.hotkey_manager.as_ref() else {
                self.quick_look_hotkey_unavailable = true;
                return Err("global shortcut manager is unavailable".to_owned());
            };
            let hotkey = quick_look_hotkey();
            match manager.register(hotkey) {
                Ok(()) => {
                    self.quick_look_hotkey = Some(hotkey);
                    Ok(())
                }
                Err(error) => {
                    self.quick_look_hotkey_unavailable = true;
                    Err(format!(
                        "could not register the ⌘Y Quick Look shortcut: {error}"
                    ))
                }
            }
        } else {
            let Some(hotkey) = self.quick_look_hotkey.take() else {
                self.quick_look_hotkey_unavailable = false;
                return Ok(());
            };
            let Some(manager) = self.hotkey_manager.as_ref() else {
                return Err("global shortcut manager is unavailable".to_owned());
            };
            manager.unregister(hotkey).map_err(|error| {
                format!("could not release the ⌘Y Quick Look shortcut: {error}")
            })?;
            self.quick_look_hotkey_unavailable = false;
            Ok(())
        }
    }

    pub fn drain_events(&self) -> Vec<IntegrationEvent> {
        let mut events = Vec::new();

        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state() == HotKeyState::Pressed {
                if self
                    .launcher_hotkey
                    .is_some_and(|hotkey| event.id() == hotkey.id())
                {
                    events.push(IntegrationEvent::ToggleLauncher);
                } else if self
                    .quick_look_hotkey
                    .is_some_and(|hotkey| event.id() == hotkey.id())
                {
                    events.push(IntegrationEvent::ToggleQuickLook);
                }
            }
        }

        if self.tray_icon.is_some() {
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id() == &self.show_menu_id {
                    events.push(IntegrationEvent::ShowLauncher);
                } else if event.id() == &self.launch_at_login_menu_id {
                    if let Some(item) = &self.launch_at_login_item {
                        events.push(IntegrationEvent::SetLaunchAtLogin(item.is_checked()));
                    }
                } else if event.id() == &self.quit_menu_id {
                    events.push(IntegrationEvent::Quit);
                }
            }

            while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                if matches!(
                    event,
                    TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    }
                ) {
                    events.push(IntegrationEvent::ShowLauncher);
                }
            }
        }

        events
    }

    pub fn set_launch_at_login_checked(&self, enabled: bool) {
        if let Some(item) = &self.launch_at_login_item {
            item.set_checked(enabled);
        }
    }
}

fn quick_look_hotkey() -> HotKey {
    HotKey::new(Some(GlobalHotKeyModifiers::SUPER), Code::KeyY)
}

fn create_tray_icon(
    show_menu_id: &MenuId,
    launch_at_login_menu_id: &MenuId,
    quit_menu_id: &MenuId,
    launch_at_login: bool,
) -> Result<(TrayIcon, CheckMenuItem), String> {
    let show = MenuItem::with_id(show_menu_id.clone(), "Show DuckGooKey", true, None);
    let launch_at_login = CheckMenuItem::with_id(
        launch_at_login_menu_id.clone(),
        "Launch at Login",
        true,
        launch_at_login,
        None,
    );
    let separator = PredefinedMenuItem::separator();
    let quit = MenuItem::with_id(quit_menu_id.clone(), "Quit DuckGooKey", true, None);
    let menu = Menu::with_items(&[&show, &launch_at_login, &separator, &quit])
        .map_err(|error| error.to_string())?;

    let tray_icon = TrayIconBuilder::new()
        .with_tooltip("DuckGooKey")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(true)
        .with_icon(menu_bar_icon()?)
        .with_icon_as_template(false)
        .build()
        .map_err(|error| error.to_string())?;

    Ok((tray_icon, launch_at_login))
}

fn menu_bar_icon() -> Result<Icon, String> {
    let image = image::load_from_memory_with_format(
        include_bytes!("../assets/icons/duckgoo-key-128.png"),
        ImageFormat::Png,
    )
    .map_err(|error| format!("could not decode the embedded brand icon: {error}"))?
    .into_rgba8();
    let (width, height) = image.dimensions();

    Icon::from_rgba(image.into_raw(), width, height)
        .map_err(|error| format!("could not create the menu bar brand icon: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_bar_icon_has_expected_dimensions() {
        assert!(menu_bar_icon().is_ok());
    }

    #[test]
    fn launcher_hotkey_is_option_space() {
        let hotkey = ShortcutBinding::default().to_hotkey();
        assert_eq!(hotkey, ShortcutBinding::DEFAULT.to_hotkey());
    }

    #[test]
    fn quick_look_hotkey_is_command_y() {
        let hotkey = quick_look_hotkey();
        assert_eq!(hotkey.mods, GlobalHotKeyModifiers::SUPER);
        assert_eq!(hotkey.key, Code::KeyY);
    }
}
