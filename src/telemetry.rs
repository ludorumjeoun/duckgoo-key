//! Minimal, opt-in anonymous release telemetry.
//!
//! Event payloads intentionally exclude queries, selected apps, file paths,
//! clipboard contents, device names, and network identifiers.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

pub const TELEMETRY_ENDPOINT: &str = "https://duckgoo.net/api/key-telemetry";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    FirstLaunch,
    ActiveDaily,
}

impl Event {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FirstLaunch => "first_launch",
            Self::ActiveDaily => "active_daily",
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not create telemetry client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("telemetry service returned {0}")]
    Response(reqwest::StatusCode),
}

#[derive(Serialize)]
struct Payload<'a> {
    event: &'a str,
    installation_id: &'a str,
    platform: &'static str,
    version: &'static str,
}

pub fn new_installation_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn current_day() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400
}

pub async fn send(event: Event, installation_id: String) -> Result<(), Error> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .build()
        .map_err(Error::Client)?;
    let response = client
        .post(TELEMETRY_ENDPOINT)
        .json(&Payload {
            event: event.as_str(),
            installation_id: &installation_id,
            platform: platform(),
            version: env!("CARGO_PKG_VERSION"),
        })
        .send()
        .await
        .map_err(Error::Client)?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(Error::Response(response.status()))
    }
}

const fn platform() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "macos-aarch64"
    } else {
        "macos-x86_64"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_ids_are_v4_uuids() {
        let id = new_installation_id();
        assert_eq!(Uuid::parse_str(&id).unwrap().get_version_num(), 4);
    }

    #[test]
    fn telemetry_events_have_stable_wire_names() {
        assert_eq!(Event::FirstLaunch.as_str(), "first_launch");
        assert_eq!(Event::ActiveDaily.as_str(), "active_daily");
    }
}
