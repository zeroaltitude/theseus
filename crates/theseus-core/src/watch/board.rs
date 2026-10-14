//! The open watches in memory (theseus-f2r2b): each one's spec, what it has
//! seen and when it checks next. Nothing here is the truth: the wake and the
//! last frame's state are, and the board is read from them after a start.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use super::check::{RunHook, Tools};
use super::step::Entry;

/// The most checks that run at once.
pub const AT_ONCE: usize = 4;

#[derive(Default)]
pub struct Board {
    entries: Mutex<HashMap<String, Entry>>,
    loaded: AtomicBool,
    tools: OnceLock<Tools>,
    /// A test's stand-in for L1 (`set_run_hook`), before the tools are built.
    hook: Mutex<Option<RunHook>>,
}

impl Board {
    /// True once, for the caller that must read the open watches in.
    pub fn first_tick(&self) -> bool {
        !self.loaded.swap(true, Ordering::AcqRel)
    }

    /// Add `e`, or replace the watch of its id.
    pub fn insert(&self, e: Entry) {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(e.wake_id.clone(), e);
    }

    /// Add `e` unless the board has its watch already (the load must not
    /// undo a watch made while it read).
    pub fn insert_new(&self, e: Entry) {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(e.wake_id.clone())
            .or_insert(e);
    }

    pub fn remove(&self, wake_id: &str) {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(wake_id);
    }

    pub fn get(&self, wake_id: &str) -> Option<Entry> {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(wake_id)
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The watches due at `now`, marked running, up to the room left under
    /// `AT_ONCE`. Soonest first.
    pub fn take_due(&self, now: u64) -> Vec<Entry> {
        let mut g = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        let running = g.values().filter(|e| e.running).count();
        let room = AT_ONCE.saturating_sub(running);
        if room == 0 {
            return vec![];
        }
        let mut due: Vec<&mut Entry> = g
            .values_mut()
            .filter(|e| !e.running && e.next_ms <= now)
            .collect();
        due.sort_by_key(|e| (e.next_ms, e.wake_id.clone()));
        due.into_iter()
            .take(room)
            .map(|e| {
                e.running = true;
                e.clone()
            })
            .collect()
    }

    /// Run `f` on the entry of `wake_id`, if the board has it.
    pub fn with<T>(&self, wake_id: &str, f: impl FnOnce(&mut Entry) -> T) -> Option<T> {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(wake_id)
            .map(f)
    }

    pub fn tools(&self, build: impl FnOnce(Option<RunHook>) -> Tools) -> Tools {
        self.tools
            .get_or_init(|| {
                let hook = self.hook.lock().unwrap_or_else(|p| p.into_inner()).clone();
                build(hook)
            })
            .clone()
    }

    /// Tests: command checks run through `hook` instead of L1 (set before the
    /// first check).
    #[cfg(test)]
    pub fn set_run_hook(&self, hook: RunHook) {
        *self.hook.lock().unwrap_or_else(|p| p.into_inner()) = Some(hook);
    }
}
