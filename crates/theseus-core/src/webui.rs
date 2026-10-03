//! The web UI's refusals (theseus-70f). The UI listens on a loopback port,
//! and a browser lets any page it shows open a WebSocket to that port, or
//! reach it by a name the page's own site resolves to 127.0.0.1 (DNS
//! rebinding). So the UI refuses a request whose `Host` is not its own
//! address and port, and a WebSocket upgrade whose `Origin` is not its own
//! page. Any local process can send both, so a connection whose client
//! socket is not the daemon's own uid is refused too, as it is accepted
//! (theseus-3qf). Health counts every refusal (`web`). The ledger has a
//! `web.refused` row for the first of each kind at once, then at most one a
//! minute, which says how many refusals it stands for: a page can retry
//! thousands of times a second, and each row is a frame in the WAL. Nothing
//! is narrated. The dev page's uses (`[web] dev_origin`, theseus-zab) are
//! counted and ledgered the same way, as `web.dev_origin`. A clean stop
//! writes what its span still holds (`flush`, theseus-sqpx), since the
//! span's timer is a task the runtime's end drops.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use theseus_protocol::LedgerKind;

use serde_json::{json, Value};
use theseus_store::{kinds, NewRecord};
use tokio::time::Instant;

use crate::ledger::LedgerRow;
use crate::store::Store;

/// At most one `web.refused` row of a kind in this span.
pub const ROW_EVERY: Duration = Duration::from_secs(60);

/// Why the web UI refused a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Its `Host` is not the UI's loopback address and port.
    Host,
    /// A WebSocket upgrade whose `Origin` is not the UI's own page.
    Origin,
    /// A connection whose client socket another uid owns: another local
    /// user's process (theseus-3qf).
    Peer,
}

impl Why {
    pub fn as_str(self) -> &'static str {
        match self {
            Why::Host => "host",
            Why::Origin => "origin",
            Why::Peer => "peer",
        }
    }

    /// What the log line says was refused.
    fn what(self) -> &'static str {
        match self {
            Why::Host | Why::Origin => "request(s) not from its own page or address",
            Why::Peer => "connection(s) from a process of another user",
        }
    }
}

/// Health's word when the port cannot check its clients' owner.
pub const PEER_UNCHECKED: &str = "this platform keeps no table of socket owners (not Linux), so \
                                  a local process of any user is served";

pub struct Refusals {
    host: Kind,
    origin: Kind,
    peer: Kind,
    /// The dev page's `/ws` upgrades, served (theseus-zab).
    dev: Kind,
    every: Duration,
}

/// What a row stands for: refusals of one kind, or the dev page's uses.
#[derive(Debug, Clone, Copy)]
enum Row {
    Refused(Why),
    DevOrigin,
}

#[derive(Default)]
struct Kind {
    total: AtomicU64,
    rows: Mutex<Rows>,
}

#[derive(Default)]
struct Rows {
    /// When this kind's last row was written.
    last: Option<Instant>,
    /// Refusals since that row, and what the latest one said.
    held: u64,
    detail: Value,
    /// A timer will write the held ones when the span is up.
    armed: bool,
}

impl Rows {
    fn take(&mut self) -> (u64, Value) {
        self.last = Some(Instant::now());
        (
            std::mem::take(&mut self.held),
            std::mem::take(&mut self.detail),
        )
    }
}

impl Default for Refusals {
    fn default() -> Self {
        Self::every(ROW_EVERY)
    }
}

impl Refusals {
    /// Refusals ledgered at most once in `every` per kind (tests shorten it).
    pub fn every(every: Duration) -> Self {
        Self {
            host: Kind::default(),
            origin: Kind::default(),
            peer: Kind::default(),
            dev: Kind::default(),
            every,
        }
    }

    fn kind(&self, row: Row) -> &Kind {
        match row {
            Row::Refused(Why::Host) => &self.host,
            Row::Refused(Why::Origin) => &self.origin,
            Row::Refused(Why::Peer) => &self.peer,
            Row::DevOrigin => &self.dev,
        }
    }

    /// Count a refusal, with what the request said. It is ledgered now when
    /// its kind has had no row for `every`, and otherwise in the row a timer
    /// writes when the span is up. Runs inside the runtime (the timer is a
    /// task).
    pub fn refuse(self: &Arc<Self>, store: &Store, why: Why, detail: Value) {
        self.note(store, Row::Refused(why), detail);
    }

    /// Count a `/ws` upgrade served for `[web] dev_origin` (theseus-zab),
    /// ledgered as `web.dev_origin` under the same rule.
    pub fn dev_origin(self: &Arc<Self>, store: &Store, detail: Value) {
        self.note(store, Row::DevOrigin, detail);
    }

    fn note(self: &Arc<Self>, store: &Store, why: Row, detail: Value) {
        let k = self.kind(why);
        k.total.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();
        let due = {
            let mut r = k.rows.lock().unwrap();
            r.held += 1;
            r.detail = detail;
            match r.last {
                Some(t) if now < t + self.every => {
                    if !r.armed {
                        r.armed = true;
                        let (me, store, at) = (self.clone(), store.clone(), t + self.every);
                        tokio::spawn(async move {
                            tokio::time::sleep_until(at).await;
                            let due = {
                                let mut r = me.kind(why).rows.lock().unwrap();
                                r.armed = false;
                                (r.held > 0).then(|| r.take())
                            };
                            if let Some((count, detail)) = due {
                                ledger(&store, why, count, detail);
                            }
                        });
                    }
                    None
                }
                _ => Some(r.take()),
            }
        };
        if let Some((count, detail)) = due {
            ledger(store, why, count, detail);
        }
    }

    /// Write every kind's held refusals and dev-page uses now (theseus-sqpx):
    /// at a clean stop, while the store is open, so what a span still holds
    /// is ledgered and not lost with its timer's task. One frame for all of
    /// them, and none when nothing is held, so a stop writes only when a row
    /// waits. A kind's timer that fires later finds nothing held.
    pub fn flush(&self, store: &Store) {
        let held = [
            Row::Refused(Why::Host),
            Row::Refused(Why::Origin),
            Row::Refused(Why::Peer),
            Row::DevOrigin,
        ]
        .into_iter()
        .filter_map(|why| {
            let mut r = self.kind(why).rows.lock().unwrap();
            (r.held > 0).then(|| {
                let (count, last) = r.take();
                entry(why, count, last)
            })
        });
        let records: Vec<NewRecord> = held
            .filter_map(|row| NewRecord::json(kinds::LEDGER, None, &row).ok())
            .collect();
        if records.is_empty() {
            return;
        }
        if let Err(e) = store.append(&records) {
            tracing::warn!(error = %format!("{e:#}"), "the web UI's held rows were not ledgered at the stop");
        }
    }

    /// Health's `web`, with the configured `[web] dev_origin`.
    pub fn status(&self, dev_origin: Option<&str>) -> theseus_protocol::WebStatus {
        theseus_protocol::WebStatus {
            refused_host: self.host.total.load(Ordering::Relaxed),
            refused_origin: self.origin.total.load(Ordering::Relaxed),
            refused_peer: self.peer.total.load(Ordering::Relaxed),
            peer_unchecked: (!cfg!(target_os = "linux")).then(|| PEER_UNCHECKED.to_string()),
            dev_origin: dev_origin.map(str::to_string),
            dev_origin_served: self.dev.total.load(Ordering::Relaxed),
        }
    }
}

fn ledger(store: &Store, why: Row, count: u64, last: Value) {
    let row = entry(why, count, last);
    if let Err(e) = store.append_ledger(&row) {
        tracing::warn!(error = %format!("{e:#}"), "the web UI's {} row was not ledgered", row.kind);
    }
}

/// A row of `count` refusals (or dev-page uses), the latest saying `last`,
/// and its log line.
fn entry(why: Row, count: u64, last: Value) -> LedgerRow {
    let (kind, data) = match why {
        Row::Refused(why) => {
            tracing::warn!(
                why = why.as_str(),
                count,
                last = %last,
                "the web UI refused {count} {}",
                why.what()
            );
            (
                LedgerKind::WebRefused,
                json!({"why": why.as_str(), "count": count, "last": last}),
            )
        }
        Row::DevOrigin => {
            tracing::warn!(
                count,
                last = %last,
                "the web UI served {count} WebSocket(s) for the dev page ([web] dev_origin)"
            );
            (
                LedgerKind::WebDevOrigin,
                json!({"count": count, "last": last}),
            )
        }
    };
    LedgerRow::new(kind, None, None, data)
}

/// A request's header, as a ledger row keeps it: text, at most 200 characters.
pub fn clip(v: Option<&[u8]>) -> Value {
    match v {
        None => Value::Null,
        Some(b) => {
            let s = String::from_utf8_lossy(b);
            let mut out: String = s.chars().take(200).collect();
            if s.chars().count() > 200 {
                out.push('…');
            }
            Value::String(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::LedgerRow;

    fn rows(store: &Store) -> Vec<Value> {
        rows_of(store, "web.refused")
    }

    fn rows_of(store: &Store, kind: &str) -> Vec<Value> {
        store
            .ledger_tail::<LedgerRow>(100)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    /// A burst is counted whole in health and ledgered as two rows, not one
    /// per request: the first at once, the rest in the span's one row.
    // On the paused clock (theseus-56r7): "within one span" is a fact of the test, and
    // the sleeps move it, not the scheduler.
    #[tokio::test(start_paused = true)]
    async fn a_burst_of_refusals_is_counted_and_ledgered_at_most_once_a_span() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(&d.path().join("store")).unwrap();
        let r = Arc::new(Refusals::every(Duration::from_millis(300)));
        for i in 0..500 {
            r.refuse(&store, Why::Origin, json!({"origin": format!("page {i}")}));
        }
        r.refuse(&store, Why::Host, json!({"host": "evil.example:7433"}));
        // Another user's process, connecting again and again (theseus-3qf).
        for port in 0..40 {
            r.refuse(
                &store,
                Why::Peer,
                json!({"client": format!("127.0.0.1:{}", 40000 + port), "uid": 65534}),
            );
        }
        assert_eq!(
            r.status(None),
            theseus_protocol::WebStatus {
                refused_host: 1,
                refused_origin: 500,
                refused_peer: 40,
                ..Default::default()
            }
        );
        let now = rows(&store);
        assert_eq!(now.len(), 3, "{now:?}");
        assert_eq!(now[0]["why"], "origin");
        assert_eq!(now[0]["count"], 1);
        assert_eq!(now[1]["why"], "host");
        assert_eq!(now[2]["why"], "peer");
        assert_eq!(now[2]["last"]["uid"], 65534);
        tokio::time::sleep(Duration::from_millis(600)).await;
        let later = rows(&store);
        assert_eq!(later.len(), 5, "{later:?}");
        let held: Vec<(&str, u64)> = later[3..]
            .iter()
            .map(|r| (r["why"].as_str().unwrap(), r["count"].as_u64().unwrap()))
            .collect();
        assert!(
            held.contains(&("origin", 499)) && held.contains(&("peer", 39)),
            "{held:?}"
        );
        let origin = later.iter().rfind(|r| r["why"] == "origin").unwrap();
        assert_eq!(origin["last"]["origin"], "page 499");
        let peer = later.iter().rfind(|r| r["why"] == "peer").unwrap();
        assert_eq!(peer["last"]["client"], "127.0.0.1:40039");
        // Quiet after that: no row without a refusal.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(rows(&store).len(), 5);
    }

    /// The dev page's uses (theseus-zab) are counted whole and ledgered as
    /// `web.dev_origin` under the same rule, apart from the refusals.
    // On the paused clock (theseus-56r7): "within one span" is a fact of the test, and
    // the sleeps move it, not the scheduler.
    #[tokio::test(start_paused = true)]
    async fn the_dev_origins_uses_are_counted_and_ledgered_at_most_once_a_span() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(&d.path().join("store")).unwrap();
        let r = Arc::new(Refusals::every(Duration::from_millis(300)));
        let dev = "http://localhost:5173";
        for i in 0..3 {
            r.dev_origin(
                &store,
                json!({"origin": dev, "client": format!("127.0.0.1:{}", 50000 + i)}),
            );
        }
        let s = r.status(Some(dev));
        assert_eq!(
            (s.dev_origin.as_deref(), s.dev_origin_served),
            (Some(dev), 3)
        );
        assert_eq!(s.refused_origin + s.refused_host + s.refused_peer, 0);
        assert_eq!(rows_of(&store, "web.dev_origin").len(), 1);
        assert!(rows(&store).is_empty());
        tokio::time::sleep(Duration::from_millis(600)).await;
        let later = rows_of(&store, "web.dev_origin");
        assert_eq!(
            later
                .iter()
                .map(|r| r["count"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2],
            "{later:?}"
        );
        assert_eq!(later[1]["last"]["client"], "127.0.0.1:50002");
        assert_eq!(r.status(None).dev_origin, None);
    }

    /// theseus-sqpx: what a span holds is written by `flush`, every kind's
    /// in one frame, each row with its count and its latest detail; nothing
    /// held, and no frame. The span's timer then finds nothing to write.
    #[tokio::test]
    async fn a_flush_writes_every_held_row_in_one_frame() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(&d.path().join("store")).unwrap();
        let r = Arc::new(Refusals::every(Duration::from_millis(300)));
        let frames = |s: &Store| s.stats().unwrap().frames_appended;
        r.flush(&store);
        assert_eq!(frames(&store), 0, "nothing held, nothing written");
        for i in 0..3 {
            r.refuse(
                &store,
                Why::Peer,
                json!({"client": format!("127.0.0.1:{}", 40000 + i), "uid": 65534}),
            );
            r.dev_origin(
                &store,
                json!({"client": format!("127.0.0.1:{}", 50000 + i)}),
            );
        }
        assert_eq!(
            (rows(&store).len(), frames(&store)),
            (1, 2),
            "the first of each at once"
        );
        r.flush(&store);
        assert_eq!(frames(&store), 3, "the held rows are one frame");
        let refused = rows(&store);
        assert_eq!(refused.len(), 2, "{refused:?}");
        assert_eq!(refused[1]["count"], 2);
        assert_eq!(refused[1]["last"]["client"], "127.0.0.1:40002");
        let dev = rows_of(&store, "web.dev_origin");
        assert_eq!(dev.len(), 2, "{dev:?}");
        assert_eq!(dev[1]["count"], 2);
        r.flush(&store);
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(
            frames(&store),
            3,
            "a second flush and the timers write nothing"
        );
    }

    #[test]
    fn a_header_is_kept_as_text_and_clipped() {
        assert_eq!(clip(None), Value::Null);
        assert_eq!(clip(Some(b"http://evil.example")), "http://evil.example");
        let long = "x".repeat(300);
        assert_eq!(
            clip(Some(long.as_bytes()))
                .as_str()
                .unwrap()
                .chars()
                .count(),
            201
        );
        assert_eq!(clip(Some(&[0xff, b'a'])), "\u{fffd}a");
    }
}
