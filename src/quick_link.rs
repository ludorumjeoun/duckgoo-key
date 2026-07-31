use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;
use url::Url;

use crate::catalog::{CatalogItem, LaunchAction};

pub const MAX_QUICK_LINK_TITLE_CHARS: usize = 120;
pub const MAX_QUICK_LINK_URL_BYTES: usize = 8 * 1024;

/// A user-defined, persistable shortcut to an HTTP(S) destination.
///
/// Fields are private so links loaded from disk and links created in the UI
/// share the same validation boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct QuickLink {
    id: u64,
    title: String,
    url: String,
}

impl QuickLink {
    pub fn new(
        id: u64,
        title: impl Into<String>,
        url: impl AsRef<str>,
    ) -> Result<Self, QuickLinkError> {
        let title = title.into().trim().to_owned();
        if title.is_empty() {
            return Err(QuickLinkError::EmptyTitle);
        }
        let title_chars = title.chars().count();
        if title_chars > MAX_QUICK_LINK_TITLE_CHARS {
            return Err(QuickLinkError::TitleTooLong {
                chars: title_chars,
                max_chars: MAX_QUICK_LINK_TITLE_CHARS,
            });
        }

        let raw_url = url.as_ref().trim();
        if raw_url.is_empty() {
            return Err(QuickLinkError::EmptyUrl);
        }
        if raw_url.len() > MAX_QUICK_LINK_URL_BYTES {
            return Err(QuickLinkError::UrlTooLong {
                bytes: raw_url.len(),
                max_bytes: MAX_QUICK_LINK_URL_BYTES,
            });
        }

        let parsed = Url::parse(raw_url).map_err(|error| QuickLinkError::InvalidUrl {
            detail: error.to_string(),
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(QuickLinkError::UnsupportedScheme {
                scheme: parsed.scheme().to_owned(),
            });
        }
        if parsed.host_str().is_none() {
            return Err(QuickLinkError::MissingHost);
        }
        let normalized_url = parsed.to_string();
        if normalized_url.len() > MAX_QUICK_LINK_URL_BYTES {
            return Err(QuickLinkError::UrlTooLong {
                bytes: normalized_url.len(),
                max_bytes: MAX_QUICK_LINK_URL_BYTES,
            });
        }

        Ok(Self {
            id,
            title,
            url: normalized_url,
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Projects persisted domain data into the launcher's searchable catalog.
    pub fn to_catalog_item(&self) -> CatalogItem {
        CatalogItem {
            id: format!("quick-link:{}", self.id),
            title: self.title.clone(),
            subtitle: Some(self.url.clone()),
            icon_path: None,
            keywords: vec!["quick link".to_owned(), "bookmark".to_owned()],
            pinnable: true,
            action: LaunchAction::OpenUrl {
                url: self.url.clone(),
            },
        }
    }
}

impl<'de> Deserialize<'de> for QuickLink {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PersistedQuickLink {
            id: u64,
            title: String,
            url: String,
        }

        let persisted = PersistedQuickLink::deserialize(deserializer)?;
        Self::new(persisted.id, persisted.title, persisted.url).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum QuickLinkError {
    #[error("quick link title cannot be empty")]
    EmptyTitle,
    #[error("quick link title is {chars} characters, exceeding the {max_chars}-character limit")]
    TitleTooLong { chars: usize, max_chars: usize },
    #[error("quick link URL cannot be empty")]
    EmptyUrl,
    #[error("quick link URL is {bytes} bytes, exceeding the {max_bytes}-byte limit")]
    UrlTooLong { bytes: usize, max_bytes: usize },
    #[error("quick link URL is invalid: {detail}")]
    InvalidUrl { detail: String },
    #[error("quick link URL must use http or https, not {scheme}")]
    UnsupportedScheme { scheme: String },
    #[error("quick link URL must include a host")]
    MissingHost,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_trims_title_and_normalizes_url() {
        let link = QuickLink::new(
            42,
            "  Rust documentation  ",
            "  HTTPS://Doc.Rust-Lang.ORG/std/index.html  ",
        )
        .unwrap();

        assert_eq!(link.id(), 42);
        assert_eq!(link.title(), "Rust documentation");
        assert_eq!(link.url(), "https://doc.rust-lang.org/std/index.html");
    }

    #[test]
    fn new_rejects_empty_titles_and_non_web_urls() {
        assert_eq!(
            QuickLink::new(1, "  ", "https://example.com").unwrap_err(),
            QuickLinkError::EmptyTitle
        );
        assert_eq!(
            QuickLink::new(1, "Local", "file:///tmp/example").unwrap_err(),
            QuickLinkError::UnsupportedScheme {
                scheme: "file".to_owned()
            }
        );
    }

    #[test]
    fn new_rejects_oversized_titles_and_urls() {
        assert!(matches!(
            QuickLink::new(
                1,
                "x".repeat(MAX_QUICK_LINK_TITLE_CHARS + 1),
                "https://example.com"
            ),
            Err(QuickLinkError::TitleTooLong { .. })
        ));
        let oversized_url = format!(
            "https://example.com/{}",
            "x".repeat(MAX_QUICK_LINK_URL_BYTES)
        );
        assert!(matches!(
            QuickLink::new(1, "Example", oversized_url),
            Err(QuickLinkError::UrlTooLong { .. })
        ));
    }

    #[test]
    fn deserialize_revalidates_persisted_data() {
        let invalid = r#"{"id":7,"title":"Example","url":"javascript:alert(1)"}"#;
        let error = serde_json::from_str::<QuickLink>(invalid).unwrap_err();

        assert!(error.to_string().contains("must use http or https"));
    }

    #[test]
    fn catalog_projection_is_stable_and_pinnable() {
        let link = QuickLink::new(9, "Example", "https://example.com/path").unwrap();
        let item = link.to_catalog_item();

        assert_eq!(item.id, "quick-link:9");
        assert_eq!(item.title, "Example");
        assert_eq!(item.subtitle.as_deref(), Some("https://example.com/path"));
        assert!(item.pinnable);
        assert_eq!(
            item.action,
            LaunchAction::OpenUrl {
                url: "https://example.com/path".to_owned()
            }
        );
    }
}
