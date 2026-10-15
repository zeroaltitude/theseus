//! Keep-warm for threads, and what it saves (theseus-ytt8, T9; the owner's
//! policy of 2026-10-13). A thread's session is a private conversation, so
//! `keep_warm` already reads it; this is what is different about a thread:
//!
//! - **Who is kept**: an active thread whose owner last wrote within `[threads]
//!   keep_warm_hours` (4), never a paused or done one. The window is the
//!   shorter of that and the profile's own `keep_warm_hours`, which still
//!   turns it off. `thread.pause` and `thread.done` forget the session at
//!   once, and a resume puts it back only at its next owner message
//!   (`KeepWarm::message`), as a restart's windows do (`keep_warm_restore`).
//! - **One request per thread, and no more.** The provider caches by prefix,
//!   at each of the request's breakpoints, and a read of a cached prefix
//!   restarts its life. A thread's request carries the layers every thread on
//!   its profile shares (header, persona, project: the same bytes, the first
//!   three breakpoints) and then its card and messages. Its read therefore
//!   restarts the shared layers' entries as well as its own tail's, so one
//!   request per thread refreshes everything the cache would lose for it, and
//!   no request is sent for the shared layers alone: a request made of the
//!   tail alone would be a different prefix, which the provider does not
//!   have. The shared layers cost their cache-read price in each request
//!   (0.025x of input on Fable 5.1), which is the price of the thread's tail
//!   staying warm, not a duplicate refresh.
//! - **The day's line**: the reads go through `keep_warm_send`'s checks, so
//!   the session's limit and the day's ceiling hold them. The tender makes
//!   the due reads most recently written first (`KeepWarm::due`), and when
//!   the line is reached it says how many due sessions it skipped (a warning,
//!   and `ThreadsCache.skipped_today`); they wait one interval and are due
//!   again.
//! - **The numbers** (`ThreadCacheStats`): read from the ledger's
//!   `provider.call` and `keep_warm` rows of the local day, by `stats_of`,
//!   which is pure. Health's `threads.cache` holds a minute's copy, so the
//!   health path never scans the ledger.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;

use theseus_protocol::thread_cache::{
    ThreadCacheEntry, ThreadCacheStats, ThreadsCache, ThreadsHealth,
};
use theseus_protocol::threads::ThreadState;
use theseus_protocol::Usage;
use theseus_store::{kinds, Page};

use crate::ledger::LedgerRow;
use crate::turn::TurnRunner;

/// What the keep-warm does with a session, as threads decide it.
#[derive(Debug, Clone, PartialEq)]
pub enum ThreadWarm {
    /// Not a thread's session: the profile's window and rules apply.
    NotThread,
    /// An active thread's: its window in hours (the profile's still applies).
    Window(f64),
    /// A thread's, and not kept (why).
    Out(String),
}

/// A minute's copy of the day's numbers: health reads this, never the ledger.
#[derive(Default)]
pub struct CacheNumbers(Mutex<Option<(Instant, ThreadsCache)>>);

/// How long a copy serves.
const FRESH: std::time::Duration = std::time::Duration::from_secs(60);

impl TurnRunner {
    /// Whether the keep-warm holds `session_id`, as the thread it serves says.
    pub fn thread_warm(&self, session_id: &str) -> ThreadWarm {
        if !self.cfg.threads.enabled {
            return ThreadWarm::NotThread;
        }
        let Ok(board) = self.threads.all(&self.store) else {
            return ThreadWarm::NotThread;
        };
        let Some(t) = board.of_session(session_id) else {
            return ThreadWarm::NotThread;
        };
        if t.state != ThreadState::Active {
            return ThreadWarm::Out(format!("its thread {} is {}", t.slug, t.state.as_str()));
        }
        match self.cfg.threads.keep_warm_hours {
            h if h > 0.0 => ThreadWarm::Window(h),
            _ => ThreadWarm::Out("[threads] keep_warm_hours is 0".into()),
        }
    }
}

/// One thing that touched a session's cache in the window.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A model call; `first` when it is its turn's first.
    Call {
        at_ms: u64,
        first: bool,
        model: String,
        usage: Usage,
    },
    /// A keep-warm read and its cost in dollars.
    Read { at_ms: u64, usd: f64 },
}

impl Event {
    fn at(&self) -> u64 {
        match self {
            Event::Call { at_ms, .. } | Event::Read { at_ms, .. } => *at_ms,
        }
    }
}

/// A session's numbers over `events` (any order). Events before `from_ms`
/// count for nothing but the gaps they leave. `ttl_ms` is the cache's
/// life; `existed` says the session was there before the first event, so a
/// first call with nothing before it in the window is a resume, not a start;
/// `price(model, usage)` is dollars, 0 for a model the catalog lacks.
pub fn stats_of(
    events: &[Event],
    ttl_ms: u64,
    existed: bool,
    from_ms: u64,
    price: impl Fn(&str, &Usage) -> f64,
) -> ThreadCacheStats {
    let mut events = events.to_vec();
    events.sort_by_key(Event::at);
    let mut s = ThreadCacheStats::default();
    let (mut prev_any, mut prev_call) = (None::<u64>, None::<u64>);
    for e in &events {
        match e {
            Event::Read { at_ms, usd } => {
                if *at_ms >= from_ms {
                    s.keep_warm_reads += 1;
                    s.keep_warm_usd += usd;
                }
                prev_any = Some(*at_ms);
            }
            Event::Call {
                at_ms,
                first,
                model,
                usage,
            } => {
                if *first && *at_ms >= from_ms {
                    let gap = prev_any.map(|p| at_ms.saturating_sub(p));
                    let cold = match gap {
                        Some(g) => g > ttl_ms,
                        None => existed,
                    };
                    if cold {
                        s.cold_resumes += 1;
                        s.rewrite_tokens += usage.cache_creation_input_tokens;
                        s.rewrite_usd += price(
                            model,
                            &Usage {
                                cache_creation_input_tokens: usage.cache_creation_input_tokens,
                                cache_creation_1h_input_tokens: usage
                                    .cache_creation_1h_input_tokens,
                                ..Usage::default()
                            },
                        );
                    } else if gap.is_some() && usage.cache_read_input_tokens > 0 {
                        s.warm_hits += 1;
                        let alone = prev_call.map(|p| at_ms.saturating_sub(p));
                        if alone.is_none_or(|g| g > ttl_ms) {
                            // Only a keep-warm read kept it: a rewrite avoided.
                            let r = usage.cache_read_input_tokens;
                            let write = price(
                                model,
                                &Usage {
                                    cache_creation_input_tokens: r,
                                    cache_creation_1h_input_tokens: r,
                                    ..Usage::default()
                                },
                            );
                            let read = price(
                                model,
                                &Usage {
                                    cache_read_input_tokens: r,
                                    ..Usage::default()
                                },
                            );
                            s.avoided += 1;
                            s.saved_usd += (write - read).max(0.0);
                        }
                    }
                }
                prev_any = Some(*at_ms);
                prev_call = Some(*at_ms);
            }
        }
    }
    s
}

impl crate::rpc::Core {
    /// The day's numbers for the sessions in `sessions` (each with whether it
    /// existed before the window): the ledger's `provider.call` and
    /// `keep_warm` rows from an hour before local midnight. A read for a
    /// person or a minute's refresh, never a turn's path; nothing while the
    /// index's shape is built.
    pub fn thread_cache_stats(
        &self,
        sessions: &BTreeMap<String, bool>,
    ) -> BTreeMap<String, ThreadCacheStats> {
        let mut out = BTreeMap::new();
        if sessions.is_empty() {
            return out;
        }
        let ttl = crate::config::CacheTtl::OneHour.ms();
        let midnight = crate::learning::local_midnight(theseus_protocol::now_unix_ms());
        let since = midnight.saturating_sub(ttl);
        let tags = [
            theseus_protocol::LedgerKind::ProviderCall,
            theseus_protocol::LedgerKind::KeepWarm,
        ]
        .map(|k| theseus_store::pages::ledger_kind(k.as_str()));
        let mut by: BTreeMap<&str, Vec<Event>> = BTreeMap::new();
        let mut before = None;
        loop {
            let page = Page {
                kind: kinds::LEDGER,
                tags: tags.to_vec(),
                after: None,
                before,
                since_ms: Some(since),
                until_ms: None,
                limit: 512,
            };
            let Ok(Some(got)) = self.store.ledger_page(&page) else {
                return out;
            };
            for r in &got.records {
                let Ok(row) = r.decode::<LedgerRow>() else {
                    continue;
                };
                let Some((sid, _)) = row
                    .session_id
                    .as_deref()
                    .and_then(|s| sessions.get_key_value(s))
                else {
                    continue;
                };
                if row.at_unix_ms < since {
                    continue;
                }
                if let Some(e) = event_of(&row) {
                    by.entry(sid.as_str()).or_default().push(e);
                }
            }
            match (got.more, got.first) {
                (true, Some(first)) => before = Some(first),
                _ => break,
            }
        }
        let catalog = &self.runner.catalog;
        let price = |m: &str, u: &Usage| catalog.get(m).map_or(0.0, |e| e.cost_usd(u));
        for (sid, events) in by {
            let s = stats_of(&events, ttl, sessions[sid], midnight, price);
            if !s.is_empty() {
                out.insert(sid.to_string(), s);
            }
        }
        out
    }

    /// The sessions of the threads the board holds, each with whether it was
    /// there before today's window.
    fn warm_thread_sessions(&self) -> BTreeMap<String, bool> {
        let Some(board) = self.runner.threads.held() else {
            return BTreeMap::new();
        };
        let start = crate::learning::local_midnight(theseus_protocol::now_unix_ms())
            .saturating_sub(crate::config::CacheTtl::OneHour.ms());
        board
            .all()
            .filter_map(|t| t.session_id.clone())
            .map(|sid| {
                let existed = self
                    .store
                    .get_session::<crate::session::SessionRecord>(&sid)
                    .ok()
                    .flatten()
                    .is_some_and(|r| r.created_at_unix_ms < start);
                (sid, existed)
            })
            .collect()
    }

    /// Health's `threads` section: the window, and the day's numbers from
    /// the minute's copy. A thread-less daemon reads nothing and says nothing.
    pub fn threads_health(&self) -> Option<ThreadsHealth> {
        let board = self.runner.threads.held()?;
        if board.is_empty() {
            return None;
        }
        Some(ThreadsHealth {
            keep_warm_hours: self.cfg.threads.keep_warm_hours,
            cache: Some(self.threads_cache()),
        })
    }

    /// The day's numbers: the copy while it is a minute old or less, else
    /// read again on the blocking pool.
    pub fn threads_cache(&self) -> ThreadsCache {
        let numbers = &self.runner.cache_numbers;
        if let Some((at, c)) = numbers.0.lock().unwrap().as_ref() {
            if at.elapsed() < FRESH {
                return c.clone();
            }
        }
        let fresh = theseus_store::blocking(|| self.read_threads_cache());
        *numbers.0.lock().unwrap() = Some((Instant::now(), fresh.clone()));
        fresh
    }

    fn read_threads_cache(&self) -> ThreadsCache {
        let sessions = self.warm_thread_sessions();
        let stats = self.thread_cache_stats(&sessions);
        let day = crate::keep_warm::today(&self.kernel);
        let Some(board) = self.runner.threads.held() else {
            return ThreadsCache {
                day,
                ..ThreadsCache::default()
            };
        };
        let mut total = ThreadCacheStats::default();
        let mut threads = Vec::new();
        let mut shown: Vec<_> = board.all().collect();
        shown.sort_by(|a, b| (b.last_active_ms, &a.slug).cmp(&(a.last_active_ms, &b.slug)));
        let mut kept = 0;
        for t in shown {
            let s = t
                .session_id
                .as_ref()
                .and_then(|s| stats.get(s))
                .cloned()
                .unwrap_or_default();
            let held = t
                .session_id
                .as_deref()
                .is_some_and(|s| self.runner.keep_warm.holds(s));
            kept += u64::from(held);
            total.add(&s);
            if held || !s.is_empty() {
                threads.push(ThreadCacheEntry {
                    slug: t.slug.clone(),
                    stats: s,
                    kept: held,
                });
            }
        }
        ThreadsCache {
            skipped_today: self.runner.keep_warm.skipped_today(&day),
            day,
            total,
            threads,
            kept,
        }
    }

    /// One thread's numbers for `thread show` and `thread list`, for the
    /// sessions `rows` name: none for a session with none today.
    pub(crate) fn thread_cache_for(
        &self,
        sessions: &[String],
    ) -> BTreeMap<String, ThreadCacheStats> {
        let mut want = BTreeMap::new();
        for sid in sessions {
            let start = crate::learning::local_midnight(theseus_protocol::now_unix_ms())
                .saturating_sub(crate::config::CacheTtl::OneHour.ms());
            let existed = self
                .store
                .get_session::<crate::session::SessionRecord>(sid)
                .ok()
                .flatten()
                .is_some_and(|r| r.created_at_unix_ms < start);
            want.insert(sid.clone(), existed);
        }
        self.thread_cache_stats(&want)
    }
}

/// A ledger row as an event: a `provider.call`'s usage (its turn's first
/// call has loop 0), or a `keep_warm` read and its cost.
fn event_of(row: &LedgerRow) -> Option<Event> {
    match row.kind.as_str() {
        "provider.call" => {
            let usage: Usage = serde_json::from_value(row.data.get("usage")?.clone()).ok()?;
            Some(Event::Call {
                at_ms: row.at_unix_ms,
                first: row.data.get("loop").and_then(|l| l.as_u64()) == Some(0),
                model: row.data.get("model")?.as_str()?.to_string(),
                usage,
            })
        }
        "keep_warm" => Some(Event::Read {
            at_ms: row.at_unix_ms,
            usd: crate::day_ceiling::spend_of(row).map_or(0.0, |m| m as f64 / 1_000_000.0),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3_600_000;
    const MIN: u64 = 60_000;

    fn call(at_min: u64, first: bool, read: u64, wrote: u64) -> Event {
        Event::Call {
            at_ms: at_min * MIN,
            first,
            model: "m".into(),
            usage: Usage {
                cache_read_input_tokens: read,
                cache_creation_input_tokens: wrote,
                cache_creation_1h_input_tokens: wrote,
                ..Usage::default()
            },
        }
    }

    /// A price per million tokens: reads 1, writes 20, nothing else.
    fn price(_: &str, u: &Usage) -> f64 {
        (u.cache_read_input_tokens as f64 * 1.0 + u.cache_creation_input_tokens as f64 * 20.0) / 1e6
    }

    /// The edge of the cache's life: a gap of exactly the TTL is warm, a
    /// minute more is a cold resume; a first call with nothing before it is a
    /// resume only of a session that existed.
    #[test]
    fn a_gap_past_the_ttl_is_a_cold_resume_and_the_edge_is_warm() {
        let at_edge = [call(0, false, 1, 1), call(60, true, 100, 0)];
        let s = stats_of(&at_edge, HOUR, true, 0, price);
        assert_eq!((s.cold_resumes, s.warm_hits), (0, 1));
        let past = [call(0, false, 1, 1), call(61, true, 0, 90_000)];
        let s = stats_of(&past, HOUR, true, 0, price);
        assert_eq!((s.cold_resumes, s.warm_hits), (1, 0));
        assert_eq!(s.rewrite_tokens, 90_000);
        assert!((s.rewrite_usd - 1.8).abs() < 1e-9, "{s:?}");
        let alone = [call(5, true, 0, 90_000)];
        assert_eq!(stats_of(&alone, HOUR, true, 0, price).cold_resumes, 1);
        assert_eq!(stats_of(&alone, HOUR, false, 0, price).cold_resumes, 0);
        // A later loop of a turn is never a resume.
        let loops = [call(0, false, 1, 1), call(300, false, 0, 90_000)];
        assert!(stats_of(&loops, HOUR, true, 0, price).is_empty());
    }

    /// A rewrite is avoided only when the turn calls alone are more than a TTL
    /// apart and a keep-warm read kept the cache; it saves a write's price
    /// less a read's, over the tokens it read.
    #[test]
    fn a_hit_only_a_keep_warm_read_made_is_a_rewrite_avoided() {
        let events = [
            call(0, true, 0, 100_000),
            Event::Read {
                at_ms: 55 * MIN,
                usd: 0.02,
            },
            Event::Read {
                at_ms: 110 * MIN,
                usd: 0.02,
            },
            call(150, true, 100_000, 10),
            call(160, true, 100_200, 10),
        ];
        let s = stats_of(&events, HOUR, false, 0, price);
        assert_eq!((s.cold_resumes, s.warm_hits, s.avoided), (0, 2, 1));
        assert_eq!(s.keep_warm_reads, 2);
        assert!((s.keep_warm_usd - 0.04).abs() < 1e-9);
        // 100,000 tokens read: written at 20 per million, read at 1.
        assert!((s.saved_usd - 1.9).abs() < 1e-9, "{s:?}");
        let none = [call(0, true, 0, 100_000), call(150, true, 0, 100_000)];
        let s = stats_of(&none, HOUR, false, 0, price);
        assert_eq!((s.cold_resumes, s.avoided, s.saved_usd), (1, 0, 0.0));
    }

    /// Events before the day's start leave their gaps and count for nothing.
    #[test]
    fn events_before_the_start_only_give_the_gaps() {
        let events = [
            call(0, false, 1, 1),
            Event::Read {
                at_ms: 10 * MIN,
                usd: 0.5,
            },
            call(80, true, 0, 7),
        ];
        let s = stats_of(&events, HOUR, true, 50 * MIN, price);
        assert_eq!(
            (s.cold_resumes, s.keep_warm_reads, s.keep_warm_usd),
            (1, 0, 0.0)
        );
    }
}
