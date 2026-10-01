//! The session event bus: every turn's notifications go to the connection that
//! asked for the turn and to every connection watching the session
//! (`session.watch`). Continuation turns, which no client asked for, reach
//! watchers only. Senders whose connection closed are pruned on publish.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use theseus_protocol::{Event, Message};
use tokio::sync::mpsc::UnboundedSender;

/// A watcher: its connection id and where its messages go.
type Watcher = (String, UnboundedSender<Message>);

#[derive(Default)]
pub struct SessionBus {
    subs: Mutex<HashMap<String, Vec<Watcher>>>,
}

impl SessionBus {
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
    }

    pub fn publish(&self, session: &str, msg: &Message, except: Option<&str>) {
        let mut g = self.subs.lock().unwrap();
        if let Some(v) = g.get_mut(session) {
            v.retain(|(c, tx)| {
                if Some(c.as_str()) == except {
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
}
