/// Domain layer — Procastimarks bookmark entity and business rules.
///
/// This module is compiled for both server and WASM targets so that the
/// `Bookmark` struct can be used as the shared data type in Leptos server
/// function signatures (serialised over the wire).
use serde::{Deserialize, Serialize};

// ── Bookmark entity ───────────────────────────────────────────────────────────

/// The central domain entity — a saved web page.
///
/// `tags` contains the normalised (lowercase, trimmed, deduplicated) tag
/// list exactly as stored in the database.  `created_at` is an ISO-8601
/// UTC timestamp string (e.g. `"2026-03-28T10:00:00Z"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub description: String,
    /// Normalised tags stored as a JSON array in SQLite.
    pub tags: Vec<String>,
    pub comment: String,
    /// ISO-8601 UTC timestamp.
    pub created_at: String,
}

// ── Tag normalisation ─────────────────────────────────────────────────────────

/// Normalises a slice of raw tag strings into a canonical tag list.
///
/// Rules (arc42 §8 Tag Normalisation):
/// 1. Trim leading and trailing ASCII whitespace.
/// 2. Convert to lowercase.
/// 3. Discard any tag that is empty after trimming.
/// 4. Deduplicate, preserving the order of first occurrence.
///
/// The result is always safe to store directly in the database.
pub fn normalise_tags(raw: &[impl AsRef<str>]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for tag in raw {
        let normalised = tag.as_ref().trim().to_lowercase();
        if !normalised.is_empty() && seen.insert(normalised.clone()) {
            result.push(normalised);
        }
    }
    result
}

// ── Error types ───────────────────────────────────────────────────────────────

/// Errors that can be returned by the `save_bookmark` server function.
///
/// `Display` and `FromStr` are implemented to satisfy the
/// `ServerFnErrorSerde` trait bound required by Leptos server functions that
/// use a custom error type.  The wire format is the `Display` string; for
/// `Internal` the payload is encoded after a `':'` separator so that the
/// original message round-trips faithfully.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum SaveBookmarkError {
    /// The URL already exists in the database.
    #[error("DuplicateUrl")]
    DuplicateUrl,
    /// An unexpected server-side error occurred.
    #[error("Internal:{0}")]
    Internal(String),
}

impl std::str::FromStr for SaveBookmarkError {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "DuplicateUrl" {
            return Ok(SaveBookmarkError::DuplicateUrl);
        }
        if let Some(msg) = s.strip_prefix("Internal:") {
            return Ok(SaveBookmarkError::Internal(msg.to_owned()));
        }
        Err(format!("unknown SaveBookmarkError variant: {s}"))
    }
}

/// Human-readable messages shown in the UI.
impl SaveBookmarkError {
    pub fn user_message(&self) -> &'static str {
        match self {
            SaveBookmarkError::DuplicateUrl => "This URL is already saved.",
            SaveBookmarkError::Internal(_) => {
                "An unexpected error occurred while saving the bookmark."
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalise_tags ────────────────────────────────────────────────────────

    /// AC-4.4: tags are stored in lowercase regardless of input casing.
    #[test]
    fn normalises_tags_to_lowercase() {
        let result = normalise_tags(&["Rust", "LEPTOS", "Axum"]);
        assert_eq!(result, vec!["rust", "leptos", "axum"]);
    }

    /// AC-4.5: leading and trailing whitespace is removed from each tag.
    #[test]
    fn trims_tag_whitespace() {
        let result = normalise_tags(&["  rust  ", "\tleptos\n", " axum"]);
        assert_eq!(result, vec!["rust", "leptos", "axum"]);
    }

    /// arc42 §8 rule 3: tags that are empty after trimming are discarded.
    #[test]
    fn discards_empty_tags_after_trim() {
        let result = normalise_tags(&["rust", "   ", "", "leptos"]);
        assert_eq!(result, vec!["rust", "leptos"]);
    }

    /// arc42 §8 rule 4: duplicate tags are removed; first occurrence wins.
    #[test]
    fn deduplicates_tags_preserving_order() {
        let result = normalise_tags(&["rust", "Rust", "RUST", "leptos", "rust"]);
        assert_eq!(result, vec!["rust", "leptos"]);
    }

    /// AC-1.5: empty input yields an empty list (tags are optional).
    #[test]
    fn normalise_empty_slice_returns_empty() {
        let empty: &[&str] = &[];
        let result = normalise_tags(empty);
        assert!(result.is_empty());
    }

    /// Mixed normalisation: casing + whitespace + duplicates in one call.
    #[test]
    fn normalises_mixed_input() {
        let result = normalise_tags(&["  Rust ", "rust", "", "  LEPTOS "]);
        assert_eq!(result, vec!["rust", "leptos"]);
    }
}
