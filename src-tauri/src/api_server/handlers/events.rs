use axum::{
    extract::{Query, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
};
use futures_util::stream::StreamExt;
use serde::Deserialize;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;

use crate::api_server::events::ApiEvent;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct EventsQuery {
    pub conversation_id: Option<String>,
}

pub async fn sse_events(
    State(state): State<Arc<AppState>>,
    Query(q): Query<EventsQuery>,
) -> impl IntoResponse {
    let rx = state.event_tx.subscribe();
    let stream = BroadcastStream::new(rx);
    let filter_conv = q.conversation_id;

    let event_stream = stream.filter_map(move |msg| {
        let filter = filter_conv.clone();
        async move {
            match msg {
                Ok(event) => {
                    // Filter by conversation_id if provided
                    if let Some(ref cid) = filter {
                        let event_conv = match &event {
                            ApiEvent::ChatToken { conversation_id, .. } => Some(conversation_id.clone()),
                            ApiEvent::ChatThinking { conversation_id, .. } => Some(conversation_id.clone()),
                            ApiEvent::ChatToolCall { conversation_id, .. } => Some(conversation_id.clone()),
                            ApiEvent::ChatToolResult { conversation_id, .. } => Some(conversation_id.clone()),
                            ApiEvent::ArtifactUpdated { conversation_id, .. } => Some(conversation_id.clone()),
                            ApiEvent::GalleryUpdated { conversation_id } => Some(conversation_id.clone()),
                            ApiEvent::ChatThinkingClear { conversation_id } => Some(conversation_id.clone()),
                            _ => None,
                        };
                        if let Some(ecid) = event_conv {
                            if &ecid != cid {
                                return None;
                            }
                        }
                    }

                    let data = serde_json::to_string(&event).ok()?;
                    Some(Ok::<Event, std::convert::Infallible>(
                        Event::default().data(data),
                    ))
                }
                Err(_) => None,
            }
        }
    });

    Sse::new(event_stream).keep_alive(KeepAlive::default())
}
