//! Which messages are the owner's own (theseus-slvl.5): `turn.submit` notes a
//! message that came from the CLI, the web UI or a binding by a person, and
//! the turn's inbound point takes the note. A message a job sent
//! (`opened_from`), one through the MCP server or an unnamed surface, a
//! prompt's, or one with files is never noted, so it is never captured: the
//! pack is not asked about it. The same rule the owner's route corrections
//! follow (`rpc/route_correct.rs`). In memory, and a minute long: a note no
//! turn took is dropped.

use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const LIFE: Duration = Duration::from_secs(60);
/// The owner's own message nodes kept, newest.
const OWNED_KEPT: usize = 8192;

/// The messages noted and not yet taken: (session, text, noted at).
#[derive(Default)]
pub struct Expected(
    Mutex<Vec<(String, String, Instant)>>,
    Mutex<Vec<(String, String, Instant)>>,
    Mutex<Owned>,
);

/// The nodes of the owner's own messages (theseus-6f8t0): a node does not say
/// whether a job sent it, so the exchange-end observer reads only the ones
/// the inbound step marked. In memory: after a restart the earlier ones are
/// not marked, and the observer leaves them out rather than guess.
#[derive(Default)]
struct Owned {
    order: VecDeque<String>,
    set: HashSet<String>,
}

impl Expected {
    pub fn note(&self, session: &str, text: &str) {
        let mut v = self.0.lock().unwrap_or_else(|e| e.into_inner());
        v.retain(|(_, _, at)| at.elapsed() < LIFE);
        v.push((session.to_string(), text.to_string(), Instant::now()));
    }

    /// Whether this message was noted; the note is spent.
    pub fn take(&self, session: &str, text: &str) -> bool {
        let mut v = self.0.lock().unwrap_or_else(|e| e.into_inner());
        v.retain(|(_, _, at)| at.elapsed() < LIFE);
        match v.iter().position(|(s, t, _)| s == session && t == text) {
            Some(i) => {
                v.remove(i);
                true
            }
            None => false,
        }
    }
}

impl Expected {
    /// A message that is the owner's own by the rule above, for the
    /// observer. Kept apart from [`Expected::note`]: capture's note is made
    /// only while `[standing] capture` and the judge are on.
    pub fn note_owner(&self, session: &str, text: &str) {
        let mut v = self.1.lock().unwrap_or_else(|e| e.into_inner());
        v.retain(|(_, _, at)| at.elapsed() < LIFE);
        v.push((session.to_string(), text.to_string(), Instant::now()));
    }

    /// Whether this message was noted for the observer; the note is spent.
    pub fn take_owner(&self, session: &str, text: &str) -> bool {
        let mut v = self.1.lock().unwrap_or_else(|e| e.into_inner());
        v.retain(|(_, _, at)| at.elapsed() < LIFE);
        match v.iter().position(|(s, t, _)| s == session && t == text) {
            Some(i) => {
                v.remove(i);
                true
            }
            None => false,
        }
    }

    /// The node written for a message taken by [`Expected::take_owner`].
    pub fn mark_owner(&self, node: &str) {
        let mut o = self.2.lock().unwrap_or_else(|e| e.into_inner());
        if o.set.insert(node.to_string()) {
            o.order.push_back(node.to_string());
            if o.order.len() > OWNED_KEPT {
                if let Some(old) = o.order.pop_front() {
                    o.set.remove(&old);
                }
            }
        }
    }

    /// Whether `node` is a marked owner message.
    pub fn is_owner(&self, node: &str) -> bool {
        self.2
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set
            .contains(node)
    }
}
