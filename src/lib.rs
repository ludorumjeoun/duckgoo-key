pub mod app;
pub mod catalog;
pub mod integrations;
pub mod platform;
pub mod store;

pub use app::run;

pub(crate) const START_HIDDEN_ARGUMENT: &str = "--hidden";
