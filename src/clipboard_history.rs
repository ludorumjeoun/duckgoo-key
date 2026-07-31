use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

/// Clipboard payloads are deliberately bounded before they enter persisted
/// application state.
pub const MAX_CLIPBOARD_ENTRY_BYTES: usize = 64 * 1024;
pub const MAX_CLIPBOARD_ENTRIES: usize = 100;

/// One validated plain-text clipboard observation.
///
/// Whitespace is used to reject blank values, but accepted text is preserved
/// byte-for-byte so restoring an entry never changes user content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClipboardEntry {
    id: u64,
    text: String,
    captured_at_ms: u64,
}

impl ClipboardEntry {
    pub fn new(
        id: u64,
        captured_at_ms: u64,
        text: impl Into<String>,
    ) -> Result<Self, ClipboardEntryError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(ClipboardEntryError::Blank);
        }

        let bytes = text.len();
        if bytes > MAX_CLIPBOARD_ENTRY_BYTES {
            return Err(ClipboardEntryError::TooLarge {
                bytes,
                max_bytes: MAX_CLIPBOARD_ENTRY_BYTES,
            });
        }

        Ok(Self {
            id,
            text,
            captured_at_ms,
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn captured_at_ms(&self) -> u64 {
        self.captured_at_ms
    }
}

impl<'de> Deserialize<'de> for ClipboardEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PersistedClipboardEntry {
            id: u64,
            text: String,
            captured_at_ms: u64,
        }

        let persisted = PersistedClipboardEntry::deserialize(deserializer)?;
        Self::new(persisted.id, persisted.captured_at_ms, persisted.text).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ClipboardEntryError {
    #[error("clipboard text cannot be blank")]
    Blank,
    #[error("clipboard text is {bytes} bytes, exceeding the {max_bytes}-byte limit")]
    TooLarge { bytes: usize, max_bytes: usize },
}

/// A bounded, newest-first collection suitable for local persistence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ClipboardHistory {
    entries: Vec<ClipboardEntry>,
}

impl ClipboardHistory {
    pub fn entries(&self) -> &[ClipboardEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Captures text and applies all history invariants in one operation.
    pub fn capture(
        &mut self,
        id: u64,
        captured_at_ms: u64,
        text: impl Into<String>,
    ) -> Result<(), ClipboardEntryError> {
        self.insert(ClipboardEntry::new(id, captured_at_ms, text)?);
        Ok(())
    }

    /// Inserts an already validated entry, replacing duplicate IDs or values.
    /// Ordering uses timestamp and then ID so persisted data normalizes
    /// deterministically even when it was written out of order.
    pub fn insert(&mut self, entry: ClipboardEntry) {
        let entry_key = (entry.captured_at_ms, entry.id);
        if self.entries.iter().any(|existing| {
            (existing.id == entry.id || existing.text == entry.text)
                && (existing.captured_at_ms, existing.id) >= entry_key
        }) {
            return;
        }

        self.entries
            .retain(|existing| existing.id != entry.id && existing.text != entry.text);
        self.entries.push(entry);
        self.entries.sort_by(|left, right| {
            right
                .captured_at_ms
                .cmp(&left.captured_at_ms)
                .then_with(|| right.id.cmp(&left.id))
        });
        self.entries.truncate(MAX_CLIPBOARD_ENTRIES);
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let previous_len = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        self.entries.len() != previous_len
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns matching entries without disturbing their newest-first order.
    pub fn search(&self, query: &str) -> Vec<&ClipboardEntry> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return self.entries.iter().collect();
        }

        self.entries
            .iter()
            .filter(|entry| entry.text.to_lowercase().contains(&query))
            .collect()
    }
}

impl<'de> Deserialize<'de> for ClipboardHistory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PersistedClipboardHistory {
            #[serde(default)]
            entries: Vec<ClipboardEntry>,
        }

        let persisted = PersistedClipboardHistory::deserialize(deserializer)?;
        let mut history = Self::default();
        for entry in persisted.entries {
            history.insert(entry);
        }
        Ok(history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, timestamp: u64, text: &str) -> ClipboardEntry {
        ClipboardEntry::new(id, timestamp, text).unwrap()
    }

    #[test]
    fn entry_rejects_blank_and_oversized_text_but_preserves_valid_whitespace() {
        assert_eq!(
            ClipboardEntry::new(1, 10, " \n\t ").unwrap_err(),
            ClipboardEntryError::Blank
        );

        let oversized = "x".repeat(MAX_CLIPBOARD_ENTRY_BYTES + 1);
        assert!(matches!(
            ClipboardEntry::new(1, 10, oversized),
            Err(ClipboardEntryError::TooLarge { .. })
        ));

        let accepted = ClipboardEntry::new(2, 20, "  keep me  \n").unwrap();
        assert_eq!(accepted.text(), "  keep me  \n");
    }

    #[test]
    fn history_is_newest_first_and_deduplicates_values() {
        let mut history = ClipboardHistory::default();
        history.insert(entry(1, 100, "first"));
        history.insert(entry(2, 200, "second"));
        history.insert(entry(3, 300, "first"));

        assert_eq!(
            history
                .entries()
                .iter()
                .map(ClipboardEntry::id)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
    }

    #[test]
    fn history_keeps_only_the_one_hundred_newest_entries() {
        let mut history = ClipboardHistory::default();
        for id in 0..150 {
            history.insert(entry(id, id, &format!("value {id}")));
        }

        assert_eq!(history.len(), MAX_CLIPBOARD_ENTRIES);
        assert_eq!(history.entries().first().unwrap().id(), 149);
        assert_eq!(history.entries().last().unwrap().id(), 50);
    }

    #[test]
    fn search_is_case_insensitive_and_preserves_history_order() {
        let mut history = ClipboardHistory::default();
        history.insert(entry(1, 100, "Rust book"));
        history.insert(entry(2, 200, "DuckGooKey"));
        history.insert(entry(3, 300, "RUST release notes"));

        let matches = history.search(" rust ");
        assert_eq!(
            matches
                .into_iter()
                .map(ClipboardEntry::id)
                .collect::<Vec<_>>(),
            vec![3, 1]
        );
    }

    #[test]
    fn delete_clear_and_round_trip_preserve_invariants() {
        let mut history = ClipboardHistory::default();
        history.insert(entry(1, 100, "old"));
        history.insert(entry(2, 200, "new"));

        assert!(history.delete(1));
        assert!(!history.delete(99));

        let serialized = serde_json::to_string(&history).unwrap();
        let restored: ClipboardHistory = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored, history);

        history.clear();
        assert!(history.is_empty());
    }

    #[test]
    fn deserialization_normalizes_order_duplicates_and_capacity() {
        let mut persisted_entries = Vec::new();
        for id in 0..105 {
            persisted_entries.push(serde_json::json!({
                "id": id,
                "text": format!("value {id}"),
                "captured_at_ms": id,
            }));
        }
        persisted_entries.push(serde_json::json!({
            "id": 999,
            "text": "value 104",
            "captured_at_ms": 999,
        }));
        let serialized = serde_json::json!({ "entries": persisted_entries }).to_string();

        let restored: ClipboardHistory = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.len(), MAX_CLIPBOARD_ENTRIES);
        assert_eq!(restored.entries().first().unwrap().id(), 999);
        assert_eq!(
            restored
                .entries()
                .iter()
                .filter(|entry| entry.text() == "value 104")
                .count(),
            1
        );
    }

    #[test]
    fn deserialization_keeps_the_newest_duplicate_regardless_of_input_order() {
        let serialized = serde_json::json!({
            "entries": [
                { "id": 2, "text": "same", "captured_at_ms": 200 },
                { "id": 1, "text": "same", "captured_at_ms": 100 }
            ]
        })
        .to_string();

        let restored: ClipboardHistory = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored.entries()[0].id(), 2);
    }
}
