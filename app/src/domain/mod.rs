/// Domain layer — Procastimarks bookmark entity and business rules.
///
/// This module is compiled for both server and WASM targets so that the
/// `Bookmark` struct can be used as the shared data type in Leptos server
/// function signatures (serialised over the wire).
use serde::{Deserialize, Serialize};

// ── Metadata ──────────────────────────────────────────────────────────────────

/// Title and description extracted from a web page, or a safe fallback.
///
/// Returned by the `fetch_metadata` server function and by `MetadataFetcher`.
/// Lives in the domain module so it is compiled for both server and WASM
/// targets (Leptos server function stubs on WASM need the type for
/// deserialization).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// Page title (`<title>` text), or the raw URL on any failure.
    pub title: String,
    /// Meta description, or empty string on any failure.
    pub description: String,
}

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
/// use a custom error type.  The wire format is the `Display` string.
///
/// **Security note:** the `Internal` variant carries the raw error detail as
/// a field for server-side logging, but the `Display` implementation emits
/// only the opaque token `"Internal"` so that internal database error messages
/// are never sent to the browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum SaveBookmarkError {
    /// The URL already exists in the database.
    #[error("DuplicateUrl")]
    DuplicateUrl,
    /// An unexpected server-side error occurred.
    ///
    /// The `String` field contains the full error message for server-side
    /// logging only; it is **not** transmitted to the client (the wire format
    /// is the fixed token `"Internal"`).
    #[error("Internal")]
    Internal(String),
}

impl std::str::FromStr for SaveBookmarkError {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "DuplicateUrl" {
            return Ok(SaveBookmarkError::DuplicateUrl);
        }
        if s == "Internal" {
            // The detail is intentionally not transmitted; reconstruct with an
            // empty string so the type round-trips without leaking internals.
            return Ok(SaveBookmarkError::Internal(String::new()));
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

    // ── SaveBookmarkError wire format ─────────────────────────────────────────

    /// The wire format of `DuplicateUrl` is the exact token "DuplicateUrl".
    #[test]
    fn duplicate_url_display_is_opaque_token() {
        assert_eq!(SaveBookmarkError::DuplicateUrl.to_string(), "DuplicateUrl");
    }

    /// The wire format of `Internal` is the fixed opaque token "Internal",
    /// regardless of the detail string, so DB errors never reach the browser.
    #[test]
    fn internal_display_does_not_leak_detail() {
        let err = SaveBookmarkError::Internal("rusqlite: disk I/O error".to_string());
        let wire = err.to_string();
        assert_eq!(
            wire, "Internal",
            "Internal wire format must not include detail"
        );
        assert!(
            !wire.contains("rusqlite"),
            "Internal wire format must not contain the error detail"
        );
    }

    /// `Internal` round-trips through `FromStr` with an empty detail field.
    #[test]
    fn internal_from_str_round_trips_without_detail() {
        let parsed = "Internal".parse::<SaveBookmarkError>().unwrap();
        assert_eq!(parsed, SaveBookmarkError::Internal(String::new()));
    }

    /// `DuplicateUrl` round-trips through `FromStr`.
    #[test]
    fn duplicate_url_from_str_round_trips() {
        let parsed = "DuplicateUrl".parse::<SaveBookmarkError>().unwrap();
        assert_eq!(parsed, SaveBookmarkError::DuplicateUrl);
    }
}
