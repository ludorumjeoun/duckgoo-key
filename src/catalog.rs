use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// An action that can be performed when a catalog item is selected.
///
/// Keeping actions as data lets the UI display and rank results without
/// constructing command lines. Platform modules are responsible for executing
/// these actions without invoking a shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LaunchAction {
    OpenApplication { path: PathBuf },
    OpenPath { path: PathBuf },
    RefreshCatalog,
    Quit,
}

impl LaunchAction {
    pub fn target(&self) -> Option<&Path> {
        match self {
            Self::OpenApplication { path } | Self::OpenPath { path } => Some(path),
            Self::RefreshCatalog | Self::Quit => None,
        }
    }
}

/// A searchable, launchable item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogItem {
    /// Stable identity used to associate persisted usage with this item.
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub action: LaunchAction,
}

impl CatalogItem {
    pub fn application(path: impl Into<PathBuf>, title: impl Into<String>) -> Self {
        let path = path.into();
        let title = title.into();
        let id = format!("application:{}", path.to_string_lossy());
        let subtitle = Some(path.to_string_lossy().into_owned());
        let keywords = path
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.eq_ignore_ascii_case(&title))
            .map(|name| vec![name.to_owned()])
            .unwrap_or_default();

        Self {
            id,
            title,
            subtitle,
            keywords,
            action: LaunchAction::OpenApplication { path },
        }
    }

    pub fn path(path: impl Into<PathBuf>, title: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            id: format!("path:{}", path.to_string_lossy()),
            title: title.into(),
            subtitle: Some(path.to_string_lossy().into_owned()),
            keywords: Vec::new(),
            action: LaunchAction::OpenPath { path },
        }
    }

    pub fn refresh_catalog() -> Self {
        Self {
            id: "builtin:refresh-catalog".to_owned(),
            title: "Refresh Applications".to_owned(),
            subtitle: Some("Rescan installed applications".to_owned()),
            keywords: vec!["reload".to_owned(), "rescan".to_owned()],
            action: LaunchAction::RefreshCatalog,
        }
    }

    pub fn quit() -> Self {
        Self {
            id: "builtin:quit".to_owned(),
            title: "Quit DuckGooKey".to_owned(),
            subtitle: Some("Close the launcher".to_owned()),
            keywords: vec!["exit".to_owned()],
            action: LaunchAction::Quit,
        }
    }

    pub fn built_in_items() -> [Self; 2] {
        [Self::refresh_catalog(), Self::quit()]
    }
}

/// Mutable user preference and launch history for a catalog item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub item_id: String,
    #[serde(default)]
    pub launch_count: u64,
    #[serde(default)]
    pub last_launched_at_ms: Option<u64>,
    #[serde(default)]
    pub pinned: bool,
}

impl UsageRecord {
    pub fn new(item_id: impl Into<String>) -> Self {
        Self {
            item_id: item_id.into(),
            launch_count: 0,
            last_launched_at_ms: None,
            pinned: false,
        }
    }

    pub fn record_launch(&mut self, launched_at_ms: u64) {
        self.launch_count = self.launch_count.saturating_add(1);
        self.last_launched_at_ms = Some(launched_at_ms);
    }

    /// Returns a deterministic score balancing recent and repeated use.
    ///
    /// Frequency grows logarithmically so that an old lifetime favorite does
    /// not permanently outrank an item used recently. Recency uses explicit
    /// buckets to avoid floating-point ordering and make persisted results
    /// stable across platforms.
    pub fn frecency_score(&self, now_ms: u64) -> u64 {
        if self.launch_count == 0 {
            return 0;
        }

        let frequency = u64::from(self.launch_count.ilog2() + 1) * 100;
        let recency = self
            .last_launched_at_ms
            .map(|last_launched| now_ms.saturating_sub(last_launched))
            .map(recency_score)
            .unwrap_or(0);

        frequency.saturating_add(recency)
    }
}

fn recency_score(age_ms: u64) -> u64 {
    const HOUR: u64 = 60 * 60 * 1_000;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    const MONTH: u64 = 30 * DAY;
    const QUARTER: u64 = 90 * DAY;

    if age_ms <= HOUR {
        1_000
    } else if age_ms <= DAY {
        800
    } else if age_ms <= WEEK {
        500
    } else if age_ms <= MONTH {
        250
    } else if age_ms <= QUARTER {
        100
    } else {
        0
    }
}

pub fn current_unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    Prefix,
    Substring,
    Subsequence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub item: CatalogItem,
    /// `None` represents an item returned for an empty query.
    pub match_kind: Option<MatchKind>,
    pub match_score: u32,
    pub pinned: bool,
    pub frecency_score: u64,
}

/// An in-memory catalog with unique item identities.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    items: Vec<CatalogItem>,
}

impl Catalog {
    pub fn new(items: impl IntoIterator<Item = CatalogItem>) -> Self {
        let mut catalog = Self::default();
        catalog.replace(items);
        catalog
    }

    pub fn items(&self) -> &[CatalogItem] {
        &self.items
    }

    pub fn replace(&mut self, items: impl IntoIterator<Item = CatalogItem>) {
        self.items.clear();
        let mut seen = HashSet::new();
        for item in items {
            if seen.insert(item.id.clone()) {
                self.items.push(item);
            }
        }
    }

    pub fn upsert(&mut self, item: CatalogItem) {
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|existing| existing.id == item.id)
        {
            *existing = item;
        } else {
            self.items.push(item);
        }
    }

    /// Searches all user-visible fields and returns at most `limit` results.
    ///
    /// Non-empty queries preserve textual relevance first. Empty queries are
    /// ordered by pinned status and then frecency, providing a useful default
    /// launcher view.
    pub fn search(
        &self,
        query: &str,
        usage: &[UsageRecord],
        now_ms: u64,
        limit: usize,
    ) -> Vec<SearchResult> {
        if limit == 0 {
            return Vec::new();
        }

        let query = query.trim().to_lowercase();
        let has_query = !query.is_empty();
        let usage_by_id = usage_index(usage);
        let mut results = self
            .items
            .iter()
            .filter_map(|item| {
                let text_match = if has_query {
                    best_item_match(item, &query)?
                } else {
                    TextMatch {
                        kind: MatchKind::Prefix,
                        score: 0,
                    }
                };
                let usage = usage_by_id.get(item.id.as_str()).copied();

                Some(SearchResult {
                    item: item.clone(),
                    match_kind: has_query.then_some(text_match.kind),
                    match_score: text_match.score,
                    pinned: usage.is_some_and(|record| record.pinned),
                    frecency_score: usage
                        .map(|record| record.frecency_score(now_ms))
                        .unwrap_or(0),
                })
            })
            .collect::<Vec<_>>();

        results.sort_by(|left, right| {
            if has_query {
                right
                    .match_score
                    .cmp(&left.match_score)
                    .then_with(|| right.pinned.cmp(&left.pinned))
                    .then_with(|| right.frecency_score.cmp(&left.frecency_score))
                    .then_with(|| compare_item_identity(&left.item, &right.item))
            } else {
                right
                    .pinned
                    .cmp(&left.pinned)
                    .then_with(|| right.frecency_score.cmp(&left.frecency_score))
                    .then_with(|| compare_item_identity(&left.item, &right.item))
            }
        });
        results.truncate(limit);
        results
    }
}

fn usage_index(usage: &[UsageRecord]) -> HashMap<&str, &UsageRecord> {
    let mut index: HashMap<&str, &UsageRecord> = HashMap::new();
    for record in usage {
        index
            .entry(record.item_id.as_str())
            .and_modify(|existing| {
                let existing_key = (
                    existing.pinned,
                    existing.last_launched_at_ms,
                    existing.launch_count,
                );
                let candidate_key = (
                    record.pinned,
                    record.last_launched_at_ms,
                    record.launch_count,
                );
                if candidate_key > existing_key {
                    *existing = record;
                }
            })
            .or_insert(record);
    }
    index
}

fn compare_item_identity(left: &CatalogItem, right: &CatalogItem) -> std::cmp::Ordering {
    left.title
        .to_lowercase()
        .cmp(&right.title.to_lowercase())
        .then_with(|| left.id.cmp(&right.id))
}

#[derive(Clone, Copy)]
struct TextMatch {
    kind: MatchKind,
    score: u32,
}

fn best_item_match(item: &CatalogItem, query: &str) -> Option<TextMatch> {
    let mut best = match_text(&item.title, query).map(|matched| TextMatch {
        kind: matched.kind,
        score: matched.score.saturating_add(3_000),
    });

    if let Some(subtitle) = item.subtitle.as_deref() {
        best = choose_better(
            best,
            match_text(subtitle, query).map(|matched| TextMatch {
                kind: matched.kind,
                score: matched.score.saturating_add(1_500),
            }),
        );
    }

    for keyword in &item.keywords {
        best = choose_better(
            best,
            match_text(keyword, query).map(|matched| TextMatch {
                kind: matched.kind,
                score: matched.score.saturating_add(500),
            }),
        );
    }

    best
}

fn choose_better(left: Option<TextMatch>, right: Option<TextMatch>) -> Option<TextMatch> {
    match (left, right) {
        (Some(left), Some(right)) if right.score > left.score => Some(right),
        (Some(left), _) => Some(left),
        (None, right) => right,
    }
}

fn match_text(candidate: &str, query: &str) -> Option<TextMatch> {
    let candidate = candidate.to_lowercase();
    if candidate.starts_with(query) {
        let length_delta = candidate
            .chars()
            .count()
            .saturating_sub(query.chars().count());
        return Some(TextMatch {
            kind: MatchKind::Prefix,
            score: 30_000u32
                .saturating_add(u32::from(candidate == query) * 1_000)
                .saturating_add(1_000u32.saturating_sub(as_u32(length_delta))),
        });
    }

    if let Some(byte_offset) = candidate.find(query) {
        let char_offset = candidate[..byte_offset].chars().count();
        return Some(TextMatch {
            kind: MatchKind::Substring,
            score: 20_000u32.saturating_add(1_000u32.saturating_sub(as_u32(char_offset))),
        });
    }

    subsequence_match(&candidate, query)
}

fn subsequence_match(candidate: &str, query: &str) -> Option<TextMatch> {
    let query = query.chars().collect::<Vec<_>>();
    if query.is_empty() {
        return None;
    }

    let mut query_index = 0;
    let mut first_match = None;
    let mut last_match = 0;
    for (candidate_index, candidate_char) in candidate.chars().enumerate() {
        if candidate_char == query[query_index] {
            first_match.get_or_insert(candidate_index);
            last_match = candidate_index;
            query_index += 1;
            if query_index == query.len() {
                break;
            }
        }
    }

    if query_index != query.len() {
        return None;
    }

    let first_match = first_match.unwrap_or(0);
    let span = last_match.saturating_sub(first_match).saturating_add(1);
    let gaps = span.saturating_sub(query.len());
    Some(TextMatch {
        kind: MatchKind::Subsequence,
        score: 10_000u32
            .saturating_add(1_000u32.saturating_sub(as_u32(gaps) * 10))
            .saturating_add(500u32.saturating_sub(as_u32(first_match))),
    })
}

fn as_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, title: &str) -> CatalogItem {
        CatalogItem {
            id: id.to_owned(),
            title: title.to_owned(),
            subtitle: None,
            keywords: Vec::new(),
            action: LaunchAction::OpenPath {
                path: PathBuf::from(format!("/{id}")),
            },
        }
    }

    #[test]
    fn search_is_case_insensitive_and_checks_keywords() {
        let mut browser = item("browser", "Safari");
        browser.keywords = vec!["Web Browser".to_owned()];
        let catalog = Catalog::new([browser]);

        assert_eq!(catalog.search("sAF", &[], 0, 10)[0].item.id, "browser");
        assert_eq!(catalog.search("WEB BRO", &[], 0, 10)[0].item.id, "browser");
    }

    #[test]
    fn prefix_outranks_substring_which_outranks_subsequence() {
        let catalog = Catalog::new([
            item("subsequence", "Code Assistant Launcher"),
            item("substring", "Super Calendar"),
            item("prefix", "Calendar"),
        ]);

        let results = catalog.search("cal", &[], 0, 10);
        assert_eq!(
            results
                .iter()
                .map(|result| result.item.id.as_str())
                .collect::<Vec<_>>(),
            ["prefix", "substring", "subsequence"]
        );
        assert_eq!(results[0].match_kind, Some(MatchKind::Prefix));
        assert_eq!(results[1].match_kind, Some(MatchKind::Substring));
        assert_eq!(results[2].match_kind, Some(MatchKind::Subsequence));
    }

    #[test]
    fn empty_query_orders_pinned_then_frecency() {
        const NOW: u64 = 100 * 24 * 60 * 60 * 1_000;
        let catalog = Catalog::new([
            item("unused", "Unused"),
            item("frequent", "Frequent"),
            item("recent", "Recent"),
            item("pinned", "Pinned"),
        ]);
        let usage = [
            UsageRecord {
                item_id: "frequent".to_owned(),
                launch_count: 128,
                last_launched_at_ms: Some(NOW - 100 * 24 * 60 * 60 * 1_000),
                pinned: false,
            },
            UsageRecord {
                item_id: "recent".to_owned(),
                launch_count: 1,
                last_launched_at_ms: Some(NOW),
                pinned: false,
            },
            UsageRecord {
                item_id: "pinned".to_owned(),
                launch_count: 0,
                last_launched_at_ms: None,
                pinned: true,
            },
        ];

        let results = catalog.search("", &usage, NOW, 10);
        assert_eq!(
            results
                .iter()
                .map(|result| result.item.id.as_str())
                .collect::<Vec<_>>(),
            ["pinned", "recent", "frequent", "unused"]
        );
    }

    #[test]
    fn search_is_deterministic_and_honors_limit() {
        let catalog = Catalog::new([
            item("z", "Same"),
            item("a", "Same"),
            item("ignored", "Different"),
        ]);

        let results = catalog.search("same", &[], 0, 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].item.id, "a");
        assert!(catalog.search("same", &[], 0, 0).is_empty());
    }

    #[test]
    fn record_launch_saturates_and_updates_timestamp() {
        let mut usage = UsageRecord {
            item_id: "app".to_owned(),
            launch_count: u64::MAX,
            last_launched_at_ms: None,
            pinned: false,
        };

        usage.record_launch(42);

        assert_eq!(usage.launch_count, u64::MAX);
        assert_eq!(usage.last_launched_at_ms, Some(42));
    }

    #[test]
    fn catalog_keeps_unique_ids_and_upserts_in_place() {
        let mut catalog = Catalog::new([item("same", "First"), item("same", "Ignored")]);
        assert_eq!(catalog.items().len(), 1);
        assert_eq!(catalog.items()[0].title, "First");

        catalog.upsert(item("same", "Replacement"));
        assert_eq!(catalog.items().len(), 1);
        assert_eq!(catalog.items()[0].title, "Replacement");
    }
}
