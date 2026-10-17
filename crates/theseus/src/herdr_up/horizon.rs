//! The future's two numbers for the Space row (theseus-peqbl): when the next
//! reminder falls due, and how many routines are due now. Read from the board's
//! wake and routine views; they get no tab (the follower's tasks are tasks and
//! conversations alone). Pure: the clock is the caller's.

use std::collections::BTreeMap;

use theseus_protocol::work::{WorkKind, WorkView};

/// One dated item of the future.
#[derive(Debug, Clone, Copy)]
struct Item {
    routine: bool,
    at_ms: u64,
}

/// The reminders and routines that will fire, by view id.
#[derive(Debug, Default)]
pub struct Horizon {
    items: BTreeMap<String, Item>,
}

impl Horizon {
    /// Take what the views say. A snapshot lists the whole future, so it replaces what
    /// was held; a change touches its own views. An ended, paused or undated view is
    /// not waiting for anything.
    pub fn apply(&mut self, views: &[WorkView], gone: &[String], snapshot: bool) {
        if snapshot {
            self.items.clear();
        }
        for id in gone {
            self.items.remove(id);
        }
        for v in views {
            let routine = v.kind == WorkKind::Routine;
            let reminder = v.kind == WorkKind::Wake && v.purpose.as_deref() == Some("remind");
            if !(routine || reminder) {
                continue;
            }
            match v.next_at_ms {
                Some(at_ms) if !v.paused && !v.state.ended() => {
                    self.items.insert(v.id.clone(), Item { routine, at_ms });
                }
                _ => {
                    self.items.remove(&v.id);
                }
            }
        }
    }

    /// When the next reminder falls due; one already overdue is the earliest.
    pub fn next_reminder(&self) -> Option<u64> {
        self.items
            .values()
            .filter(|i| !i.routine)
            .map(|i| i.at_ms)
            .min()
    }

    /// Routines whose time has come and have not run yet.
    pub fn routines_due(&self, now_ms: u64) -> u32 {
        self.items
            .values()
            .filter(|i| i.routine && i.at_ms <= now_ms)
            .count() as u32
    }

    /// The next time (unix ms) a routine becomes due, for the one timer the row keeps.
    pub fn next_due(&self, now_ms: u64) -> Option<u64> {
        self.items
            .values()
            .filter(|i| i.routine && i.at_ms > now_ms)
            .map(|i| i.at_ms)
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: &str, kind: &str, purpose: Option<&str>, at: u64, state: &str) -> WorkView {
        let mut v = serde_json::json!({
            "id": id, "kind": kind, "position": 1, "at_ms": 1, "root": id,
            "title": id, "state": state, "next_at_ms": at,
            "attention": {"level": "idle", "label": state, "since_ms": 1},
            "started_at_ms": 1, "state_since_ms": 1});
        if let Some(p) = purpose {
            v["purpose"] = p.into();
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn the_next_reminder_and_the_routines_due() {
        let mut h = Horizon::default();
        h.apply(
            &[
                view("w1", "wake", Some("remind"), 5_000, "waiting"),
                view("w2", "wake", Some("remind"), 3_000, "waiting"),
                view("w3", "wake", Some("check"), 1_000, "waiting"),
                view("r1", "routine", None, 2_000, "waiting"),
                view("r2", "routine", None, 9_000, "waiting"),
            ],
            &[],
            true,
        );
        assert_eq!(h.next_reminder(), Some(3_000), "a check is not a reminder");
        assert_eq!(h.routines_due(4_000), 1);
        assert_eq!(h.next_due(4_000), Some(9_000));
        // The reminder fires and the routine runs: both leave the future.
        h.apply(
            &[
                view("w2", "wake", Some("remind"), 3_000, "done"),
                view("r1", "routine", None, 2_000, "done"),
            ],
            &[],
            false,
        );
        assert_eq!(h.next_reminder(), Some(5_000));
        assert_eq!(h.routines_due(4_000), 0);
        h.apply(&[], &["w1".into()], false);
        assert_eq!(h.next_reminder(), None);
    }
}
