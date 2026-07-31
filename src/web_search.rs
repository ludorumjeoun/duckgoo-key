use url::Url;

use crate::catalog::{CatalogItem, LaunchAction};

const DUCKDUCKGO_SEARCH_URL: &str = "https://duckduckgo.com/";
const MAX_QUERY_BYTES: usize = 2 * 1024;

/// Creates the dynamic DuckDuckGo fallback shown for a non-empty root query.
pub fn web_search_item(query: &str) -> Option<CatalogItem> {
    let query = normalize_query(query)?;
    let mut url = Url::parse(DUCKDUCKGO_SEARCH_URL).ok()?;
    url.query_pairs_mut().append_pair("q", &query);

    Some(CatalogItem {
        id: format!("web-search:{query}"),
        title: format!("Search DuckDuckGo for “{query}”"),
        subtitle: Some("Search the web with DuckDuckGo".to_owned()),
        icon_path: None,
        keywords: vec![
            "web".to_owned(),
            "internet".to_owned(),
            "duckduckgo".to_owned(),
            "search".to_owned(),
        ],
        action: LaunchAction::OpenUrl {
            url: url.to_string(),
        },
        pinnable: false,
    })
}

fn normalize_query(query: &str) -> Option<String> {
    let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
    (!query.is_empty() && query.len() <= MAX_QUERY_BYTES).then_some(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_non_pinnable_duckduckgo_result() {
        let item = web_search_item("rust desktop launcher").unwrap();

        assert_eq!(item.id, "web-search:rust desktop launcher");
        assert_eq!(item.title, "Search DuckDuckGo for “rust desktop launcher”");
        assert!(!item.pinnable);
        assert!(matches!(item.action, LaunchAction::OpenUrl { .. }));
    }

    #[test]
    fn encodes_untrusted_query_text_as_a_query_parameter() {
        let item = web_search_item("rust & url? #fragment").unwrap();
        let LaunchAction::OpenUrl { url } = item.action else {
            panic!("web result must open a URL");
        };
        let url = Url::parse(&url).unwrap();

        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("duckduckgo.com"));
        assert_eq!(url.fragment(), None);
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "q")
                .map(|(_, value)| value.into_owned()),
            Some("rust & url? #fragment".to_owned())
        );
    }

    #[test]
    fn normalizes_whitespace_for_stable_display_and_identity() {
        let item = web_search_item("  duckgoo\n\tkey  ").unwrap();

        assert_eq!(item.id, "web-search:duckgoo key");
        assert_eq!(item.title, "Search DuckDuckGo for “duckgoo key”");
    }

    #[test]
    fn ignores_empty_or_oversized_queries() {
        assert!(web_search_item("").is_none());
        assert!(web_search_item(" \n\t ").is_none());
        assert!(web_search_item(&"a".repeat(MAX_QUERY_BYTES + 1)).is_none());
    }
}
