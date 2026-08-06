use url::Url;

use crate::catalog::{CatalogItem, LaunchAction};
use crate::store::SearchEngine;

const GOOGLE_SEARCH_URL: &str = "https://www.google.com/search";
const DUCKDUCKGO_SEARCH_URL: &str = "https://duckduckgo.com/";
const MAX_QUERY_BYTES: usize = 2 * 1024;

/// Creates the dynamic web-search fallback shown for a non-empty root query.
pub fn web_search_item(query: &str, engine: SearchEngine) -> Option<CatalogItem> {
    let query = normalize_query(query)?;
    let (search_url, engine_id) = match engine {
        SearchEngine::Google => (GOOGLE_SEARCH_URL, "google"),
        SearchEngine::DuckDuckGo => (DUCKDUCKGO_SEARCH_URL, "duckduckgo"),
    };
    let mut url = Url::parse(search_url).ok()?;
    url.query_pairs_mut().append_pair("q", &query);

    Some(CatalogItem {
        id: format!("web-search:{engine_id}:{query}"),
        title: format!("Search {engine} for “{query}”"),
        subtitle: Some(format!("Search the web with {engine}")),
        icon_path: None,
        keywords: vec![
            "web".to_owned(),
            "internet".to_owned(),
            engine_id.to_owned(),
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
    fn creates_a_non_pinnable_google_result_by_default() {
        let item = web_search_item("rust desktop launcher", SearchEngine::default()).unwrap();

        assert_eq!(item.id, "web-search:google:rust desktop launcher");
        assert_eq!(item.title, "Search Google for “rust desktop launcher”");
        assert!(!item.pinnable);
        assert!(matches!(item.action, LaunchAction::OpenUrl { .. }));
    }

    #[test]
    fn encodes_untrusted_query_text_as_a_query_parameter_for_each_engine() {
        for (engine, expected_host) in [
            (SearchEngine::Google, "www.google.com"),
            (SearchEngine::DuckDuckGo, "duckduckgo.com"),
        ] {
            let item = web_search_item("rust & url? #fragment", engine).unwrap();
            let LaunchAction::OpenUrl { url } = item.action else {
                panic!("web result must open a URL");
            };
            let url = Url::parse(&url).unwrap();

            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some(expected_host));
            assert_eq!(url.fragment(), None);
            assert_eq!(
                url.query_pairs()
                    .find(|(key, _)| key == "q")
                    .map(|(_, value)| value.into_owned()),
                Some("rust & url? #fragment".to_owned())
            );
        }
    }

    #[test]
    fn normalizes_whitespace_for_stable_display_and_identity() {
        let item = web_search_item("  duckgoo\n\tkey  ", SearchEngine::Google).unwrap();

        assert_eq!(item.id, "web-search:google:duckgoo key");
        assert_eq!(item.title, "Search Google for “duckgoo key”");
    }

    #[test]
    fn ignores_empty_or_oversized_queries() {
        assert!(web_search_item("", SearchEngine::Google).is_none());
        assert!(web_search_item(" \n\t ", SearchEngine::Google).is_none());
        assert!(web_search_item(&"a".repeat(MAX_QUERY_BYTES + 1), SearchEngine::Google).is_none());
    }
}
