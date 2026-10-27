//! The keep-warm read across a long `job_wait` (theseus-7m7w.3). A model that
//! waits ten minutes on a job leaves its request's cache entry to lapse (the
//! provider keeps it five minutes, or an hour where the profile says so), and
//! its next call then writes the whole prefix again. While the wait runs, the
//! wait itself sends the turn's own last request again at the smallest output
//! (`keep_warm::MAX_TOKENS`), shortly before the entry lapses: that read
//! restarts the entry, and the next call reads it.
//!
//! **What is kept**: when a response asks for a `job_wait` (`stash_for_wait`),
//! the request just sent is kept in memory for its session, once per such
//! response (the one place a turn copies a request, and only when a wait is
//! asked for), with when it was sent and how often it needs reading. It is
//! dropped when the turn ends (`KeepWarm::turn_ended`) or the next response
//! with a wait replaces it. Nothing is stored.
//!
//! **The read** is `TurnRunner::keep_warm_send`, as the between-turns read is:
//! reserved against the session's spend limit and the day ceiling, booked in
//! one frame as a `keep_warm` row (its data says `during_wait`), counted in
//! health and `theseus.keep_warm.reads`. The wait runs it beside its other
//! events (`Warmth::read`), so a settled job, a person's message or a stop
//! ends the wait at once, and a read the wait leaves unfinished is dropped
//! unbooked, as a failed one is.
//!
//! **No new work per call**: a response with no `job_wait` pays one scan of its
//! calls' names; a wait shorter than the first read is due pays one lock; a
//! longer one pays one timer, and one read per cache lifetime.

use std::sync::{Arc, Mutex};

use tokio::time::{Duration, Instant};

use crate::compiler::RequestSpec;
use crate::config::CacheTtl;
use crate::keep_warm::Built;
use crate::provider::ToolUse;
use crate::toolrun::TurnCtx;
use crate::turn::{Target, TurnRunner};

/// How long before a five-minute entry lapses its read is due: the provider's
/// clock starts at its own receipt of the last call, and a read that lands
/// after the entry expired writes it whole again.
pub const LEAD_5M: Duration = Duration::from_secs(60);

/// A turn's last request, kept for the reads of a wait.
pub struct Stash {
    built: Built,
    /// How often a read is due: the entry's lifetime less its lead.
    period: Duration,
    /// When the next read is due.
    due: Mutex<Instant>,
}

/// What a wait holds of its session's stash.
pub struct Warmth {
    stash: Arc<Stash>,
}

impl Warmth {
    /// When the next read is due, if it falls before `until`: a wait that ends
    /// first reads nothing.
    pub fn due_before(&self, until: Instant) -> Option<Instant> {
        let due = *self.stash.due.lock().unwrap();
        (due < until).then_some(due)
    }

    /// Take the read that is due at `now`: false when another wait of the same
    /// response took it, or it is not due yet.
    pub fn claim(&self, now: Instant) -> bool {
        let mut due = self.stash.due.lock().unwrap();
        if *due > now {
            return false;
        }
        *due = now + self.stash.period;
        true
    }

    /// The read: the kept request again at the smallest output, through the
    /// one send the between-turns reads use.
    pub async fn read(&self, runner: &TurnRunner, session_id: &str) -> crate::keep_warm::Outcome {
        let b = &self.stash.built;
        let built = Built {
            request: {
                let mut r = b.request.clone();
                r.max_tokens = crate::keep_warm::MAX_TOKENS;
                r
            },
            target: b.target.clone(),
            execution_id: b.execution_id.clone(),
            tokens: b.tokens,
            during_wait: true,
        };
        let execution = runner.kernel.execution(&built.execution_id).ok().flatten();
        runner.keep_warm_send(session_id, built, execution).await
    }
}

impl crate::keep_warm::KeepWarm {
    /// The stash of `session_id`'s turn, if its last response asked for a wait
    /// that outlasts the first read.
    pub fn warmth(&self, session_id: &str) -> Option<Warmth> {
        let stash = self.waits.lock().unwrap().get(session_id)?.clone();
        Some(Warmth { stash })
    }
}

/// Whether the request asks the provider to cache: the top-level breakpoint, or
/// the one the task view moved into the last block of the last message
/// (`task_graph::view::attach_text`), which every conversation with a task has.
fn caches(request: &crate::provider::ProviderRequest) -> bool {
    request.cache_control.is_some()
        || request
            .messages
            .last()
            .and_then(|m| m["content"].as_array())
            .is_some_and(|c| c.iter().any(|b| b.get("cache_control").is_some()))
}

impl TurnRunner {
    /// A response is about to run its calls: keep the request it answered when
    /// a call is a `job_wait` and a read could be due before the wait ends. The
    /// kept request is the one sent, so its read is the cache the next call
    /// reads. Only a request that caches, and a prefix worth keeping
    /// (`[cache] keep_warm_min_tokens`: a small prompt's rewrite is cents).
    ///
    /// `going_back` is a fallback's answer whose turn returns to the routed
    /// model (theseus-wf482). A provider's cache is its model's, so keeping
    /// this request would warm the fallback's prefix while the next call
    /// reads the routed one, and the routed prefix would expire under the
    /// wait: nothing is kept. **The task wait's room is still recorded**, as
    /// it bounds the fan-in of the wait this very response opens, whichever
    /// model answered it.
    pub fn stash_for_wait(
        &self,
        tc: &TurnCtx<'_>,
        uses: &[ToolUse],
        compiled: &crate::compiler::Compiled,
        spec: &RequestSpec,
        target: &Target,
        going_back: bool,
    ) {
        let tokens = compiled.estimate.tokens;
        // A task wait's fan-in is bounded by the room this request leaves
        // before the clearing line: recorded before the early returns below,
        // since a small or uncached conversation has a room too.
        let task_wait = uses
            .iter()
            .any(|u| crate::task_wait::is_task_wait_wire(&u.name));
        if task_wait {
            self.keep_warm
                .set_room(tc.session_id, tokens, compiled.budget.limit_tokens);
        }
        if going_back {
            return;
        }
        let waits = task_wait
            || uses
                .iter()
                .any(|u| u.name == theseus_tools::wire_name(theseus_tools::jobs::WAIT));
        if !waits || !caches(&compiled.request) || tokens < self.cfg.cache.keep_warm_min_tokens {
            return;
        }
        let ttl = spec.conversation_ttl.min(spec.cache_ttl);
        let period = match ttl {
            CacheTtl::FiveMinutes => Duration::from_millis(ttl.ms()) - LEAD_5M,
            CacheTtl::OneHour => Duration::from_millis(self.cfg.cache.interval_ms()),
        };
        let stash = Stash {
            built: Built {
                request: compiled.request.clone(),
                target: target.clone(),
                execution_id: tc.execution_id.to_string(),
                tokens,
                during_wait: true,
            },
            period,
            due: Mutex::new(Instant::now() + period),
        };
        self.keep_warm
            .waits
            .lock()
            .unwrap()
            .insert(tc.session_id.to_string(), Arc::new(stash));
    }
}
