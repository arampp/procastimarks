/// SQLite schema initialisation.
///
/// # ATAM mandatory conditions
///
/// * **C-3** — Three DML triggers created with `CREATE TRIGGER IF NOT EXISTS`.
/// * **C-6** — `PRAGMA journal_mode=WAL` issued once at startup *before* any query.
///
/// Both conditions are enforced in [`init_db`] and verified by the tests below.
use anyhow::Context;
use rusqlite::Connection;

/// Initialise (or verify) the SQLite database at `database_url`.
///
/// * Issues `PRAGMA journal_mode=WAL` first (C-6).
/// * Creates all tables and the FTS5 virtual table if they do not exist.
/// * Creates the three DML triggers that keep the FTS index in sync (C-3).
///
/// The function is **idempotent**: calling it on an already-initialised database
/// is safe and produces no error.
pub fn init_db(database_url: &str) -> anyhow::Result<()> {
    let conn = Connection::open(database_url)
        .with_context(|| format!("Cannot open SQLite database at {database_url}"))?;
    run_schema(&conn)
}

/// Run schema initialisation on an existing connection.
///
/// Exposed separately so tests can pass an in-memory connection without
/// going through the filesystem.
pub fn run_schema(conn: &Connection) -> anyhow::Result<()> {
    // C-6: WAL mode MUST be set before any other query.
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .context("Failed to enable WAL mode")?;

    conn.execute_batch(SCHEMA_SQL)
        .context("Failed to initialise schema")?;

    Ok(())
}

/// All DDL executed at startup.
///
/// `CREATE … IF NOT EXISTS` makes every statement idempotent.
const SCHEMA_SQL: &str = r#"
-- ── Main table ──────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS bookmarks (
    id          INTEGER  PRIMARY KEY AUTOINCREMENT,
    url         TEXT     NOT NULL UNIQUE,
    title       TEXT     NOT NULL DEFAULT '',
    description TEXT     NOT NULL DEFAULT '',
    tags        TEXT     NOT NULL DEFAULT '[]',   -- JSON array of strings
    comment     TEXT     NOT NULL DEFAULT '',
    created_at  TEXT     NOT NULL
        DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- ── FTS5 virtual table (content table = bookmarks) ───────────────────────────
-- `content` mode keeps a reference to the source table; triggers maintain sync.
CREATE VIRTUAL TABLE IF NOT EXISTS bookmarks_fts USING fts5(
    title,
    description,
    comment,
    tags,
    content='bookmarks',
    content_rowid='id'
);

-- ── Trigger: after INSERT — add row to FTS index (C-3) ───────────────────────
CREATE TRIGGER IF NOT EXISTS bookmarks_ai
AFTER INSERT ON bookmarks BEGIN
    INSERT INTO bookmarks_fts (rowid, title, description, comment, tags)
    VALUES (new.id, new.title, new.description, new.comment, new.tags);
END;

-- ── Trigger: after UPDATE — update FTS index (C-3) ───────────────────────────
CREATE TRIGGER IF NOT EXISTS bookmarks_au
AFTER UPDATE ON bookmarks BEGIN
    INSERT INTO bookmarks_fts (bookmarks_fts, rowid, title, description, comment, tags)
    VALUES ('delete', old.id, old.title, old.description, old.comment, old.tags);
    INSERT INTO bookmarks_fts (rowid, title, description, comment, tags)
    VALUES (new.id, new.title, new.description, new.comment, new.tags);
END;

-- ── Trigger: before DELETE — remove row from FTS index (C-3) ─────────────────
CREATE TRIGGER IF NOT EXISTS bookmarks_bd
BEFORE DELETE ON bookmarks BEGIN
    INSERT INTO bookmarks_fts (bookmarks_fts, rowid, title, description, comment, tags)
    VALUES ('delete', old.id, old.title, old.description, old.comment, old.tags);
END;
"#;

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    /// Open a fresh in-memory database and apply the schema.
    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_schema(&conn).expect("Schema initialisation must succeed");
        conn
    }

    // ── US-2 acceptance: schema is idempotent ─────────────────────────────

    /// Running the schema twice on the same connection must not error.
    ///
    /// Covers AC: "Schema initialisation is idempotent."
    #[test]
    fn schema_init_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_schema(&conn).expect("first init");
        run_schema(&conn).expect("second init must not error");
    }

    /// `bookmarks` table must have the required columns.
    #[test]
    fn bookmarks_table_has_required_columns() {
        let conn = setup();
        // Insert a minimal row — if a required column is missing this will panic.
        conn.execute(
            "INSERT INTO bookmarks (url, title, description, tags, comment, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "https://example.com",
                "Example",
                "A description",
                r#"["rust","leptos"]"#,
                "my note",
                "2026-01-01T00:00:00Z"
            ],
        )
        .expect("Insert must succeed");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bookmarks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    /// `url` column must carry a UNIQUE constraint.
    #[test]
    fn duplicate_url_is_rejected() {
        let conn = setup();
        conn.execute(
            "INSERT INTO bookmarks (url) VALUES (?1)",
            params!["https://example.com"],
        )
        .unwrap();

        let result = conn.execute(
            "INSERT INTO bookmarks (url) VALUES (?1)",
            params!["https://example.com"],
        );
        assert!(result.is_err(), "Duplicate URL must be rejected");
    }

    // ── US-2 / US-15 acceptance: FTS5 insert trigger (C-3, C-4) ─────────────

    /// After inserting a bookmark the FTS index must return it for a title search.
    ///
    /// Covers:
    /// * US-2 AC: "Integration test: insert a bookmark, query FTS5 — record is found"
    /// * US-15 AC: "Insert a bookmark → FTS5 query for the title returns that bookmark"
    /// * ATAM C-4 (insert signal)
    #[test]
    fn fts5_indexes_bookmark_on_insert() {
        let conn = setup();
        conn.execute(
            "INSERT INTO bookmarks (url, title, description, tags, comment)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "https://rust-lang.org",
                "The Rust Programming Language",
                "A systems programming language",
                r#"["rust","programming"]"#,
                ""
            ],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bookmarks_fts WHERE bookmarks_fts MATCH 'rust'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "FTS5 must find the inserted bookmark");
    }

    // ── US-15 acceptance: FTS5 delete trigger ────────────────────────────────

    /// After deleting a bookmark the FTS index must no longer return it.
    ///
    /// Covers US-15 AC: "Delete a bookmark → FTS5 query for the title returns zero rows"
    #[test]
    fn fts5_removes_bookmark_on_delete() {
        let conn = setup();
        conn.execute(
            "INSERT INTO bookmarks (url, title) VALUES (?1, ?2)",
            params!["https://example.com/delete-me", "DeleteTarget"],
        )
        .unwrap();

        conn.execute(
            "DELETE FROM bookmarks WHERE url = ?1",
            params!["https://example.com/delete-me"],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bookmarks_fts WHERE bookmarks_fts MATCH 'DeleteTarget'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "FTS5 must not contain the deleted bookmark");
    }

    // ── US-15 acceptance: no cross-contamination ─────────────────────────────

    /// Querying for bookmark A's title must not return bookmark B.
    ///
    /// Covers US-15 AC: "Insert bookmark A and B → query for A's title returns only A"
    #[test]
    fn fts5_no_cross_contamination() {
        let conn = setup();
        conn.execute(
            "INSERT INTO bookmarks (url, title) VALUES (?1, ?2)",
            params!["https://alpha.com", "AlphaUniqueTitle"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO bookmarks (url, title) VALUES (?1, ?2)",
            params!["https://beta.com", "BetaUniqueTitle"],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bookmarks_fts WHERE bookmarks_fts MATCH 'AlphaUniqueTitle'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "Only bookmark A must match A's title");
    }

    // ── C-6: WAL mode ────────────────────────────────────────────────────────

    /// After schema init the journal mode must be WAL (C-6).
    #[test]
    fn journal_mode_is_wal() {
        // WAL mode on an in-memory database returns "memory" not "wal" —
        // this is a SQLite quirk.  We test on a real tempfile instead.
        let dir = std::env::temp_dir();
        let path = dir.join(format!("procastimarks_wal_test_{}.db", std::process::id()));
        {
            let conn = Connection::open(&path).unwrap();
            run_schema(&conn).unwrap();
            let mode: String = conn
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "wal", "journal_mode must be WAL (C-6)");
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
    }
}
