//! What ran since the last trim (issue 21, step one): the record that names
//! a memory peak's cause. Every entry point of work feeds it with a name,
//! how long it took and, where the code knows it cheaply, how much it
//! returned or read; the trim reads it. Only when a trim finds the resident
//! set over `resident::PEAK_FLOOR` does the record leave memory (a ledger row
//! and a log line, `Core::record_peak`); any other trim drops it.
//!
//! - **Counts by name, never events.** A window holds one aggregate per
//!   `(kind, name)`: its count, its wall time (sum and longest) and its bytes
//!   (sum and largest). A cockpit that asks for `health` every 2 s for an
//!   hour is one entry, not 1,800, so the window is bounded by the number of
//!   distinct names (`MAX_NAMES`), and a feeding costs one lock and one hash
//!   lookup.
//! - **What still runs is named too.** A turn or a pass that began before the
//!   trim and has not ended holds memory the trim cannot return; its guard
//!   registers at the start, and the drain lists it as `running` with the
//!   time it has run.
//! - **No content.** Method and job names, counts, sizes and times: the
//!   names are the daemon's own (an RPC method, a pass's function), and a
//!   method name a client invented is held to 64 bytes and the first 512
//!   names of a window.
//!
//! The feeders: `Core::handle` (every RPC method, with the answer's size),
//! `learning::tender::tend` (each nightly pass, named by its closure's path),
//! the heartbeat and the driver's tick (`harness`), the push's seed and its
//! frames, the memory pass's sessions, the keep-warm's rounds, the self
//! pipeline's tick, the index tender's one request function (`tender::call`,
//! with the answer's size), and the AWS cross-check (with its ledger tail's
//! weight by kind, `aws::crosscheck::ledgered_of_tail`).
//! `docs/design/memory-peak-record.md` says how to read a row.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// Distinct names one window holds; the rest count under [`OTHER`].
pub const MAX_NAMES: usize = 512;
/// The longest name kept whole (a method name a client made up is cut).
pub const MAX_NAME: usize = 64;
/// What a name past [`MAX_NAMES`] is counted under.
pub const OTHER: &str = "(other)";
/// Entries a row carries: the longest-running, then the largest.
const BY_TIME: usize = 24;
const BY_BYTES: usize = 16;

/// What kind of entry point fed an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A method served over the protocol (every client: the CLI, the
    /// cockpit, Discord, a turn's own).
    Rpc,
    /// A background pass: a nightly run, a heartbeat, a driver tick.
    Job,
    /// A request to the index tender.
    Index,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Rpc => "rpc",
            Kind::Job => "job",
            Kind::Index => "index",
        }
    }
}

#[derive(Default, Clone)]
struct Agg {
    count: u64,
    us: u64,
    max_us: u64,
    bytes: u64,
    max_bytes: u64,
}

struct Running {
    kind: Kind,
    name: String,
    at: Instant,
}

#[derive(Default)]
struct Inner {
    by_name: HashMap<Kind, HashMap<String, Agg>>,
    names: usize,
    running: HashMap<u64, Running>,
    next: u64,
    since_ms: u64,
}

/// One name's aggregate over a window, or a piece of work still running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    pub name: String,
    pub count: u64,
    pub ms: u64,
    pub max_ms: u64,
    pub bytes: u64,
    pub max_bytes: u64,
    /// Not ended at the drain: `ms` is how long it has run.
    pub running: bool,
}

impl Entry {
    /// The row's form of one entry.
    pub fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({"kind": self.kind.as_str(), "name": self.name,
            "count": self.count, "ms": self.ms, "max_ms": self.max_ms});
        if self.bytes > 0 {
            v["bytes"] = self.bytes.into();
            v["max_bytes"] = self.max_bytes.into();
        }
        if self.running {
            v["running"] = true.into();
        }
        v
    }
}

/// The window a drain took: when it began and ended, the entries a row
/// carries, and how many names it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub since_ms: u64,
    pub until_ms: u64,
    pub entries: Vec<Entry>,
    pub left_out: u64,
}

/// The record every entry point of work feeds, until a trim's turn drains it.
#[derive(Default)]
pub struct Activity {
    inner: Mutex<Inner>,
}

impl Window {
    /// The first `n` entries in one line, for the log: `name xCount Ms`, and
    /// the size where there is one.
    pub fn summary(&self, n: usize) -> String {
        let mut parts: Vec<String> = self
            .entries
            .iter()
            .take(n)
            .map(|e| {
                let mut s = format!("{} x{} {}ms", e.name, e.count, e.ms);
                if e.bytes > 0 {
                    s.push_str(&format!(" {}KiB", e.bytes >> 10));
                }
                if e.running {
                    s.push_str(" (still running)");
                }
                s
            })
            .collect();
        let more = (self.entries.len() + self.left_out as usize).saturating_sub(n);
        if more > 0 {
            parts.push(format!("and {more} more"));
        }
        parts.join(", ")
    }
}

/// `Activity::counted`, in the process's own record.
pub fn counted(kind: Kind, name: &str, n: u64, bytes: u64) {
    Activity::global().counted(kind, name, n, bytes);
}

/// Work begins, in the process's own record: for the passes with no core in
/// hand. The guard's drop is the work's end.
pub fn ran(kind: Kind, name: &str) -> Ran<'static> {
    Activity::global().ran(kind, name)
}

/// The bytes a JSON value takes written out, counted without writing them.
pub fn json_len(v: &serde_json::Value) -> u64 {
    struct Count(u64);
    impl std::io::Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 += b.len() as u64;
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut c = Count(0);
    let _ = serde_json::to_writer(&mut c, v);
    c.0
}

/// A piece of work, from its start to its end: dropped, it feeds the window
/// its wall time and the bytes it was told of.
pub struct Ran<'a> {
    act: &'a Activity,
    id: u64,
    bytes: u64,
}

impl Ran<'_> {
    /// What the work returned or read, in bytes; told twice, they add.
    pub fn bytes(&mut self, n: u64) {
        self.bytes = self.bytes.saturating_add(n);
    }
}

impl Drop for Ran<'_> {
    fn drop(&mut self) {
        self.act.ended(self.id, self.bytes);
    }
}

/// The name as kept: its first [`MAX_NAME`] bytes, at a character boundary
/// and before any known value.
fn kept(name: &str) -> &str {
    crate::scrub::safe::head_bytes(name, MAX_NAME).0
}

impl Activity {
    /// The process's own, which `Resident` holds and the passes with no core
    /// in hand (`tend`) feed.
    pub fn global() -> &'static Arc<Activity> {
        static GLOBAL: OnceLock<Arc<Activity>> = OnceLock::new();
        GLOBAL.get_or_init(|| {
            let a = Activity::default();
            a.inner.lock().unwrap().since_ms = theseus_protocol::now_unix_ms();
            Arc::new(a)
        })
    }

    /// Work begins: the guard says so, and its drop is the work's end.
    pub fn ran(&self, kind: Kind, name: &str) -> Ran<'_> {
        let mut g = self.inner.lock().unwrap();
        let id = g.next;
        g.next += 1;
        g.running.insert(
            id,
            Running {
                kind,
                name: kept(name).to_string(),
                at: Instant::now(),
            },
        );
        drop(g);
        Ran {
            act: self,
            id,
            bytes: 0,
        }
    }

    /// A count and a size the code knows without having timed them: what a
    /// pass read, by part (`n` of them, `bytes` in all).
    pub fn counted(&self, kind: Kind, name: &str, n: u64, bytes: u64) {
        self.feed(kind, kept(name), n, 0, bytes);
    }

    fn ended(&self, id: u64, bytes: u64) {
        let mut g = self.inner.lock().unwrap();
        let Some(r) = g.running.remove(&id) else {
            return;
        };
        let us = u64::try_from(r.at.elapsed().as_micros()).unwrap_or(u64::MAX);
        drop(g);
        self.feed(r.kind, &r.name, 1, us, bytes);
    }

    fn feed(&self, kind: Kind, name: &str, count: u64, us: u64, bytes: u64) {
        let mut g = self.inner.lock().unwrap();
        let full = g.names >= MAX_NAMES;
        let by_kind = g.by_name.entry(kind).or_default();
        let name = if full && !by_kind.contains_key(name) {
            OTHER
        } else {
            name
        };
        let is_new = !by_kind.contains_key(name);
        let a = by_kind.entry(name.to_string()).or_default();
        a.count += count;
        a.us = a.us.saturating_add(us);
        a.max_us = a.max_us.max(us);
        a.bytes = a.bytes.saturating_add(bytes);
        a.max_bytes = a.max_bytes.max(bytes);
        if is_new {
            g.names += 1;
        }
    }

    /// Take the window since the last drain, and begin the next. Work that
    /// has not ended stays registered and is listed as `running`.
    pub fn drain(&self) -> Window {
        let now = theseus_protocol::now_unix_ms();
        let mut g = self.inner.lock().unwrap();
        let by_name = std::mem::take(&mut g.by_name);
        g.names = 0;
        let since_ms = std::mem::replace(&mut g.since_ms, now);
        let mut all: Vec<Entry> = by_name
            .into_iter()
            .flat_map(|(kind, names)| {
                names.into_iter().map(move |(name, a)| Entry {
                    kind,
                    name,
                    count: a.count,
                    ms: a.us / 1000,
                    max_ms: a.max_us / 1000,
                    bytes: a.bytes,
                    max_bytes: a.max_bytes,
                    running: false,
                })
            })
            .collect();
        all.extend(g.running.values().map(|r| Entry {
            kind: r.kind,
            name: r.name.clone(),
            count: 1,
            ms: u64::try_from(r.at.elapsed().as_millis()).unwrap_or(u64::MAX),
            max_ms: u64::try_from(r.at.elapsed().as_millis()).unwrap_or(u64::MAX),
            bytes: 0,
            max_bytes: 0,
            running: true,
        }));
        drop(g);
        // The longest-running first, then the largest of the rest; ties by
        // name, so a row reads the same twice.
        all.sort_by(|a, b| (b.ms, &a.name).cmp(&(a.ms, &b.name)));
        let mut rest = all.split_off(BY_TIME.min(all.len()));
        rest.sort_by(|a, b| (b.bytes, &a.name).cmp(&(a.bytes, &b.name)));
        let more = rest.split_off(BY_BYTES.min(rest.len()));
        all.extend(rest);
        Window {
            since_ms,
            until_ms: now,
            entries: all,
            left_out: more.len() as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(w: &'a Window, name: &str) -> &'a Entry {
        w.entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} is not in {:?}", w.entries))
    }

    #[test]
    fn a_name_is_counted_with_its_time_and_bytes() {
        let a = Activity::default();
        for n in [10u64, 30] {
            let mut r = a.ran(Kind::Rpc, "ontology.list");
            r.bytes(n);
        }
        drop(a.ran(Kind::Job, "learning::tend"));
        let w = a.drain();
        let e = find(&w, "ontology.list");
        assert_eq!(
            (e.kind, e.count, e.bytes, e.max_bytes),
            (Kind::Rpc, 2, 40, 30)
        );
        assert!(!e.running);
        assert_eq!(find(&w, "learning::tend").kind, Kind::Job);
        assert_eq!(w.left_out, 0);
    }

    #[test]
    fn a_drain_begins_the_next_window() {
        let a = Activity::default();
        drop(a.ran(Kind::Rpc, "health"));
        assert_eq!(a.drain().entries.len(), 1);
        let second = a.drain();
        assert!(second.entries.is_empty(), "{:?}", second.entries);
        assert!(second.since_ms <= second.until_ms);
    }

    #[test]
    fn work_that_has_not_ended_is_named_as_running_and_stays() {
        let a = Activity::default();
        let held = a.ran(Kind::Rpc, "turn.submit");
        let w = a.drain();
        let e = find(&w, "turn.submit");
        assert!(e.running && e.count == 1);
        drop(held);
        let after = a.drain();
        let e = find(&after, "turn.submit");
        assert!(!e.running, "its end is counted in the window it ended in");
    }

    #[test]
    fn names_are_bounded_in_number_and_length() {
        let a = Activity::default();
        for i in 0..MAX_NAMES + 20 {
            drop(a.ran(Kind::Rpc, &format!("made.up.{i}")));
        }
        drop(a.ran(Kind::Rpc, &"é".repeat(100)));
        let g = a.inner.lock().unwrap();
        let rpc = &g.by_name[&Kind::Rpc];
        assert!(rpc.len() <= MAX_NAMES + 1, "{}", rpc.len());
        assert_eq!(rpc[OTHER].count, 21, "past the bound, counted together");
        assert!(rpc.keys().all(|k| k.len() <= MAX_NAME));
    }

    #[test]
    fn a_row_carries_the_longest_then_the_largest() {
        let a = Activity::default();
        for i in 0..100 {
            let mut r = a.ran(Kind::Rpc, &format!("m{i:03}"));
            r.bytes(i);
        }
        let w = a.drain();
        assert_eq!(w.entries.len(), BY_TIME + BY_BYTES);
        assert_eq!(w.left_out, 100 - (BY_TIME + BY_BYTES) as u64);
        // The largest of what the time cut left is kept.
        assert!(w.entries.iter().any(|e| e.name == "m099"));
    }
}
