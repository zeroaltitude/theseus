//! The tender's status, read once and shared (theseus-id8d): every caller of
//! [`IndexTender::health`] (health's index block, `index.status`, the memory
//! pass, the gauges' sampler) takes the last read while it is younger than
//! [`STATUS_EVERY`], so the cockpit's health every 2 s, the CLI's status line
//! and the TUI together cost one connection to the tender a second at most.
//! Callers that arrive while a read is out wait for its answer, each no
//! longer than its own deadline. A status shown because the tender did not
//! answer says how old it is, and past [`STALE_AFTER`] it says `stale`
//! (theseus-uazd: a status hours old was shown with only an age).
//!
//! The cache is a struct the index block's readers can extend (index-memory
//! reads its backlog and the tender's RSS from the same status).

use std::sync::PoisonError;
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::index::{method, IndexHealth, IndexStatus};
use theseus_protocol::TenderStatus;
use tokio::time::Instant;

use super::{call, down_why, CallError, IndexTender};

/// How long one status read serves every caller.
pub const STATUS_EVERY: Duration = Duration::from_secs(1);

/// A status shown past this age, because the tender did not answer since,
/// says `stale`.
pub const STALE_AFTER: Duration = Duration::from_secs(180);

/// What the last reads left.
#[derive(Default)]
pub(super) struct Cache {
    /// The last status the tender answered, and when (unix ms).
    last: Option<(IndexStatus, u64)>,
    /// The last read, answered or not: when it ended, and why it failed.
    tried: Option<(Instant, Option<CallError>)>,
    /// Status reads sent to the tender, for health's tests.
    reads: u64,
}

impl IndexTender {
    /// Health's `index` block: the tender's own `index.status`, read at most
    /// once each [`STATUS_EVERY`] and asked under `deadline`
    /// ([`HEALTH_DEADLINE`](super::HEALTH_DEADLINE) for `health`,
    /// [`STATUS_DEADLINE`](super::STATUS_DEADLINE) for `index.status`). A
    /// running tender that does not answer in time is shown by its last
    /// answer, and says how old it is (`stale` past [`STALE_AFTER`]); one
    /// that is not running is `down`, and why.
    pub async fn health(&self, deadline: Duration) -> IndexHealth {
        let Some(tender) = self.status() else {
            return IndexHealth {
                state: "off".into(),
                why: Some("[index] enabled = false".into()),
                ..IndexHealth::default()
            };
        };
        let began = Instant::now();
        if let Some(h) = self.fresh(&tender) {
            return h;
        }
        // One read at a time: a caller that waits here takes that read's
        // answer, and waits no longer than its own deadline.
        let Ok(_asking) = tokio::time::timeout(deadline, self.asking.lock()).await else {
            let late = CallError::NoAnswer(format!("no answer within {} ms", deadline.as_millis()));
            return self.answer(&tender, Some(&late));
        };
        if let Some(h) = self.fresh(&tender) {
            return h;
        }
        self.cache().reads += 1;
        // The caller's deadline counts from its call, and its words name it.
        let left = deadline.saturating_sub(began.elapsed());
        let socket = self.socket();
        let ask = call::<IndexStatus>(&socket, method::STATUS, Value::Null, deadline);
        let asked = tokio::time::timeout(left, ask).await.unwrap_or_else(|_| {
            Err(CallError::NoAnswer(format!(
                "no answer within {} ms",
                deadline.as_millis()
            )))
        });
        let mut c = self.cache();
        let failed = match asked {
            Ok(s) => {
                c.last = Some((s, theseus_protocol::now_unix_ms()));
                None
            }
            Err(e) => Some(e),
        };
        c.tried = Some((Instant::now(), failed.clone()));
        drop(c);
        self.answer(&tender, failed.as_ref())
    }

    /// The last read's answer, while it is younger than [`STATUS_EVERY`].
    fn fresh(&self, tender: &TenderStatus) -> Option<IndexHealth> {
        let failed = {
            let c = self.cache();
            let (at, failed) = c.tried.as_ref()?;
            if at.elapsed() >= self.status_every {
                return None;
            }
            failed.clone()
        };
        Some(self.answer(tender, failed.as_ref()))
    }

    /// The block, from what the cache holds and how the last read went.
    fn answer(&self, tender: &TenderStatus, failed: Option<&CallError>) -> IndexHealth {
        let c = self.cache();
        let shown = c.last.as_ref().filter(|_| tender.state == "running");
        match (failed, &c.last, shown) {
            (None, Some((s, _)), _) => IndexHealth {
                state: s.state.clone(),
                why: None,
                tender: Some(tender.clone()),
                status: Some(s.clone()),
            },
            (Some(e), _, Some((s, at))) => {
                let age = theseus_protocol::now_unix_ms().saturating_sub(*at);
                IndexHealth {
                    state: s.state.clone(),
                    why: Some(old_words(e, age)),
                    tender: Some(tender.clone()),
                    status: Some(s.clone()),
                }
            }
            _ => IndexHealth {
                state: "down".into(),
                why: Some(down_why(tender, failed)),
                tender: Some(tender.clone()),
                status: None,
            },
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The status reads sent to the tender since this supervisor was made.
    pub fn status_reads(&self) -> u64 {
        self.cache().reads
    }

    /// Make the last answer `ms` older, as if the tender had not answered
    /// for that long (tests: the stale words without a three-minute wait).
    #[cfg(test)]
    pub(crate) fn age_status(&self, ms: u64) {
        if let Some((_, at)) = self.cache().last.as_mut() {
            *at = at.saturating_sub(ms);
        }
    }
}

/// Why a status shown is its last answer, and how old that is: `stale` first
/// once it is older than [`STALE_AFTER`].
fn old_words(e: &CallError, age_ms: u64) -> String {
    if age_ms >= STALE_AFTER.as_millis() as u64 {
        let mins = age_ms / 60_000;
        let age = if mins >= 120 {
            format!("{} h", mins / 60)
        } else {
            format!("{mins} min")
        };
        format!("stale: its status is {age} old, and its socket did not answer ({e})")
    } else {
        format!(
            "its socket did not answer ({e}): its status as of {:.1} s ago",
            age_ms as f64 / 1000.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The age's words: seconds while it is recent, `stale` in minutes past
    /// three, and in hours past two.
    #[test]
    fn an_old_status_says_stale_plainly() {
        let e = CallError::NoAnswer("no answer within 100 ms".into());
        assert_eq!(
            old_words(&e, 2_500),
            "its socket did not answer (no answer within 100 ms): its status as of 2.5 s ago"
        );
        assert_eq!(
            old_words(&e, 179_999),
            "its socket did not answer (no answer within 100 ms): its status as of 180.0 s ago"
        );
        assert_eq!(
            old_words(&e, 180_000),
            "stale: its status is 3 min old, and its socket did not answer (no answer within 100 ms)"
        );
        assert_eq!(
            old_words(&e, 5 * 3_600_000 + 60_000),
            "stale: its status is 5 h old, and its socket did not answer (no answer within 100 ms)"
        );
    }
}
