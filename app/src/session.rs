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
    time::Instant,
};
use uuid::Uuid;

/// Metadata stored for each active session.
#[derive(Debug, Clone)]
pub struct Session {
    /// Wall-clock time the session was created (for future expiry support).
    pub created_at: Instant,
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
pub fn create_session(store: &SessionStore) -> String {
    let token = Uuid::new_v4().to_string();
    store
        .write()
        .expect("session store RwLock poisoned")
        .insert(
            token.clone(),
            Session {
                created_at: Instant::now(),
            },
        );
    token
}

/// Return `true` if `token` refers to a live session in `store`.
pub fn is_valid_session(store: &SessionStore, token: &str) -> bool {
    store
        .read()
        .expect("session store RwLock poisoned")
        .contains_key(token)
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
}
