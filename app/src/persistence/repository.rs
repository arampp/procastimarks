/// Bookmark persistence — `BookmarkRepository`.
///
/// All SQL access for the bookmark entity lives here.  The repository takes a
/// shared `Arc<Mutex<Connection>>` so it can be cloned cheaply and used from
/// Leptos server functions (which run on the Tokio thread pool).
///
/// # Design decisions
///
/// * **`Arc<Mutex<Connection>>`** — `rusqlite::Connection` is not `Send` when
///   compiled without the `unlock_notify` feature, so we wrap it in a `Mutex`
///   rather than a `RwLock`.  A single SQLite file in WAL mode allows one
///   writer at a time; the `Mutex` enforces this at the Rust level.
///
/// * **`ON CONFLICT(url) DO NOTHING`** — The UNIQUE constraint on `url` is the
///   canonical source of truth for duplicate detection.  `rows_affected()` is
///   checked after the INSERT to distinguish a successful insert from a silent
///   conflict.
///
/// * **`json_each`** — Tag autocomplete queries the JSON array stored in
///   `bookmarks.tags` using SQLite's built-in `json_each` table-valued function,
///   avoiding the need for a separate join table.
use std::sync::{Arc, Mutex};

use anyhow::Context;
use rusqlite::{params, Connection};

use crate::domain::{normalise_tags, Bookmark};

// ── Insert result ─────────────────────────────────────────────────────────────

/// Outcome of a `BookmarkRepository::insert` call.
#[derive(Debug, PartialEq, Eq)]
pub enum InsertResult {
    /// The bookmark was successfully stored.
    Inserted(Bookmark),
    /// A bookmark with the same URL already exists; nothing was written.
    DuplicateUrl,
}

// ── Repository ────────────────────────────────────────────────────────────────

/// Data-access object for bookmark records.
#[derive(Clone)]
pub struct BookmarkRepository {
    conn: Arc<Mutex<Connection>>,
}

impl BookmarkRepository {
    /// Create a repository that uses the supplied connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a new bookmark.
    ///
    /// Tags are normalised (lowercase + trim + deduplicate) before storage.
    ///
    /// Returns:
    /// * `Ok(InsertResult::Inserted(bookmark))` — the row was created.
    /// * `Ok(InsertResult::DuplicateUrl)` — a bookmark with that URL already
    ///   exists; nothing was written.
    /// * `Err(_)` — an unexpected I/O, constraint, or mutex-poison error.
    pub fn insert(
        &self,
        url: &str,
        title: &str,
        description: &str,
        raw_tags: &[impl AsRef<str>],
        comment: &str,
    ) -> anyhow::Result<InsertResult> {
        let tags = normalise_tags(raw_tags);
        let tags_json = serde_json::to_string(&tags).context("Failed to serialise tags to JSON")?;

        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("DB mutex poisoned"))?;

        let rows = conn
            .execute(
                "INSERT INTO bookmarks (url, title, description, tags, comment)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(url) DO NOTHING",
                params![url, title, description, tags_json, comment],
            )
            .context("Failed to insert bookmark")?;

        if rows == 0 {
            return Ok(InsertResult::DuplicateUrl);
        }

        let id = conn.last_insert_rowid();

        let bookmark = conn
            .query_row(
                "SELECT id, url, title, description, tags, comment, created_at
                 FROM bookmarks WHERE id = ?1",
                params![id],
                row_to_bookmark,
            )
            .context("Failed to fetch newly inserted bookmark")?;

        Ok(InsertResult::Inserted(bookmark))
    }

    /// Return all distinct tags whose value starts with `prefix`, sorted
    /// alphabetically.
    ///
    /// Uses SQLite's `json_each` to explode the JSON tag array into individual
    /// rows, then filters by prefix and deduplicates across all bookmarks.
    ///
    /// The prefix is escaped before embedding in the `LIKE` pattern so that
    /// literal `%` and `_` characters in a tag prefix are treated as literals,
    /// not SQL wildcards.  The `ESCAPE '\'` clause tells SQLite to honour
    /// the backslash escape.
    pub fn fetch_tags(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("DB mutex poisoned"))?;
        let like_pattern = format!("{}%", escape_like(prefix));

        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT value
                 FROM bookmarks, json_each(bookmarks.tags)
                 WHERE value LIKE ?1 ESCAPE '\\'
                 ORDER BY value",
            )
            .context("Failed to prepare fetch_tags statement")?;

        let tags = stmt
            .query_map(params![like_pattern], |row| row.get::<_, String>(0))
            .context("Failed to execute fetch_tags query")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to collect fetch_tags results")?;

        Ok(tags)
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Escape a raw prefix string for safe use in a SQLite `LIKE` pattern with
/// `ESCAPE '\'`.
///
/// The characters `%`, `_`, and `\` are SQLite `LIKE` metacharacters; this
/// function prepends a `\` before each so they are treated as literals.
fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

// ── Row mapper ────────────────────────────────────────────────────────────────

fn row_to_bookmark(row: &rusqlite::Row<'_>) -> rusqlite::Result<Bookmark> {
    let tags_json: String = row.get(4)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Bookmark {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        tags,
        comment: row.get(5)?,
        created_at: row.get(6)?,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::schema::run_schema;

    /// Open a fresh in-memory database, apply the schema, and wrap it in a
    /// `BookmarkRepository`.
    fn setup() -> BookmarkRepository {
        let conn = Connection::open_in_memory().unwrap();
        run_schema(&conn).expect("Schema initialisation must succeed");
        BookmarkRepository::new(Arc::new(Mutex::new(conn)))
    }

    // ── insert ────────────────────────────────────────────────────────────────

    /// AC-1.4: all fields are persisted and returned in the result.
    #[test]
    fn insert_stores_all_fields() {
        let repo = setup();
        let result = repo
            .insert(
                "https://example.com/article",
                "An Interesting Article",
                "A summary.",
                &["rust", "programming"],
                "Check the async section.",
            )
            .unwrap();

        match result {
            InsertResult::Inserted(bm) => {
                assert_eq!(bm.url, "https://example.com/article");
                assert_eq!(bm.title, "An Interesting Article");
                assert_eq!(bm.description, "A summary.");
                assert_eq!(bm.tags, vec!["rust", "programming"]);
                assert_eq!(bm.comment, "Check the async section.");
                assert!(!bm.created_at.is_empty());
                assert!(bm.id > 0);
            }
            InsertResult::DuplicateUrl => panic!("Expected Inserted, got DuplicateUrl"),
        }
    }

    /// AC-1.5: tags and comment may be empty.
    #[test]
    fn insert_with_empty_tags_and_comment() {
        let repo = setup();
        let no_tags: &[&str] = &[];
        let result = repo
            .insert("https://example.com/simple", "Simple", "", no_tags, "")
            .unwrap();

        match result {
            InsertResult::Inserted(bm) => {
                assert!(bm.tags.is_empty());
                assert_eq!(bm.comment, "");
            }
            InsertResult::DuplicateUrl => panic!("Expected Inserted, got DuplicateUrl"),
        }
    }

    /// AC-1.6: inserting a duplicate URL returns `DuplicateUrl`; row count unchanged.
    #[test]
    fn insert_duplicate_url_returns_duplicate_result() {
        let repo = setup();
        let no_tags: &[&str] = &[];

        repo.insert("https://example.com/dupe", "First", "", no_tags, "")
            .unwrap();
        let result = repo
            .insert("https://example.com/dupe", "Second", "", no_tags, "")
            .unwrap();

        assert_eq!(result, InsertResult::DuplicateUrl);

        // Only one row should exist.
        let conn = repo.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bookmarks WHERE url = ?1",
                params!["https://example.com/dupe"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "Only one row should exist after duplicate insert");
    }

    /// AC-4.4 + AC-4.5: tags are lowercased and trimmed before storage.
    #[test]
    fn insert_normalises_tags_before_storage() {
        let repo = setup();
        let result = repo
            .insert(
                "https://example.com/normalise",
                "T",
                "",
                &["  Rust ", "LEPTOS", "rust"],
                "",
            )
            .unwrap();

        match result {
            InsertResult::Inserted(bm) => {
                // "rust" and "leptos" only — uppercase and duplicate merged.
                assert_eq!(bm.tags, vec!["rust", "leptos"]);
            }
            InsertResult::DuplicateUrl => panic!("Expected Inserted"),
        }
    }

    // ── fetch_tags ────────────────────────────────────────────────────────────

    /// AC-4.1: prefix query returns matching tags in alphabetical order.
    #[test]
    fn fetch_tags_returns_prefix_matches() {
        let repo = setup();
        let no_tags: &[&str] = &[];
        repo.insert("https://a.com", "A", "", &["rust", "reqwest", "rayon"], "")
            .unwrap();
        repo.insert("https://b.com", "B", "", no_tags, "").unwrap();

        let tags = repo.fetch_tags("r").unwrap();
        assert_eq!(tags, vec!["rayon", "reqwest", "rust"]);
    }

    /// AC-4.3: no match returns an empty list.
    #[test]
    fn fetch_tags_returns_empty_when_no_match() {
        let repo = setup();
        repo.insert("https://a.com", "A", "", &["rust", "leptos"], "")
            .unwrap();

        let tags = repo.fetch_tags("xyz").unwrap();
        assert!(tags.is_empty());
    }

    /// AC-4.1: tags from multiple bookmarks are deduplicated in the result.
    #[test]
    fn fetch_tags_deduplicates_across_bookmarks() {
        let repo = setup();
        repo.insert("https://a.com", "A", "", &["rust", "leptos"], "")
            .unwrap();
        repo.insert("https://b.com", "B", "", &["rust", "axum"], "")
            .unwrap();

        // "rust" appears in both bookmarks but must appear once.
        let tags = repo.fetch_tags("").unwrap();
        let rust_count = tags.iter().filter(|t| t.as_str() == "rust").count();
        assert_eq!(rust_count, 1, "rust must appear exactly once");
        assert!(tags.contains(&"leptos".to_string()));
        assert!(tags.contains(&"axum".to_string()));
    }

    /// Empty prefix returns all tags.
    #[test]
    fn fetch_tags_empty_prefix_returns_all() {
        let repo = setup();
        repo.insert("https://a.com", "A", "", &["rust", "leptos", "axum"], "")
            .unwrap();

        let tags = repo.fetch_tags("").unwrap();
        assert_eq!(tags, vec!["axum", "leptos", "rust"]);
    }
}
