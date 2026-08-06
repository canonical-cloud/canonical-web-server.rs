use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::broadcast;
use uuid::Uuid;

const MAX_SOCKETS_PER_SUBJECT: usize = 4;
const MAX_SOCKETS_PER_INSTANCE: usize = 1_024;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteEvent {
    #[serde(skip)]
    pub owner_subject: String,
    pub quote_id: Uuid,
    pub status: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct QuoteHub {
    sender: broadcast::Sender<QuoteEvent>,
    connections: Arc<Mutex<ConnectionCounts>>,
}

#[derive(Default)]
struct ConnectionCounts {
    total: usize,
    by_subject: HashMap<String, usize>,
}

pub struct SocketPermit {
    connections: Arc<Mutex<ConnectionCounts>>,
    subject: String,
}

impl Drop for SocketPermit {
    fn drop(&mut self) {
        let mut counts = self
            .connections
            .lock()
            .expect("quote WebSocket connection counter lock poisoned");
        counts.total = counts.total.saturating_sub(1);
        if let Some(count) = counts.by_subject.get_mut(&self.subject) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.by_subject.remove(&self.subject);
            }
        }
    }
}

impl QuoteHub {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            connections: Arc::new(Mutex::new(ConnectionCounts::default())),
        }
    }

    pub fn publish(&self, event: QuoteEvent) {
        // No receivers is normal. REST/HTMX reads remain authoritative.
        let _ = self.sender.send(event);
    }

    pub fn try_acquire_socket(&self, subject: &str) -> Option<SocketPermit> {
        let mut counts = self
            .connections
            .lock()
            .expect("quote WebSocket connection counter lock poisoned");
        let subject_count = counts.by_subject.get(subject).copied().unwrap_or_default();
        if counts.total >= MAX_SOCKETS_PER_INSTANCE || subject_count >= MAX_SOCKETS_PER_SUBJECT {
            return None;
        }
        counts.total += 1;
        counts
            .by_subject
            .insert(subject.to_owned(), subject_count + 1);
        Some(SocketPermit {
            connections: self.connections.clone(),
            subject: subject.to_owned(),
        })
    }

    pub async fn serve(
        &self,
        socket: WebSocket,
        owner_subject: String,
        quote_id: Uuid,
        permit: SocketPermit,
    ) {
        let _permit = permit;
        let mut events = self.sender.subscribe();
        let (mut sender, mut receiver) = socket.split();
        let hello = serde_json::json!({
            "type": "hello",
            "protocolVersion": 1,
            "quoteId": quote_id,
            "heartbeatSeconds": HEARTBEAT_INTERVAL.as_secs()
        });
        if sender.send(Message::text(hello.to_string())).await.is_err() {
            return;
        }
        let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
        heartbeat.tick().await;

        loop {
            tokio::select! {
                inbound = receiver.next() => {
                    match inbound {
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        Some(Ok(Message::Text(text))) if text.as_str() == "ping" => {
                            if sender.send(Message::text(r#"{"type":"pong"}"#)).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(_)) => {}
                    }
                }
                event = events.recv() => {
                    match event {
                        Ok(event)
                            if event.quote_id == quote_id
                                && event.owner_subject == owner_subject =>
                        {
                            let message = serde_json::json!({
                                "type": "quote.status",
                                "quote": event
                            });
                            if sender.send(Message::text(message.to_string())).await.is_err() {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if sender
                                .send(Message::text(r#"{"type":"resync_required"}"#))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = heartbeat.tick() => {
                    if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sockets_are_bounded_per_subject() {
        let hub = QuoteHub::new(8);
        let permits = (0..MAX_SOCKETS_PER_SUBJECT)
            .map(|_| hub.try_acquire_socket("subject-1").unwrap())
            .collect::<Vec<_>>();
        assert!(hub.try_acquire_socket("subject-1").is_none());
        assert!(hub.try_acquire_socket("subject-2").is_some());
        drop(permits);
        assert!(hub.try_acquire_socket("subject-1").is_some());
    }
}
