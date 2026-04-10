use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

use crate::state::AppState;

/// Bearer-token authentication middleware.
/// If `mobile_api_token` is None or empty in settings, all requests pass through.
/// Accepts the token via:
///   1. `Authorization: Bearer <token>` header  (standard)
///   2. `?token=<token>` query parameter        (needed for EventSource / SSE)
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let required_token = {
        let s = state.settings.lock().unwrap();
        s.mobile_api_token.clone().filter(|t| !t.is_empty())
    };

    if let Some(token) = required_token {
        // Check Authorization header first
        let header_token = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").map(|t| t.to_string()));

        // Fall back to ?token= query param (for EventSource connections)
        let query_token = req
            .uri()
            .query()
            .and_then(|q| {
                q.split('&')
                    .find(|p| p.starts_with("token="))
                    .and_then(|p| p.strip_prefix("token="))
                    .map(|t| urlencoding::decode(t).unwrap_or_default().into_owned())
            });

        let provided = header_token.or(query_token);

        match provided {
            Some(ref t) if t == &token => {}
            _ => return Err(StatusCode::UNAUTHORIZED),
        }
    }

    Ok(next.run(req).await)
}
