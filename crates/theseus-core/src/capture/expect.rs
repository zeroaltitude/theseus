//! Which messages are the owner's own (theseus-slvl.5): `turn.submit` notes a
//! message that came from the CLI, the web UI or a binding by a person, and
//! the turn's inbound point takes the note. A message a job sent
//! (`opened_from`), one through the MCP server or an unnamed surface, a
//! prompt's, or one with files is never noted, so it is never captured: the
//! pack is not asked about it. The same rule the owner's route corrections
//! follow (`rpc/route_correct.rs`). In memory, and a minute long: a note no
//! turn took is dropped.

use std::sync::Mutex;
use std::time::{Duration, Instant};

const LIFE: Duration = Duration::from_secs(60);

/// The messages noted and not yet taken: (session, text, noted at).
#[derive(Default)]
pub struct Expected(Mutex<Vec<(String, String, Instant)>>);

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
