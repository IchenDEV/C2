//! Per-client SSE stream accounting and `resources/subscribe` state.
//!
//! Bounded: at most [`MAX_STREAMS_PER_CLIENT`] open streams per credential. A stream holds a
//! [`StreamGuard`]; dropping the guard (client disconnect, task end, panic) releases the slot, so
//! counts can never leak.
//!
//! MCP 2025-06-18 compatibility: `resources/subscribe` stores a per-client subscription and the
//! newest `GET /external-mcp` stream delivers `notifications/resources/updated` (a message must
//! be sent on only one stream, never broadcast). MCP 2026-07-28 `subscriptions/listen` is a
//! per-request POST stream and keeps its filter inside the stream task, not here.

use crate::external_mcp::events::EVENTS_RESOURCE_URI;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub const MAX_STREAMS_PER_CLIENT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// `GET /external-mcp` (2025-06-18 Streamable HTTP).
    Get,
    /// `POST subscriptions/listen` (2026-07-28).
    Listen,
    /// `POST events/stream` (MCP Events draft).
    Events,
}

#[derive(Debug, Default)]
struct ClientState {
    /// Open streams in opening order.
    streams: Vec<(String, StreamKind)>,
    resource_subscriptions: HashSet<String>,
}

#[derive(Debug, Default)]
pub struct SubscriptionRegistry {
    inner: Mutex<HashMap<String, ClientState>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamLimitError {
    TooManyStreams,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionError {
    UnknownResource,
}

/// Releases the stream slot on drop.
#[derive(Debug)]
pub struct StreamGuard {
    registry: Arc<SubscriptionRegistry>,
    client_id: String,
    stream_id: String,
}

impl StreamGuard {
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }

    /// True when this is the newest open `GET` stream of its client (the one that carries
    /// `resources/subscribe` notifications).
    pub fn is_primary_get(&self) -> bool {
        self.registry.is_primary_get(&self.client_id, &self.stream_id)
    }

    pub fn resource_subscribed(&self, uri: &str) -> bool {
        self.registry.is_subscribed(&self.client_id, uri)
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.registry.close_stream(&self.client_id, &self.stream_id);
    }
}

impl SubscriptionRegistry {
    pub fn open_stream(
        self: &Arc<Self>,
        client_id: &str,
        kind: StreamKind,
    ) -> Result<StreamGuard, StreamLimitError> {
        let mut guard = self.inner.lock().expect("subscription registry poisoned");
        let state = guard.entry(client_id.to_string()).or_default();
        if state.streams.len() >= MAX_STREAMS_PER_CLIENT {
            return Err(StreamLimitError::TooManyStreams);
        }
        let stream_id = Uuid::new_v4().to_string();
        state.streams.push((stream_id.clone(), kind));
        Ok(StreamGuard {
            registry: Arc::clone(self),
            client_id: client_id.to_string(),
            stream_id,
        })
    }

    fn close_stream(&self, client_id: &str, stream_id: &str) {
        let mut guard = self.inner.lock().expect("subscription registry poisoned");
        if let Some(state) = guard.get_mut(client_id) {
            state.streams.retain(|(id, _)| id != stream_id);
            if state.streams.is_empty() && state.resource_subscriptions.is_empty() {
                guard.remove(client_id);
            }
        }
    }

    pub fn stream_count(&self, client_id: &str) -> usize {
        let guard = self.inner.lock().expect("subscription registry poisoned");
        guard.get(client_id).map_or(0, |state| state.streams.len())
    }

    pub fn subscribe_resource(&self, client_id: &str, uri: &str) -> Result<(), SubscriptionError> {
        if uri != EVENTS_RESOURCE_URI {
            return Err(SubscriptionError::UnknownResource);
        }
        let mut guard = self.inner.lock().expect("subscription registry poisoned");
        guard
            .entry(client_id.to_string())
            .or_default()
            .resource_subscriptions
            .insert(uri.to_string());
        Ok(())
    }

    pub fn unsubscribe_resource(&self, client_id: &str, uri: &str) {
        let mut guard = self.inner.lock().expect("subscription registry poisoned");
        if let Some(state) = guard.get_mut(client_id) {
            state.resource_subscriptions.remove(uri);
            if state.streams.is_empty() && state.resource_subscriptions.is_empty() {
                guard.remove(client_id);
            }
        }
    }

    pub fn is_subscribed(&self, client_id: &str, uri: &str) -> bool {
        let guard = self.inner.lock().expect("subscription registry poisoned");
        guard
            .get(client_id)
            .is_some_and(|state| state.resource_subscriptions.contains(uri))
    }

    fn is_primary_get(&self, client_id: &str, stream_id: &str) -> bool {
        let guard = self.inner.lock().expect("subscription registry poisoned");
        guard
            .get(client_id)
            .and_then(|state| {
                state
                    .streams
                    .iter()
                    .rev()
                    .find(|(_, kind)| *kind == StreamKind::Get)
            })
            .is_some_and(|(id, _)| id == stream_id)
    }

    /// Drop every subscription of a client (credential revoked or expired).
    pub fn purge_client_subscriptions(&self, client_id: &str) {
        let mut guard = self.inner.lock().expect("subscription registry poisoned");
        if let Some(state) = guard.get_mut(client_id) {
            state.resource_subscriptions.clear();
            if state.streams.is_empty() {
                guard.remove(client_id);
            }
        }
    }
}

/// Honored subset of a `subscriptions/listen` filter. Only resource subscriptions to
/// [`EVENTS_RESOURCE_URI`] are honored; list-changed notifications never fire on this server
/// (the catalog is static), so promising them would be dishonest and they are omitted from the
/// acknowledgement, as the 2026-07-28 spec asks for unsupported types.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListenFilter {
    pub resource_uris: BTreeSet<String>,
    /// Requested notification types the server declined, for diagnostics and tests.
    pub declined: Vec<String>,
}

pub fn parse_listen_filter(params: &Value) -> ListenFilter {
    let notifications = params.get("notifications").unwrap_or(&Value::Null);
    let mut filter = ListenFilter::default();
    for flag in ["toolsListChanged", "promptsListChanged", "resourcesListChanged"] {
        if notifications.get(flag).and_then(Value::as_bool) == Some(true) {
            filter.declined.push(flag.to_string());
        }
    }
    for uri in notifications
        .get("resourceSubscriptions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if uri == EVENTS_RESOURCE_URI {
            filter.resource_uris.insert(uri.to_string());
        } else {
            filter.declined.push(format!("resourceSubscriptions:{uri}"));
        }
    }
    filter
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stream_cap_is_released_by_dropping_the_guard() {
        let reg = Arc::new(SubscriptionRegistry::default());
        let mut guards: Vec<_> = (0..MAX_STREAMS_PER_CLIENT)
            .map(|_| reg.open_stream("c1", StreamKind::Get).unwrap())
            .collect();
        assert_eq!(
            reg.open_stream("c1", StreamKind::Listen).unwrap_err(),
            StreamLimitError::TooManyStreams
        );
        assert!(reg.open_stream("c2", StreamKind::Get).is_ok(), "caps are per client");
        guards.pop();
        assert!(reg.open_stream("c1", StreamKind::Events).is_ok());
        drop(guards);
        assert_eq!(reg.stream_count("c1"), 0);
    }

    #[test]
    fn only_the_newest_get_stream_is_primary() {
        let reg = Arc::new(SubscriptionRegistry::default());
        let first = reg.open_stream("c", StreamKind::Get).unwrap();
        let listen = reg.open_stream("c", StreamKind::Listen).unwrap();
        let second = reg.open_stream("c", StreamKind::Get).unwrap();
        assert!(!first.is_primary_get() && !listen.is_primary_get() && second.is_primary_get());
        drop(second);
        assert!(first.is_primary_get());
    }

    #[test]
    fn resource_subscription_is_per_client_and_known_uri_only() {
        let reg = Arc::new(SubscriptionRegistry::default());
        assert_eq!(
            reg.subscribe_resource("c", "file:///x"),
            Err(SubscriptionError::UnknownResource)
        );
        reg.subscribe_resource("c", EVENTS_RESOURCE_URI).unwrap();
        assert!(reg.is_subscribed("c", EVENTS_RESOURCE_URI));
        assert!(!reg.is_subscribed("other", EVENTS_RESOURCE_URI));
        reg.purge_client_subscriptions("c");
        assert!(!reg.is_subscribed("c", EVENTS_RESOURCE_URI));
    }

    #[test]
    fn listen_filter_honors_only_the_events_resource() {
        let filter = parse_listen_filter(&json!({
            "notifications": {
                "toolsListChanged": true,
                "resourceSubscriptions": [EVENTS_RESOURCE_URI, "file:///other"]
            }
        }));
        assert_eq!(
            filter.resource_uris,
            BTreeSet::from([EVENTS_RESOURCE_URI.to_string()])
        );
        assert_eq!(filter.declined.len(), 2);
        assert_eq!(parse_listen_filter(&json!({})), ListenFilter::default());
    }
}
