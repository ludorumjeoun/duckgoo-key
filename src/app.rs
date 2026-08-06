use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::event;
use iced::keyboard::{self, key::Named};
use iced::mouse;
use iced::theme;
use iced::widget::{
    self, button, container, image, pick_list, scrollable, text, text_input, toggler,
};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Event, Fill, Shadow, Size,
    Subscription, Task, Theme, Vector, time, window,
};

use crate::app_icon;
use crate::calculator;
use crate::catalog::{
    Catalog, CatalogItem, LaunchAction, SearchMode, SearchResult, UsageRecord, current_unix_time_ms,
};
use crate::commands::{self, SystemCommand};
use crate::integrations::{DesktopIntegrations, IntegrationEvent};
use crate::platform;
use crate::quick_link::QuickLink;
use crate::shortcut::ShortcutBinding;
use crate::store::{Store, StoreData};
use crate::telemetry;
use crate::updater;
use crate::web_search;

const RESULT_LIMIT: usize = 6;
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const AUTO_DISCOVERY_INTERVAL: Duration = Duration::from_secs(15);
const FILE_SEARCH_TICK_INTERVAL: Duration = Duration::from_millis(100);
const FILE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(220);
const CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(750);
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const FILE_RESULT_LIMIT: usize = 50;
const ROOT_FILE_RESULT_LIMIT: usize = 2;

const TEXT_PRIMARY: Color = Color {
    r: 0.95,
    g: 0.96,
    b: 0.98,
    a: 1.0,
};
const TEXT_SECONDARY: Color = Color {
    r: 0.60,
    g: 0.64,
    b: 0.72,
    a: 1.0,
};
const ACCENT: Color = Color {
    r: 0.40,
    g: 0.78,
    b: 0.67,
    a: 1.0,
};
const DANGER: Color = Color {
    r: 0.96,
    g: 0.48,
    b: 0.48,
    a: 1.0,
};

pub fn run() -> iced::Result {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    iced::daemon(boot, update, view)
        .title("DuckGooKey")
        .theme(app_theme)
        .style(app_style)
        .subscription(subscription)
        .run()
}

fn app_theme(_state: &Launcher, _window: window::Id) -> Theme {
    Theme::Dark
}

fn app_style(_state: &Launcher, _theme: &Theme) -> theme::Style {
    theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: TEXT_PRIMARY,
    }
}

struct Launcher {
    launcher_window: window::Id,
    query_input: widget::Id,
    quick_link_title_input: widget::Id,
    quick_link_url_input: widget::Id,
    page: Page,
    search_mode: Option<SearchMode>,
    query: String,
    calculator_result: Option<CatalogItem>,
    web_search_result: Option<CatalogItem>,
    selected: usize,
    visible: bool,
    window_focused: bool,
    loading: bool,
    launching: bool,
    launch_at_login: bool,
    launch_at_login_pending: bool,
    capturing_shortcut: bool,
    input_source_picker_open: bool,
    input_sources: Vec<platform::InputSource>,
    input_source_error: Option<String>,
    input_source_to_restore: Option<String>,
    file_search_revision: u64,
    file_search_pending: bool,
    file_search_in_progress: bool,
    file_query_changed_at: Instant,
    file_results: Vec<CatalogItem>,
    file_search_error: Option<String>,
    quick_look_path: Option<PathBuf>,
    clipboard_change_count: Option<i64>,
    clipboard_error: Option<String>,
    update_checking: bool,
    update_installing: bool,
    available_update: Option<updater::AvailableUpdate>,
    quick_link_title: String,
    quick_link_url: String,
    editing_quick_link_id: Option<u64>,
    quick_links_return_page: Page,
    pending_confirmation: Option<PendingConfirmation>,
    confirmation_return_page: Page,
    catalog: Catalog,
    brand_icon: image::Handle,
    icon_handles: HashMap<String, image::Handle>,
    catalog_revision: u64,
    store_data: StoreData,
    store: Option<Store>,
    integrations: Option<DesktopIntegrations>,
    notice: Option<Notice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Launcher,
    Settings,
    QuickLinks,
    Confirmation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingConfirmation {
    SystemCommand {
        item_id: String,
        command: SystemCommand,
    },
    DeleteQuickLink {
        id: u64,
        title: String,
    },
    ClearClipboardHistory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogScanSource {
    Startup,
    Manual,
    Automatic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InputSourceChoice {
    identifier: Option<String>,
    label: String,
}

impl InputSourceChoice {
    fn keep_current() -> Self {
        Self {
            identifier: None,
            label: "Keep current input source".to_owned(),
        }
    }

    fn available(source: &platform::InputSource) -> Self {
        Self {
            identifier: Some(source.identifier.clone()),
            label: source.localized_name.clone(),
        }
    }

    fn unavailable(identifier: &str) -> Self {
        Self {
            identifier: Some(identifier.to_owned()),
            label: format!("Unavailable · {identifier}"),
        }
    }
}

impl std::fmt::Display for InputSourceChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

#[derive(Debug, Clone)]
enum Message {
    WindowOpened(window::Id),
    WindowFocused(window::Id),
    WindowUnfocused(window::Id),
    WindowCloseRequested(window::Id),
    QueryChanged(String),
    CheckQueryFocus,
    QueryFocusChanged(bool),
    KeyPressed {
        window: window::Id,
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        status: event::Status,
    },
    ActivateItem(String),
    TogglePinned(String),
    ExitSearchMode,
    OpenSettings,
    CloseSettings,
    OpenQuickLinks,
    CloseQuickLinks,
    QuickLinkTitleChanged(String),
    QuickLinkUrlChanged(String),
    SaveQuickLink,
    EditQuickLink(u64),
    RequestDeleteQuickLink(u64),
    CancelQuickLinkEdit,
    SetClipboardHistoryEnabled(bool),
    SetAnonymousUsageStatsEnabled(bool),
    SetUpdateChecksEnabled(bool),
    CheckForUpdates,
    UpdateCheckTick,
    UpdateCheckFinished {
        manual: bool,
        result: Result<updater::CheckResult, String>,
    },
    InstallAvailableUpdate,
    UpdateInstallPrepared(Result<(), String>),
    RequestClearClipboardHistory,
    ConfirmPending,
    CancelConfirmation,
    BeginShortcutCapture,
    ResetShortcut,
    InputSourcePickerOpened,
    InputSourcePickerClosed,
    SetPreferredInputSource(InputSourceChoice),
    SetLaunchAtLogin(bool),
    CatalogScanTick,
    FileSearchTick,
    FileSearchFinished {
        revision: u64,
        result: Result<Vec<PathBuf>, String>,
    },
    FileIconsLoaded {
        revision: u64,
        handles: HashMap<String, image::Handle>,
    },
    ClipboardPollTick,
    CatalogLoaded {
        source: CatalogScanSource,
        result: Result<Vec<CatalogItem>, String>,
    },
    IconsLoaded {
        revision: u64,
        handles: HashMap<String, image::Handle>,
    },
    ActionFinished {
        item_id: Option<String>,
        action_label: String,
        result: Result<(), String>,
    },
    RevealFinished {
        title: String,
        result: Result<(), String>,
    },
    LaunchAtLoginFinished {
        enabled: bool,
        result: Result<(), String>,
    },
    TelemetryFinished {
        event: telemetry::Event,
        result: Result<(), String>,
    },
    PollNativeEvents,
}

#[derive(Clone)]
struct Notice {
    text: String,
    error: bool,
}

impl Notice {
    fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
        }
    }
}

fn boot() -> (Launcher, Task<Message>) {
    let query_input = widget::Id::unique();
    let quick_link_title_input = widget::Id::unique();
    let quick_link_url_input = widget::Id::unique();
    let (store, mut store_data, mut notice) = load_store();
    let mut telemetry_events = schedule_startup_telemetry(&mut store_data);
    if !telemetry_events.is_empty() {
        let saved = store
            .as_ref()
            .is_some_and(|store| store.save(&store_data).is_ok());
        if !saved {
            telemetry_events.clear();
            if notice.is_none() {
                notice = Some(Notice::error(
                    "Anonymous usage statistics could not be prepared because local settings are unavailable",
                ));
            }
        }
    }
    let update_checks_enabled = store_data.settings.update_checks_enabled;
    let (input_sources, input_source_error) = load_input_sources();
    let (clipboard_change_count, clipboard_error) = if store_data.settings.clipboard_history_enabled
    {
        match platform::clipboard_change_count() {
            Ok(change_count) => (Some(change_count), None),
            Err(error) => (None, Some(error.to_string())),
        }
    } else {
        (None, None)
    };
    let catalog = Catalog::new(base_catalog_items(&store_data, Vec::new()));
    let launch_at_login = platform::launch_at_login_enabled();
    let starts_hidden = std::env::args_os()
        .skip(1)
        .any(|argument| argument == std::ffi::OsStr::new(crate::START_HIDDEN_ARGUMENT));
    let mut window_settings = window::Settings {
        size: Size::new(720.0, 520.0),
        position: window::Position::Centered,
        visible: !starts_hidden,
        resizable: false,
        closeable: true,
        minimizable: false,
        decorations: false,
        transparent: true,
        blur: false,
        level: window::Level::AlwaysOnTop,
        exit_on_close_request: false,
        ..window::Settings::default()
    };

    #[cfg(target_os = "macos")]
    {
        window_settings.platform_specific.title_hidden = true;
        window_settings.platform_specific.titlebar_transparent = true;
        window_settings.platform_specific.fullsize_content_view = true;
    }

    let (launcher_window, opened) = window::open(window_settings);
    let state = Launcher {
        launcher_window,
        query_input,
        quick_link_title_input,
        quick_link_url_input,
        page: Page::Launcher,
        search_mode: None,
        query: String::new(),
        calculator_result: None,
        web_search_result: None,
        selected: 0,
        visible: !starts_hidden,
        window_focused: false,
        loading: true,
        launching: false,
        launch_at_login,
        launch_at_login_pending: false,
        capturing_shortcut: false,
        input_source_picker_open: false,
        input_sources,
        input_source_error,
        input_source_to_restore: None,
        file_search_revision: 0,
        file_search_pending: false,
        file_search_in_progress: false,
        file_query_changed_at: Instant::now(),
        file_results: Vec::new(),
        file_search_error: None,
        quick_look_path: None,
        clipboard_change_count,
        clipboard_error,
        update_checking: update_checks_enabled,
        update_installing: false,
        available_update: None,
        quick_link_title: String::new(),
        quick_link_url: String::new(),
        editing_quick_link_id: None,
        quick_links_return_page: Page::Settings,
        pending_confirmation: None,
        confirmation_return_page: Page::Launcher,
        catalog,
        brand_icon: image::Handle::from_bytes(
            include_bytes!("../assets/icons/duckgoo-key-128.png").to_vec(),
        ),
        icon_handles: HashMap::new(),
        catalog_revision: 0,
        store_data,
        store,
        integrations: None,
        notice,
    };

    (
        state,
        Task::batch([
            opened.map(Message::WindowOpened),
            discover_catalog(CatalogScanSource::Startup),
            if update_checks_enabled {
                check_for_updates(false)
            } else {
                Task::none()
            },
            telemetry_tasks(telemetry_events),
        ]),
    )
}

fn load_input_sources() -> (Vec<platform::InputSource>, Option<String>) {
    match platform::available_input_sources() {
        Ok(input_sources) => (input_sources, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    }
}

fn load_store() -> (Option<Store>, StoreData, Option<Notice>) {
    let store = match Store::open_default() {
        Ok(store) => store,
        Err(error) => {
            return (
                None,
                StoreData::default(),
                Some(Notice::error(format!("History is unavailable: {error}"))),
            );
        }
    };

    match store.load() {
        Ok(outcome) => {
            let notice = outcome.recovered_corrupt_file.map(|path| {
                Notice::error(format!(
                    "Recovered damaged history; original saved at {}",
                    path.display()
                ))
            });
            (Some(store), outcome.data, notice)
        }
        Err(error) => (
            None,
            StoreData::default(),
            Some(Notice::error(format!(
                "Settings and history could not be loaded: {error}"
            ))),
        ),
    }
}

fn update(state: &mut Launcher, message: Message) -> Task<Message> {
    match message {
        Message::WindowOpened(window) if window == state.launcher_window => {
            state.window_focused = false;
            let integrations = DesktopIntegrations::initialize(
                state.launch_at_login,
                state.store_data.settings.shortcut,
            );
            if !integrations.warnings().is_empty() {
                state.notice = Some(Notice::error(integrations.warnings().join(" ")));
            }
            state.integrations = Some(integrations);

            if state.visible {
                focus_search_input(state)
            } else {
                Task::none()
            }
        }
        Message::WindowOpened(_) => Task::none(),
        Message::WindowFocused(window) if window == state.launcher_window => {
            state.window_focused = true;
            if state.visible && state.page == Page::Launcher {
                activate_search_input_source(state);
            }
            Task::none()
        }
        Message::WindowFocused(_) => Task::none(),
        Message::WindowUnfocused(window) if window == state.launcher_window => {
            let quick_look_open = quick_look_is_open(state);
            let should_hide =
                state.window_focused && state.visible && !state.launching && !quick_look_open;
            if state.visible {
                restore_input_source(state);
            }
            state.window_focused = false;
            if should_hide {
                hide_launcher(state)
            } else {
                Task::none()
            }
        }
        Message::WindowUnfocused(_) => Task::none(),
        Message::WindowCloseRequested(window) if window == state.launcher_window => {
            hide_launcher(state)
        }
        Message::WindowCloseRequested(_) => Task::none(),
        Message::QueryChanged(query) => {
            close_quick_look(state);
            state.query = query;
            state.selected = 0;
            refresh_dynamic_results(state);
            if state.search_mode.is_none() {
                reset_file_search(state);
                state.file_search_pending = state.query.trim().chars().count() >= 2;
            }
            Task::none()
        }
        Message::CheckQueryFocus => {
            widget::operation::is_focused(state.query_input.clone()).map(Message::QueryFocusChanged)
        }
        Message::QueryFocusChanged(focused) => {
            if focused && state.visible && state.page == Page::Launcher {
                activate_search_input_source(state);
            } else {
                restore_input_source(state);
            }
            Task::none()
        }
        Message::KeyPressed {
            window,
            key,
            modifiers,
            status,
        } if window == state.launcher_window && state.visible => {
            let escape = matches!(key.as_ref(), keyboard::Key::Named(Named::Escape));
            let result_copy = state.page == Page::Launcher
                && modifiers.command()
                && matches!(key.as_ref(), keyboard::Key::Character(character) if character.eq_ignore_ascii_case("c"));
            let result_quick_look = state.page == Page::Launcher
                && modifiers.command()
                && matches!(key.as_ref(), keyboard::Key::Character(character) if character.eq_ignore_ascii_case("y"));
            if status == event::Status::Captured
                && (!escape || (state.page == Page::Settings && state.input_source_picker_open))
                && !result_copy
                && !result_quick_look
            {
                Task::none()
            } else {
                handle_key(state, key, modifiers)
            }
        }
        Message::KeyPressed { .. } => Task::none(),
        Message::ActivateItem(item_id) => activate_item(state, &item_id),
        Message::TogglePinned(item_id) => {
            toggle_pinned(state, &item_id);
            Task::none()
        }
        Message::ExitSearchMode => exit_search_mode(state),
        Message::OpenSettings => open_settings(state),
        Message::CloseSettings => close_settings(state),
        Message::OpenQuickLinks => open_quick_links(state),
        Message::CloseQuickLinks => close_quick_links(state),
        Message::QuickLinkTitleChanged(title) => {
            state.quick_link_title = title;
            Task::none()
        }
        Message::QuickLinkUrlChanged(url) => {
            state.quick_link_url = url;
            Task::none()
        }
        Message::SaveQuickLink => {
            save_quick_link(state);
            Task::none()
        }
        Message::EditQuickLink(id) => edit_quick_link(state, id),
        Message::RequestDeleteQuickLink(id) => request_delete_quick_link(state, id),
        Message::CancelQuickLinkEdit => {
            reset_quick_link_form(state);
            Task::none()
        }
        Message::SetClipboardHistoryEnabled(enabled) => {
            set_clipboard_history_enabled(state, enabled);
            Task::none()
        }
        Message::SetAnonymousUsageStatsEnabled(enabled) => {
            set_anonymous_usage_stats_enabled(state, enabled)
        }
        Message::SetUpdateChecksEnabled(enabled) => {
            set_update_checks_enabled(state, enabled);
            Task::none()
        }
        Message::CheckForUpdates => {
            if state.update_checking || state.update_installing {
                Task::none()
            } else {
                state.update_checking = true;
                check_for_updates(true)
            }
        }
        Message::UpdateCheckTick => {
            if !state.store_data.settings.update_checks_enabled
                || state.update_checking
                || state.update_installing
                || state.available_update.is_some()
            {
                Task::none()
            } else {
                state.update_checking = true;
                check_for_updates(false)
            }
        }
        Message::UpdateCheckFinished { manual, result } => {
            state.update_checking = false;
            match result {
                Ok(updater::CheckResult::UpToDate) => {
                    state.available_update = None;
                    if manual {
                        state.notice = Some(Notice::info(format!(
                            "DuckGooKey {} is up to date",
                            env!("CARGO_PKG_VERSION")
                        )));
                    }
                }
                Ok(updater::CheckResult::Available(update)) => {
                    state.notice = Some(Notice::info(format!(
                        "DuckGooKey {} is ready to install",
                        update.version
                    )));
                    state.available_update = Some(update);
                }
                Err(error) => {
                    if manual {
                        state.notice = Some(Notice::error(format!(
                            "Could not check for updates: {error}"
                        )));
                    } else {
                        tracing::warn!(error = %error, "Automatic update check failed");
                    }
                }
            }
            Task::none()
        }
        Message::InstallAvailableUpdate => {
            let Some(update) = state.available_update.clone() else {
                return Task::none();
            };
            if state.update_installing {
                return Task::none();
            }
            state.update_installing = true;
            Task::perform(
                async move {
                    updater::download_and_prepare_install(update)
                        .await
                        .map_err(|error| error.to_string())
                },
                Message::UpdateInstallPrepared,
            )
        }
        Message::UpdateInstallPrepared(result) => {
            state.update_installing = false;
            match result {
                Ok(()) => iced::exit(),
                Err(error) => {
                    state.notice = Some(Notice::error(format!(
                        "Could not install the update: {error}"
                    )));
                    Task::none()
                }
            }
        }
        Message::RequestClearClipboardHistory => request_clear_clipboard_history(state),
        Message::ConfirmPending => confirm_pending(state),
        Message::CancelConfirmation => cancel_confirmation(state),
        Message::BeginShortcutCapture => {
            state.capturing_shortcut = true;
            state.notice = Some(Notice::info(
                "Press a shortcut with Command, Option, Control, or Shift",
            ));
            Task::none()
        }
        Message::ResetShortcut => {
            state.capturing_shortcut = false;
            apply_shortcut(state, ShortcutBinding::default());
            Task::none()
        }
        Message::InputSourcePickerOpened => {
            state.input_source_picker_open = true;
            Task::none()
        }
        Message::InputSourcePickerClosed => {
            state.input_source_picker_open = false;
            Task::none()
        }
        Message::SetPreferredInputSource(choice) => {
            state.input_source_picker_open = false;
            apply_input_source_preference(state, choice);
            Task::none()
        }
        Message::SetLaunchAtLogin(enabled) => {
            if state.launch_at_login_pending || enabled == state.launch_at_login {
                return Task::none();
            }
            state.launch_at_login_pending = true;
            Task::perform(
                async move { platform::set_launch_at_login(enabled).map_err(|error| error.to_string()) },
                move |result| Message::LaunchAtLoginFinished { enabled, result },
            )
        }
        Message::CatalogScanTick => {
            if state.loading {
                Task::none()
            } else {
                state.loading = true;
                discover_catalog(CatalogScanSource::Automatic)
            }
        }
        Message::FileSearchTick => maybe_start_file_search(state),
        Message::FileSearchFinished { revision, result } => {
            state.file_search_in_progress = false;
            if state.search_mode.is_none() && revision == state.file_search_revision {
                match result {
                    Ok(paths) => {
                        state.file_results = paths
                            .into_iter()
                            .filter(|path| path.exists())
                            .map(|path| {
                                let title = path
                                    .file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                                let mut item = CatalogItem::path(path, title);
                                item.pinnable = false;
                                item
                            })
                            .collect();
                        state.file_search_error = None;
                        state.selected =
                            state.selected.min(state.results().len().saturating_sub(1));
                        return load_file_icons(revision, state.file_results.clone());
                    }
                    Err(error) => {
                        state.file_results.clear();
                        state.file_search_error = Some(error);
                    }
                }
            }
            Task::none()
        }
        Message::FileIconsLoaded { revision, handles } => {
            if state.search_mode.is_none() && revision == state.file_search_revision {
                state.icon_handles.extend(handles);
            }
            Task::none()
        }
        Message::ClipboardPollTick => {
            poll_clipboard_history(state);
            Task::none()
        }
        Message::CatalogLoaded { source, result } => {
            state.loading = false;
            match result {
                Ok(applications) => {
                    let application_count = applications.len();
                    let changed = state.application_items() != applications.as_slice();
                    if changed || source != CatalogScanSource::Automatic {
                        rebuild_catalog(state, applications.iter().cloned());
                        state.selected =
                            state.selected.min(state.results().len().saturating_sub(1));
                        state.catalog_revision = state.catalog_revision.wrapping_add(1);
                        let revision = state.catalog_revision;

                        match source {
                            CatalogScanSource::Startup if state.notice.is_none() => {
                                state.notice = Some(Notice::info(format!(
                                    "{application_count} applications ready"
                                )));
                            }
                            CatalogScanSource::Manual => {
                                state.notice = Some(Notice::info(format!(
                                    "Refreshed {application_count} applications"
                                )));
                            }
                            CatalogScanSource::Automatic if changed => {
                                state.notice = Some(Notice::info(format!(
                                    "Application list updated · {application_count} available"
                                )));
                            }
                            CatalogScanSource::Startup | CatalogScanSource::Automatic => {}
                        }

                        return load_icons(revision, applications);
                    }
                }
                Err(error) => {
                    if source == CatalogScanSource::Automatic {
                        tracing::warn!(error = %error, "Automatic application discovery failed");
                    } else {
                        state.notice = Some(Notice::error(format!(
                            "Applications could not be indexed: {error}"
                        )));
                    }
                }
            }
            Task::none()
        }
        Message::IconsLoaded { revision, handles } => {
            if revision == state.catalog_revision {
                state.icon_handles = handles;
            }
            Task::none()
        }
        Message::ActionFinished {
            item_id,
            action_label,
            result,
        } => {
            state.launching = false;
            match result {
                Ok(()) => {
                    if let Some(item_id) = item_id {
                        record_launch(state, &item_id);
                    }
                    hide_launcher(state)
                }
                Err(error) => {
                    state.notice =
                        Some(Notice::error(format!("Could not {action_label}: {error}")));
                    focus_search_input(state)
                }
            }
        }
        Message::RevealFinished { title, result } => {
            state.launching = false;
            match result {
                Ok(()) => hide_launcher(state),
                Err(error) => {
                    state.notice = Some(Notice::error(format!(
                        "Could not show {title} in Finder: {error}"
                    )));
                    focus_search_input(state)
                }
            }
        }
        Message::LaunchAtLoginFinished { enabled, result } => {
            state.launch_at_login_pending = false;
            match result {
                Ok(()) => {
                    state.launch_at_login = enabled;
                    if let Some(integrations) = &state.integrations {
                        integrations.set_launch_at_login_checked(enabled);
                    }
                    state.notice = Some(Notice::info(if enabled {
                        "DuckGooKey will launch when you sign in"
                    } else {
                        "Launch at login disabled"
                    }));
                }
                Err(error) => {
                    if let Some(integrations) = &state.integrations {
                        integrations.set_launch_at_login_checked(state.launch_at_login);
                    }
                    state.notice = Some(Notice::error(format!(
                        "Launch at login could not be changed: {error}"
                    )));
                }
            }
            Task::none()
        }
        Message::TelemetryFinished { event, result } => {
            if let Err(error) = result {
                tracing::debug!(event = event.as_str(), error = %error, "Anonymous usage event was not sent");
            }
            Task::none()
        }
        Message::PollNativeEvents => poll_native_events(state),
    }
}

fn handle_key(
    state: &mut Launcher,
    key: keyboard::Key,
    modifiers: keyboard::Modifiers,
) -> Task<Message> {
    match state.page {
        Page::Settings => {
            if state.input_source_picker_open {
                return if matches!(key.as_ref(), keyboard::Key::Named(Named::Escape)) {
                    state.input_source_picker_open = false;
                    Task::none()
                } else {
                    Task::none()
                };
            }

            if state.capturing_shortcut {
                if matches!(key.as_ref(), keyboard::Key::Named(Named::Escape)) {
                    state.capturing_shortcut = false;
                    state.notice = Some(Notice::info("Shortcut change cancelled"));
                    return Task::none();
                }

                match ShortcutBinding::try_from_iced(&key, modifiers) {
                    Ok(shortcut) => {
                        state.capturing_shortcut = false;
                        apply_shortcut(state, shortcut);
                    }
                    Err(error) => {
                        state.notice = Some(Notice::error(error.to_string()));
                    }
                }
                return Task::none();
            }

            return match key.as_ref() {
                keyboard::Key::Named(Named::Tab) if modifiers.shift() => {
                    widget::operation::focus_previous()
                }
                keyboard::Key::Named(Named::Tab) => widget::operation::focus_next(),
                keyboard::Key::Named(Named::Escape) => close_settings(state),
                _ => Task::none(),
            };
        }
        Page::QuickLinks => {
            return match key.as_ref() {
                keyboard::Key::Named(Named::Enter) if modifiers.command() => {
                    save_quick_link(state);
                    Task::none()
                }
                keyboard::Key::Named(Named::Tab) if modifiers.shift() => {
                    widget::operation::focus_previous()
                }
                keyboard::Key::Named(Named::Tab) => widget::operation::focus_next(),
                keyboard::Key::Named(Named::Escape) => close_quick_links(state),
                _ => Task::none(),
            };
        }
        Page::Confirmation => {
            return match key.as_ref() {
                keyboard::Key::Named(Named::Enter) => confirm_pending(state),
                keyboard::Key::Named(Named::Tab) if modifiers.shift() => {
                    widget::operation::focus_previous()
                }
                keyboard::Key::Named(Named::Tab) => widget::operation::focus_next(),
                keyboard::Key::Named(Named::Escape) => cancel_confirmation(state),
                _ => Task::none(),
            };
        }
        Page::Launcher => {}
    }

    match key.as_ref() {
        keyboard::Key::Named(Named::Tab) => {
            widget::operation::is_focused(state.query_input.clone()).map(Message::QueryFocusChanged)
        }
        keyboard::Key::Named(Named::ArrowDown) => {
            let result_count = state.results().len();
            if result_count > 0 {
                state.selected = (state.selected + 1).min(result_count - 1);
                sync_quick_look_to_selection(state);
            }
            Task::none()
        }
        keyboard::Key::Named(Named::ArrowUp) => {
            state.selected = state.selected.saturating_sub(1);
            sync_quick_look_to_selection(state);
            Task::none()
        }
        keyboard::Key::Named(Named::Enter) if modifiers.command() => reveal_selected_path(state),
        keyboard::Key::Named(Named::Enter) => activate_selected(state),
        keyboard::Key::Character(character)
            if modifiers.command() && character.eq_ignore_ascii_case("y") =>
        {
            toggle_selected_quick_look(state);
            Task::none()
        }
        keyboard::Key::Character(character)
            if modifiers.command()
                && character.eq_ignore_ascii_case("d")
                && state.search_mode == Some(SearchMode::Clipboard) =>
        {
            delete_selected_clipboard_entry(state);
            Task::none()
        }
        keyboard::Key::Named(Named::Escape) => {
            if quick_look_is_open(state) {
                close_quick_look(state);
                Task::none()
            } else if state.search_mode.is_some() {
                exit_search_mode(state)
            } else {
                hide_launcher(state)
            }
        }
        keyboard::Key::Character(character)
            if modifiers.command() && character.eq_ignore_ascii_case("c") =>
        {
            copy_selected(state)
        }
        keyboard::Key::Character(character)
            if modifiers.command() && character.eq_ignore_ascii_case("p") =>
        {
            if let Some(result) = state.results().get(state.selected)
                && result.item.pinnable
            {
                let item_id = result.item.id.clone();
                toggle_pinned(state, &item_id);
            }
            Task::none()
        }
        _ => Task::none(),
    }
}

fn activate_selected(state: &mut Launcher) -> Task<Message> {
    close_quick_look(state);
    let results = state.results();
    let Some(result) = results.get(state.selected) else {
        return Task::none();
    };
    let item_id = result.item.id.clone();
    activate_item(state, &item_id)
}

fn toggle_selected_quick_look(state: &mut Launcher) {
    let Some(path) = selected_file_or_folder_path(state) else {
        state.notice = Some(Notice::info(
            "Quick Look is available for files and folders",
        ));
        return;
    };

    if state.quick_look_path.as_ref() == Some(&path) && quick_look_is_open(state) {
        close_quick_look(state);
        return;
    }

    show_quick_look(state, path);
}

fn sync_quick_look_to_selection(state: &mut Launcher) {
    if !quick_look_is_open(state) {
        return;
    }
    let Some(path) = selected_file_or_folder_path(state) else {
        close_quick_look(state);
        return;
    };
    if state.quick_look_path.as_ref() != Some(&path) {
        show_quick_look(state, path);
    }
}

fn selected_file_or_folder_path(state: &Launcher) -> Option<PathBuf> {
    let item = state.results().get(state.selected)?.item.clone();
    match item.action {
        LaunchAction::OpenPath { path } => Some(path),
        _ => None,
    }
}

fn show_quick_look(state: &mut Launcher, path: PathBuf) {
    match platform::show_quick_look(&path) {
        Ok(()) => state.quick_look_path = Some(path),
        Err(error) => {
            state.quick_look_path = None;
            state.notice = Some(Notice::error(format!("Could not open Quick Look: {error}")));
        }
    }
}

fn close_quick_look(state: &mut Launcher) {
    if state.quick_look_path.take().is_some()
        && let Err(error) = platform::close_quick_look()
    {
        tracing::debug!(error = %error, "Could not close Quick Look");
    }
}

fn quick_look_is_open(state: &mut Launcher) -> bool {
    if state.quick_look_path.is_none() {
        return false;
    }
    match platform::quick_look_is_open() {
        Ok(true) => true,
        Ok(false) => {
            state.quick_look_path = None;
            false
        }
        Err(error) => {
            tracing::debug!(error = %error, "Could not check Quick Look status");
            state.quick_look_path = None;
            false
        }
    }
}

fn reveal_selected_path(state: &mut Launcher) -> Task<Message> {
    if state.launching {
        return Task::none();
    }
    close_quick_look(state);

    let Some(item) = state
        .results()
        .get(state.selected)
        .map(|result| result.item.clone())
    else {
        return Task::none();
    };
    let Some(path) = item.action.target().map(PathBuf::from) else {
        state.notice = Some(Notice::info(
            "Only applications, files, and folders can be shown in Finder",
        ));
        return Task::none();
    };

    restore_input_source(state);
    state.launching = true;
    let title = item.title;
    state.notice = Some(Notice::info(format!("Showing {title} in Finder…")));
    Task::perform(
        async move { platform::reveal_in_file_manager(&path).map_err(|error| error.to_string()) },
        move |result| Message::RevealFinished { title, result },
    )
}

fn copy_selected(state: &mut Launcher) -> Task<Message> {
    if state.launching {
        return Task::none();
    }
    close_quick_look(state);

    let Some(item) = state
        .results()
        .get(state.selected)
        .map(|result| result.item.clone())
    else {
        return Task::none();
    };
    let Some(payload) = item
        .action
        .copy_payload()
        .map(|payload| payload.into_owned())
    else {
        state.notice = Some(Notice::info("This item has nothing to copy"));
        return Task::none();
    };
    let label = match item.action {
        LaunchAction::OpenApplication { .. } | LaunchAction::OpenPath { .. } => "Copied path",
        LaunchAction::OpenUrl { .. } => "Copied URL",
        LaunchAction::CopyText { .. } => "Copied to clipboard",
        _ => "Copied",
    };
    copy_text_and_hide(state, &payload, label)
}

fn copy_text_and_hide(state: &mut Launcher, value: &str, label: &str) -> Task<Message> {
    match platform::copy_text(value) {
        Ok(change_count) => {
            state.clipboard_change_count = Some(change_count);
            state.clipboard_error = None;
            state.notice = Some(Notice::info(label));
            hide_launcher(state)
        }
        Err(error) => {
            state.notice = Some(Notice::error(format!(
                "Could not copy to the clipboard: {error}"
            )));
            focus_search_input(state)
        }
    }
}

fn delete_selected_clipboard_entry(state: &mut Launcher) {
    let Some(item_id) = state
        .results()
        .get(state.selected)
        .map(|result| result.item.id.clone())
    else {
        return;
    };
    let Some(id) = item_id
        .strip_prefix("clipboard:")
        .and_then(|id| id.parse::<u64>().ok())
    else {
        return;
    };
    let mut candidate = state.store_data.clone();
    if candidate.clipboard_history.delete(id) && save_candidate(state, &candidate).is_ok() {
        state.store_data = candidate;
        state.selected = state.selected.min(state.results().len().saturating_sub(1));
        state.notice = Some(Notice::info("Clipboard entry removed"));
    }
}

fn activate_item(state: &mut Launcher, item_id: &str) -> Task<Message> {
    if state.launching {
        return Task::none();
    }

    let Some(item) = state
        .results()
        .into_iter()
        .find(|result| result.item.id == item_id)
        .map(|result| result.item)
    else {
        return Task::none();
    };

    match item.action {
        LaunchAction::RefreshCatalog => {
            if state.loading {
                return Task::none();
            }
            state.loading = true;
            state.notice = Some(Notice::info("Refreshing applications…"));
            discover_catalog(CatalogScanSource::Manual)
        }
        LaunchAction::Quit => quit_launcher(state),
        LaunchAction::ManageQuickLinks => {
            record_launch(state, &item.id);
            open_quick_links(state)
        }
        LaunchAction::EnterSearchMode { mode } => {
            record_launch(state, &item.id);
            enter_search_mode(state, mode)
        }
        LaunchAction::CopyText { text } => copy_text_and_hide(state, &text, "Copied to clipboard"),
        LaunchAction::SystemCommand { command } => {
            if command.requires_confirmation() {
                state.pending_confirmation = Some(PendingConfirmation::SystemCommand {
                    item_id: item.id,
                    command,
                });
                state.confirmation_return_page = Page::Launcher;
                state.page = Page::Confirmation;
                restore_input_source(state);
                Task::none()
            } else {
                execute_system_command(state, item.id, command)
            }
        }
        action @ (LaunchAction::OpenApplication { .. }
        | LaunchAction::OpenPath { .. }
        | LaunchAction::OpenUrl { .. }) => {
            restore_input_source(state);
            state.launching = true;
            state.notice = Some(Notice::info(format!("Opening {}…", item.title)));
            let item_id = item.pinnable.then_some(item.id);
            Task::perform(
                async move { platform::launch(&action).map_err(|error| error.to_string()) },
                move |result| Message::ActionFinished {
                    item_id,
                    action_label: "open item".to_owned(),
                    result,
                },
            )
        }
    }
}

fn execute_system_command(
    state: &mut Launcher,
    item_id: String,
    command: SystemCommand,
) -> Task<Message> {
    restore_input_source(state);
    state.page = Page::Launcher;
    state.launching = true;
    state.notice = Some(Notice::info(format!("Running {}…", command.title())));
    Task::perform(
        async move { platform::execute_system_command(&command).map_err(|error| error.to_string()) },
        move |result| Message::ActionFinished {
            item_id: Some(item_id),
            action_label: format!("run {}", command.title()),
            result,
        },
    )
}

fn base_catalog_items(
    store_data: &StoreData,
    applications: impl IntoIterator<Item = CatalogItem>,
) -> Vec<CatalogItem> {
    CatalogItem::built_in_items()
        .into_iter()
        .chain(commands::system_command_items())
        .chain(
            store_data
                .quick_links
                .iter()
                .map(QuickLink::to_catalog_item),
        )
        .chain(applications)
        .collect()
}

fn rebuild_catalog(state: &mut Launcher, applications: impl IntoIterator<Item = CatalogItem>) {
    state
        .catalog
        .replace(base_catalog_items(&state.store_data, applications));
}

fn discover_catalog(source: CatalogScanSource) -> Task<Message> {
    Task::perform(
        async { platform::discover_applications().map_err(|error| error.to_string()) },
        move |result| Message::CatalogLoaded { source, result },
    )
}

fn load_icons(revision: u64, applications: Vec<CatalogItem>) -> Task<Message> {
    Task::perform(
        async move {
            applications
                .into_iter()
                .filter_map(|item| {
                    let path = item.icon_path.as_deref()?;
                    match app_icon::load_icns(path) {
                        Ok(handle) => Some((item.id, handle)),
                        Err(error) => {
                            tracing::debug!(
                                path = %path.display(),
                                error = %error,
                                "Could not decode an application icon"
                            );
                            None
                        }
                    }
                })
                .collect()
        },
        move |handles| Message::IconsLoaded { revision, handles },
    )
}

fn load_file_icons(revision: u64, files: Vec<CatalogItem>) -> Task<Message> {
    Task::perform(
        async move {
            files
                .into_iter()
                .filter_map(|item| {
                    let path = item.action.target()?;
                    let png = platform::file_icon_png(path)?;
                    Some((item.id, image::Handle::from_bytes(png)))
                })
                .collect()
        },
        move |handles| Message::FileIconsLoaded { revision, handles },
    )
}

fn toggle_pinned(state: &mut Launcher, item_id: &str) {
    let record = usage_for_mut(&mut state.store_data.usage, item_id);
    record.pinned = !record.pinned;
    let pinned = record.pinned;
    state.notice = Some(Notice::info(if pinned {
        "Pinned to the top"
    } else {
        "Removed from pinned items"
    }));
    let _ = save_state(state);
}

fn record_launch(state: &mut Launcher, item_id: &str) {
    usage_for_mut(&mut state.store_data.usage, item_id).record_launch(current_unix_time_ms());
    let _ = save_state(state);
}

fn usage_for_mut<'a>(usage: &'a mut Vec<UsageRecord>, item_id: &str) -> &'a mut UsageRecord {
    if let Some(index) = usage.iter().position(|record| record.item_id == item_id) {
        return &mut usage[index];
    }

    usage.push(UsageRecord::new(item_id));
    usage
        .last_mut()
        .expect("a usage record was inserted immediately before access")
}

fn save_state(state: &mut Launcher) -> Result<(), String> {
    let result = persist_store_data(state.store.as_ref(), &state.store_data);
    report_persistence_error(state, result)
}

fn save_candidate(state: &mut Launcher, candidate: &StoreData) -> Result<(), String> {
    let result = persist_store_data(state.store.as_ref(), candidate);
    report_persistence_error(state, result)
}

fn persist_store_data(store: Option<&Store>, data: &StoreData) -> Result<(), String> {
    let store = store.ok_or_else(|| "Local settings storage is unavailable".to_owned())?;
    store.save(data).map_err(|error| error.to_string())
}

fn report_persistence_error(
    state: &mut Launcher,
    result: Result<(), String>,
) -> Result<(), String> {
    result.map_err(|error| {
        let message = format!("Settings and history could not be saved: {error}");
        state.notice = Some(Notice::error(message.clone()));
        message
    })
}

fn open_settings(state: &mut Launcher) -> Task<Message> {
    close_quick_look(state);
    restore_input_source(state);
    state.page = Page::Settings;
    state.search_mode = None;
    state.capturing_shortcut = false;
    state.input_source_picker_open = false;
    state.query.clear();
    clear_dynamic_results(state);
    reset_file_search(state);
    state.selected = 0;
    refresh_input_sources(state);
    Task::none()
}

fn open_quick_links(state: &mut Launcher) -> Task<Message> {
    close_quick_look(state);
    restore_input_source(state);
    state.quick_links_return_page = state.page;
    state.page = Page::QuickLinks;
    state.search_mode = None;
    state.query.clear();
    clear_dynamic_results(state);
    reset_file_search(state);
    state.selected = 0;
    reset_quick_link_form(state);
    widget::operation::focus(state.quick_link_title_input.clone())
}

fn close_quick_links(state: &mut Launcher) -> Task<Message> {
    let return_page = state.quick_links_return_page;
    reset_quick_link_form(state);
    state.page = return_page;
    if return_page == Page::Launcher {
        focus_search_input(state)
    } else {
        Task::none()
    }
}

fn reset_quick_link_form(state: &mut Launcher) {
    state.quick_link_title.clear();
    state.quick_link_url.clear();
    state.editing_quick_link_id = None;
}

fn save_quick_link(state: &mut Launcher) {
    let editing = state.editing_quick_link_id.is_some();
    let mut candidate = state.store_data.clone();
    let id = state
        .editing_quick_link_id
        .unwrap_or(candidate.next_quick_link_id.max(1));
    let quick_link = match QuickLink::new(id, &state.quick_link_title, &state.quick_link_url) {
        Ok(quick_link) => quick_link,
        Err(error) => {
            state.notice = Some(Notice::error(error.to_string()));
            return;
        }
    };

    if let Some(index) = candidate
        .quick_links
        .iter()
        .position(|existing| existing.id() == id)
    {
        candidate.quick_links[index] = quick_link;
    } else if editing {
        state.notice = Some(Notice::error("The Quick Link no longer exists"));
        return;
    } else {
        let allocated_id = candidate.allocate_quick_link_id();
        debug_assert_eq!(allocated_id, id);
        candidate.quick_links.push(quick_link);
    }

    if save_candidate(state, &candidate).is_ok() {
        let applications = state.application_items();
        state.store_data = candidate;
        rebuild_catalog(state, applications);
        state.notice = Some(Notice::info(if editing {
            "Quick Link updated"
        } else {
            "Quick Link added"
        }));
        reset_quick_link_form(state);
    }
}

fn edit_quick_link(state: &mut Launcher, id: u64) -> Task<Message> {
    let Some(quick_link) = state
        .store_data
        .quick_links
        .iter()
        .find(|quick_link| quick_link.id() == id)
    else {
        return Task::none();
    };
    state.quick_link_title = quick_link.title().to_owned();
    state.quick_link_url = quick_link.url().to_owned();
    state.editing_quick_link_id = Some(id);
    widget::operation::focus(state.quick_link_title_input.clone())
}

fn request_delete_quick_link(state: &mut Launcher, id: u64) -> Task<Message> {
    let Some(quick_link) = state
        .store_data
        .quick_links
        .iter()
        .find(|quick_link| quick_link.id() == id)
    else {
        return Task::none();
    };
    state.pending_confirmation = Some(PendingConfirmation::DeleteQuickLink {
        id,
        title: quick_link.title().to_owned(),
    });
    state.confirmation_return_page = Page::QuickLinks;
    state.page = Page::Confirmation;
    Task::none()
}

fn set_clipboard_history_enabled(state: &mut Launcher, enabled: bool) {
    if state.store_data.settings.clipboard_history_enabled == enabled {
        return;
    }

    let mut candidate = state.store_data.clone();
    candidate.settings.clipboard_history_enabled = enabled;
    if enabled {
        match platform::clipboard_change_count() {
            Ok(change_count) => {
                if save_candidate(state, &candidate).is_err() {
                    return;
                }
                state.store_data = candidate;
                state.clipboard_change_count = Some(change_count);
                state.clipboard_error = None;
                state.notice = Some(Notice::info(
                    "Clipboard History enabled · new text copies stay local",
                ));
            }
            Err(error) => {
                state.clipboard_error = Some(error.to_string());
                state.notice = Some(Notice::error(format!(
                    "Clipboard History could not be enabled: {error}"
                )));
            }
        }
    } else {
        if save_candidate(state, &candidate).is_err() {
            return;
        }
        state.store_data = candidate;
        state.clipboard_change_count = None;
        state.clipboard_error = None;
        state.notice = Some(Notice::info("Clipboard History disabled"));
    }
}

fn set_update_checks_enabled(state: &mut Launcher, enabled: bool) {
    if state.store_data.settings.update_checks_enabled == enabled {
        return;
    }

    let mut candidate = state.store_data.clone();
    candidate.settings.update_checks_enabled = enabled;
    if save_candidate(state, &candidate).is_err() {
        return;
    }
    state.store_data = candidate;
    state.notice = Some(Notice::info(if enabled {
        "Automatic update checks enabled"
    } else {
        "Automatic update checks disabled"
    }));
}

fn set_anonymous_usage_stats_enabled(state: &mut Launcher, enabled: bool) -> Task<Message> {
    if state.store_data.settings.anonymous_usage_stats_enabled == enabled {
        return Task::none();
    }

    let mut candidate = state.store_data.clone();
    candidate.settings.anonymous_usage_stats_enabled = enabled;
    let events = if enabled {
        schedule_startup_telemetry(&mut candidate)
    } else {
        candidate.telemetry_installation_id = None;
        candidate.telemetry_last_active_day = None;
        Vec::new()
    };

    if save_candidate(state, &candidate).is_err() {
        return Task::none();
    }

    state.store_data = candidate;
    state.notice = Some(Notice::info(if enabled {
        "Anonymous usage statistics enabled · searches, apps, files, and clipboard content stay private"
    } else {
        "Anonymous usage statistics disabled"
    }));
    telemetry_tasks(events)
}

fn schedule_startup_telemetry(data: &mut StoreData) -> Vec<(telemetry::Event, String)> {
    if !data.settings.anonymous_usage_stats_enabled {
        return Vec::new();
    }

    let mut events = Vec::new();
    let installation_id = match data.telemetry_installation_id.as_deref() {
        Some(id) if !id.is_empty() => id.to_owned(),
        _ => {
            let id = telemetry::new_installation_id();
            data.telemetry_installation_id = Some(id.clone());
            events.push((telemetry::Event::FirstLaunch, id.clone()));
            id
        }
    };
    let day = telemetry::current_day();
    if data.telemetry_last_active_day != Some(day) {
        data.telemetry_last_active_day = Some(day);
        events.push((telemetry::Event::ActiveDaily, installation_id));
    }
    events
}

fn telemetry_tasks(events: Vec<(telemetry::Event, String)>) -> Task<Message> {
    Task::batch(events.into_iter().map(|(event, installation_id)| {
        Task::perform(
            async move {
                telemetry::send(event, installation_id)
                    .await
                    .map_err(|error| error.to_string())
            },
            move |result| Message::TelemetryFinished { event, result },
        )
    }))
}

fn check_for_updates(manual: bool) -> Task<Message> {
    Task::perform(
        async { updater::check().await.map_err(|error| error.to_string()) },
        move |result| Message::UpdateCheckFinished { manual, result },
    )
}

fn request_clear_clipboard_history(state: &mut Launcher) -> Task<Message> {
    if state.store_data.clipboard_history.is_empty() {
        return Task::none();
    }
    state.pending_confirmation = Some(PendingConfirmation::ClearClipboardHistory);
    state.confirmation_return_page = Page::Settings;
    state.page = Page::Confirmation;
    Task::none()
}

fn confirm_pending(state: &mut Launcher) -> Task<Message> {
    let Some(pending) = state.pending_confirmation.take() else {
        state.page = state.confirmation_return_page;
        return Task::none();
    };

    match pending {
        PendingConfirmation::SystemCommand { item_id, command } => {
            execute_system_command(state, item_id, command)
        }
        PendingConfirmation::DeleteQuickLink { id, .. } => {
            let mut candidate = state.store_data.clone();
            candidate
                .quick_links
                .retain(|quick_link| quick_link.id() != id);
            let item_id = format!("quick-link:{id}");
            candidate.usage.retain(|record| record.item_id != item_id);
            state.page = Page::QuickLinks;
            if save_candidate(state, &candidate).is_ok() {
                let applications = state.application_items();
                state.store_data = candidate;
                rebuild_catalog(state, applications);
                state.notice = Some(Notice::info("Quick Link deleted"));
                reset_quick_link_form(state);
            }
            Task::none()
        }
        PendingConfirmation::ClearClipboardHistory => {
            let mut candidate = state.store_data.clone();
            candidate.clipboard_history.clear();
            state.page = Page::Settings;
            if save_candidate(state, &candidate).is_ok() {
                state.store_data = candidate;
                state.notice = Some(Notice::info("Clipboard History cleared"));
            }
            Task::none()
        }
    }
}

fn cancel_confirmation(state: &mut Launcher) -> Task<Message> {
    state.pending_confirmation = None;
    state.page = state.confirmation_return_page;
    if state.page == Page::Launcher {
        focus_search_input(state)
    } else {
        Task::none()
    }
}

fn enter_search_mode(state: &mut Launcher, mode: SearchMode) -> Task<Message> {
    if mode == SearchMode::Clipboard && !state.store_data.settings.clipboard_history_enabled {
        state.notice = Some(Notice::info(
            "Enable Clipboard History in Settings before using it",
        ));
        return open_settings(state);
    }

    state.search_mode = Some(mode);
    state.query.clear();
    clear_dynamic_results(state);
    state.selected = 0;
    reset_file_search(state);
    focus_search_input(state)
}

fn exit_search_mode(state: &mut Launcher) -> Task<Message> {
    state.search_mode = None;
    state.query.clear();
    clear_dynamic_results(state);
    state.selected = 0;
    reset_file_search(state);
    focus_search_input(state)
}

fn maybe_start_file_search(state: &mut Launcher) -> Task<Message> {
    if state.search_mode.is_some()
        || !state.file_search_pending
        || state.file_search_in_progress
        || state.file_query_changed_at.elapsed() < FILE_SEARCH_DEBOUNCE
    {
        return Task::none();
    }

    let query = state.query.trim().to_owned();
    if query.chars().count() < 2 {
        state.file_search_pending = false;
        return Task::none();
    }
    let revision = state.file_search_revision;
    state.file_search_pending = false;
    state.file_search_in_progress = true;
    Task::perform(
        async move {
            platform::search_files(&query, FILE_RESULT_LIMIT).map_err(|error| error.to_string())
        },
        move |result| Message::FileSearchFinished { revision, result },
    )
}

fn reset_file_search(state: &mut Launcher) {
    state.file_search_revision = state.file_search_revision.wrapping_add(1);
    state.file_search_pending = false;
    state.file_query_changed_at = Instant::now();
    for item in &state.file_results {
        state.icon_handles.remove(&item.id);
    }
    state.file_results.clear();
    state.file_search_error = None;
}

fn poll_clipboard_history(state: &mut Launcher) {
    if !state.store_data.settings.clipboard_history_enabled {
        return;
    }

    let Some(previous) = state.clipboard_change_count else {
        match platform::clipboard_change_count() {
            Ok(change_count) => state.clipboard_change_count = Some(change_count),
            Err(error) => state.clipboard_error = Some(error.to_string()),
        }
        return;
    };

    match platform::read_clipboard_text_if_changed(previous) {
        Ok(snapshot) => {
            state.clipboard_error = None;
            let Some(text) = snapshot.text else {
                state.clipboard_change_count = Some(snapshot.change_count);
                return;
            };
            let mut candidate = state.store_data.clone();
            let id = candidate.allocate_clipboard_entry_id();
            if candidate
                .clipboard_history
                .capture(id, current_unix_time_ms(), text)
                .is_ok()
            {
                if save_candidate(state, &candidate).is_ok() {
                    state.store_data = candidate;
                    state.clipboard_change_count = Some(snapshot.change_count);
                }
            } else {
                state.clipboard_change_count = Some(snapshot.change_count);
            }
        }
        Err(error) => {
            let error = error.to_string();
            if state.clipboard_error.as_deref() != Some(&error) {
                tracing::warn!(error = %error, "Clipboard History polling failed");
            }
            state.clipboard_error = Some(error);
        }
    }
}

fn apply_shortcut(state: &mut Launcher, shortcut: ShortcutBinding) {
    let previous = state.store_data.settings.shortcut;
    let already_active = state
        .integrations
        .as_ref()
        .is_some_and(|integrations| integrations.is_shortcut_active(shortcut));
    if shortcut == previous && already_active {
        state.notice = Some(Notice::info(format!(
            "{} is already the launcher shortcut",
            shortcut
        )));
        return;
    }

    let Some(integrations) = state.integrations.as_mut() else {
        state.notice = Some(Notice::error(
            "Global shortcuts are not available in this session",
        ));
        return;
    };

    if let Err(error) = integrations.change_shortcut(shortcut) {
        state.notice = Some(Notice::error(format!("Could not use {shortcut}: {error}")));
        return;
    }

    state.store_data.settings.shortcut = shortcut;
    if shortcut == previous {
        state.notice = Some(Notice::info(format!(
            "Launcher shortcut activated as {shortcut}"
        )));
        return;
    }

    if let Err(error) = save_state(state) {
        state.store_data.settings.shortcut = previous;
        let rollback = state
            .integrations
            .as_mut()
            .and_then(|integrations| integrations.change_shortcut(previous).err());
        state.notice = Some(Notice::error(match rollback {
            Some(rollback) => {
                format!("{error}. Restoring the previous shortcut also failed: {rollback}")
            }
            None => format!("{error}. The previous shortcut was restored."),
        }));
        return;
    }

    state.notice = Some(Notice::info(format!(
        "Launcher shortcut changed to {shortcut}"
    )));
}

fn apply_input_source_preference(state: &mut Launcher, choice: InputSourceChoice) {
    let previous = state.store_data.settings.preferred_input_source.clone();
    if choice.identifier == previous {
        state.notice = Some(Notice::info(match choice.identifier {
            Some(_) => format!("{} is already used while searching", choice.label),
            None => "The current input source is already kept".to_owned(),
        }));
        return;
    }

    state.store_data.settings.preferred_input_source = choice.identifier.clone();
    if let Err(error) = save_state(state) {
        state.store_data.settings.preferred_input_source = previous;
        state.notice = Some(Notice::error(format!(
            "{error}. The previous input source preference was restored."
        )));
        return;
    }

    state.notice = Some(Notice::info(match choice.identifier {
        Some(_) => format!("{} will be used while searching", choice.label),
        None => "DuckGooKey will keep the current input source".to_owned(),
    }));
}

fn refresh_input_sources(state: &mut Launcher) {
    let (input_sources, input_source_error) = load_input_sources();
    state.input_sources = input_sources;
    state.input_source_error = input_source_error;
}

fn activate_search_input_source(state: &mut Launcher) {
    if !state.visible || state.page != Page::Launcher || state.input_source_to_restore.is_some() {
        return;
    }

    let Some(preferred) = state.store_data.settings.preferred_input_source.as_deref() else {
        return;
    };

    let current = match platform::current_input_source_identifier() {
        Ok(current) => current,
        Err(error) => {
            state.notice = Some(Notice::error(format!(
                "Could not read the current input source: {error}"
            )));
            return;
        }
    };
    if current == preferred {
        state.input_source_to_restore = Some(current);
        return;
    }

    match platform::select_input_source(preferred) {
        Ok(()) => state.input_source_to_restore = Some(current),
        Err(error) => {
            state.notice = Some(Notice::error(format!(
                "Could not switch the search input source: {error}"
            )));
        }
    }
}

fn restore_input_source(state: &mut Launcher) {
    let Some(identifier) = state.input_source_to_restore.take() else {
        return;
    };
    if let Err(error) = platform::select_input_source(&identifier) {
        state.notice = Some(Notice::error(format!(
            "Could not restore the previous input source: {error}"
        )));
    }
}

fn poll_native_events(state: &mut Launcher) -> Task<Message> {
    let events = state
        .integrations
        .as_ref()
        .map(DesktopIntegrations::drain_events)
        .unwrap_or_default();
    let tasks = events
        .into_iter()
        .map(|event| handle_integration_event(state, event))
        .collect::<Vec<_>>();
    Task::batch(tasks)
}

fn handle_integration_event(state: &mut Launcher, event: IntegrationEvent) -> Task<Message> {
    match event {
        IntegrationEvent::ToggleLauncher => {
            if state.capturing_shortcut {
                return Task::none();
            }
            if state.visible {
                hide_launcher(state)
            } else {
                show_launcher(state)
            }
        }
        IntegrationEvent::ShowLauncher => show_launcher(state),
        IntegrationEvent::SetLaunchAtLogin(enabled) => {
            update(state, Message::SetLaunchAtLogin(enabled))
        }
        IntegrationEvent::Quit => quit_launcher(state),
    }
}

fn show_launcher(state: &mut Launcher) -> Task<Message> {
    if !state.visible {
        state.page = Page::Launcher;
        state.search_mode = None;
        state.capturing_shortcut = false;
        state.input_source_picker_open = false;
        state.query.clear();
        clear_dynamic_results(state);
        reset_file_search(state);
        state.selected = 0;
    }
    state.visible = true;
    state.window_focused = false;

    focus_search_input(state)
}

fn focus_search_input(state: &mut Launcher) -> Task<Message> {
    activate_search_input_source(state);
    window::set_mode(state.launcher_window, window::Mode::Windowed)
        .chain(window::gain_focus(state.launcher_window))
        .chain(widget::operation::focus(state.query_input.clone()))
}

fn hide_launcher(state: &mut Launcher) -> Task<Message> {
    close_quick_look(state);
    restore_input_source(state);
    state.visible = false;
    state.window_focused = false;
    state.page = Page::Launcher;
    state.search_mode = None;
    state.capturing_shortcut = false;
    state.input_source_picker_open = false;
    reset_file_search(state);
    window::set_mode(state.launcher_window, window::Mode::Hidden)
}

fn close_settings(state: &mut Launcher) -> Task<Message> {
    state.page = Page::Launcher;
    state.capturing_shortcut = false;
    state.input_source_picker_open = false;
    focus_search_input(state)
}

fn quit_launcher(state: &mut Launcher) -> Task<Message> {
    close_quick_look(state);
    restore_input_source(state);
    iced::exit()
}

fn view(state: &Launcher, _window: window::Id) -> Element<'_, Message> {
    let content = match state.page {
        Page::Launcher => launcher_view(state),
        Page::Settings => settings_view(state),
        Page::QuickLinks => quick_links_view(state),
        Page::Confirmation => confirmation_view(state),
    };

    container(content)
        .padding(18)
        .width(Fill)
        .height(Fill)
        .style(panel_style)
        .into()
}

#[derive(Clone, Copy)]
enum HeaderContext {
    Launcher,
    Settings,
    Clipboard,
    QuickLinks,
    Confirmation,
}

fn header(state: &Launcher, context: HeaderContext) -> Element<'_, Message> {
    let (subtitle, pill, trailing): (&str, &str, Element<'_, Message>) = match context {
        HeaderContext::Launcher => (
            "Find and launch instantly",
            "",
            button(text("⚙").size(17))
                .padding([7, 10])
                .on_press(Message::OpenSettings)
                .style(icon_button_style)
                .into(),
        ),
        HeaderContext::Settings => (
            "Preferences",
            "SETTINGS",
            button(text("‹  Back").size(13))
                .padding([7, 10])
                .on_press(Message::CloseSettings)
                .style(secondary_button_style)
                .into(),
        ),
        HeaderContext::Clipboard => (
            "Local text history",
            "CLIPBOARD",
            button(text("‹  All").size(13))
                .padding([7, 10])
                .on_press(Message::ExitSearchMode)
                .style(secondary_button_style)
                .into(),
        ),
        HeaderContext::QuickLinks => (
            "Saved websites",
            "QUICK LINKS",
            button(text("‹  Back").size(13))
                .padding([7, 10])
                .on_press(Message::CloseQuickLinks)
                .style(secondary_button_style)
                .into(),
        ),
        HeaderContext::Confirmation => (
            "Review before running",
            "CONFIRM",
            button(text("‹  Cancel").size(13))
                .padding([7, 10])
                .on_press(Message::CancelConfirmation)
                .style(secondary_button_style)
                .into(),
        ),
    };

    let context_badge: Element<'_, Message> = if matches!(context, HeaderContext::Launcher) {
        container(text(state.store_data.settings.shortcut.to_string()).size(12))
            .padding([6, 10])
            .style(shortcut_pill_style)
            .into()
    } else {
        container(text(pill).size(10).color(TEXT_SECONDARY))
            .padding([6, 9])
            .style(section_pill_style)
            .into()
    };

    iced::widget::row![
        container(
            image(state.brand_icon.clone())
                .width(34)
                .height(34)
                .content_fit(ContentFit::Contain),
        )
        .center_x(34)
        .center_y(34)
        .style(transparent_style),
        iced::widget::column![
            text("DuckGooKey").size(17).color(TEXT_PRIMARY),
            text(subtitle).size(11).color(TEXT_SECONDARY),
        ]
        .spacing(1),
        widget::Space::new().width(Fill),
        context_badge,
        trailing,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

fn selected_shortcut_hint(state: &Launcher, results: &[SearchResult]) -> String {
    let escape = if state.quick_look_path.is_some() {
        "esc close preview"
    } else if state.search_mode.is_some() {
        "esc all"
    } else {
        "esc hide"
    };
    let Some(item) = results.get(state.selected).map(|result| &result.item) else {
        return format!("↑↓ navigate   {escape}");
    };

    let primary = match &item.action {
        LaunchAction::OpenApplication { .. } => "↵ open   ⌘↵ Finder   ⌘C path",
        LaunchAction::OpenPath { .. } => "↵ open   ⌘Y Quick Look   ⌘↵ Finder   ⌘C path",
        LaunchAction::OpenUrl { .. } => "↵ open   ⌘C URL",
        LaunchAction::CopyText { .. } => "↵ copy   ⌘C copy",
        LaunchAction::SystemCommand { command } if command.requires_confirmation() => "↵ review",
        LaunchAction::SystemCommand { .. } => "↵ run",
        LaunchAction::EnterSearchMode { .. } | LaunchAction::ManageQuickLinks => "↵ open",
        LaunchAction::RefreshCatalog => "↵ refresh",
        LaunchAction::Quit => "↵ quit",
    };
    let pin = if item.pinnable { "   ⌘P pin" } else { "" };
    let delete = if state.search_mode == Some(SearchMode::Clipboard) {
        "   ⌘D delete"
    } else {
        ""
    };
    format!("↑↓ navigate   {primary}{pin}{delete}   {escape}")
}

fn launcher_view(state: &Launcher) -> Element<'_, Message> {
    let placeholder = match state.search_mode {
        Some(SearchMode::Clipboard) => "Filter clipboard history…",
        None => "Search apps, files, commands, and more…",
    };
    let header_context = match state.search_mode {
        Some(SearchMode::Clipboard) => HeaderContext::Clipboard,
        None => HeaderContext::Launcher,
    };
    let search = text_input(placeholder, &state.query)
        .id(state.query_input.clone())
        .on_input(Message::QueryChanged)
        .padding([13, 15])
        .size(18)
        .style(search_input_style);

    let results = state.results();
    let result_count = results.len();
    let mut result_list = iced::widget::column![].spacing(2);
    if results.is_empty() && !state.loading {
        let (empty_title, empty_detail) = match state.search_mode {
            Some(SearchMode::Clipboard)
                if state.clipboard_error.is_some()
                    && state.store_data.clipboard_history.is_empty() =>
            {
                (
                    "Clipboard unavailable",
                    state.clipboard_error.as_deref().unwrap_or("Unknown error"),
                )
            }
            Some(SearchMode::Clipboard) if state.store_data.clipboard_history.is_empty() => (
                "Clipboard history is empty",
                "New text copies appear here while history is enabled",
            ),
            Some(SearchMode::Clipboard) => (
                "No clipboard matches",
                "Try a shorter search or copy new text",
            ),
            None => ("No matches", "Try a shorter application name"),
        };
        result_list = result_list.push(
            container(
                iced::widget::column![
                    text(empty_title).size(16).color(TEXT_PRIMARY),
                    text(empty_detail).size(12).color(TEXT_SECONDARY),
                ]
                .spacing(4)
                .align_x(Alignment::Center),
            )
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill),
        );
    } else {
        for (index, result) in results.iter().cloned().enumerate() {
            let icon = state.icon_handles.get(&result.item.id).cloned();
            result_list = result_list.push(result_row(
                result,
                icon,
                index == state.selected,
                state.launching,
            ));
        }
    }

    let status = if let Some(notice) = &state.notice {
        text(notice.text.clone())
            .size(11)
            .color(if notice.error { DANGER } else { TEXT_SECONDARY })
    } else if state.loading {
        text("Indexing applications…")
            .size(11)
            .color(TEXT_SECONDARY)
    } else if state.file_search_pending || state.file_search_in_progress {
        text("Searching files…").size(11).color(TEXT_SECONDARY)
    } else if state.file_search_error.is_some() {
        text("File search unavailable").size(11).color(DANGER)
    } else {
        text("Ready").size(11).color(TEXT_SECONDARY)
    };

    let shortcut_hint = selected_shortcut_hint(state, &results);

    let footer = iced::widget::row![
        status,
        widget::Space::new().width(Fill),
        text(format!("{result_count} shown   ·   {shortcut_hint}"))
            .size(11)
            .color(TEXT_SECONDARY),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    iced::widget::column![
        header(state, header_context),
        search,
        container(result_list).height(Fill),
        footer,
    ]
    .spacing(11)
    .into()
}

fn settings_view(state: &Launcher) -> Element<'_, Message> {
    let login_control = toggler(state.launch_at_login)
        .on_toggle(Message::SetLaunchAtLogin)
        .size(22);
    let login_status = if state.launch_at_login_pending {
        "Updating…"
    } else if state.launch_at_login {
        "Enabled"
    } else {
        "Disabled"
    };
    let login_card = container(
        iced::widget::row![
            iced::widget::column![
                text("Launch at login").size(14).color(TEXT_PRIMARY),
                text("Start quietly in the menu bar after you sign in")
                    .size(11)
                    .color(TEXT_SECONDARY),
            ]
            .spacing(4)
            .width(Fill),
            text(login_status).size(11).color(if state.launch_at_login {
                ACCENT
            } else {
                TEXT_SECONDARY
            }),
            login_control,
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding(15)
    .width(Fill)
    .style(settings_card_style);

    let update_detail = if state.update_installing {
        "Downloading and preparing a verified update…".to_owned()
    } else if state.update_checking {
        "Checking the DuckGooKey update channel…".to_owned()
    } else if let Some(update) = &state.available_update {
        format!("Version {} is ready", update.version)
    } else {
        format!(
            "Version {} · checks every 6 hours",
            env!("CARGO_PKG_VERSION")
        )
    };
    let update_action: Element<'_, Message> = if let Some(update) = &state.available_update {
        let update_button_label = if state.update_installing {
            "Preparing…".to_owned()
        } else {
            format!("Install {}", update.version)
        };
        button(text(update_button_label).size(12))
            .padding([8, 12])
            .on_press_maybe((!state.update_installing).then_some(Message::InstallAvailableUpdate))
            .style(accent_button_style)
            .into()
    } else {
        button(
            text(if state.update_checking {
                "Checking…"
            } else {
                "Check now"
            })
            .size(12),
        )
        .padding([8, 12])
        .on_press_maybe((!state.update_checking).then_some(Message::CheckForUpdates))
        .style(secondary_button_style)
        .into()
    };
    let updates_card = container(
        iced::widget::row![
            iced::widget::column![
                text("Updates").size(14).color(TEXT_PRIMARY),
                text(update_detail).size(11).color(TEXT_SECONDARY),
            ]
            .spacing(4)
            .width(Fill),
            update_action,
            toggler(state.store_data.settings.update_checks_enabled)
                .on_toggle(Message::SetUpdateChecksEnabled)
                .size(22),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(15)
    .width(Fill)
    .style(settings_card_style);

    let clipboard_enabled = state.store_data.settings.clipboard_history_enabled;
    let clipboard_count = state.store_data.clipboard_history.len();
    let clipboard_detail = state.clipboard_error.as_deref().map_or_else(
        || {
            format!(
                "Text only · local · {clipboard_count}/{} entries",
                crate::clipboard_history::MAX_CLIPBOARD_ENTRIES
            )
        },
        |error| format!("Clipboard unavailable · {error}"),
    );
    let clipboard_card = container(
        iced::widget::row![
            iced::widget::column![
                text("Clipboard History").size(14).color(TEXT_PRIMARY),
                text(clipboard_detail)
                    .size(11)
                    .color(if state.clipboard_error.is_some() {
                        DANGER
                    } else {
                        TEXT_SECONDARY
                    }),
            ]
            .spacing(4)
            .width(Fill),
            button(text("Clear").size(12))
                .padding([8, 12])
                .on_press_maybe(
                    (!state.store_data.clipboard_history.is_empty())
                        .then_some(Message::RequestClearClipboardHistory)
                )
                .style(secondary_button_style),
            toggler(clipboard_enabled)
                .on_toggle(Message::SetClipboardHistoryEnabled)
                .size(22),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(15)
    .width(Fill)
    .style(settings_card_style);

    let anonymous_usage_stats_card = container(
        iced::widget::row![
            iced::widget::column![
                text("Anonymous usage stats").size(14).color(TEXT_PRIMARY),
                text(
                    "First launch and one daily activity signal · never sends searches, apps, files, or clipboard content"
                )
                .size(11)
                .color(TEXT_SECONDARY),
            ]
            .spacing(4)
            .width(Fill),
            toggler(state.store_data.settings.anonymous_usage_stats_enabled)
                .on_toggle(Message::SetAnonymousUsageStatsEnabled)
                .size(22),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(15)
    .width(Fill)
    .style(settings_card_style);

    let quick_link_count = state.store_data.quick_links.len();
    let quick_links_card = container(
        iced::widget::row![
            iced::widget::column![
                text("Quick Links").size(14).color(TEXT_PRIMARY),
                text(format!("{quick_link_count} saved websites"))
                    .size(11)
                    .color(TEXT_SECONDARY),
            ]
            .spacing(4)
            .width(Fill),
            button(text("Manage").size(12))
                .padding([8, 12])
                .on_press(Message::OpenQuickLinks)
                .style(secondary_button_style),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(15)
    .width(Fill)
    .style(settings_card_style);

    let shortcut_button_label = if state.capturing_shortcut {
        "Press shortcut…"
    } else {
        "Change"
    };
    let shortcut_card = container(
        iced::widget::row![
            iced::widget::column![
                text("Global shortcut").size(14).color(TEXT_PRIMARY),
                text(if state.capturing_shortcut {
                    "Press a modified key · Escape cancels"
                } else {
                    "Show or hide DuckGooKey from anywhere"
                })
                .size(11)
                .color(if state.capturing_shortcut {
                    ACCENT
                } else {
                    TEXT_SECONDARY
                }),
            ]
            .spacing(4)
            .width(Fill),
            container(text(state.store_data.settings.shortcut.to_string()).size(13))
                .padding([7, 11])
                .style(shortcut_pill_style),
            button(text(shortcut_button_label).size(12))
                .padding([8, 12])
                .on_press(Message::BeginShortcutCapture)
                .style(if state.capturing_shortcut {
                    accent_button_style
                } else {
                    secondary_button_style
                }),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(15)
    .width(Fill)
    .style(settings_card_style);

    let reset_shortcut = button(text("Restore Option+Space").size(12))
        .padding([8, 12])
        .on_press_maybe(
            (state.store_data.settings.shortcut != ShortcutBinding::default())
                .then_some(Message::ResetShortcut),
        )
        .style(secondary_button_style);

    let preferred_input_source = state.store_data.settings.preferred_input_source.as_deref();
    let input_source_choices = input_source_choices(&state.input_sources, preferred_input_source);
    let selected_input_source =
        selected_input_source_choice(preferred_input_source, &input_source_choices);
    let input_source_detail = state
        .input_source_error
        .as_deref()
        .unwrap_or("Switch while search is focused, then restore your previous input source");
    let input_source_picker = pick_list(
        input_source_choices,
        Some(selected_input_source),
        Message::SetPreferredInputSource,
    )
    .width(260)
    .padding([8, 10])
    .text_size(12)
    .menu_height(220)
    .on_open(Message::InputSourcePickerOpened)
    .on_close(Message::InputSourcePickerClosed)
    .style(input_source_pick_list_style)
    .menu_style(input_source_menu_style);
    let input_source_card = container(
        iced::widget::row![
            iced::widget::column![
                text("Search input source").size(14).color(TEXT_PRIMARY),
                text(input_source_detail)
                    .size(11)
                    .color(if state.input_source_error.is_some() {
                        DANGER
                    } else {
                        TEXT_SECONDARY
                    }),
            ]
            .spacing(4)
            .width(Fill),
            input_source_picker,
        ]
        .spacing(14)
        .align_y(Alignment::Center),
    )
    .padding(15)
    .width(Fill)
    .style(settings_card_style);

    let app_count = state.application_items().len();
    let discovery_card = container(
        iced::widget::row![
            iced::widget::column![
                text("Application discovery").size(14).color(TEXT_PRIMARY),
                text(format!(
                    "{app_count} applications · Automatically rescans every 15 seconds"
                ))
                .size(11)
                .color(TEXT_SECONDARY),
            ]
            .spacing(4)
            .width(Fill),
            button(
                text(if state.loading {
                    "Scanning…"
                } else {
                    "Scan now"
                })
                .size(12)
            )
            .padding([8, 12])
            .on_press_maybe(
                (!state.loading)
                    .then(|| { Message::ActivateItem("builtin:refresh-catalog".to_owned()) })
            )
            .style(secondary_button_style),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(15)
    .width(Fill)
    .style(settings_card_style);

    let status = if let Some(notice) = &state.notice {
        text(notice.text.clone())
            .size(11)
            .color(if notice.error { DANGER } else { TEXT_SECONDARY })
    } else {
        text("Settings are saved automatically")
            .size(11)
            .color(TEXT_SECONDARY)
    };

    let settings_sections = iced::widget::column![
        text("GENERAL").size(10).color(TEXT_SECONDARY),
        login_card,
        updates_card,
        text("KEYBOARD").size(10).color(TEXT_SECONDARY),
        shortcut_card,
        reset_shortcut,
        input_source_card,
        text("FEATURES").size(10).color(TEXT_SECONDARY),
        quick_links_card,
        clipboard_card,
        anonymous_usage_stats_card,
        text("CATALOG").size(10).color(TEXT_SECONDARY),
        discovery_card,
    ]
    .spacing(10);

    iced::widget::column![
        header(state, HeaderContext::Settings),
        scrollable(settings_sections).height(Fill),
        iced::widget::row![
            status,
            widget::Space::new().width(Fill),
            text("esc back").size(11).color(TEXT_SECONDARY),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(15)
    .into()
}

fn quick_links_view(state: &Launcher) -> Element<'_, Message> {
    let editing = state.editing_quick_link_id.is_some();
    let form_title = if editing {
        "Edit Quick Link"
    } else {
        "Add Quick Link"
    };
    let title_input = text_input("Name", &state.quick_link_title)
        .id(state.quick_link_title_input.clone())
        .on_input(Message::QuickLinkTitleChanged)
        .padding([11, 13])
        .size(14)
        .style(search_input_style);
    let url_input = text_input("https://example.com", &state.quick_link_url)
        .id(state.quick_link_url_input.clone())
        .on_input(Message::QuickLinkUrlChanged)
        .padding([11, 13])
        .size(14)
        .style(search_input_style);
    let can_save =
        !state.quick_link_title.trim().is_empty() && !state.quick_link_url.trim().is_empty();
    let save_label = if editing { "Save changes" } else { "Add link" };
    let mut form_actions = iced::widget::row![widget::Space::new().width(Fill)].spacing(8);
    if editing {
        form_actions = form_actions.push(
            button(text("Cancel edit").size(12))
                .padding([8, 12])
                .on_press(Message::CancelQuickLinkEdit)
                .style(secondary_button_style),
        );
    }
    form_actions = form_actions.push(
        button(text(save_label).size(12))
            .padding([8, 12])
            .on_press_maybe(can_save.then_some(Message::SaveQuickLink))
            .style(accent_button_style),
    );
    let form = container(
        iced::widget::column![
            text(form_title).size(14).color(TEXT_PRIMARY),
            title_input,
            url_input,
            form_actions,
        ]
        .spacing(9),
    )
    .padding(15)
    .width(Fill)
    .style(settings_card_style);

    let mut links = iced::widget::column![].spacing(8);
    if state.store_data.quick_links.is_empty() {
        links = links.push(
            container(
                iced::widget::column![
                    text("No Quick Links yet").size(15).color(TEXT_PRIMARY),
                    text("Save a website to make it searchable from the launcher")
                        .size(11)
                        .color(TEXT_SECONDARY),
                ]
                .spacing(4)
                .align_x(Alignment::Center),
            )
            .height(130)
            .center_x(Fill)
            .center_y(Fill),
        );
    } else {
        for quick_link in &state.store_data.quick_links {
            let id = quick_link.id();
            links = links.push(
                container(
                    iced::widget::row![
                        iced::widget::column![
                            text(quick_link.title().to_owned())
                                .size(14)
                                .color(TEXT_PRIMARY),
                            text(quick_link.url().to_owned())
                                .size(11)
                                .color(TEXT_SECONDARY),
                        ]
                        .spacing(3)
                        .width(Fill),
                        button(text("Edit").size(12))
                            .padding([8, 12])
                            .on_press(Message::EditQuickLink(id))
                            .style(secondary_button_style),
                        button(text("Delete").size(12))
                            .padding([8, 12])
                            .on_press(Message::RequestDeleteQuickLink(id))
                            .style(secondary_button_style),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding(12)
                .width(Fill)
                .style(settings_card_style),
            );
        }
    }

    let status = state.notice.as_ref().map_or_else(
        || {
            text("Only HTTP and HTTPS links are accepted")
                .size(11)
                .color(TEXT_SECONDARY)
        },
        |notice| {
            text(notice.text.clone()).size(11).color(if notice.error {
                DANGER
            } else {
                TEXT_SECONDARY
            })
        },
    );

    iced::widget::column![
        header(state, HeaderContext::QuickLinks),
        form,
        text("SAVED LINKS").size(10).color(TEXT_SECONDARY),
        scrollable(links).height(Fill),
        iced::widget::row![
            status,
            widget::Space::new().width(Fill),
            text("⌘↵ save   ·   esc back")
                .size(11)
                .color(TEXT_SECONDARY),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(13)
    .into()
}

fn confirmation_view(state: &Launcher) -> Element<'_, Message> {
    let (title, detail, confirm_label) = match state.pending_confirmation.as_ref() {
        Some(PendingConfirmation::SystemCommand { command, .. }) => (
            command.title().to_owned(),
            command
                .confirmation_prompt()
                .unwrap_or(command.subtitle())
                .to_owned(),
            command.title().to_owned(),
        ),
        Some(PendingConfirmation::DeleteQuickLink { title, .. }) => (
            "Delete Quick Link?".to_owned(),
            format!("Remove “{title}” from DuckGooKey?"),
            "Delete".to_owned(),
        ),
        Some(PendingConfirmation::ClearClipboardHistory) => (
            "Clear Clipboard History?".to_owned(),
            "Delete every locally saved clipboard text entry?".to_owned(),
            "Clear history".to_owned(),
        ),
        None => (
            "Nothing to confirm".to_owned(),
            "Return to the previous screen.".to_owned(),
            "Return".to_owned(),
        ),
    };

    let confirmation_card = container(
        iced::widget::column![
            container(text("!").size(24).color(DANGER))
                .width(52)
                .height(52)
                .center_x(Fill)
                .center_y(Fill)
                .style(danger_badge_style),
            text(title).size(20).color(TEXT_PRIMARY),
            text(detail).size(13).color(TEXT_SECONDARY),
            iced::widget::row![
                button(text("Cancel").size(13))
                    .padding([10, 18])
                    .on_press(Message::CancelConfirmation)
                    .style(secondary_button_style),
                button(text(confirm_label).size(13))
                    .padding([10, 18])
                    .on_press(Message::ConfirmPending)
                    .style(danger_button_style),
            ]
            .spacing(9),
        ]
        .spacing(13)
        .align_x(Alignment::Center),
    )
    .padding(28)
    .max_width(470)
    .style(settings_card_style);

    iced::widget::column![
        header(state, HeaderContext::Confirmation),
        container(confirmation_card)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill),
        iced::widget::row![
            text("This action only runs after confirmation")
                .size(11)
                .color(TEXT_SECONDARY),
            widget::Space::new().width(Fill),
            text("↵ confirm   ·   esc cancel")
                .size(11)
                .color(TEXT_SECONDARY),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(15)
    .into()
}

fn input_source_choices(
    input_sources: &[platform::InputSource],
    preferred: Option<&str>,
) -> Vec<InputSourceChoice> {
    let mut choices = vec![InputSourceChoice::keep_current()];
    choices.extend(input_sources.iter().map(InputSourceChoice::available));

    if let Some(preferred) = preferred
        && !choices
            .iter()
            .any(|choice| choice.identifier.as_deref() == Some(preferred))
    {
        choices.push(InputSourceChoice::unavailable(preferred));
    }

    choices
}

fn selected_input_source_choice(
    preferred: Option<&str>,
    choices: &[InputSourceChoice],
) -> InputSourceChoice {
    choices
        .iter()
        .find(|choice| choice.identifier.as_deref() == preferred)
        .cloned()
        .unwrap_or_else(InputSourceChoice::keep_current)
}

fn result_row(
    result: SearchResult,
    icon_handle: Option<image::Handle>,
    selected: bool,
    launching: bool,
) -> Element<'static, Message> {
    let monogram = result
        .item
        .title
        .chars()
        .next()
        .unwrap_or('D')
        .to_uppercase()
        .collect::<String>();
    let icon: Element<'static, Message> = if let Some(handle) = icon_handle {
        container(
            image(handle)
                .width(40)
                .height(40)
                .content_fit(ContentFit::Contain),
        )
        .center_x(42)
        .center_y(42)
        .into()
    } else {
        container(text(monogram).size(14).color(TEXT_PRIMARY))
            .center_x(40)
            .center_y(40)
            .style(move |_| result_icon_style(selected))
            .into()
    };

    let mut labels =
        iced::widget::column![text(result.item.title.clone()).size(14).color(TEXT_PRIMARY),]
            .spacing(2);
    if let Some(subtitle) = result.item.subtitle.clone() {
        labels = labels.push(text(subtitle).size(11).color(TEXT_SECONDARY));
    }

    let action_label = match &result.item.action {
        LaunchAction::OpenApplication { .. }
        | LaunchAction::OpenPath { .. }
        | LaunchAction::OpenUrl { .. }
        | LaunchAction::EnterSearchMode { .. }
        | LaunchAction::ManageQuickLinks => "OPEN",
        LaunchAction::CopyText { .. } => "COPY",
        LaunchAction::SystemCommand { command } if command.requires_confirmation() => "REVIEW",
        LaunchAction::SystemCommand { .. } => "RUN",
        LaunchAction::RefreshCatalog => "REFRESH",
        LaunchAction::Quit => "QUIT",
    };
    let item_id = result.item.id.clone();
    let open = button(
        iced::widget::row![icon, labels.width(Fill)]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([6, 9])
    .on_press_maybe((!launching).then(|| Message::ActivateItem(item_id.clone())))
    .style(move |_, status| result_button_style(selected, status));

    let trailing: Element<'static, Message> = if result.item.pinnable {
        let pin_label = if result.pinned { "★" } else { "☆" };
        button(text(pin_label).size(16))
            .width(48)
            .padding([15, 10])
            .on_press(Message::TogglePinned(item_id))
            .style(move |_, status| pin_button_style(result.pinned, status))
            .into()
    } else {
        container(text(action_label).size(9).color(TEXT_SECONDARY))
            .width(56)
            .center_x(56)
            .into()
    };

    iced::widget::row![open, trailing]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

fn subscription(_state: &Launcher) -> Subscription<Message> {
    Subscription::batch([
        event::listen_with(listen_for_events),
        time::every(EVENT_POLL_INTERVAL).map(|_| Message::PollNativeEvents),
        time::every(AUTO_DISCOVERY_INTERVAL).map(|_| Message::CatalogScanTick),
        time::every(FILE_SEARCH_TICK_INTERVAL).map(|_| Message::FileSearchTick),
        time::every(CLIPBOARD_POLL_INTERVAL).map(|_| Message::ClipboardPollTick),
        time::every(UPDATE_CHECK_INTERVAL).map(|_| Message::UpdateCheckTick),
    ])
}

fn listen_for_events(event: Event, status: event::Status, window: window::Id) -> Option<Message> {
    match event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            repeat: false,
            ..
        }) => Some(Message::KeyPressed {
            window,
            key,
            modifiers,
            status,
        }),
        Event::Window(window::Event::Focused) => Some(Message::WindowFocused(window)),
        Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused(window)),
        Event::Window(window::Event::CloseRequested) => Some(Message::WindowCloseRequested(window)),
        Event::Mouse(mouse::Event::ButtonReleased(_)) => Some(Message::CheckQueryFocus),
        _ => None,
    }
}

impl Launcher {
    fn results(&self) -> Vec<SearchResult> {
        match self.search_mode {
            Some(SearchMode::Clipboard) => self
                .store_data
                .clipboard_history
                .search(&self.query)
                .into_iter()
                .take(RESULT_LIMIT)
                .map(|entry| {
                    dynamic_search_result(CatalogItem {
                        id: format!("clipboard:{}", entry.id()),
                        title: clipboard_preview(entry.text()),
                        subtitle: Some(format!("Clipboard text · {} bytes", entry.text().len())),
                        icon_path: None,
                        keywords: Vec::new(),
                        pinnable: false,
                        action: LaunchAction::CopyText {
                            text: entry.text().to_owned(),
                        },
                    })
                })
                .collect(),
            None => {
                let file_count = self.file_results.len().min(ROOT_FILE_RESULT_LIMIT);
                let reserved = usize::from(self.calculator_result.is_some())
                    + file_count
                    + usize::from(self.web_search_result.is_some());
                let mut results = self.catalog.search(
                    &self.query,
                    &self.store_data.usage,
                    current_unix_time_ms(),
                    RESULT_LIMIT.saturating_sub(reserved),
                );
                if let Some(item) = self.calculator_result.clone() {
                    results.insert(0, dynamic_search_result(item));
                }
                results.extend(
                    self.file_results
                        .iter()
                        .take(file_count)
                        .cloned()
                        .map(dynamic_search_result),
                );
                if let Some(item) = self.web_search_result.clone() {
                    results.push(dynamic_search_result(item));
                }
                results
            }
        }
    }

    fn application_items(&self) -> Vec<CatalogItem> {
        self.catalog
            .items()
            .iter()
            .filter(|item| matches!(item.action, LaunchAction::OpenApplication { .. }))
            .cloned()
            .collect()
    }
}

fn refresh_dynamic_results(state: &mut Launcher) {
    if state.search_mode.is_some() {
        clear_dynamic_results(state);
        return;
    }

    state.calculator_result = calculator::calculator_item(&state.query);
    state.web_search_result = web_search::web_search_item(&state.query);
}

fn clear_dynamic_results(state: &mut Launcher) {
    state.calculator_result = None;
    state.web_search_result = None;
}

fn dynamic_search_result(item: CatalogItem) -> SearchResult {
    SearchResult {
        item,
        match_kind: None,
        match_score: 0,
        pinned: false,
        frecency_score: 0,
    }
}

fn clipboard_preview(value: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 72;

    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = collapsed
        .chars()
        .take(MAX_PREVIEW_CHARS)
        .collect::<String>();
    if collapsed.chars().count() > MAX_PREVIEW_CHARS {
        preview.push('…');
    }
    preview
}

impl Drop for Launcher {
    fn drop(&mut self) {
        if let Some(identifier) = self.input_source_to_restore.take() {
            let _ = platform::select_input_source(&identifier);
        }
    }
}

fn transparent_style(_: &Theme) -> container::Style {
    container::Style::default().background(Color::TRANSPARENT)
}

fn panel_style(_: &Theme) -> container::Style {
    container::Style::default()
        .color(TEXT_PRIMARY)
        .background(Color {
            r: 0.055,
            g: 0.063,
            b: 0.082,
            a: 0.985,
        })
        .border(Border {
            radius: 16.0.into(),
            width: 1.0,
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.12,
            },
        })
}

fn shortcut_pill_style(_: &Theme) -> container::Style {
    container::Style::default()
        .color(TEXT_PRIMARY)
        .background(Color {
            r: 0.10,
            g: 0.13,
            b: 0.16,
            a: 0.96,
        })
        .border(Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.10,
            },
        })
}

fn section_pill_style(_: &Theme) -> container::Style {
    container::Style::default()
        .background(Color {
            r: ACCENT.r,
            g: ACCENT.g,
            b: ACCENT.b,
            a: 0.08,
        })
        .border(Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color {
                r: ACCENT.r,
                g: ACCENT.g,
                b: ACCENT.b,
                a: 0.18,
            },
        })
}

fn settings_card_style(_: &Theme) -> container::Style {
    container::Style::default()
        .color(TEXT_PRIMARY)
        .background(Color {
            r: 0.078,
            g: 0.087,
            b: 0.11,
            a: 0.92,
        })
        .border(Border {
            radius: 12.0.into(),
            width: 1.0,
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.075,
            },
        })
}

fn danger_badge_style(_: &Theme) -> container::Style {
    container::Style::default()
        .background(Color {
            r: DANGER.r,
            g: DANGER.g,
            b: DANGER.b,
            a: 0.12,
        })
        .border(Border {
            radius: 14.0.into(),
            width: 1.0,
            color: Color {
                r: DANGER.r,
                g: DANGER.g,
                b: DANGER.b,
                a: 0.32,
            },
        })
}

fn input_source_pick_list_style(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let emphasized = !matches!(status, pick_list::Status::Active);
    pick_list::Style {
        text_color: TEXT_PRIMARY,
        placeholder_color: TEXT_SECONDARY,
        handle_color: if emphasized { ACCENT } else { TEXT_SECONDARY },
        background: Background::Color(Color {
            r: 0.10,
            g: 0.12,
            b: 0.15,
            a: 0.98,
        }),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: if emphasized {
                Color {
                    r: ACCENT.r,
                    g: ACCENT.g,
                    b: ACCENT.b,
                    a: 0.65,
                }
            } else {
                Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.12,
                }
            },
        },
    }
}

fn input_source_menu_style(_: &Theme) -> widget::overlay::menu::Style {
    widget::overlay::menu::Style {
        background: Background::Color(Color {
            r: 0.075,
            g: 0.085,
            b: 0.11,
            a: 1.0,
        }),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.12,
            },
        },
        text_color: TEXT_PRIMARY,
        selected_text_color: TEXT_PRIMARY,
        selected_background: Background::Color(Color {
            r: ACCENT.r,
            g: ACCENT.g,
            b: ACCENT.b,
            a: 0.24,
        }),
        shadow: Shadow {
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.40,
            },
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
    }
}

fn search_input_style(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(Color {
            r: 0.09,
            g: 0.10,
            b: 0.13,
            a: 0.96,
        }),
        border: Border {
            radius: 12.0.into(),
            width: 1.0,
            color: if focused {
                Color { a: 0.52, ..ACCENT }
            } else {
                Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.09,
                }
            },
        },
        icon: TEXT_SECONDARY,
        placeholder: TEXT_SECONDARY,
        value: TEXT_PRIMARY,
        selection: Color {
            r: ACCENT.r,
            g: ACCENT.g,
            b: ACCENT.b,
            a: 0.35,
        },
    }
}

fn icon_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let active = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if active {
            Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.08,
            }
        } else {
            Color::TRANSPARENT
        })),
        text_color: if active { TEXT_PRIMARY } else { TEXT_SECONDARY },
        border: Border {
            radius: 9.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..button::Style::default()
    }
}

fn secondary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let disabled = matches!(status, button::Status::Disabled);
    let active = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if active {
            Color {
                r: 0.16,
                g: 0.18,
                b: 0.22,
                a: 1.0,
            }
        } else {
            Color {
                r: 0.11,
                g: 0.12,
                b: 0.15,
                a: 1.0,
            }
        })),
        text_color: if disabled {
            Color {
                a: 0.35,
                ..TEXT_SECONDARY
            }
        } else {
            TEXT_PRIMARY
        },
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: if disabled { 0.04 } else { 0.09 },
            },
        },
        ..button::Style::default()
    }
}

fn accent_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let pressed = matches!(status, button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(Color {
            r: if pressed { 0.30 } else { ACCENT.r },
            g: if pressed { 0.66 } else { ACCENT.g },
            b: if pressed { 0.56 } else { ACCENT.b },
            a: 1.0,
        })),
        text_color: Color {
            r: 0.035,
            g: 0.07,
            b: 0.06,
            a: 1.0,
        },
        border: Border {
            radius: 8.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..button::Style::default()
    }
}

fn danger_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let pressed = matches!(status, button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(Color {
            r: if pressed { 0.77 } else { DANGER.r },
            g: if pressed { 0.30 } else { DANGER.g },
            b: if pressed { 0.30 } else { DANGER.b },
            a: 1.0,
        })),
        text_color: Color {
            r: 0.12,
            g: 0.025,
            b: 0.025,
            a: 1.0,
        },
        border: Border {
            radius: 8.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..button::Style::default()
    }
}

fn result_button_style(selected: bool, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let background = if selected {
        Color {
            r: 0.075,
            g: 0.20,
            b: 0.17,
            a: 0.98,
        }
    } else if hovered {
        Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.055,
        }
    } else {
        Color::TRANSPARENT
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: TEXT_PRIMARY,
        border: Border {
            radius: 10.0.into(),
            width: if selected { 1.0 } else { 0.0 },
            color: Color {
                r: ACCENT.r,
                g: ACCENT.g,
                b: ACCENT.b,
                a: 0.20,
            },
        },
        ..button::Style::default()
    }
}

fn result_icon_style(selected: bool) -> container::Style {
    container::Style::default()
        .background(if selected {
            Color {
                r: 0.17,
                g: 0.38,
                b: 0.32,
                a: 1.0,
            }
        } else {
            Color {
                r: 0.14,
                g: 0.15,
                b: 0.19,
                a: 1.0,
            }
        })
        .border(Border {
            radius: 9.0.into(),
            width: 1.0,
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.08,
            },
        })
}

fn pin_button_style(pinned: bool, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hovered {
            Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.06,
            }
        } else {
            Color::TRANSPARENT
        })),
        text_color: if pinned { ACCENT } else { TEXT_SECONDARY },
        border: Border {
            radius: 10.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..button::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher_with_item(item: CatalogItem) -> Launcher {
        Launcher {
            launcher_window: window::Id::unique(),
            query_input: widget::Id::unique(),
            quick_link_title_input: widget::Id::unique(),
            quick_link_url_input: widget::Id::unique(),
            page: Page::Launcher,
            search_mode: None,
            query: String::new(),
            calculator_result: None,
            web_search_result: None,
            selected: 0,
            visible: true,
            window_focused: true,
            loading: false,
            launching: false,
            launch_at_login: false,
            launch_at_login_pending: false,
            capturing_shortcut: false,
            input_source_picker_open: false,
            input_sources: Vec::new(),
            input_source_error: None,
            input_source_to_restore: None,
            file_search_revision: 0,
            file_search_pending: false,
            file_search_in_progress: false,
            file_query_changed_at: Instant::now(),
            file_results: Vec::new(),
            file_search_error: None,
            quick_look_path: None,
            clipboard_change_count: None,
            clipboard_error: None,
            update_checking: false,
            update_installing: false,
            available_update: None,
            quick_link_title: String::new(),
            quick_link_url: String::new(),
            editing_quick_link_id: None,
            quick_links_return_page: Page::Launcher,
            pending_confirmation: None,
            confirmation_return_page: Page::Launcher,
            catalog: Catalog::new([item]),
            brand_icon: image::Handle::from_bytes(Vec::new()),
            icon_handles: HashMap::new(),
            catalog_revision: 0,
            store_data: StoreData::default(),
            store: None,
            integrations: None,
            notice: None,
        }
    }

    #[test]
    fn usage_for_mut_reuses_an_existing_record() {
        let mut usage = vec![UsageRecord::new("application:test")];

        usage_for_mut(&mut usage, "application:test").pinned = true;

        assert_eq!(usage.len(), 1);
        assert!(usage[0].pinned);
    }

    #[test]
    fn usage_for_mut_creates_a_record_for_a_new_item() {
        let mut usage = Vec::new();

        let record = usage_for_mut(&mut usage, "application:new");

        assert_eq!(record.item_id, "application:new");
        assert_eq!(usage.len(), 1);
    }

    #[test]
    fn root_results_put_calculator_first_and_keep_web_search_as_fallback() {
        let mut launcher = launcher_with_item(CatalogItem::application(
            "/Applications/Example.app",
            "Example",
        ));
        let _ = update(&mut launcher, Message::QueryChanged("= 2 + 2".to_owned()));

        let results = launcher.results();

        assert_eq!(results[0].item.id, "calculator:2 + 2");
        assert_eq!(results[0].item.title, "4");
        assert!(matches!(
            results.last().map(|result| &result.item.action),
            Some(LaunchAction::OpenUrl { .. })
        ));
        assert!(results.iter().all(|result| !result.item.pinnable));
    }

    #[test]
    fn failed_quick_link_persistence_does_not_mutate_session_state() {
        let mut launcher = launcher_with_item(CatalogItem::refresh_catalog());
        launcher.quick_link_title = "Rust".to_owned();
        launcher.quick_link_url = "https://www.rust-lang.org".to_owned();
        let next_id = launcher.store_data.next_quick_link_id;

        save_quick_link(&mut launcher);

        assert!(launcher.store_data.quick_links.is_empty());
        assert_eq!(launcher.store_data.next_quick_link_id, next_id);
        assert!(launcher.notice.as_ref().is_some_and(|notice| notice.error));
    }

    #[test]
    fn failed_clipboard_clear_keeps_existing_history() {
        let mut launcher = launcher_with_item(CatalogItem::refresh_catalog());
        launcher
            .store_data
            .clipboard_history
            .capture(1, 100, "keep me")
            .unwrap();
        launcher.page = Page::Confirmation;
        launcher.pending_confirmation = Some(PendingConfirmation::ClearClipboardHistory);
        launcher.confirmation_return_page = Page::Settings;

        let _ = confirm_pending(&mut launcher);

        assert_eq!(launcher.store_data.clipboard_history.len(), 1);
        assert_eq!(launcher.page, Page::Settings);
        assert!(launcher.notice.as_ref().is_some_and(|notice| notice.error));
    }

    #[test]
    fn captured_quick_link_submit_is_not_handled_twice() {
        let mut launcher = launcher_with_item(CatalogItem::refresh_catalog());
        launcher.page = Page::QuickLinks;
        launcher.quick_link_title = "Rust".to_owned();
        launcher.quick_link_url = "https://www.rust-lang.org".to_owned();
        let window = launcher.launcher_window;

        let _ = update(
            &mut launcher,
            Message::KeyPressed {
                window,
                key: keyboard::Key::Named(Named::Enter),
                modifiers: keyboard::Modifiers::COMMAND,
                status: event::Status::Captured,
            },
        );

        assert!(launcher.notice.is_none());
        assert!(launcher.store_data.quick_links.is_empty());
    }

    #[test]
    fn captured_command_y_reaches_quick_look_handling() {
        let mut launcher = launcher_with_item(CatalogItem::application(
            "/Applications/Example.app",
            "Example",
        ));
        let window = launcher.launcher_window;

        let _ = update(
            &mut launcher,
            Message::KeyPressed {
                window,
                key: keyboard::Key::Character("y".into()),
                modifiers: keyboard::Modifiers::COMMAND,
                status: event::Status::Captured,
            },
        );

        assert_eq!(
            launcher.notice.as_ref().map(|notice| notice.text.as_str()),
            Some("Quick Look is available for files and folders")
        );
    }

    #[test]
    fn clipboard_results_are_filtered_newest_first_and_copy_the_full_text() {
        let mut launcher = launcher_with_item(CatalogItem::refresh_catalog());
        launcher.search_mode = Some(SearchMode::Clipboard);
        launcher
            .store_data
            .clipboard_history
            .capture(1, 100, "older Rust text")
            .unwrap();
        launcher
            .store_data
            .clipboard_history
            .capture(2, 200, "newer RUST\ntext")
            .unwrap();
        launcher.query = "rust".to_owned();

        let results = launcher.results();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].item.id, "clipboard:2");
        assert_eq!(results[0].item.title, "newer RUST text");
        assert_eq!(
            results[0].item.action,
            LaunchAction::CopyText {
                text: "newer RUST\ntext".to_owned()
            }
        );
        assert!(results.iter().all(|result| !result.item.pinnable));
    }

    #[test]
    fn root_results_include_current_spotlight_results_after_catalog_items() {
        let mut launcher = launcher_with_item(CatalogItem::application(
            "/Applications/Report Viewer.app",
            "Report Viewer",
        ));
        let _ = update(&mut launcher, Message::QueryChanged("report".to_owned()));
        launcher.file_results = vec![CatalogItem::path(
            "/Users/example/Documents/report.pdf",
            "report.pdf",
        )];
        launcher.file_results[0].pinnable = false;

        let results = launcher.results();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].item.title, "Report Viewer");
        assert_eq!(results[1].item.title, "report.pdf");
        assert!(matches!(
            results[2].item.action,
            LaunchAction::OpenUrl { .. }
        ));
        assert!(matches!(
            results[1].item.action,
            LaunchAction::OpenPath { .. }
        ));
    }

    #[test]
    fn clipboard_preview_collapses_whitespace_and_is_unicode_safe() {
        let long = format!("  one\n two   {}  ", "한".repeat(80));
        let preview = clipboard_preview(&long);

        assert!(preview.starts_with("one two "));
        assert!(preview.ends_with('…'));
        assert_eq!(preview.chars().count(), 73);
    }

    #[test]
    fn command_enter_reveals_an_application_without_recording_a_launch() {
        let item = CatalogItem::application("/Applications/Example.app", "Example");
        let mut launcher = launcher_with_item(item);

        let _reveal = handle_key(
            &mut launcher,
            keyboard::Key::Named(Named::Enter),
            keyboard::Modifiers::COMMAND,
        );

        assert!(launcher.launching);
        assert_eq!(
            launcher.notice.as_ref().map(|notice| notice.text.as_str()),
            Some("Showing Example in Finder…")
        );
        assert!(launcher.store_data.usage.is_empty());

        let _hide = update(
            &mut launcher,
            Message::RevealFinished {
                title: "Example".to_owned(),
                result: Ok(()),
            },
        );

        assert!(!launcher.launching);
        assert!(!launcher.visible);
        assert!(launcher.store_data.usage.is_empty());
    }

    #[test]
    fn command_enter_does_not_reveal_a_built_in_action() {
        let mut launcher = launcher_with_item(CatalogItem::refresh_catalog());

        let _reveal = handle_key(
            &mut launcher,
            keyboard::Key::Named(Named::Enter),
            keyboard::Modifiers::COMMAND,
        );

        assert!(!launcher.launching);
        assert_eq!(
            launcher.notice.as_ref().map(|notice| notice.text.as_str()),
            Some("Only applications, files, and folders can be shown in Finder")
        );
    }

    #[test]
    fn input_source_choices_keep_an_unavailable_saved_identifier_visible() {
        let sources = vec![platform::InputSource {
            identifier: "com.apple.keylayout.ABC".to_owned(),
            localized_name: "ABC".to_owned(),
        }];

        let choices = input_source_choices(&sources, Some("third.party.input-method"));
        let selected = selected_input_source_choice(Some("third.party.input-method"), &choices);

        assert_eq!(choices.len(), 3);
        assert_eq!(choices[0], InputSourceChoice::keep_current());
        assert_eq!(
            selected.identifier.as_deref(),
            Some("third.party.input-method")
        );
        assert!(selected.label.starts_with("Unavailable"));
    }

    #[test]
    fn input_source_choices_select_available_sources_by_stable_identifier() {
        let sources = vec![platform::InputSource {
            identifier: "com.apple.keylayout.ABC".to_owned(),
            localized_name: "ABC".to_owned(),
        }];

        let choices = input_source_choices(&sources, Some("com.apple.keylayout.ABC"));
        let selected = selected_input_source_choice(Some("com.apple.keylayout.ABC"), &choices);

        assert_eq!(choices.len(), 2);
        assert_eq!(selected.label, "ABC");
        assert_eq!(
            selected.identifier.as_deref(),
            Some("com.apple.keylayout.ABC")
        );
    }

    #[test]
    fn startup_telemetry_requires_opt_in_and_is_daily() {
        let mut data = StoreData::default();
        assert!(schedule_startup_telemetry(&mut data).is_empty());
        assert!(data.telemetry_installation_id.is_none());

        data.settings.anonymous_usage_stats_enabled = true;
        let events = schedule_startup_telemetry(&mut data);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, telemetry::Event::FirstLaunch);
        assert_eq!(events[1].0, telemetry::Event::ActiveDaily);
        assert!(data.telemetry_installation_id.is_some());
        assert!(schedule_startup_telemetry(&mut data).is_empty());
    }
}
