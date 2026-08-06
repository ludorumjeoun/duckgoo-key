pub mod app;
pub mod app_icon;
pub mod calculator;
pub mod catalog;
pub mod clipboard_history;
pub mod commands;
pub mod integrations;
pub mod platform;
pub mod quick_link;
pub mod shortcut;
pub mod store;
pub mod telemetry;
pub mod updater;
pub mod web_search;

pub use app::run;

pub(crate) const START_HIDDEN_ARGUMENT: &str = "--hidden";
