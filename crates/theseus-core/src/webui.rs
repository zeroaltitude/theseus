//! The web UI's refusals (theseus-70f). The UI listens on a loopback port,
//! and a browser lets any page it shows open a WebSocket to that port, or
//! reach it by a name the page's own site resolves to 127.0.0.1 (DNS
//! rebinding). So the UI refuses a request whose `Host` is not its own
//! address and port, and a WebSocket upgrade whose `Origin` is not its own
//! page. Health counts every refusal (`web`). The ledger has a `web.refused`
//! row for the first of each kind at once, then at most one a minute, which
//! says how many refusals it stands for: a page can retry thousands of times
//! a second, and each row is a frame in the WAL. Nothing is narrated.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
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
}

impl Why {
    pub fn as_str(self) -> &'static str {
        match self {
            Why::Host => "host",
            Why::Origin => "origin",
        }
    }
}

pub struct Refusals {
    host: Kind,
    origin: Kind,
    every: Duration,
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
            every,
        }
    }

    fn kind(&self, why: Why) -> &Kind {
        match why {
            Why::Host => &self.host,
            Why::Origin => &self.origin,
        }
    }

    /// Count a refusal, with what the request said. It is ledgered now when
    /// its kind has had no row for `every`, and otherwise in the row a timer
    /// writes when the span is up. Runs inside the runtime (the timer is a
    /// task).
    pub fn refuse(self: &Arc<Self>, store: &Store, why: Why, detail: Value) {
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

    pub fn status(&self) -> theseus_protocol::WebStatus {
        theseus_protocol::WebStatus {
            refused_host: self.host.total.load(Ordering::Relaxed),
            refused_origin: self.origin.total.load(Ordering::Relaxed),
        }
    }
}

fn ledger(store: &Store, why: Why, count: u64, last: Value) {
    tracing::warn!(
        why = why.as_str(),
        count,
        last = %last,
        "the web UI refused {count} request(s) not from its own page or address"
    );
    let row = LedgerRow::new(
        "web.refused",
        None,
        None,
        json!({"why": why.as_str(), "count": count, "last": last}),
    );
    if let Err(e) = store.append_ledger(&row) {
        tracing::warn!(error = %format!("{e:#}"), "the web UI's refusals were not ledgered");
    }
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
        store
            .ledger_tail::<LedgerRow>(100)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == "web.refused")
            .map(|(_, r)| r.data)
            .collect()
    }

    /// A burst is counted whole in health and ledgered as two rows, not one
    /// per request: the first at once, the rest in the span's one row.
    #[tokio::test]
    async fn a_burst_of_refusals_is_counted_and_ledgered_at_most_once_a_span() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(&d.path().join("store")).unwrap();
        let r = Arc::new(Refusals::every(Duration::from_millis(300)));
        for i in 0..500 {
            r.refuse(&store, Why::Origin, json!({"origin": format!("page {i}")}));
        }
        r.refuse(&store, Why::Host, json!({"host": "evil.example:7433"}));
        assert_eq!(
            r.status(),
            theseus_protocol::WebStatus {
                refused_host: 1,
                refused_origin: 500
            }
        );
        let now = rows(&store);
        assert_eq!(now.len(), 2, "{now:?}");
        assert_eq!(now[0]["why"], "origin");
        assert_eq!(now[0]["count"], 1);
        assert_eq!(now[1]["why"], "host");
        tokio::time::sleep(Duration::from_millis(600)).await;
        let later = rows(&store);
        assert_eq!(later.len(), 3, "{later:?}");
        assert_eq!(later[2]["count"], 499);
        assert_eq!(later[2]["last"]["origin"], "page 499");
        // Quiet after that: no row without a refusal.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(rows(&store).len(), 3);
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
