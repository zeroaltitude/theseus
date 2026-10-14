//! Threads on the work board (theseus-5i8i). The board holds the thread
//! groups (`ThreadGroup`, from the threads' board in memory) and stamps each
//! view with the thread its tree belongs to: the thread whose session is the
//! view's root conversation. Nothing here reads the store, writes a frame, or
//! sets a timer: a group arrives from `Core`'s thread feed (a thread's write
//! or bind), and a stamp is a lookup in two maps.
//!
//! A change of which session a thread runs on restamps the views in place at
//! once, so a snapshot is right, and marks them stale, so the next frame sends
//! them again with a newer position (as the warm-up's changes are).

use std::collections::HashMap;

use theseus_protocol::ThreadGroup;

use super::{Out, WorkBoard};

impl WorkBoard {
    /// The thread a view under `root` (an execution id) belongs to.
    pub(super) fn thread_for_root(&self, root: &str) -> Option<String> {
        let session = &self.execs.get(root)?.x.session_id;
        self.thread_of.get(session).cloned()
    }

    /// Take the threads' groups. A stat a group did not read (`None`) keeps
    /// the one held. Returns what changed, to send, none when nothing did.
    pub(in crate::push) fn set_threads(
        &mut self,
        groups: Vec<ThreadGroup>,
        at_ms: u64,
    ) -> Option<Out> {
        let mut changed = Vec::new();
        for g in groups {
            match self.thread_groups.get_mut(&g.slug) {
                Some(held) => {
                    let mut next = held.clone();
                    next.merge(g);
                    if next != *held {
                        *held = next.clone();
                        changed.push(next);
                    }
                }
                None => {
                    self.thread_groups.insert(g.slug.clone(), g.clone());
                    changed.push(g);
                }
            }
        }
        let of: HashMap<String, String> = self
            .thread_groups
            .values()
            .filter_map(|g| Some((g.session_id.clone()?, g.slug.clone())))
            .collect();
        if of != self.thread_of {
            self.thread_of = of;
            self.restamp();
        }
        (!changed.is_empty()).then(|| Out {
            position: self.position,
            at_ms,
            threads: changed,
            ..Out::default()
        })
    }

    /// The groups, by slug.
    pub(in crate::push) fn threads(&self) -> Vec<ThreadGroup> {
        self.thread_groups.values().cloned().collect()
    }

    /// Stamp every view again; those whose thread changed go out with the
    /// next frame.
    fn restamp(&mut self) {
        let stamps: Vec<(String, Option<String>)> = self
            .views
            .values()
            .map(|v| (v.id.clone(), self.thread_for_root(&v.root)))
            .collect();
        for (id, thread) in stamps {
            if let Some(v) = self.views.get_mut(&id) {
                if v.thread != thread {
                    v.thread = thread;
                    self.stale.insert(id);
                }
            }
        }
    }
}
