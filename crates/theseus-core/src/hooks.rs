//! Hooks (spec §3.17). Every event is defined and registerable; in M0 no
//! handler is installed, and the turn runner visits every site anyway.
//!
//! Kinds decide the execution contract:
//! - Gate: may block; fail closed on handler error or timeout; zero handlers = allow.
//! - Transform: may rewrite its payload within policy; never widens authority.
//! - Claim: first handler to claim wins.
//! - Observe: fan-out, results ignored, never on the critical path.
//!
//! Remote handlers registered over the protocol are Observe-only in M0: they
//! receive a `hook.event` notification and cannot block or transform.

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::{notify, HookEventNotification, Message, Notification};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    Gate,
    Transform,
    Claim,
    Observe,
}

impl HookKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HookKind::Gate => "gate",
            HookKind::Transform => "transform",
            HookKind::Claim => "claim",
            HookKind::Observe => "observe",
        }
    }
}

macro_rules! hook_events {
    ($( $variant:ident => ($name:literal, $kind:ident) ),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum HookEvent { $( $variant, )* }

        impl HookEvent {
            pub const ALL: &'static [HookEvent] = &[ $( HookEvent::$variant, )* ];
            pub fn name(self) -> &'static str { match self { $( HookEvent::$variant => $name, )* } }
            pub fn kind(self) -> HookKind { match self { $( HookEvent::$variant => HookKind::$kind, )* } }
        }

        impl FromStr for HookEvent {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, String> {
                match s { $( $name => Ok(HookEvent::$variant), )* _ => Err(format!("unknown hook event {s:?}")) }
            }
        }
    };
}

hook_events! {
    // lifecycle
    ServerStarted        => ("server.started", Observe),
    ServerStopping       => ("server.stopping", Observe),
    SessionOpened        => ("session.opened", Observe),
    // turn / loop
    TurnStarting         => ("turn.starting", Gate),
    InputReceived        => ("input.received", Transform),
    ContextBuilt         => ("context.built", Transform),
    PreModelCall         => ("model.pre_call", Gate),
    PostModelCall        => ("model.post_call", Observe),
    ToolProposed         => ("tool.proposed", Gate),
    PreToolCall          => ("tool.pre_call", Gate),
    PostToolCall         => ("tool.post_call", Transform),
    AdvancerDecided      => ("advancer.decided", Observe),
    LoopEnded            => ("loop.ended", Observe),
    MessageSending       => ("message.sending", Transform),
    ReplyClaim           => ("reply.claim", Claim),
    TurnEnded            => ("turn.ended", Observe),
    // external actions and completions
    ActionPlanned        => ("action.planned", Gate),
    ActionDispatched     => ("action.dispatched", Observe),
    CompletionReceived   => ("completion.received", Observe),
    ConfirmRequested     => ("confirm.requested", Observe),
    // memory
    MemoryIngest         => ("memory.ingest", Transform),
    MemoryRecall         => ("memory.recall", Transform),
    // judgments
    JudgmentMade         => ("judgment.made", Observe),
    // ledger
    LedgerRow            => ("ledger.row", Observe),
}

#[derive(Debug, Clone, Serialize)]
pub struct HandlerRecord {
    pub event: HookEvent,
    pub handler_id: String,
    pub client: String,
}

#[derive(Clone)]
enum Sink {
    Remote(mpsc::UnboundedSender<Message>),
}

#[derive(Clone)]
struct Handler {
    record: HandlerRecord,
    sink: Sink,
}

/// Outcome of dispatching a Gate/Transform/Claim event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No handler objected (or none existed). Payload possibly transformed.
    Proceed(Value),
    Blocked {
        reason: String,
    },
    Claimed {
        handler_id: String,
    },
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SiteVisit {
    pub event: String,
    pub kind: String,
    pub handlers: u32,
    pub outcome: String,
}

#[derive(Default)]
struct Inner {
    handlers: BTreeMap<HookEvent, Vec<Handler>>,
}

/// The registry. Cheap to clone; shared by the server and the turn runner.
#[derive(Clone, Default)]
pub struct Hooks {
    inner: Arc<Mutex<Inner>>,
}

impl Hooks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_remote(
        &self,
        event: HookEvent,
        handler_id: String,
        client: String,
        tx: mpsc::UnboundedSender<Message>,
    ) -> HandlerRecord {
        let record = HandlerRecord {
            event,
            handler_id,
            client,
        };
        let mut g = self.inner.lock().unwrap();
        let list = g.handlers.entry(event).or_default();
        list.retain(|h| {
            !(h.record.handler_id == record.handler_id && h.record.client == record.client)
        });
        list.push(Handler {
            record: record.clone(),
            sink: Sink::Remote(tx),
        });
        record
    }

    pub fn unregister(&self, event: HookEvent, handler_id: &str, client: &str) -> bool {
        let mut g = self.inner.lock().unwrap();
        let Some(list) = g.handlers.get_mut(&event) else {
            return false;
        };
        let before = list.len();
        list.retain(|h| !(h.record.handler_id == handler_id && h.record.client == client));
        before != list.len()
    }

    /// Drop every handler owned by a client (connection closed).
    pub fn unregister_client(&self, client: &str) -> usize {
        let mut g = self.inner.lock().unwrap();
        let mut n = 0;
        for list in g.handlers.values_mut() {
            let before = list.len();
            list.retain(|h| h.record.client != client);
            n += before - list.len();
        }
        n
    }

    pub fn handlers(&self) -> Vec<HandlerRecord> {
        let g = self.inner.lock().unwrap();
        g.handlers
            .values()
            .flatten()
            .map(|h| h.record.clone())
            .collect()
    }

    pub fn count(&self, event: HookEvent) -> u32 {
        let g = self.inner.lock().unwrap();
        g.handlers.get(&event).map(|l| l.len() as u32).unwrap_or(0)
    }

    /// Visit a hook site. Remote handlers receive an Observe notification for
    /// any kind; only in-process handlers (none yet) could gate, transform, or
    /// claim, so with none installed the outcome is always `Proceed(payload)`.
    pub fn dispatch(
        &self,
        event: HookEvent,
        turn_id: Option<&str>,
        session_id: Option<&str>,
        payload: Value,
    ) -> (Outcome, SiteVisit) {
        let handlers: Vec<Handler> = {
            let g = self.inner.lock().unwrap();
            g.handlers.get(&event).cloned().unwrap_or_default()
        };
        for h in &handlers {
            match &h.sink {
                Sink::Remote(tx) => {
                    let n = Notification::new(
                        notify::HOOK_EVENT,
                        HookEventNotification {
                            event: event.name().into(),
                            handler_id: h.record.handler_id.clone(),
                            turn_id: turn_id.map(str::to_string),
                            session_id: session_id.map(str::to_string),
                            payload: payload.clone(),
                        },
                    );
                    let _ = tx.send(Message::Notification(n));
                }
            }
        }
        let visit = SiteVisit {
            event: event.name().into(),
            kind: event.kind().as_str().into(),
            handlers: handlers.len() as u32,
            outcome: "proceed".into(),
        };
        (Outcome::Proceed(payload), visit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_round_trips_by_name() {
        for e in HookEvent::ALL {
            assert_eq!(HookEvent::from_str(e.name()).unwrap(), *e);
        }
        assert!(HookEvent::from_str("nope").is_err());
    }

    #[test]
    fn dispatch_with_no_handlers_proceeds() {
        let h = Hooks::new();
        for e in HookEvent::ALL {
            let (o, v) = h.dispatch(*e, None, None, serde_json::json!({"x": 1}));
            assert_eq!(o, Outcome::Proceed(serde_json::json!({"x": 1})));
            assert_eq!(v.handlers, 0);
        }
    }

    #[test]
    fn remote_observer_receives_notification() {
        let h = Hooks::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        h.register_remote(HookEvent::TurnEnded, "obs".into(), "c1".into(), tx);
        assert_eq!(h.count(HookEvent::TurnEnded), 1);
        h.dispatch(HookEvent::TurnEnded, Some("t1"), None, Value::Null);
        let Message::Notification(n) = rx.try_recv().unwrap() else {
            panic!("expected notification")
        };
        assert_eq!(n.method, notify::HOOK_EVENT);
        assert_eq!(h.unregister_client("c1"), 1);
    }
}
