//! Scope-owned forwarding from core broadcasts to the desktop host protocol.

use codetwo_core::kernel::{
    async_trait, CommandRealm, Context, Injection, Plugin, PluginError, PluginResult, Service,
};
use codetwo_core::plugins::events::{ConnectorEvent, PluginRuntimeChanged, PluginsChanged};
use codetwo_core::plugins::{EventBus, TerminalEvent, TerminalOutputEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::EventSink;

pub struct HostEventsPlugin {
    host: EventSink,
}

/// Marker service for Runtimes that contribute desktop-hosted actions.
pub struct DesktopHostActions;

impl Service for DesktopHostActions {
    const NAME: &'static str = "desktop-host-actions";
}

impl HostEventsPlugin {
    pub fn new(host: EventSink) -> Self {
        Self { host }
    }
}

#[derive(Serialize, Clone)]
struct PtyOutput {
    id: String,
    data: String,
    project_path: Option<String>,
}

#[derive(Serialize, Clone)]
struct PtyTitle {
    id: String,
    title: String,
    project_path: Option<String>,
}

#[derive(Serialize, Clone)]
struct PtyExit {
    id: String,
    project_path: Option<String>,
}

#[async_trait]
impl Plugin for HostEventsPlugin {
    fn name(&self) -> &str {
        "desktop-events"
    }

    fn inject(&self) -> Injection {
        Injection::required(["bus", "engine"])
    }

    fn description(&self) -> Option<&str> {
        Some("Forwards core engine and terminal broadcasts to the desktop renderer.")
    }

    async fn apply(&self, ctx: Context, _config: Value) -> PluginResult {
        ctx.provide(Arc::new(DesktopHostActions))?;

        #[derive(Deserialize)]
        struct RevealSessionArgs {
            session: String,
        }
        let host = self.host.clone();
        ctx.command_extension_public("desktop.reveal_session", move |args| {
            let host = host.clone();
            async move {
                let args: RevealSessionArgs = serde_json::from_value(args)
                    .map_err(|error| PluginError::new(format!("bad arguments: {error}")))?;
                if args.session.is_empty()
                    || args.session.len() > 128
                    || !args
                        .session
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
                {
                    return Err(PluginError::new("session id is invalid"));
                }
                host.emit(
                    "desktop-reveal-session",
                    serde_json::json!({
                        "session": args.session,
                    }),
                )
                .map_err(PluginError::new)?;
                Ok(Value::Bool(true))
            }
        })?;

        let mut engine_events = ctx
            .get::<EventBus>()
            .ok_or_else(|| PluginError::new("event bus is unavailable"))?
            .subscribe();
        let host = self.host.clone();
        let share = ctx
            .get::<codetwo_core::plugins::EngineService>()
            .and_then(|engine| engine.share_store());
        ctx.spawn(async move {
            loop {
                match engine_events.recv().await {
                    Ok(event) => {
                        // Native renderer exit: same creator-only policy as remote; the host
                        // owner is the trusted `local-owner` actor, not a privileged observer.
                        if let Some(share) = &share {
                            if !codetwo_core::share_policy::share_event_visible(share, "", &event) {
                                continue;
                            }
                        }
                        let _ = host.emit("engine-event", event);
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        eprintln!("engine-event pump lagged; dropped {count} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let host = self.host.clone();
        ctx.on::<TerminalOutputEvent, _>(move |event| {
            let project_path = match &event.realm {
                CommandRealm::Global => None,
                CommandRealm::Project(path) => Some(path.clone()),
            };
            match &event.event {
                TerminalEvent::Data { id, data } => {
                    let _ = host.emit(
                        "pty-output",
                        PtyOutput {
                            id: id.clone(),
                            data: data.clone(),
                            project_path,
                        },
                    );
                }
                TerminalEvent::Title { id, title } => {
                    let _ = host.emit(
                        "pty-title",
                        PtyTitle {
                            id: id.clone(),
                            title: title.clone(),
                            project_path,
                        },
                    );
                }
                TerminalEvent::Exit { id } => {
                    let _ = host.emit(
                        "pty-exit",
                        PtyExit {
                            id: id.clone(),
                            project_path,
                        },
                    );
                }
            }
            None
        });

        let host = self.host.clone();
        ctx.on::<PluginsChanged, _>(move |_| {
            let _ = host.emit("plugins-changed", ());
            None
        });

        let host = self.host.clone();
        ctx.on::<PluginRuntimeChanged, _>(move |_| {
            let _ = host.emit("plugins-changed", ());
            None
        });

        let host = self.host.clone();
        ctx.on::<ConnectorEvent, _>(move |event| {
            let _ = host.emit("plugin-connector-event", (*event).clone());
            None
        });

        Ok(())
    }
}
