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
    store: OnceLock<Store>,
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
        let held: Option<Value> = store.get_meta(KEY).ok().flatten();
        if let Some(v) = held {
            let day = v["day"].as_str().unwrap_or_default();
            if day == today() {
                let n = v["count"].as_u64().unwrap_or(0) as u32;
                let mut s = self.state.lock().unwrap();
                *s = Day {
                    day: day.to_string(),
                    count: s.count.max(n),
                };
            }
        }
        let _ = self.store.set(store);
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

    fn keep(&self, d: &Day) {
        if let Some(store) = self.store.get() {
            if let Err(e) = store.put_meta(KEY, &json!({"day": d.day, "count": d.count})) {
                tracing::warn!(error = %format!("{e:#}"), "google: the day's send count was not kept");
            }
        }
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
        self.rolled(|d| self.refusal(d.count)).map_or(Ok(()), Err)
    }

    /// Count one send before it is sent.
    pub fn reserve(&self) -> Result<(), String> {
        self.rolled(|d| {
            if let Some(why) = self.refusal(d.count) {
                return Err(why);
            }
            d.count += 1;
            self.keep(d);
            Ok(())
        })
    }

    /// Give one back: the send was refused before it could go out.
    pub fn release(&self) {
        self.rolled(|d| {
            d.count = d.count.saturating_sub(1);
            self.keep(d);
        });
    }
}
