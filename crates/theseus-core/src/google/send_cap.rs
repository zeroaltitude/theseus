//! The daily send brake (the Google design, §4.3; `[google] max_sends_per_day`,
//! 25; 0 is no sends): a runaway brake under Gmail's own limits. Sends are
//! counted per local day, and the count survives a restart (a META record,
//! written when the store is attached), so a restart is no reset. A send by
//! `google.call` counts as one by `gmail.send`: the escape hatch cannot route
//! around it.

use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::store::Store;

const KEY: &str = "google.sends";

struct Day {
    day: String,
    count: u32,
}

pub struct Sends {
    max: u32,
    state: Mutex<Day>,
    store: OnceLock<Result<Store, String>>,
}

fn today() -> String {
    crate::judge::spend::local_day(theseus_protocol::now_unix_ms())
}

impl Sends {
    pub fn new(max: u32) -> Self {
        Self {
            max,
            state: Mutex::new(Day {
                day: today(),
                count: 0,
            }),
            store: OnceLock::new(),
        }
    }

    /// Keep the count in `store` from now on, starting from what it holds for
    /// today. After serving, never on the start path.
    pub fn attach(&self, store: Store) {
        let loaded = (|| -> Result<Store, String> {
            let held: Option<Value> = store
                .get_meta(KEY)
                .map_err(|e| format!("send count could not be read: {e:#}"))?;
            if let Some(v) = held {
                let day = v["day"].as_str().ok_or("send count has no day")?;
                if day == today() {
                    let n = v["count"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .ok_or("send count is invalid")?;
                    let mut s = self.state.lock().unwrap();
                    *s = Day {
                        day: day.to_string(),
                        count: s.count.max(n),
                    };
                }
            }
            Ok(store)
        })();
        if let Err(why) = &loaded {
            tracing::warn!(error = %why, "google: sends blocked because the day's count was not read");
        }
        let _ = self.store.set(loaded);
    }

    fn rolled<R>(&self, f: impl FnOnce(&mut Day) -> R) -> R {
        let mut s = self.state.lock().unwrap();
        if s.day != today() {
            *s = Day {
                day: today(),
                count: 0,
            };
        }
        f(&mut s)
    }

    fn keep(&self, d: &Day) -> Result<(), String> {
        let store = self
            .store
            .get()
            .ok_or("send count store is not attached")?
            .as_ref()
            .map_err(Clone::clone)?;
        store
            .put_meta(KEY, &json!({"day": d.day, "count": d.count}))
            .map_err(|e| format!("send count could not be kept; send refused: {e:#}"))
    }

    /// Sends counted today.
    pub fn used(&self) -> u32 {
        self.rolled(|d| d.count)
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    fn refusal(&self, count: u32) -> Option<String> {
        if self.max == 0 {
            return Some("sends are off: `[google] max_sends_per_day` is 0".into());
        }
        (count >= self.max).then(|| {
            format!(
                "today's send cap is reached ({count} of {}): `[google] max_sends_per_day` is a \
                 brake per local day, and a send by google_call counts too",
                self.max
            )
        })
    }

    /// Whether a send would be allowed now, counting nothing: a plan asks it,
    /// so a send that cannot go is invalid input and asks the owner nothing.
    pub fn allows(&self) -> Result<(), String> {
        self.store
            .get()
            .ok_or("send count store is not attached")?
            .as_ref()
            .map_err(Clone::clone)?;
        self.rolled(|d| self.refusal(d.count)).map_or(Ok(()), Err)
    }

    /// Count one send before it is sent.
    pub fn reserve(&self) -> Result<(), String> {
        self.rolled(|d| {
            if let Some(why) = self.refusal(d.count) {
                return Err(why);
            }
            d.count += 1;
            self.keep(d)
        })
    }

    /// Give one back: the send was refused before it could go out.
    pub fn release(&self) {
        self.rolled(|d| {
            d.count = d.count.saturating_sub(1);
            if let Err(why) = self.keep(d) {
                tracing::warn!(error = %why, "google: the returned send reservation was not kept");
            }
        });
    }
}

#[test]
fn a_send_requires_a_durable_reservation() {
    let _clock = crate::test_support::pin_mid_period();
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let sends = Sends::new(25);
    sends.attach(store.clone());
    store.fail_plain_frame(|_| true);
    assert!(sends.reserve().is_err());
    assert_eq!(sends.used(), 1);
}

#[test]
fn an_unreadable_send_count_does_not_reset_the_cap() {
    let _clock = crate::test_support::pin_mid_period();
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .put_meta(KEY, &json!({"day": today(), "count": "broken"}))
        .unwrap();
    let sends = Sends::new(25);
    sends.attach(store);
    assert!(sends.reserve().is_err());
}

#[test]
fn an_unavailable_counter_refuses_before_approval() {
    let _clock = crate::test_support::pin_mid_period();
    let unattached = Sends::new(25);
    assert!(unattached.allows().is_err());
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .put_meta(KEY, &json!({"day": today(), "count": "broken"}))
        .unwrap();
    let corrupt = Sends::new(25);
    corrupt.attach(store);
    assert!(corrupt.allows().is_err());
    assert_eq!(corrupt.used(), 0);
    let unreadable = Sends::new(25);
    assert!(unreadable
        .store
        .set(Err("send count could not be read".into()))
        .is_ok());
    assert!(unreadable.allows().is_err());
    assert_eq!(unreadable.used(), 0);
}
