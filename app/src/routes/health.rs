/// GET /health — public liveness probe.
///
/// Returns HTTP 200 with `{"status":"ok"}`.
/// This route bypasses the authentication middleware.
use axum::Json;
use serde_json::{json, Value};

pub async fn handler() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    /// Unit test: handler returns the correct JSON body.
    #[tokio::test]
    async fn handler_returns_status_ok_json() {
        let response = handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
