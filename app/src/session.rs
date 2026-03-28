/// In-memory session store (ATAM C-5).
///
/// Session tokens are cryptographically random UUID v4 strings.
/// The store is a `HashMap` wrapped in `Arc<RwLock<…>>` so it can be shared
/// across Axum handler threads without copying.
///
/// **Tech debt TD-4**: the store lives in process memory.  Restarting the
/// container invalidates all active sessions; the owner must present the
/// `api_key` once per browser session after a restart.  A persistent store
/// (e.g., a `sessions` table in SQLite) is deferred post-MVP.
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use uuid::Uuid;

/// How long a session remains valid after creation.
///
/// Sessions older than this are considered expired and will be cleaned up on
/// the next write (lazy eviction).
const SESSION_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

/// Metadata stored for each active session.
#[derive(Debug, Clone)]
pub struct Session {
    /// Monotonic time when the session was created (used for TTL-based expiry).
    pub created_at: Instant,
}

impl Session {
    /// Returns `true` if this session has not yet exceeded `SESSION_TTL`.
    fn is_live(&self) -> bool {
        self.created_at.elapsed() < SESSION_TTL
    }
}

/// The session store type required by ATAM C-5.
///
/// `Arc` allows cheap cloning into Axum state / middleware.
/// `RwLock` allows concurrent reads (cookie validation) and exclusive writes
/// (new session creation) without a mutex bottleneck.
pub type SessionStore = Arc<RwLock<HashMap<String, Session>>>;

/// Construct an empty session store.
pub fn new_store() -> SessionStore {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Insert a newly generated session token and return it.
///
/// The token is a UUID v4 string: cryptographically random, URL-safe, and
/// distinct from the API key (satisfies AC-6.1).
///
/// Expired sessions are lazily evicted on each call so stale entries do not
/// accumulate indefinitely.  The map can still hold up to one entry per live
/// session created within `SESSION_TTL`; callers that need a hard cap on
/// concurrent sessions should add an explicit limit before inserting.
///
/// If the `RwLock` is poisoned (a previous writer panicked while holding it),
/// the lock is recovered rather than propagating a panic — the middleware will
/// continue to function.
pub fn create_session(store: &SessionStore) -> String {
    let token = Uuid::new_v4().to_string();
    let mut map = store.write().unwrap_or_else(|e| e.into_inner());

    // Lazy TTL eviction: remove all expired sessions before inserting the new one.
    map.retain(|_, session| session.is_live());

    map.insert(
        token.clone(),
        Session {
            created_at: Instant::now(),
        },
    );
    token
}

/// Return `true` if `token` refers to a live, non-expired session in `store`.
///
/// If the `RwLock` is poisoned the lock is recovered; a poisoned store is
/// treated as having no valid sessions (fail closed → 401, not crash).
pub fn is_valid_session(store: &SessionStore, token: &str) -> bool {
    store
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(token)
        .map(|s| s.is_live())
        .unwrap_or(false)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_store_is_empty() {
        let store = new_store();
        assert!(store.read().unwrap().is_empty());
    }

    #[test]
    fn create_session_returns_non_empty_token() {
        let store = new_store();
        let token = create_session(&store);
        assert!(!token.is_empty());
    }

    #[test]
    fn created_session_is_valid() {
        let store = new_store();
        let token = create_session(&store);
        assert!(is_valid_session(&store, &token));
    }

    #[test]
    fn unknown_token_is_invalid() {
        let store = new_store();
        assert!(!is_valid_session(&store, "not-a-real-token"));
    }

    #[test]
    fn each_token_is_unique() {
        let store = new_store();
        let t1 = create_session(&store);
        let t2 = create_session(&store);
        assert_ne!(t1, t2);
    }

    #[test]
    fn token_does_not_equal_static_string() {
        // Sanity-check: UUID v4 will never equal a known literal.
        let store = new_store();
        let token = create_session(&store);
        assert_ne!(token, "test-api-key");
    }

    #[test]
    fn expired_session_is_evicted_on_next_create() {
        let store = new_store();
        // Manually insert an already-expired session.
        {
            let mut map = store.write().unwrap();
            map.insert(
                "old-token".to_string(),
                Session {
                    created_at: Instant::now() - SESSION_TTL - Duration::from_secs(1),
                },
            );
        }
        assert_eq!(store.read().unwrap().len(), 1);

        // Creating a new session triggers eviction.
        create_session(&store);

        let map = store.read().unwrap();
        assert!(
            !map.contains_key("old-token"),
            "expired session must be evicted"
        );
        assert_eq!(map.len(), 1, "only the new session should remain");
    }

    #[test]
    fn expired_session_token_is_not_valid() {
        let store = new_store();
        {
            let mut map = store.write().unwrap();
            map.insert(
                "expired-token".to_string(),
                Session {
                    created_at: Instant::now() - SESSION_TTL - Duration::from_secs(1),
                },
            );
        }
        assert!(!is_valid_session(&store, "expired-token"));
    }

    #[test]
    fn poisoned_lock_does_not_panic_on_read() {
        let store = new_store();
        // Poison the lock by panicking inside a write guard.
        let store_clone = Arc::clone(&store);
        let _ = std::panic::catch_unwind(move || {
            let _guard = store_clone.write().unwrap();
            panic!("intentional poison");
        });
        assert!(store.is_poisoned());
        // is_valid_session must recover — must not panic.
        assert!(!is_valid_session(&store, "any-token"));
    }

    #[test]
    fn poisoned_lock_does_not_panic_on_write() {
        let store = new_store();
        let store_clone = Arc::clone(&store);
        let _ = std::panic::catch_unwind(move || {
            let _guard = store_clone.write().unwrap();
            panic!("intentional poison");
        });
        assert!(store.is_poisoned());
        // create_session must recover — must not panic.
        let token = create_session(&store);
        assert!(!token.is_empty());
    }
}
