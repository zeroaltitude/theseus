//! The tender's status, read once and shared (theseus-id8d): every caller of
//! [`IndexTender::health`] (health's index block, `index.status`, the memory
//! pass, the gauges' sampler) takes the last read while it is younger than
//! [`STATUS_EVERY`], so the cockpit's health every 2 s, the CLI's status line
//! and the TUI together cost one connection to the tender a second at most.
//! Callers that arrive while a read is out wait for its answer, each no
//! longer than its own deadline. A status shown because the read did not
//! come back says how old it is, and past [`STALE_AFTER`] it says `stale`
//! (theseus-uazd: a status hours old was shown with only an age); and it says
//! which: a read still out at the caller's deadline, or one that ended without
//! an answer (theseus-la5oc).
//!
//! The cache is a struct the index block's readers can extend (index-memory
//! reads its backlog and the tender's RSS from the same status).

use std::sync::PoisonError;
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::index::{method, IndexHealth, IndexStatus};
use theseus_protocol::TenderStatus;
use tokio::time::Instant;

use super::{call, down_why, CallError, IndexTender, STATUS_DEADLINE};

/// How long one status read serves every caller.
pub const STATUS_EVERY: Duration = Duration::from_secs(1);

/// A status shown past this age, because the tender did not answer since,
/// says `stale`.
pub const STALE_AFTER: Duration = Duration::from_secs(180);

/// Why the status shown is not a fresh answer. Two different conditions
/// (theseus-la5oc, the b2-tasks-and-tender review's F3): a caller's deadline
/// passed with the read still out, which goes on under [`STATUS_DEADLINE`] and
/// may yet land, and a read that ended without an answer, which will not.
#[derive(Debug, Clone)]
pub(super) enum Miss {
    /// The caller stopped waiting at its own deadline; the read is still out.
    Out(Duration),
    /// The read ended in this error.
    Ended(CallError),
}

impl Miss {
    /// The call error that words a tender with no status to show.
    fn error(&self) -> CallError {
        match self {
            Miss::Out(d) => CallError::NoAnswer(format!("no answer within {} ms", d.as_millis())),
            Miss::Ended(e) => e.clone(),
        }
    }
}

/// What the last reads left.
#[derive(Default)]
pub(super) struct Cache {
    /// The last status the tender answered, and when (unix ms).
    last: Option<(IndexStatus, u64)>,
    /// The last read, answered or not: when it ended or its caller left, and
    /// why it is not an answer.
    tried: Option<(Instant, Option<Miss>)>,
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
        let Ok(asking) = tokio::time::timeout(deadline, self.asking.clone().lock_owned()).await
        else {
            return self.answer(&tender, Some(&Miss::Out(deadline)));
        };
        if let Some(h) = self.fresh(&tender) {
            return h;
        }
        self.cache().reads += 1;
        // The read is its own task, bounded by `STATUS_DEADLINE` and not by
        // the caller's deadline: the caller stops waiting at its own, but the
        // read it started goes on and lands in the cache. Cut with its
        // caller, a tender whose status took longer than health's 100 ms was
        // never read at all, and its line said "did not answer" for as long
        // as it stayed slow.
        let (socket, cache) = (self.socket(), self.cache.clone());
        let read = tokio::spawn(async move {
            let _asking = asking;
            let asked =
                call::<IndexStatus>(&socket, method::STATUS, Value::Null, STATUS_DEADLINE).await;
            let mut c = cache.lock().unwrap_or_else(PoisonError::into_inner);
            let failed = match asked {
                Ok(s) => {
                    c.last = Some((s, theseus_protocol::now_unix_ms()));
                    None
                }
                Err(e) => Some(Miss::Ended(e)),
            };
            c.tried = Some((Instant::now(), failed.clone()));
            failed
        });
        // The caller's deadline counts from its call, and its words name it.
        let left = deadline.saturating_sub(began.elapsed());
        let failed = match tokio::time::timeout(left, read).await {
            Ok(Ok(failed)) => failed,
            Ok(Err(e)) => Some(Miss::Ended(CallError::NoAnswer(format!(
                "the status read ended: {e}"
            )))),
            Err(_) => {
                let late = Miss::Out(deadline);
                let mut c = self.cache();
                // Never over a read that landed in the meantime.
                if c.tried
                    .as_ref()
                    .is_none_or(|(at, _)| at.elapsed() >= self.status_every)
                {
                    c.tried = Some((Instant::now(), Some(late.clone())));
                }
                Some(late)
            }
        };
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
    fn answer(&self, tender: &TenderStatus, failed: Option<&Miss>) -> IndexHealth {
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
                why: Some(down_why(tender, failed.map(Miss::error).as_ref())),
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
/// once it is older than [`STALE_AFTER`]. A read still out says so, and does
/// not say the socket did not answer, which a slow socket that does answer
/// would make false; one that ended says the socket did not answer, and why.
fn old_words(m: &Miss, age_ms: u64) -> String {
    let seconds = age_ms as f64 / 1000.0;
    if age_ms < STALE_AFTER.as_millis() as u64 {
        return match m {
            Miss::Out(d) => format!(
                "its status read is still out after {} ms: its status as of {seconds:.1} s ago",
                d.as_millis()
            ),
            Miss::Ended(e) => {
                format!("its socket did not answer ({e}): its status as of {seconds:.1} s ago")
            }
        };
    }
    let mins = age_ms / 60_000;
    let age = if mins >= 120 {
        format!("{} h", mins / 60)
    } else {
        format!("{mins} min")
    };
    match m {
        Miss::Out(d) => format!(
            "stale: its status is {age} old, and its latest read is still out after {} ms",
            d.as_millis()
        ),
        Miss::Ended(e) => {
            format!("stale: its status is {age} old, and its socket did not answer ({e})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The age's words: seconds while it is recent, `stale` in minutes past
    /// three, and in hours past two.
    #[test]
    fn an_old_status_says_stale_plainly() {
        let e = Miss::Ended(CallError::NoAnswer("no answer within 100 ms".into()));
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

    /// Two conditions, two sentences (theseus-la5oc, the review's F3): a read
    /// still out past the caller's deadline is not "did not answer", and a
    /// read that ended is not "still out", at every age.
    #[test]
    fn a_read_still_out_and_a_read_that_ended_are_told_apart() {
        let out = Miss::Out(Duration::from_millis(100));
        let ended = Miss::Ended(CallError::NoAnswer("connecting to a socket: gone".into()));
        for age in [2_500, 179_999, 180_000, 5 * 3_600_000 + 60_000] {
            let (o, e) = (old_words(&out, age), old_words(&ended, age));
            assert!(
                o.contains("still out after 100 ms") && !o.contains("did not answer"),
                "{age}: {o}"
            );
            assert!(
                e.contains("did not answer (connecting to a socket: gone)")
                    && !e.contains("still out"),
                "{age}: {e}"
            );
        }
        assert_eq!(
            old_words(&out, 2_500),
            "its status read is still out after 100 ms: its status as of 2.5 s ago"
        );
        assert_eq!(
            old_words(&out, 180_000),
            "stale: its status is 3 min old, and its latest read is still out after 100 ms"
        );
        assert_eq!(
            old_words(&out, 5 * 3_600_000 + 60_000),
            "stale: its status is 5 h old, and its latest read is still out after 100 ms"
        );
        // With no status to show, a read still out is worded as it was.
        assert_eq!(out.error().to_string(), "no answer within 100 ms");
    }
}
