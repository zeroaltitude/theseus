//! A transient failure's retry inside its turn (`[model.retries]`,
//! theseus-7gir.21). A call that fails with a class that passes with time is
//! made again by the loop's next call, after a backoff that doubles each time,
//! at most `transient` times in the turn (2 by default). Past them, or with
//! none (`transient = 0`), the turn fails, and the session's driver retries it
//! later with its own backoff (theseus-ljr). A headless run (`theseus --spawn
//! theseusd ask`) ends with its turn, so it gets no later retry, and the bench
//! profile sets `transient`.
//!
//! A call cut at its whole-call bound (`ProviderError::Timeout { phase: Total
//! }`: a stream that was alive, cut at `Timeouts::whole_call`) is retried at
//! most once a turn, whatever `transient` allows: the same call would most
//! likely run as long again (theseus-5pg8u). Other classes keep their count.
//! Where the catalog names a fallback model, that one retry is made on it
//! instead (`fall_back_on_timeout`), and it spends the turn's whole-call
//! retry: a fallback that is cut too ends the turn.

use super::*;
use crate::provider::TimeoutPhase;

/// A turn's in-turn retries so far.
#[derive(Debug, Default)]
pub(super) struct Retried {
    /// All of them; the backoff's step.
    count: u32,
    /// Of those, the whole-call timeouts.
    whole_call: u32,
}

/// Whether `failure` is a call cut at its whole-call bound.
fn is_whole_call(failure: &Failure) -> bool {
    matches!(
        failure.source.downcast_ref::<ProviderError>(),
        Some(ProviderError::Timeout {
            phase: TimeoutPhase::Total,
            ..
        })
    )
}

impl TurnRunner {
    /// Whether the loop makes `failure`'s call again: a class that passes
    /// with time, while this turn's retries, `retried`, are under
    /// `[model.retries] transient`. It waits out the backoff first, on
    /// tokio's timer; a `/stop` ends the wait at once, and the loop's next
    /// step then sees the stop.
    pub(super) async fn retry_after<'a>(
        &self,
        t: &mut Turn<'a>,
        failure: &Failure,
        retried: &mut Retried,
        spec: &mut RequestSpec,
        slot: &'a OnceLock<Target>,
    ) -> Result<bool> {
        let r = &self.cfg.model.retries;
        let whole = is_whole_call(failure);
        if whole
            && failure.transient
            && retried.whole_call == 0
            && self.fall_back_on_timeout(t, spec, slot)
        {
            retried.whole_call += 1;
            t.record(&fact::turn::LoopCut {
                decision: "timeout_fallback",
            });
            return Ok(true);
        }
        if !failure.transient
            || self.stopping()
            || retried.count >= r.transient
            || (whole && retried.whole_call >= 1)
        {
            return Ok(false);
        }
        let wait = r.wait(retried.count);
        retried.count += 1;
        retried.whole_call += u32::from(whole);
        let (w0, started) = (t.trace.now_us(), Instant::now());
        {
            let armed = self.stops.arm(t.tc.execution_id);
            let woken = armed.notify.notified();
            tokio::pin!(woken);
            woken.as_mut().enable();
            if stopped_by(&t.tc)?.is_none() {
                tokio::select! {
                    () = tokio::time::sleep(wait) => {}
                    () = woken => {}
                }
            }
        }
        t.record(&fact::turn::ModelRetried {
            model: &t.target.model,
            class: &failure.class,
            retry: retried.count,
            of: r.transient,
            w0,
            waited_ms: started.elapsed().as_millis() as u64,
        });
        t.record(&fact::turn::LoopCut {
            decision: "transient_retry",
        });
        Ok(true)
    }
}
