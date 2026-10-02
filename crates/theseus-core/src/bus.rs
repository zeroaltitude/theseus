//! The session event bus: every turn's notifications go to the connection that
//! asked for the turn and to every connection watching the session
//! (`session.watch`). Continuation turns, which no client asked for, reach
//! watchers only. Senders whose connection closed are pruned on publish.
//!
//! The push (theseus-in3): `execution.changed`, `confirm.requested`, and
//! `confirm.resolved` also go to every `executions.watch` subscriber, the
//! all-session watchers, once to a connection that is both.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use theseus_protocol::{notify, Event, Message};
use tokio::sync::mpsc::UnboundedSender;

/// A watcher: its connection id and where its messages go.
type Watcher = (String, UnboundedSender<Message>);

#[derive(Default)]
pub struct SessionBus {
    subs: Mutex<HashMap<String, Vec<Watcher>>>,
    /// The `executions.watch` subscribers (theseus-in3). The lock order is
    /// `subs`, then `all`.
    all: Mutex<Vec<Watcher>>,
}

/// What goes to every session's watchers as well as the session's own: the
/// push's notifications.
fn wide(msg: &Message) -> bool {
    match msg {
        Message::Notification(n) => matches!(
            n.method.as_str(),
            notify::EXECUTION_CHANGED | notify::CONFIRM_REQUESTED | notify::CONFIRM_RESOLVED
        ),
        _ => false,
    }
}

impl SessionBus {
    /// Watch every session's executions and questions (`executions.watch`),
    /// replacing an earlier watch of the same connection.
    pub fn watch_all(&self, conn: &str, tx: UnboundedSender<Message>) {
        let mut all = self.all.lock().unwrap();
        all.retain(|(c, _)| c != conn);
        all.push((conn.to_string(), tx));
    }

    /// End a connection's `executions.watch`: true if it had one.
    pub fn unwatch_all(&self, conn: &str) -> bool {
        let mut all = self.all.lock().unwrap();
        let before = all.len();
        all.retain(|(c, _)| c != conn);
        all.len() < before
    }

    /// The connections with an `executions.watch`.
    pub fn all_watchers(&self) -> usize {
        self.all.lock().unwrap().len()
    }

    pub fn watch(&self, session: &str, conn: &str, tx: UnboundedSender<Message>) {
        let mut g = self.subs.lock().unwrap();
        let v = g.entry(session.to_string()).or_default();
        v.retain(|(c, _)| c != conn);
        v.push((conn.to_string(), tx));
    }

    pub fn unwatch(&self, session: &str, conn: &str) {
        if let Some(v) = self.subs.lock().unwrap().get_mut(session) {
            v.retain(|(c, _)| c != conn);
        }
    }

    /// Forget a closed connection everywhere.
    pub fn drop_conn(&self, conn: &str) {
        let mut g = self.subs.lock().unwrap();
        for v in g.values_mut() {
            v.retain(|(c, _)| c != conn);
        }
        g.retain(|_, v| !v.is_empty());
        self.all.lock().unwrap().retain(|(c, _)| c != conn);
    }

    /// Send to the session's watchers, but not to `except` (the connection
    /// that asked for the turn, which got it directly). The push's
    /// notifications go to the all-session watchers too, once each.
    pub fn publish(&self, session: &str, msg: &Message, except: Option<&str>) {
        let mut g = self.subs.lock().unwrap();
        let mut sent = HashSet::new();
        if let Some(v) = g.get_mut(session) {
            v.retain(|(c, tx)| {
                if Some(c.as_str()) == except {
                    return true;
                }
                sent.insert(c.clone());
                tx.send(msg.clone()).is_ok()
            });
        }
        if wide(msg) {
            self.all.lock().unwrap().retain(|(c, tx)| {
                if Some(c.as_str()) == except || sent.contains(c) {
                    return true;
                }
                tx.send(msg.clone()).is_ok()
            });
        }
    }

    /// Send to every connection that watches a session, once each: news that
    /// holds for every session, such as a tightening (theseus-sgh).
    pub fn publish_all(&self, msg: &Message) {
        let mut g = self.subs.lock().unwrap();
        let mut sent = std::collections::HashSet::new();
        for v in g.values_mut() {
            v.retain(|(c, tx)| !sent.insert(c.clone()) || tx.send(msg.clone()).is_ok());
        }
    }

    pub fn watchers(&self, session: &str) -> usize {
        self.subs
            .lock()
            .unwrap()
            .get(session)
            .map(Vec::len)
            .unwrap_or(0)
    }
}

/// Where a turn's notifications go: the requesting connection, if any, and the bus.
#[derive(Clone)]
pub struct EventSink {
    pub direct: Option<(String, UnboundedSender<Message>)>,
    pub bus: Arc<SessionBus>,
    pub session_id: String,
}

impl EventSink {
    pub fn new(
        bus: Arc<SessionBus>,
        session_id: &str,
        direct: Option<(String, UnboundedSender<Message>)>,
    ) -> Self {
        Self {
            direct,
            bus,
            session_id: session_id.to_string(),
        }
    }

    pub fn send(&self, e: Event) {
        let m = Message::from(e);
        if let Some((_, tx)) = &self.direct {
            let _ = tx.send(m.clone());
        }
        self.bus.publish(
            &self.session_id,
            &m,
            self.direct.as_ref().map(|(c, _)| c.as_str()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn publish_reaches_watchers_but_not_the_requester_twice_and_prunes_closed() {
        let bus = Arc::new(SessionBus::default());
        let (tx_a, mut rx_a) = unbounded_channel();
        let (tx_b, mut rx_b) = unbounded_channel();
        let (tx_c, rx_c) = unbounded_channel();
        bus.watch("s", "a", tx_a.clone());
        bus.watch("s", "b", tx_b);
        bus.watch("s", "c", tx_c);
        drop(rx_c);
        let sink = EventSink::new(bus.clone(), "s", Some(("a".into(), tx_a)));
        sink.send(Event::NodeWritten(Default::default()));
        assert!(rx_a.try_recv().is_ok(), "direct delivery");
        assert!(rx_a.try_recv().is_err(), "not again through the bus");
        assert!(rx_b.try_recv().is_ok());
        assert_eq!(bus.watchers("s"), 2, "closed watcher pruned");
        bus.drop_conn("b");
        assert_eq!(bus.watchers("s"), 1);
    }

    /// News for every session reaches each watching connection once, however
    /// many sessions it watches.
    #[test]
    fn publish_all_reaches_each_connection_once() {
        let bus = SessionBus::default();
        let (tx_a, mut rx_a) = unbounded_channel();
        let (tx_b, mut rx_b) = unbounded_channel();
        bus.watch("s1", "a", tx_a.clone());
        bus.watch("s2", "a", tx_a);
        bus.watch("s2", "b", tx_b);
        bus.publish_all(&Message::from(Event::NodeWritten(Default::default())));
        assert!(rx_a.try_recv().is_ok());
        assert!(
            rx_a.try_recv().is_err(),
            "once, though it watches two sessions"
        );
        assert!(rx_b.try_recv().is_ok());
    }

    /// The push's notifications reach the session's watchers and every
    /// all-session watcher, once to a connection that is both, and never the
    /// requester twice; anything else stays with the session's watchers.
    #[test]
    fn the_push_reaches_all_session_watchers_once() {
        use theseus_protocol::{ConfirmResolved, ExecutionView};
        let bus = SessionBus::default();
        let (tx_a, mut rx_a) = unbounded_channel();
        let (tx_b, mut rx_b) = unbounded_channel();
        let (tx_c, mut rx_c) = unbounded_channel();
        bus.watch("s", "a", tx_a.clone());
        bus.watch_all("a", tx_a);
        bus.watch_all("b", tx_b.clone());
        bus.watch_all("b", tx_b);
        bus.watch_all("c", tx_c);
        assert_eq!(bus.all_watchers(), 3, "a second watch replaces the first");
        let view: ExecutionView = serde_json::from_value(serde_json::json!({
            "execution_id": "exe_1", "session_id": "s", "kind": "task", "state": "running",
            "attention": {"level": "working", "label": "turn 1", "since_ms": 0}
        }))
        .unwrap();
        bus.publish(
            "s",
            &Message::from(Event::ExecutionChanged(view)),
            Some("c"),
        );
        let count = |rx: &mut tokio::sync::mpsc::UnboundedReceiver<Message>| {
            std::iter::from_fn(|| rx.try_recv().ok()).count()
        };
        assert_eq!(count(&mut rx_a), 1, "once, though it is both");
        assert_eq!(count(&mut rx_b), 1);
        assert_eq!(count(&mut rx_c), 0, "the requester got it directly");
        bus.publish(
            "s2",
            &Message::from(Event::ConfirmResolved(ConfirmResolved::default())),
            None,
        );
        assert_eq!((count(&mut rx_a), count(&mut rx_b)), (1, 1));
        bus.publish(
            "s",
            &Message::from(Event::NodeWritten(Default::default())),
            None,
        );
        assert_eq!(
            (count(&mut rx_a), count(&mut rx_b)),
            (1, 0),
            "a node is the session's"
        );
        assert!(bus.unwatch_all("b") && !bus.unwatch_all("b"));
        bus.drop_conn("a");
        assert_eq!(bus.all_watchers(), 1);
    }
}
