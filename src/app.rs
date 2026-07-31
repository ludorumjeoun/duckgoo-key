use std::time::Duration;

use iced::event;
use iced::keyboard::{self, key::Named};
use iced::theme;
use iced::widget::{self, button, container, text, text_input};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Fill, Shadow, Size, Subscription, Task,
    Theme, Vector, time, window,
};

use crate::catalog::{
    Catalog, CatalogItem, LaunchAction, SearchResult, UsageRecord, current_unix_time_ms,
};
use crate::integrations::{DesktopIntegrations, IntegrationEvent};
use crate::platform;
use crate::store::Store;

const RESULT_LIMIT: usize = 8;
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);

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
    query: String,
    selected: usize,
    visible: bool,
    window_focused: bool,
    loading: bool,
    launching: bool,
    launch_at_login: bool,
    catalog: Catalog,
    usage: Vec<UsageRecord>,
    store: Option<Store>,
    integrations: Option<DesktopIntegrations>,
    notice: Option<Notice>,
}

#[derive(Debug, Clone)]
enum Message {
    WindowOpened(window::Id),
    WindowFocused(window::Id),
    WindowUnfocused(window::Id),
    WindowCloseRequested(window::Id),
    QueryChanged(String),
    KeyPressed {
        window: window::Id,
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
    },
    ActivateItem(String),
    TogglePinned(String),
    CatalogLoaded(Result<Vec<CatalogItem>, String>),
    LaunchFinished {
        item_id: String,
        result: Result<(), String>,
    },
    LaunchAtLoginFinished {
        enabled: bool,
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
    let (store, usage, notice) = load_store();
    let launch_at_login = platform::launch_at_login_enabled();
    let starts_hidden = std::env::args_os()
        .skip(1)
        .any(|argument| argument == std::ffi::OsStr::new(crate::START_HIDDEN_ARGUMENT));
    let mut window_settings = window::Settings {
        size: Size::new(760.0, 620.0),
        position: window::Position::Centered,
        visible: !starts_hidden,
        resizable: false,
        closeable: true,
        minimizable: false,
        decorations: false,
        transparent: true,
        blur: true,
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
        query: String::new(),
        selected: 0,
        visible: !starts_hidden,
        window_focused: false,
        loading: true,
        launching: false,
        launch_at_login,
        catalog: Catalog::new(CatalogItem::built_in_items()),
        usage,
        store,
        integrations: None,
        notice,
    };

    (
        state,
        Task::batch([opened.map(Message::WindowOpened), discover_catalog()]),
    )
}

fn load_store() -> (Option<Store>, Vec<UsageRecord>, Option<Notice>) {
    let store = match Store::open_default() {
        Ok(store) => store,
        Err(error) => {
            return (
                None,
                Vec::new(),
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
            (Some(store), outcome.data.usage, notice)
        }
        Err(error) => (
            None,
            Vec::new(),
            Some(Notice::error(format!(
                "History could not be loaded: {error}"
            ))),
        ),
    }
}

fn update(state: &mut Launcher, message: Message) -> Task<Message> {
    match message {
        Message::WindowOpened(window) if window == state.launcher_window => {
            state.window_focused = false;
            let integrations = DesktopIntegrations::initialize(state.launch_at_login);
            if !integrations.warnings().is_empty() {
                state.notice = Some(Notice::error(integrations.warnings().join(" ")));
            }
            state.integrations = Some(integrations);

            if state.visible {
                window::gain_focus(state.launcher_window)
                    .chain(widget::operation::focus(state.query_input.clone()))
            } else {
                Task::none()
            }
        }
        Message::WindowOpened(_) => Task::none(),
        Message::WindowFocused(window) if window == state.launcher_window => {
            state.window_focused = true;
            Task::none()
        }
        Message::WindowFocused(_) => Task::none(),
        Message::WindowUnfocused(window) if window == state.launcher_window => {
            let should_hide = state.window_focused && state.visible && !state.launching;
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
            state.query = query;
            state.selected = 0;
            Task::none()
        }
        Message::KeyPressed {
            window,
            key,
            modifiers,
        } if window == state.launcher_window && state.visible => handle_key(state, key, modifiers),
        Message::KeyPressed { .. } => Task::none(),
        Message::ActivateItem(item_id) => activate_item(state, &item_id),
        Message::TogglePinned(item_id) => {
            toggle_pinned(state, &item_id);
            Task::none()
        }
        Message::CatalogLoaded(result) => {
            state.loading = false;
            match result {
                Ok(applications) => {
                    let application_count = applications.len();
                    state.catalog.replace(
                        CatalogItem::built_in_items()
                            .into_iter()
                            .chain(applications),
                    );
                    state.selected = state.selected.min(state.results().len().saturating_sub(1));
                    if state.notice.is_none() {
                        state.notice = Some(Notice::info(format!(
                            "{application_count} applications ready"
                        )));
                    }
                }
                Err(error) => {
                    state.notice = Some(Notice::error(format!(
                        "Applications could not be indexed: {error}"
                    )));
                }
            }
            Task::none()
        }
        Message::LaunchFinished { item_id, result } => {
            state.launching = false;
            match result {
                Ok(()) => {
                    record_launch(state, &item_id);
                    hide_launcher(state)
                }
                Err(error) => {
                    state.notice = Some(Notice::error(format!("Could not open item: {error}")));
                    window::gain_focus(state.launcher_window)
                        .chain(widget::operation::focus(state.query_input.clone()))
                }
            }
        }
        Message::LaunchAtLoginFinished { enabled, result } => {
            match result {
                Ok(()) => {
                    state.launch_at_login = enabled;
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
        Message::PollNativeEvents => poll_native_events(state),
    }
}

fn handle_key(
    state: &mut Launcher,
    key: keyboard::Key,
    modifiers: keyboard::Modifiers,
) -> Task<Message> {
    match key.as_ref() {
        keyboard::Key::Named(Named::ArrowDown) => {
            let result_count = state.results().len();
            if result_count > 0 {
                state.selected = (state.selected + 1).min(result_count - 1);
            }
            Task::none()
        }
        keyboard::Key::Named(Named::ArrowUp) => {
            state.selected = state.selected.saturating_sub(1);
            Task::none()
        }
        keyboard::Key::Named(Named::Enter) => activate_selected(state),
        keyboard::Key::Named(Named::Escape) => hide_launcher(state),
        keyboard::Key::Character(character)
            if modifiers.command() && character.eq_ignore_ascii_case("p") =>
        {
            if let Some(result) = state.results().get(state.selected) {
                let item_id = result.item.id.clone();
                toggle_pinned(state, &item_id);
            }
            Task::none()
        }
        _ => Task::none(),
    }
}

fn activate_selected(state: &mut Launcher) -> Task<Message> {
    let results = state.results();
    let Some(result) = results.get(state.selected) else {
        return Task::none();
    };
    let item_id = result.item.id.clone();
    activate_item(state, &item_id)
}

fn activate_item(state: &mut Launcher, item_id: &str) -> Task<Message> {
    if state.launching {
        return Task::none();
    }

    let Some(item) = state
        .catalog
        .items()
        .iter()
        .find(|item| item.id == item_id)
        .cloned()
    else {
        return Task::none();
    };

    match item.action {
        LaunchAction::RefreshCatalog => {
            state.loading = true;
            state.notice = Some(Notice::info("Refreshing applications…"));
            discover_catalog()
        }
        LaunchAction::Quit => iced::exit(),
        action @ (LaunchAction::OpenApplication { .. } | LaunchAction::OpenPath { .. }) => {
            state.launching = true;
            state.notice = Some(Notice::info(format!("Opening {}…", item.title)));
            let item_id = item.id;
            Task::perform(
                async move { platform::launch(&action).map_err(|error| error.to_string()) },
                move |result| Message::LaunchFinished { item_id, result },
            )
        }
    }
}

fn discover_catalog() -> Task<Message> {
    Task::perform(
        async { platform::discover_applications().map_err(|error| error.to_string()) },
        Message::CatalogLoaded,
    )
}

fn toggle_pinned(state: &mut Launcher, item_id: &str) {
    let record = usage_for_mut(&mut state.usage, item_id);
    record.pinned = !record.pinned;
    let pinned = record.pinned;
    state.notice = Some(Notice::info(if pinned {
        "Pinned to the top"
    } else {
        "Removed from pinned items"
    }));
    save_usage(state);
}

fn record_launch(state: &mut Launcher, item_id: &str) {
    usage_for_mut(&mut state.usage, item_id).record_launch(current_unix_time_ms());
    save_usage(state);
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

fn save_usage(state: &mut Launcher) {
    let Some(store) = &state.store else {
        return;
    };

    if let Err(error) = store.save_usage(&state.usage) {
        state.notice = Some(Notice::error(format!(
            "History could not be saved: {error}"
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
            if state.visible {
                hide_launcher(state)
            } else {
                show_launcher(state)
            }
        }
        IntegrationEvent::ShowLauncher => show_launcher(state),
        IntegrationEvent::SetLaunchAtLogin(enabled) => Task::perform(
            async move { platform::set_launch_at_login(enabled).map_err(|error| error.to_string()) },
            move |result| Message::LaunchAtLoginFinished { enabled, result },
        ),
        IntegrationEvent::Quit => iced::exit(),
    }
}

fn show_launcher(state: &mut Launcher) -> Task<Message> {
    if !state.visible {
        state.query.clear();
        state.selected = 0;
    }
    state.visible = true;
    state.window_focused = false;

    window::set_mode(state.launcher_window, window::Mode::Windowed)
        .chain(window::gain_focus(state.launcher_window))
        .chain(widget::operation::focus(state.query_input.clone()))
}

fn hide_launcher(state: &mut Launcher) -> Task<Message> {
    state.visible = false;
    state.window_focused = false;
    window::set_mode(state.launcher_window, window::Mode::Hidden)
}

fn view(state: &Launcher, _window: window::Id) -> Element<'_, Message> {
    let brand = iced::widget::row![
        container(text("DK").size(12).color(ACCENT))
            .width(30)
            .height(30)
            .center_x(Fill)
            .center_y(Fill)
            .style(monogram_style),
        iced::widget::column![
            text("DuckGooKey").size(16).color(TEXT_PRIMARY),
            text("Rust desktop launcher").size(11).color(TEXT_SECONDARY),
        ]
        .spacing(1),
        widget::Space::new().width(Fill),
        text("⌥ Space").size(12).color(TEXT_SECONDARY),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    let search = text_input("Search applications and actions…", &state.query)
        .id(state.query_input.clone())
        .on_input(Message::QueryChanged)
        .padding([14, 16])
        .size(20)
        .style(search_input_style);

    let results = state.results();
    let mut result_list = iced::widget::column![].spacing(5);
    if results.is_empty() && !state.loading {
        result_list = result_list.push(
            container(
                iced::widget::column![
                    text("No matches").size(16).color(TEXT_PRIMARY),
                    text("Try a shorter application name")
                        .size(12)
                        .color(TEXT_SECONDARY),
                ]
                .spacing(4)
                .align_x(Alignment::Center),
            )
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill),
        );
    } else {
        for (index, result) in results.into_iter().enumerate() {
            result_list = result_list.push(result_row(
                result,
                index,
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
    } else {
        text("Ready").size(11).color(TEXT_SECONDARY)
    };

    let footer = iced::widget::row![
        status,
        widget::Space::new().width(Fill),
        text("↑↓ navigate   ↵ open   ⌘P pin   esc hide")
            .size(11)
            .color(TEXT_SECONDARY),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let content =
        iced::widget::column![brand, search, container(result_list).height(Fill), footer,]
            .spacing(12);

    let panel = container(content)
        .padding(18)
        .width(Fill)
        .height(Fill)
        .style(panel_style);

    container(panel)
        .padding(12)
        .width(Fill)
        .height(Fill)
        .style(transparent_style)
        .into()
}

fn result_row(
    result: SearchResult,
    index: usize,
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
    let icon = container(text(monogram).size(14).color(TEXT_PRIMARY))
        .width(36)
        .height(36)
        .center_x(Fill)
        .center_y(Fill)
        .style(move |_| result_icon_style(selected));

    let mut labels =
        iced::widget::column![text(result.item.title.clone()).size(14).color(TEXT_PRIMARY),]
            .spacing(2);
    if let Some(subtitle) = result.item.subtitle.clone() {
        labels = labels.push(text(subtitle).size(11).color(TEXT_SECONDARY));
    }

    let shortcut = if index < 9 {
        (index + 1).to_string()
    } else {
        String::new()
    };
    let item_id = result.item.id.clone();
    let open = button(
        iced::widget::row![
            icon,
            labels.width(Fill),
            text(shortcut).size(11).color(TEXT_SECONDARY),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([7, 9])
    .on_press_maybe((!launching).then(|| Message::ActivateItem(item_id.clone())))
    .style(move |_, status| result_button_style(selected, status));

    let pin_label = if result.pinned { "★" } else { "☆" };
    let pin = button(text(pin_label).size(16))
        .width(42)
        .padding([15, 10])
        .on_press(Message::TogglePinned(item_id))
        .style(move |_, status| pin_button_style(result.pinned, status));

    iced::widget::row![open, pin]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

fn subscription(_state: &Launcher) -> Subscription<Message> {
    Subscription::batch([
        event::listen_with(listen_for_events),
        time::every(EVENT_POLL_INTERVAL).map(|_| Message::PollNativeEvents),
    ])
}

fn listen_for_events(event: Event, _status: event::Status, window: window::Id) -> Option<Message> {
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
        }),
        Event::Window(window::Event::Focused) => Some(Message::WindowFocused(window)),
        Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused(window)),
        Event::Window(window::Event::CloseRequested) => Some(Message::WindowCloseRequested(window)),
        _ => None,
    }
}

impl Launcher {
    fn results(&self) -> Vec<SearchResult> {
        self.catalog.search(
            &self.query,
            &self.usage,
            current_unix_time_ms(),
            RESULT_LIMIT,
        )
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
            a: 0.96,
        })
        .border(Border {
            radius: 18.0.into(),
            width: 1.0,
            color: Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.10,
            },
        })
        .shadow(Shadow {
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.45,
            },
            offset: Vector::new(0.0, 12.0),
            blur_radius: 36.0,
        })
}

fn monogram_style(_: &Theme) -> container::Style {
    container::Style::default()
        .background(Color {
            r: 0.08,
            g: 0.22,
            b: 0.19,
            a: 1.0,
        })
        .border(Border {
            radius: 9.0.into(),
            width: 1.0,
            color: Color {
                r: ACCENT.r,
                g: ACCENT.g,
                b: ACCENT.b,
                a: 0.35,
            },
        })
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
            width: if focused { 1.5 } else { 1.0 },
            color: if focused {
                Color { a: 0.75, ..ACCENT }
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

fn result_button_style(selected: bool, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let background = if selected {
        Color {
            r: 0.10,
            g: 0.26,
            b: 0.22,
            a: 0.96,
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
                a: 0.32,
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
}
