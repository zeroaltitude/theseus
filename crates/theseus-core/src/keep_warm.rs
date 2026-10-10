//! The keep-warm read (theseus-ezeg; the owner's design of 2026-10-08). A
//! long conversation's prompt is mostly its cached prefix, and a cache entry
//! lives an hour at most after its last read. After the last turn of a
//! conversation a person talks in ends, the daemon sends one request every
//! `[cache] keep_warm_minutes` that reads the prefix and so restarts its
//! entry's hour, for up to the profile's `keep_warm_hours` after the
//! session's last message. The next message then reads the prefix (0.025x of
//! input on Fable 5.1) instead of writing it whole again (2x).
//!
//! **The request** is the session's next compile, made as its next turn
//! makes it (`TurnRunner::keep_warm_request`: the session's last target, its
//! place's view and spec, its walk, the transcript and the current
//! compilation), up to its last message, the model's answer; then a short
//! user line, since a request may not end on the model's turn, and the
//! smallest output (`MAX_TOKENS`). Its bytes up to the line are the next
//! turn's request's first bytes (`tests_keep_warm`), so its read is the
//! entry the next turn reads. No tool runs, and nothing is stored but its
//! row: no node, no compilation, no turn, no message in any surface.
//!
//! **Where it pays, and where it runs**:
//! - only a conversation (never a task), in a private place (the CLI, the TUI,
//!   the web UI, the owner's DM), whose last message a person sent
//!   (`KeepWarm::message`, from `turn.submit`: never a job's, an MCP client's
//!   or an unnamed surface's); the judge and Jev make no turn, so none;
//! - whose profile caches for an hour and keeps warm (`keep_warm_hours` > 0),
//!   and whose prefix is estimated at `[cache] keep_warm_min_tokens` or more;
//! - never while a turn of the session runs or waits to run (its execution
//!   `running` or `queued`), and never from a stopping daemon
//!   (`Outbox::stopping`, the stopping class of `turn/stopping_step.rs`): its
//!   tender ends there;
//! - a new message starts its window again; a retired session, an erased or
//!   imported one, or one whose window closed is forgotten.
//!
//! **Money**: each read is a model call like any other. Its worst case is
//! reserved against the session's spend limit (refused past it unless the
//! limit notifies, `Kernel::overdraws`) and held on the day ceiling
//! (`TurnRunner::day_hold`, which says the day's refusal once); its cost is
//! booked to the session in one frame with its `keep_warm` row (provider,
//! model, usage, cost: `Kernel::book_spend`), which the day ceiling reads back
//! at a start (`day_ceiling::SPEND_KINDS`) and the money river shows. A
//! refused or failed read logs once and stops that session's reads until its
//! next message. A failed call books nothing: a read the provider may have
//! billed is at most its prefix at the read price.
//!
//! **Off every turn's path** (FAST): a turn's end only notes the time in
//! memory (`turn_ended`); the tender (`Core::tend_keep_warm_after_serving`)
//! waits on tokio's timer and a `Notify`, holds the core by `Weak`, and
//! compiles and calls in its own task. **After a restart** the windows are
//! rebuilt from the store (`TurnRunner::keep_warm_restore`): each live private
//! conversation's newest person's message (its newest user node; the
//! session's last activity when the index's shape is not built), and its
//! last activity as its last call. No stored field: nothing changes the
//! store's format. What a restart cannot tell is whether that message was a
//! job's, so a job's message into an existing private conversation keeps it
//! warm after a restart, where it would not have before it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_protocol::{LedgerKind, PlaceClass};
use tokio::time::{Duration, Instant};

use crate::config::CacheTtl;
use crate::session::SessionRecord;
use crate::turn::TurnRunner;
use theseus_protocol::SessionKind;

/// The read's output: the smallest the provider takes.
pub const MAX_TOKENS: u32 = 1;

/// The user line after the model's last answer: a request may not end on
/// the model's turn. Never stored and never shown.
pub const LINE: &str = "(cache keep-warm: no answer is needed)";

/// The sessions kept warm, in memory: when each one's person last wrote,
/// when it last called, and why its reads stopped.
#[derive(Default)]
pub struct KeepWarm {
    sessions: Mutex<HashMap<String, Kept>>,
    /// Wakes the tender: a turn ended, so the soonest due moved.
    pub wake: Arc<tokio::sync::Notify>,
    today: Mutex<Today>,
    /// Where `theseus.keep_warm.reads` counts, once telemetry exports.
    pub telemetry: std::sync::OnceLock<crate::telemetry::Telemetry>,
}

#[derive(Debug, Clone)]
struct Kept {
    /// A person's last message: the window counts from it.
    message_at: Instant,
    /// The session's last call: a turn's end, or a read.
    call_at: Instant,
    /// Why its reads stopped, until its next message.
    stopped: Option<String>,
    /// Its last read, in Unix ms: a call the cold-rewrite rule counts.
    read_ms: u64,
}

#[derive(Debug, Default)]
struct Today {
    day: String,
    reads: u64,
    micros: u64,
}

/// What one due read came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Sent and booked: its cost, and what it read and wrote.
    Read {
        cost_micros: u64,
        cache_read: u64,
        cache_written: u64,
    },
    /// A turn runs or waits to run: the turn's own end notes it again.
    Busy,
    /// Not kept warm, and forgotten (why).
    Forgotten(String),
    /// Refused or failed: no more reads until its next message (why).
    Stopped(String),
    /// The daemon's stop has begun.
    Stopping,
}

impl KeepWarm {
    /// A message arrived in `session_id`: a person's starts its window again
    /// and lifts a stop; any other sender's changes nothing.
    pub fn message(&self, session_id: &str, person: bool) {
        if !person {
            return;
        }
        let now = Instant::now();
        self.sessions.lock().unwrap().insert(
            session_id.to_string(),
            Kept {
                message_at: now,
                call_at: now,
                stopped: None,
                read_ms: 0,
            },
        );
    }

    /// A turn of `session_id` ended: its last call was now. Only a session a
    /// person wrote in is held; the tender is woken for the next due read.
    pub fn turn_ended(&self, session_id: &str) {
        if let Some(k) = self.sessions.lock().unwrap().get_mut(session_id) {
            k.call_at = Instant::now();
            self.wake.notify_one();
        }
    }

    /// A session as a restart finds it: its person's message and its last
    /// call, each this long ago.
    pub fn restore(&self, session_id: &str, message_ago: Duration, call_ago: Duration) {
        let now = Instant::now();
        let at = |ago: Duration| now.checked_sub(ago).unwrap_or(now);
        self.sessions
            .lock()
            .unwrap()
            .entry(session_id.to_string())
            .or_insert(Kept {
                message_at: at(message_ago),
                call_at: at(call_ago.min(message_ago)),
                stopped: None,
                read_ms: 0,
            });
        self.wake.notify_one();
    }

    /// When the soonest read is due, if any is.
    pub fn next_due(&self, interval: Duration) -> Option<Instant> {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .filter(|k| k.stopped.is_none())
            .map(|k| k.call_at + interval)
            .min()
    }

    /// The sessions due now, the longest overdue first.
    pub fn due(&self, interval: Duration) -> Vec<String> {
        let now = Instant::now();
        let mut due: Vec<(Instant, String)> = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, k)| k.stopped.is_none() && k.call_at + interval <= now)
            .map(|(s, k)| (k.call_at, s.clone()))
            .collect();
        due.sort();
        due.into_iter().map(|(_, s)| s).collect()
    }

    /// How long ago `session_id`'s person last wrote.
    fn since_message(&self, session_id: &str) -> Option<Duration> {
        let s = self.sessions.lock().unwrap();
        s.get(session_id).map(|k| k.message_at.elapsed())
    }

    /// A read was made now.
    fn called(&self, session_id: &str) {
        if let Some(k) = self.sessions.lock().unwrap().get_mut(session_id) {
            k.call_at = Instant::now();
        }
    }

    /// `session_id`'s last read, in Unix ms (0: none in this run).
    pub fn read_ms(&self, session_id: &str) -> u64 {
        let s = self.sessions.lock().unwrap();
        s.get(session_id).map_or(0, |k| k.read_ms)
    }

    fn read_at(&self, session_id: &str, ms: u64) {
        if let Some(k) = self.sessions.lock().unwrap().get_mut(session_id) {
            k.read_ms = ms;
        }
    }

    /// Not a session to keep warm (any more).
    pub fn forget(&self, session_id: &str) {
        self.sessions.lock().unwrap().remove(session_id);
    }

    /// Its reads stop until its next message, said once in the log.
    fn stop(&self, session_id: &str, why: &str) {
        if let Some(k) = self.sessions.lock().unwrap().get_mut(session_id) {
            if k.stopped.is_none() {
                tracing::warn!(
                    session_id,
                    why,
                    "keep-warm: no more reads for this session until its next message"
                );
            }
            k.stopped = Some(why.to_string());
        }
    }

    /// A busy session's read waits an interval more: its turn's end notes
    /// it again sooner.
    fn postpone(&self, session_id: &str) {
        self.called(session_id);
    }

    /// Count a read made today (`day` is the local date).
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

    /// Health's part: sessions kept warm now, those whose reads stopped,
    /// and today's reads and dollars (`day` is today's local date).
    pub fn counts(&self, day: &str) -> (u64, u64, u64, f64) {
        let s = self.sessions.lock().unwrap();
        let stopped = s.values().filter(|k| k.stopped.is_some()).count() as u64;
        let kept = s.len() as u64 - stopped;
        let t = self.today.lock().unwrap();
        match t.day == day {
            true => (
                kept,
                stopped,
                t.reads,
                theseus_kernel::micros_to_usd(t.micros),
            ),
            false => (kept, stopped, 0, 0.0),
        }
    }
}

/// Today's local date, as the day ceiling names it.
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
}

impl TurnRunner {
    /// The request a keep-warm read of `session_id` sends: the session's
    /// next compile up to its last message, then `LINE`, at `MAX_TOKENS`.
    /// `Err` says why there is none, and whether to forget the session
    /// (`true`) or stop its reads (`false`).
    pub fn keep_warm_request(
        &self,
        rec: &SessionRecord,
        live: &str,
    ) -> Result<Built, (bool, String)> {
        let sid = rec.session_id.as_str();
        let forget = |why: &str| Err((true, why.to_string()));
        if rec.kind == SessionKind::Task || rec.imported.is_some() || rec.retired.is_some() {
            return forget("a task, an imported or a retired session");
        }
        let place = self.view_of(sid);
        if place.class != PlaceClass::Private {
            return forget("a shared place");
        }
        let Some(execution_id) = rec.execution_id.clone() else {
            return forget("no execution");
        };
        let target = self
            .target_for_session(rec, live)
            .map_err(|e| (true, format!("{e:#}")))?;
        let prof = self.cfg.all_profiles().get(&target.profile);
        if target.cache_ttl != CacheTtl::OneHour || prof.is_none_or(|p| p.keep_warm_hours() <= 0.0)
        {
            return forget("its profile does not cache for an hour, or keeps nothing warm");
        }
        let paths = self.cfg.context_paths(target.persona.as_deref());
        let mut spec = self.spec_of(&target, rec.kind, place, self.context_files.peek(&paths));
        spec.walk = self.walk(sid, place.class);
        let built = (|| -> anyhow::Result<_> {
            let nodes = self.store.transcript(sid)?;
            let current = match rec.compilation_id.as_deref() {
                Some(id) => self.store.get_compilation(id)?,
                None => None,
            };
            let recalls = nodes
                .iter()
                .filter(|(_, n)| n.kind == crate::stub::Kind::Recall);
            let sources = self
                .memory
                .read_sources(&self.store, recalls.map(|(_, n)| &**n));
            let situation = crate::compiler::situation::given(false, false, false);
            let input = crate::compiler::CompileInput {
                session_id: sid,
                current: current.as_ref(),
                nodes: &nodes,
                last_position: self.store.last_position(),
                spec: &spec,
                catalog: &self.catalog,
                force: None,
                window_override: None,
                blobs: Some(self.store.blobs()),
                hidden: &rec.not_shown,
                strip: None,
                overflowed: None,
                sources: &sources,
                signals: None,
                assembled: None,
                situation: &situation,
            };
            Ok((current.is_some(), crate::compiler::compile(input)))
        })()
        .map_err(|e| (false, format!("its compile failed: {e:#}")))?;
        let (had, compiled) = built;
        // A recompile writes the prefix anew whatever is read now.
        if !had || compiled.new_compilation {
            return Err((
                false,
                "its next turn recompiles, so its cache is not read".into(),
            ));
        }
        let tokens = compiled.estimate.tokens;
        if tokens < self.cfg.cache.keep_warm_min_tokens {
            return forget("its prefix is under [cache] keep_warm_min_tokens");
        }
        let mut request = compiled.request;
        request.max_tokens = MAX_TOKENS;
        let last_is_model = request
            .messages
            .last()
            .is_some_and(|m| m["role"] == "assistant");
        if last_is_model {
            request
                .messages
                .push(json!({"role": "user", "content": [{"type": "text", "text": LINE}]}));
        }
        Ok(Built {
            request,
            target,
            execution_id,
            tokens,
        })
    }

    /// One due read of `session_id`: checked, built, reserved, sent and
    /// booked, or why not. The build reads the transcript and compiles it,
    /// so it runs where a wait for the disk may (`theseus_store::blocking`).
    pub async fn keep_warm_once(&self, session_id: &str, live: &str) -> Outcome {
        if self.outbox.stopping() {
            return Outcome::Stopping;
        }
        match theseus_store::blocking(|| self.keep_warm_prepare(session_id, live)) {
            Ok((b, execution)) => self.keep_warm_send(session_id, b, execution).await,
            Err(o) => o,
        }
    }

    /// The checks before a read, and its request: or what it came to.
    fn keep_warm_prepare(
        &self,
        session_id: &str,
        live: &str,
    ) -> Result<(Built, Option<theseus_kernel::Execution>), Outcome> {
        let kw = &self.keep_warm;
        let rec = match self.store.get_session::<SessionRecord>(session_id) {
            Ok(Some(r)) => r,
            Ok(None) => {
                kw.forget(session_id);
                return Err(Outcome::Forgotten("no such session".into()));
            }
            Err(e) => {
                let why = format!("its record did not read: {e:#}");
                kw.stop(session_id, &why);
                return Err(Outcome::Stopped(why));
            }
        };
        let execution = rec
            .execution_id
            .as_deref()
            .and_then(|id| self.kernel.execution(id).ok().flatten());
        if execution.as_ref().is_some_and(|e| {
            matches!(
                e.state,
                theseus_kernel::ExecState::Running | theseus_kernel::ExecState::Queued
            )
        }) {
            kw.postpone(session_id);
            return Err(Outcome::Busy);
        }
        let b = match self.keep_warm_request(&rec, live) {
            Ok(b) => b,
            Err((true, why)) => {
                kw.forget(session_id);
                return Err(Outcome::Forgotten(why));
            }
            Err((false, why)) => {
                kw.stop(session_id, &why);
                return Err(Outcome::Stopped(why));
            }
        };
        let hours = self
            .cfg
            .all_profiles()
            .get(&b.target.profile)
            .map_or(0.0, |p| p.keep_warm_hours());
        let window = Duration::from_secs_f64(hours * 3600.0);
        if kw.since_message(session_id).is_none_or(|ago| ago >= window) {
            kw.forget(session_id);
            return Err(Outcome::Forgotten("its window closed".into()));
        }
        Ok((b, execution))
    }

    /// Reserve, send and book a built read.
    async fn keep_warm_send(
        &self,
        sid: &str,
        b: Built,
        execution: Option<theseus_kernel::Execution>,
    ) -> Outcome {
        let kw = &self.keep_warm;
        let stop = |why: String| {
            kw.stop(sid, &why);
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
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let resp = match provider.stream_message(&b.request, &mut quiet).await {
            Ok(r) => r,
            Err(e) => {
                drop(hold);
                return stop(format!(
                    "its call failed after {} ms: {e:#}",
                    started.elapsed().as_millis()
                ));
            }
        };
        let cost = self.priced(&resp, &b.target.model, need);
        let u = &resp.usage;
        let data = json!({
            "provider": b.target.provider,
            "model": resp.model,
            "profile": b.target.profile,
            "usage": u,
            "prefix_tokens": b.tokens,
            "request_id": resp.request_id,
            "timing": {"total_ms": started.elapsed().as_millis() as u64},
        });
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
        kw.called(sid);
        kw.read_at(sid, theseus_protocol::now_unix_ms());
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

    /// After a restart: each live private conversation within its window,
    /// from its newest person's message and its last activity.
    pub fn keep_warm_restore(&self, live: &str) -> anyhow::Result<u64> {
        let now = theseus_protocol::now_unix_ms();
        let longest = self
            .cfg
            .all_profiles()
            .values()
            .map(|p| p.keep_warm_hours())
            .fold(0.0, f64::max);
        let horizon = (longest * 3_600_000.0) as u64;
        let mut n = 0;
        for rec in self.store.live_sessions::<SessionRecord>()? {
            let sid = rec.session_id.as_str();
            if rec.turns == 0 || now.saturating_sub(rec.last_active_ms) >= horizon {
                continue;
            }
            if rec.kind == SessionKind::Task || rec.retired.is_some() {
                continue;
            }
            if self.view_of(sid).class != PlaceClass::Private {
                continue;
            }
            let Ok(target) = self.target_for_session(&rec, live) else {
                continue;
            };
            if target.cache_ttl != CacheTtl::OneHour {
                continue;
            }
            let message_ms = self.newest_message_ms(sid).unwrap_or(rec.last_active_ms);
            let ago = |ms: u64| Duration::from_millis(now.saturating_sub(ms));
            self.keep_warm
                .restore(sid, ago(message_ms), ago(rec.last_active_ms));
            n += 1;
        }
        Ok(n)
    }

    /// The newest user node's time, through the index (none while its
    /// shape is built).
    fn newest_message_ms(&self, sid: &str) -> Option<u64> {
        use theseus_store::{kinds, Page, Store as _};
        let page = Page {
            kind: kinds::NODE,
            tags: vec![theseus_store::pages::ledger_kind_session(
                "user_message",
                sid,
            )],
            limit: 1,
            ..Page::default()
        };
        let got = self.store.inner().page(&page).ok()??;
        let r = got.records.first()?;
        let n: crate::node::Node = r.decode().ok()?;
        Some(n.created_at_ms)
    }
}

/// How long after serving the windows a restart left are rebuilt: once the
/// start's own work is done.
const RESTORE_AFTER: Duration = Duration::from_secs(10);

impl crate::rpc::Core {
    /// The keep-warm tender (theseus-ezeg), after serving, never on the
    /// start path: it rebuilds the windows a restart left, then waits on
    /// tokio's timer for the soonest due read, or on a turn's end, and makes
    /// the reads due. It holds the core by `Weak` between reads, and ends
    /// once the daemon's stop has begun.
    pub fn tend_keep_warm_after_serving(self: &Arc<Self>) {
        let core = Arc::downgrade(self);
        let wake = self.runner.keep_warm.wake.clone();
        let outbox = self.outbox.clone();
        tokio::spawn(async move {
            tokio::time::sleep(RESTORE_AFTER).await;
            {
                let Some(c) = core.upgrade() else { return };
                let live = c.live_profile().0;
                let c2 = c.clone();
                match tokio::task::spawn_blocking(move || c2.runner.keep_warm_restore(&live)).await
                {
                    Ok(Ok(n)) if n > 0 => {
                        tracing::info!(sessions = n, "keep-warm: windows rebuilt after the start")
                    }
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        tracing::warn!(error = %format!("{e:#}"), "keep-warm: the windows were not rebuilt; they start again at each session's next message")
                    }
                    Err(_) => return,
                }
            }
            loop {
                let Some(c) = core.upgrade() else { return };
                if c.outbox.stopping() {
                    return;
                }
                let interval = Duration::from_millis(c.cfg.cache.interval_ms());
                let next = c.runner.keep_warm.next_due(interval);
                drop(c);
                // Ended with the stop, with no polling (`Outbox::stopped`).
                let until = async {
                    match next {
                        None => std::future::pending().await,
                        Some(at) => tokio::time::sleep_until(at).await,
                    }
                };
                tokio::select! {
                    () = until => {}
                    () = wake.notified() => {}
                    () = outbox.stopped() => return,
                }
                let Some(c) = core.upgrade() else { return };
                for sid in c.runner.keep_warm.due(interval) {
                    let live = c.live_profile().0;
                    if c.runner.keep_warm_once(&sid, &live).await == Outcome::Stopping {
                        return;
                    }
                }
            }
        });
    }

    /// Health's `cache` block: each profile's TTL and where it comes from,
    /// and the keep-warm's state.
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
                    inherited: from == "model" && name != "default",
                    keep_warm_hours: match ttl {
                        CacheTtl::OneHour => p.keep_warm_hours(),
                        CacheTtl::FiveMinutes => 0.0,
                    },
                })
            })
            .collect();
        let (kept, stopped, reads_today, usd_today) =
            self.runner.keep_warm.counts(&today(&self.kernel));
        theseus_protocol::PromptCacheHealth {
            profiles,
            keep_warm_minutes: self.cfg.cache.keep_warm_minutes,
            kept,
            stopped,
            reads_today,
            usd_today,
        }
    }
}
