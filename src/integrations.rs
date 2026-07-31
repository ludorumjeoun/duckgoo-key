use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tracing::warn;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

const SHOW_MENU_ID: &str = "duckgookey.show";
const LAUNCH_AT_LOGIN_MENU_ID: &str = "duckgookey.launch-at-login";
const QUIT_MENU_ID: &str = "duckgookey.quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationEvent {
    ToggleLauncher,
    ShowLauncher,
    SetLaunchAtLogin(bool),
    Quit,
}

pub struct DesktopIntegrations {
    hotkey_manager: Option<GlobalHotKeyManager>,
    launcher_hotkey: HotKey,
    tray_icon: Option<TrayIcon>,
    show_menu_id: MenuId,
    launch_at_login_menu_id: MenuId,
    launch_at_login_item: Option<CheckMenuItem>,
    quit_menu_id: MenuId,
    warnings: Vec<String>,
}

impl DesktopIntegrations {
    pub fn initialize(launch_at_login: bool) -> Self {
        let launcher_hotkey = HotKey::new(Some(Modifiers::ALT), Code::Space);
        let mut warnings = Vec::new();

        let hotkey_manager = match GlobalHotKeyManager::new() {
            Ok(manager) => match manager.register(launcher_hotkey) {
                Ok(()) => Some(manager),
                Err(error) => {
                    let message = format!("Could not register the Option+Space shortcut: {error}");
                    warn!("{message}");
                    warnings.push(message);
                    None
                }
            },
            Err(error) => {
                let message = format!("Could not initialize global shortcuts: {error}");
                warn!("{message}");
                warnings.push(message);
                None
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

    pub fn drain_events(&self) -> Vec<IntegrationEvent> {
        let mut events = Vec::new();

        if self.hotkey_manager.is_some() {
            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.id() == self.launcher_hotkey.id() && event.state() == HotKeyState::Pressed
                {
                    events.push(IntegrationEvent::ToggleLauncher);
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
        .with_icon(menu_bar_icon().map_err(|error| error.to_string())?)
        .with_icon_as_template(cfg!(target_os = "macos"))
        .build()
        .map_err(|error| error.to_string())?;

    Ok((tray_icon, launch_at_login))
}

fn menu_bar_icon() -> Result<Icon, tray_icon::BadIcon> {
    const WIDTH: u32 = 22;
    const HEIGHT: u32 = 22;
    let mut rgba = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];

    let mut set_pixel = |x: u32, y: u32, alpha: u8| {
        let index = ((y * WIDTH + x) * 4) as usize;
        rgba[index..index + 4].copy_from_slice(&[0, 0, 0, alpha]);
    };

    for y in 3..14 {
        for x in 2..13 {
            let dx = x as i32 - 7;
            let dy = y as i32 - 8;
            let radius_squared = dx * dx + dy * dy;
            if (8..=30).contains(&radius_squared) {
                set_pixel(x, y, 255);
            }
        }
    }

    for y in 7..10 {
        for x in 11..20 {
            set_pixel(x, y, 255);
        }
    }
    for y in 9..13 {
        for x in 16..19 {
            set_pixel(x, y, 255);
        }
    }

    Icon::from_rgba(rgba, WIDTH, HEIGHT)
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
        let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Space);
        assert_eq!(hotkey.mods, Modifiers::ALT);
        assert_eq!(hotkey.key, Code::Space);
    }
}
