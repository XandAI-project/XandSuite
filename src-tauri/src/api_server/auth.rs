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
        let auth_header = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok());

        match auth_header {
            Some(h) if h.starts_with("Bearer ") => {
                let provided = &h["Bearer ".len()..];
                if provided != token {
                    return Err(StatusCode::UNAUTHORIZED);
                }
            }
            _ => return Err(StatusCode::UNAUTHORIZED),
        }
    }

    Ok(next.run(req).await)
}
