//! The keep-warm read, and what it costs (theseus-ezeg, theseus-7m7w.3). A turn
//! that waits on a job leaves its request's cache entry to lapse; while the
//! wait runs, it reads the turn's own last request again at the smallest
//! output (`keep_warm_wait`), and the next call reads the entry. That is the
//! only keep-warm read left: the reads BETWEEN turns went with the transcript
//! (issue 44), since a view turn builds its request afresh and has no
//! transcript prefix to keep.
//!
//! **Money**: each read is a model call like any other. Its worst case is
//! reserved against the session's spend limit (refused past it unless the
//! limit notifies, `Kernel::overdraws`, and then only up to its hard multiple,
//! `Kernel::hard_stop`) and held on the day ceiling
//! (`TurnRunner::day_hold`, which says the day's refusal once); its cost is
//! booked to the session in one frame with its `keep_warm` row (provider,
//! model, usage, cost: `Kernel::book_spend`), which the day ceiling reads back
//! at a start (`day_ceiling::SPEND_KINDS`) and the money river shows. A
//! refused or failed read logs once and is not retried by the wait. A failed
//! call books nothing: a read the provider may have billed is at most its
//! prefix at the read price.
//!
//! **In memory, never stored**: the request a wait reads, the room a task
//! wait records, and the model and effort each session's last request
//! carried (the sticky effort rule's, theseus-rwpbg.4). A turn's end drops
//! the first two.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_protocol::LedgerKind;

use crate::turn::TurnRunner;

/// The read's output: the smallest the provider takes.
pub const MAX_TOKENS: u32 = 1;

/// A wait's kept request, the room a task wait records, the effort each
/// session's last request carried, and the day's reads, in memory.
#[derive(Default)]
pub struct KeepWarm {
    today: Mutex<Today>,
    /// A turn's last request while its response's `job_wait` runs
    /// (`keep_warm_wait`), by session.
    pub(crate) waits: Mutex<HashMap<String, Arc<crate::keep_warm_wait::Stash>>>,
    /// The tokens and the window of each session's last request that called a
    /// task wait: the room a fan-in has before the clearing line
    /// (`keep_warm_wait::stash_for_wait`, `task_wait`).
    rooms: Mutex<HashMap<String, (u64, u64)>>,
    /// The model and effort each session's last request carried (`sent`).
    sent: Mutex<HashMap<String, Sent>>,
    /// Where `theseus.keep_warm.reads` counts, once telemetry exports.
    pub telemetry: std::sync::OnceLock<crate::telemetry::Telemetry>,
}

/// A session's last request: its model, its effort, and whether the sticky
/// rule held the effort (theseus-u5a37).
type Sent = (String, Option<crate::config::Effort>, bool);

#[derive(Debug, Default)]
struct Today {
    day: String,
    reads: u64,
    micros: u64,
}

/// What one read came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Sent and booked: its cost, and what it read and wrote.
    Read {
        cost_micros: u64,
        cache_read: u64,
        cache_written: u64,
    },
    /// Refused or failed (why).
    Stopped(String),
    /// The daemon's stop has begun.
    Stopping,
}

impl KeepWarm {
    /// Record the size of `session`'s request, and the window it fits.
    pub(crate) fn set_room(&self, session: &str, tokens: u64, limit: u64) {
        self.rooms
            .lock()
            .unwrap()
            .insert(session.to_string(), (tokens, limit));
    }

    /// The tokens and the window of `session`'s last request that called a
    /// task wait, when it did.
    pub(crate) fn room(&self, session: &str) -> Option<(u64, u64)> {
        self.rooms.lock().unwrap().get(session).copied()
    }

    /// A turn of `session_id` ended: its wait's request and its room go.
    pub fn turn_ended(&self, session_id: &str) {
        self.waits.lock().unwrap().remove(session_id);
        self.rooms.lock().unwrap().remove(session_id);
    }

    /// A real request of `session_id` went out on `model` at `effort`: the
    /// sticky rule holds to it (theseus-rwpbg.4). In memory: a restart
    /// forgets it, and the first request after applies Jev's level as asked.
    pub fn sent(
        &self,
        session_id: &str,
        model: &str,
        effort: Option<crate::config::Effort>,
        held: bool,
    ) {
        self.sent
            .lock()
            .unwrap()
            .insert(session_id.to_string(), (model.to_string(), effort, held));
    }

    /// The effort `session_id`'s last request carried, if it went to `model`.
    pub fn last_effort(&self, session_id: &str, model: &str) -> Option<crate::config::Effort> {
        let sent = self.sent.lock().unwrap();
        sent.get(session_id)
            .filter(|(m, ..)| m == model)
            .and_then(|(_, e, _)| *e)
    }

    /// Whether the sticky rule held the effort `session_id`'s last request
    /// carried, if it went to `model` (theseus-u5a37).
    pub fn last_hold(&self, session_id: &str, model: &str) -> bool {
        let sent = self.sent.lock().unwrap();
        sent.get(session_id)
            .is_some_and(|(m, _, held)| m == model && *held)
    }

    /// A read, and what it cost, on `day`.
    fn count(&self, day: &str, micros: u64) {
        let mut t = self.today.lock().unwrap();
        if t.day != day {
            *t = Today {
                day: day.to_string(),
                ..Today::default()
            };
        }
        t.reads += 1;
        t.micros += micros;
    }

    /// Today's reads and what they cost, in dollars.
    pub fn counts(&self, day: &str) -> (u64, f64) {
        let t = self.today.lock().unwrap();
        match t.day == day {
            true => (t.reads, t.micros as f64 / 1_000_000.0),
            false => (0, 0.0),
        }
    }
}

pub fn today(kernel: &theseus_kernel::Kernel) -> String {
    let c = kernel.day_ceiling();
    c.day_of(c.now()).0
}

/// The request built, and what it needs to go.
pub struct Built {
    pub request: crate::provider::ProviderRequest,
    pub target: crate::turn::Target,
    pub execution_id: String,
    /// The prefix's estimate, in tokens.
    pub tokens: u64,
    /// Sent by a `job_wait` to keep its turn's cache (`keep_warm_wait`).
    pub during_wait: bool,
}

/// A keep-warm read's call, under its session, so its `request.redacted` row
/// names it: a read resends the whole transcript every few minutes.
fn read_call<'a>(
    sid: &str,
    provider: &'a dyn crate::provider::Provider,
    request: &'a crate::provider::ProviderRequest,
    quiet: crate::provider::DeltaSink<'a>,
) -> crate::provider::ProviderFuture<'a> {
    let call = crate::exits::Call::of_session("keep_warm", sid);
    crate::exits::for_call(call, || provider.stream_message(request, quiet))
}

impl TurnRunner {
    /// Reserve, send and book a built read.
    pub(crate) async fn keep_warm_send(
        &self,
        sid: &str,
        b: Built,
        execution: Option<theseus_kernel::Execution>,
    ) -> Outcome {
        let kw = &self.keep_warm;
        let stop = |why: String| {
            tracing::warn!(session = sid, why = %why, "keep-warm: a read was not made");
            self.tools_count_keep_warm(if why.contains("ceiling") || why.contains("limit") {
                "refused"
            } else {
                "failed"
            });
            Outcome::Stopped(why)
        };
        let Some(provider) = self.providers.get(&b.target.provider).cloned() else {
            return stop(format!(
                "its provider {:?} is not configured",
                b.target.provider
            ));
        };
        let est = crate::compiler::estimate(
            &b.request,
            crate::catalog::TokenRates::of(&b.target.model),
            None,
        );
        let need = self
            .catalog
            .get(&b.target.model)
            .map_or(0, |p| p.reserve_micros(MAX_TOKENS, est.upper));
        // The session's spend limit: refused past it, unless it notifies.
        if let Some(e) = &execution {
            if need > e.budget.available() && !self.kernel.overdraws(e) {
                return stop(format!(
                    "its read would need {}, past what the session's spend limit leaves",
                    crate::narrative::dollars(need)
                ));
            }
            // A limit that notifies still stops at its hard multiple, for a
            // read as for a turn's call (theseus-jse4).
            if self
                .kernel
                .hard_stop(e, theseus_protocol::PROVIDER_TOOL, need)
                .is_err()
            {
                return stop(format!(
                    "its read would take the session past its hard stop, {} times its spend limit \
                     (`[kernel] spend_limit_hard_multiple`)",
                    self.kernel.config().spend_hard_multiple
                ));
            }
        }
        // The day ceiling: refused and said once a day (`day_refused`).
        let hold = match self.day_hold(need, "keep-warm read") {
            Ok(h) => h,
            Err(why) => return stop(why),
        };
        if self.outbox.stopping() {
            return Outcome::Stopping;
        }
        let started = std::time::Instant::now();
        let started_ms = theseus_protocol::now_unix_ms();
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let resp = match read_call(sid, provider.as_ref(), &b.request, &mut quiet).await {
            Ok(r) => r,
            Err(e) => {
                drop(hold);
                return stop(format!(
                    "its call failed after {} ms: {e:#}",
                    started.elapsed().as_millis()
                ));
            }
        };
        let cost = self.priced(&resp, &b.target.model, need, started_ms);
        let u = &resp.usage;
        let mut data = json!({
            "provider": b.target.provider,
            "model": resp.model,
            "profile": b.target.profile,
            "usage": u,
            "prefix_tokens": b.tokens,
            "request_id": resp.request_id,
            "timing": {"total_ms": started.elapsed().as_millis() as u64},
        });
        if b.during_wait {
            data["during_wait"] = json!(true);
        }
        let booked = self
            .kernel
            .book_spend(&b.execution_id, cost, LedgerKind::KeepWarm, data);
        drop(hold);
        if let Err(e) = booked {
            // Spent all the same: the day counts it even unbooked.
            let c = self.kernel.day_ceiling();
            c.book(c.now(), cost);
            return stop(format!("its row was not written: {e:#}"));
        }
        kw.count(&today(&self.kernel), cost);
        self.tools_count_keep_warm("read");
        Outcome::Read {
            cost_micros: cost,
            cache_read: u.cache_read_input_tokens,
            cache_written: u.cache_creation_input_tokens,
        }
    }

    /// `theseus.keep_warm.reads`, by outcome.
    fn tools_count_keep_warm(&self, outcome: &str) {
        if let Some(t) = self.keep_warm.telemetry.get() {
            t.record_keep_warm(outcome);
        }
    }
}

impl crate::rpc::Core {
    /// Health's `cache` block: each profile's TTL and where it comes from,
    /// and today's reads. The fields of the reads between turns (`kept`,
    /// `stopped`, a profile's `keep_warm_hours`, `keep_warm_minutes`) are
    /// still sent, as zero, for a cockpit page left open since the upgrade.
    pub fn cache_health(&self) -> theseus_protocol::PromptCacheHealth {
        let profiles = self
            .cfg
            .all_profiles()
            .iter()
            .filter_map(|(name, p)| {
                let (ttl, from) = self.cfg.ttl_of(name)?;
                Some(theseus_protocol::ProfileTtl {
                    profile: name.clone(),
                    model: p.model.clone(),
                    ttl: ttl.as_str().into(),
                    inherited: from == "cache" && name != "default",
                    keep_warm_hours: 0.0,
                })
            })
            .collect();
        let (reads_today, usd_today) = self.runner.keep_warm.counts(&today(&self.kernel));
        theseus_protocol::PromptCacheHealth {
            profiles,
            keep_warm_minutes: 0.0,
            kept: 0,
            stopped: 0,
            reads_today,
            usd_today,
        }
    }
}
