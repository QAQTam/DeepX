//! POST /api/chat — User message → agent turn → SSE stream of Agent2Ui events.

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use dsx_proto::{Agent2Ui, Ui2Agent};
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::AppState;

#[derive(Deserialize)]
pub struct ChatRequest {
    pub text: String,
}

/// POST /api/chat — SSE endpoint.
///
/// Sends the user message to the serialized agent loop, collects
/// Agent2Ui events, and streams them as SSE message/error/done events.
pub async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let (reply_tx, mut reply_rx) = broadcast::channel::<Agent2Ui>(256);

    // Send to agent loop
    let _ = state.input_tx.send((Ui2Agent::UserInput { text: req.text }, reply_tx));

    let stream = async_stream::stream! {
        loop {
            match reply_rx.recv().await {
                Ok(frame) => {
                    let json = serde_json::to_string(&frame).unwrap_or_default();
                    match &frame {
                        Agent2Ui::Error { .. } => {
                            yield Ok(Event::default().event("error").data(json));
                            break;
                        }
                        Agent2Ui::Done => {
                            yield Ok(Event::default().event("done").data(json));
                            break;
                        }
                        _ => {
                            yield Ok(Event::default().event("message").data(json));
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE client lagged by {n} messages");
                    continue;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
