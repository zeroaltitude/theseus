//! A mail watch's quiet looks, tallied (theseus-55j04).
//!
//! A look with no new mail is one `history.list` call, so one `google.called`
//! row: 480 a day at the 3-minute default, all saying "nothing". A look is
//! **quiet** when it succeeded, matched nothing, did not start again from the
//! profile, and every one of its calls came to `ok`. Its rows go into the
//! entry's tally instead of the ledger, and the tally is written as one
//! content-free row (`quiet_looks`, `calls`, `quota_units`, `ms`) when the
//! watch next has something else to write, when its first look is
//! `QUIET_ROW_MS` old at a later look, or when the watch leaves the board: so
//! about 24 rows a day, and every call still counted.
//!
//! The tally is memory only, like the cursor's `kept_ms`: a restart loses at
//! most an hour's of it.

use serde_json::{json, Value};

/// How old a tally's first look may get before a later look writes it.
pub const QUIET_ROW_MS: u64 = 3_600_000;

/// What the quiet looks since the last written row made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Quiet {
    looks: u64,
    calls: u64,
    quota_units: u64,
    ms: u64,
    /// When the first of them was made: what `QUIET_ROW_MS` counts from.
    pub since_ms: u64,
}

impl Quiet {
    /// Add a quiet look's `rows`, made at `now`.
    pub fn add(&mut self, rows: &[Value], now: u64) {
        if self.looks == 0 {
            self.since_ms = now;
        }
        let sum = |k: &str| rows.iter().filter_map(|r| r[k].as_u64()).sum::<u64>();
        self.looks += 1;
        self.calls += rows.len() as u64;
        self.quota_units += sum("quota_units");
        self.ms += sum("ms");
    }

    /// Whether the first look in the tally is `QUIET_ROW_MS` old at `now`.
    pub fn due(&self, now: u64) -> bool {
        self.looks > 0 && now.saturating_sub(self.since_ms) >= QUIET_ROW_MS
    }

    /// The tally as the one row it is written as, if it holds any look. The
    /// caller empties it (`std::mem::take`).
    pub fn take(&self) -> Option<Value> {
        (self.looks > 0).then(|| {
            json!({
                "service": "gmail", "tool": crate::google::mail::SEARCH,
                "method": "gmail.users.history.list", "class": "read", "status": 200,
                "outcome": "ok", "quiet_looks": self.looks, "calls": self.calls,
                "quota_units": self.quota_units, "ms": self.ms,
            })
        })
    }
}

/// Whether a look's `rows` and `result` make it quiet.
pub fn is_quiet(rows: &[Value], result: &Result<super::cond::Observed, String>) -> bool {
    let nothing = result
        .as_ref()
        .is_ok_and(|o| !o.exists && o.mail.as_ref().is_some_and(|m| !m.rebaselined));
    nothing && rows.iter().all(|r| r["outcome"] == "ok")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(units: u64, ms: u64) -> Value {
        json!({"outcome": "ok", "quota_units": units, "ms": ms})
    }

    #[test]
    fn a_tally_sums_its_looks_and_is_due_an_hour_after_the_first() {
        let mut q = Quiet::default();
        assert!(q.take().is_none() && !q.due(u64::MAX));
        q.add(&[row(2, 30)], 1_000);
        q.add(&[row(2, 40), row(5, 10)], 200_000);
        let t = q.take().unwrap();
        assert_eq!(t["quiet_looks"], 2);
        assert_eq!(t["calls"], 3);
        assert_eq!(t["quota_units"], 9);
        assert_eq!(t["ms"], 80);
        assert!(!q.due(1_000 + QUIET_ROW_MS - 1));
        assert!(q.due(1_000 + QUIET_ROW_MS));
    }
}
