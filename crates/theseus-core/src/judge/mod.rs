//! The judge (M5, step 23a; design `m5-judgment.md` §2.1): where the core
//! asks Jev its typed questions, through `theseus-judge`, and records every
//! answer. In 23a one pack judges, `loop.v1`, in shadow: at each turn the
//! baseline ended with no tool calls, after the turn, and nothing acts on it.
//! In 25a two more, `classify.v1` and `role.v1`, judge each message a person
//! sends, in one request (`inbound`).
//! M6's 32c adds `rerank.v1` in shadow, after a turn's recall (`rerank`).
//!
//! - **Off the start path.** `JudgeService::new` reads nothing and sends
//!   nothing. The client, its breaker, and its sink's task are built by the
//!   first judgment, and the key is read from the secret board at each call
//!   (a key not yet settled skips the judgment: `no_key`).
//! - **Off the turn's path.** The turn's end spawns the judgment and returns
//!   ([`JudgeService::at_loop_end`]); everything else (the transcript's read,
//!   the state's build, its blob, the reservation, the call, the record) is
//!   the spawned task's. A slow, failing, or rate-limited Jev changes no
//!   turn's request bytes or result.
//! - **Recorded.** Every judgment, answered, skipped, or failed, is a
//!   `judge.call` row keyed by its id and scoped `judge:<pack id>`, written in
//!   the sink's batched frames (`sink`), with its state in a blob written
//!   before the row.
//! - **Priced, inside the shadow budget** (`spend`): an unpriced model is
//!   never called, and a judgment that would pass the day's limit is skipped
//!   and counted, never queued.
//! - **At the gate** (step 24, `gate`): `security.v1` and `security.v3` in
//!   shadow, asked of every call that acts once the gate has decided; the
//!   call never waits on them. `security.v3` is live as notices (`notice`,
//!   the owner's decision of 2026-10-04): after an open call it is sure was
//!   risky, the owner hears of it, under `security.v1`'s brake.

pub mod categorize;
pub mod citation;
pub mod compile;
pub mod gate;
pub mod inbound;
pub mod ladder;
pub mod loop_end;
pub mod mark;
pub mod memory;
pub mod notice;
pub mod rerank;
pub mod sink;
pub mod spend;

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::breaker::{BreakerConfig, Status};
use theseus_judge::{
    Ask, ClientConfig, DecisionPoint, Input, JevClient, JevJudge, JevPrice, Judge, KeySource, Mode,
    Outcome, Pack, Recording, Urgency,
};
use theseus_protocol::judge::JudgeHealth;
use zeroize::Zeroizing;

use crate::config::{JudgeConfig, PackMode};
use crate::scrub::Scrubber;
use crate::secrets::SecretBoard;
use crate::store::Store;

pub use compile::AtCompile;
pub use loop_end::LoopEnd;
pub use mark::Dispatch;
use spend::{Reserve, ShadowBudget};

/// The packs this build wires in, and the line each stands at on the
/// ladder before a `pack.mode` row of its own (26a): every pack in shadow
/// but the three the owner put live on 2026-10-04, which the ladder adopts
/// as his promotions (`ladder::adopt`): `route.v1`, `rerank.v1`, and
/// `security.v3` as notices while `[judge.packs."security.v3"] notices` is
/// on ([`crate::config::JudgeConfig::mode_of`]).
pub const WIRED: &[(&str, PackMode)] = &[
    (LOOP_PACK, PackMode::Shadow),
    (gate::SECURITY_PACK, PackMode::Shadow),
    (gate::SECURITY_CANDIDATE, PackMode::Live),
    (inbound::CLASSIFY_PACK, PackMode::Shadow),
    (inbound::ROLE_PACK, PackMode::Shadow),
    (inbound::ROUTE_PACK, PackMode::Live),
    (compile::CONTINUE_PACK, PackMode::Shadow),
    (categorize::PACK, PackMode::Shadow),
    (rerank::RERANK_PACK, PackMode::Live),
    (memory::MEMORY_PACK, PackMode::Shadow),
    (memory::ATTRIBUTION_PACK, PackMode::Shadow),
    (citation::CITATION_PACK, PackMode::Shadow),
];

/// The mode `WIRED` gives `pack` (shadow for one it does not list).
pub fn wired_mode(pack: &str) -> PackMode {
    WIRED
        .iter()
        .find(|(p, _)| *p == pack)
        .map_or(PackMode::Shadow, |(_, m)| *m)
}

/// JUDGE_STOP (§2.4), at `loop_end`.
pub const LOOP_PACK: &str = "loop.v1";

/// The sink writes its frame at most this long after a judgment lands.
pub const FLUSH_EVERY: Duration = Duration::from_secs(2);

/// The key, read from the secret board at each call: the daemon settles
/// secrets after it serves.
struct BoardKey {
    secrets: Arc<SecretBoard>,
    name: String,
}

impl KeySource for BoardKey {
    fn key(&self) -> Option<Zeroizing<String>> {
        let s = self.secrets.get(&self.name)?;
        let k = s.expose().trim();
        (!k.is_empty()).then(|| Zeroizing::new(k.to_string()))
    }
}

/// The core's scrubber, as the builders take it: every state is scrubbed of
/// the board's values and the shapes of secrets before it leaves.
struct ScrubWith(Arc<Scrubber>);

impl theseus_judge::Scrub for ScrubWith {
    fn scrub(&self, text: &str) -> String {
        self.0.scrub(text).0
    }
}

/// What the first judgment builds: the judge (client, breaker, and the
/// recording that hands each judgment to the sink). It holds no store.
pub(crate) struct Built {
    judge: Recording<JevJudge, sink::Channel>,
}

impl Built {
    /// The judge's client and breaker, unrecorded: the owner's runs (25d)
    /// write their own rows, in their own frames and scopes.
    pub(crate) fn jev(&self) -> &JevJudge {
        self.judge.inner()
    }
}

pub struct JudgeService {
    cfg: JudgeConfig,
    store: Store,
    secrets: Arc<SecretBoard>,
    scrubber: Arc<Scrubber>,
    budget: ShadowBudget,
    built: OnceLock<Arc<Built>>,
    flush: Duration,
    prices: BTreeMap<String, JevPrice>,
    /// Judgments minted at their dispatch whose rows the sink has not yet
    /// written (or that never went out): a press finds them here first.
    pending: Mutex<HashSet<String>>,
    /// `categorize.v1`'s point (28b): the core it reads, and its decisions.
    categorize: categorize::Point,
    rerank_deadline: rerank::RerankDeadline,
    /// `route.v1` verdicts that came after their turn's wait, by session:
    /// each applies from the session's next message (25e).
    late: Mutex<std::collections::HashMap<String, crate::routing::Verdict>>,
    /// Each session's last routed turn, when and on what profile: a pin of
    /// another profile within 10 minutes after it is `route.v1`'s ladder
    /// event (26a's `pins_per_day`). In memory, as `late` is.
    routed: Mutex<std::collections::HashMap<String, (u64, String)>>,
    /// A test's judge in Jev's place for the rerank (a channel for Jev).
    #[cfg(test)]
    rerank_judge: OnceLock<Arc<dyn Judge>>,
    /// `security.v3`'s notices' brake (`notice`): today's notices and
    /// noise labels, and a pause.
    brake: notice::Brake,
    me: Weak<JudgeService>,
    /// Where the judge's facts say their sentences, and their metrics go
    /// (23b): set by the core as it builds, and as its telemetry is built.
    narrator: OnceLock<Arc<crate::narrative::Narrator>>,
    telemetry: OnceLock<crate::telemetry::Telemetry>,
    /// Each pack's mode (26a): read at the first read after serving, then
    /// kept.
    ladder: ladder::Ladder,
}

impl JudgeService {
    /// The service, built with the core: nothing read, built, or sent.
    pub fn new(
        cfg: JudgeConfig,
        store: Store,
        secrets: Arc<SecretBoard>,
        scrubber: Arc<Scrubber>,
    ) -> Arc<Self> {
        Self::with_parts(
            cfg,
            store,
            secrets,
            scrubber,
            FLUSH_EVERY,
            crate::catalog::judge_prices(),
        )
    }

    /// The same, with the sink's window and the prices (tests shorten the
    /// one and empty the other).
    pub fn with_parts(
        cfg: JudgeConfig,
        store: Store,
        secrets: Arc<SecretBoard>,
        scrubber: Arc<Scrubber>,
        flush: Duration,
        prices: BTreeMap<String, JevPrice>,
    ) -> Arc<Self> {
        let budget = ShadowBudget::new(cfg.shadow_limit_usd_per_day);
        let ladder = ladder::Ladder::new(store.clone());
        Arc::new_cyclic(|me| Self {
            cfg,
            ladder,
            store,
            secrets,
            scrubber,
            budget,
            built: OnceLock::new(),
            flush,
            prices,
            pending: Mutex::new(HashSet::new()),
            categorize: Default::default(),
            rerank_deadline: rerank::RerankDeadline::default(),
            late: Mutex::default(),
            routed: Mutex::default(),
            #[cfg(test)]
            rerank_judge: OnceLock::new(),
            brake: notice::Brake::default(),
            me: me.clone(),
            narrator: OnceLock::new(),
            telemetry: OnceLock::new(),
        })
    }

    /// A test's judge in Jev's place for the rerank's call.
    #[cfg(test)]
    pub(crate) fn rerank_with(&self, judge: Arc<dyn Judge>) {
        let _ = self.rerank_judge.set(judge);
    }

    /// The narrative the judge's facts speak in (the core's).
    pub fn narrate_to(&self, narrator: Arc<crate::narrative::Narrator>) {
        let _ = self.narrator.set(narrator);
        let me = self.me.clone();
        self.ladder.say_to(Arc::new(move |row| {
            if let Some(svc) = me.upgrade() {
                svc.announce(None, None, &crate::fact::ladder::PackModeSet { row });
            }
        }));
    }

    /// The ladder (26a): each pack version's mode.
    pub fn ladder(&self) -> &ladder::Ladder {
        &self.ladder
    }

    /// What `pack` does in `session` (26a): off, shadow, or live, and the
    /// arm its judgment records. Every point asks this one function. The
    /// ladder's mode under the config's ceiling (`mode_of`: it lowers, never
    /// raises); a canary acts in its canary arm and judges the control in
    /// shadow.
    pub fn mode_for(&self, pack: &str, session: &str) -> ladder::Given {
        if !self.cfg.enabled {
            return ladder::Given::OFF;
        }
        self.ladder.given(&self.cfg, pack, session)
    }

    /// Whether `pack` is on at all (`mode_for`, whose `off` is no session's).
    pub fn pack_on(&self, pack: &str) -> bool {
        self.mode_for(pack, "").on()
    }

    /// The mode a judgment of `pack` is asked in, its arm written into its
    /// context beside the session the context names.
    pub fn ask_mode(&self, pack: &str, context: &mut serde_json::Value) -> Mode {
        let session = context["session"].as_str().unwrap_or_default().to_string();
        let given = self.mode_for(pack, &session);
        if let Some(o) = context.as_object_mut() {
            o.insert("pack_arm".into(), json!(given.arm.as_str()));
        }
        given.judge_mode()
    }

    /// Today, the ladder's local day (`2026-10-04`).
    pub fn today(&self) -> String {
        spend::local_day(self.ladder.now())
    }

    /// An operator's label on one of `pack`'s judgments, as its rules count
    /// it: a label in words (`noise`, `useful`, `wrong role`) only.
    pub fn land_label(&self, pack: &str, label: &serde_json::Value) {
        if let Some(l) = label.as_str() {
            let day = self.today();
            self.land(
                pack,
                theseus_judge::learn::CanaryEvent::Label {
                    day,
                    label: l.into(),
                },
            );
        }
    }

    /// An event `pack`'s rollback rules count (26a): kept, then the rules
    /// checked. Nothing with the judge off.
    pub fn land(&self, pack: &str, event: theseus_judge::learn::CanaryEvent) {
        if self.cfg.enabled {
            self.ladder.land(pack, event);
        }
    }

    /// The telemetry its metrics go to, once the core has built it.
    pub fn export_to(&self, telemetry: crate::telemetry::Telemetry) {
        let _ = self.telemetry.set(telemetry);
    }

    /// Say a fact whose row is written: its sentences, as `session`'s and
    /// `turn`'s (none: the judge's own), to no one's notification.
    pub(crate) fn announce<F: crate::fact::Fact>(
        &self,
        session: Option<&str>,
        turn: Option<&str>,
        f: &F,
    ) {
        if let Some(n) = self.narrator.get() {
            self.rec(n, session, turn).announce(f);
        }
    }

    fn rec<'a>(
        &'a self,
        narrator: &'a crate::narrative::Narrator,
        session: Option<&'a str>,
        turn: Option<&'a str>,
    ) -> crate::fact::Rec<'a> {
        crate::fact::Rec {
            narrator,
            session,
            turn,
            to: crate::fact::To::Nobody,
            store: &self.store,
        }
    }

    pub fn config(&self) -> &JudgeConfig {
        &self.cfg
    }

    /// The client, the breaker, and the sink's task, on the first judgment.
    fn built(&self) -> anyhow::Result<Arc<Built>> {
        if let Some(b) = self.built.get() {
            return Ok(b.clone());
        }
        let client = JevClient::new(
            ClientConfig {
                api_base: self.cfg.api_base.clone(),
                connect: Duration::from_secs(self.cfg.connect_secs),
                total: Duration::from_secs(self.cfg.total_secs),
                max_in_flight: self.cfg.max_in_flight,
            },
            Arc::new(BoardKey {
                secrets: self.secrets.clone(),
                name: self.cfg.key_secret.clone(),
            }),
        )?;
        // Rerank's failures and timeouts move only its own breaker (32d):
        // five slow reranks in a row stop no other pack.
        let judge = JevJudge::new(client, self.prices.clone(), BreakerConfig::default())
            .with_breaker(
                rerank::BREAKER,
                &[rerank::RERANK_PACK],
                BreakerConfig::default(),
            );
        let (channel, rx) = sink::Channel::new();
        let b = Arc::new(Built {
            judge: Recording::new(judge, channel),
        });
        if self.built.set(b.clone()).is_ok() {
            tokio::spawn(sink::run(rx, self.me.clone(), self.flush));
        }
        Ok(self.built.get().cloned().unwrap_or(b))
    }

    /// The client, the breaker, and the sink's task, built as the first
    /// judgment builds them: for the owner's runs (25d).
    pub(crate) fn jev(&self) -> anyhow::Result<Arc<Built>> {
        self.built()
    }

    /// The scrubber every state is built with.
    pub(crate) fn scrub(&self) -> impl theseus_judge::Scrub {
        ScrubWith(self.scrubber.clone())
    }

    /// Whether `loop.v1` judges a turn that ended so, decided before its
    /// last frame (23b): pure, from the config, the pack's sample, and the
    /// turn's id, with the judgment's id minted now. `None`: not judged.
    pub fn plan_loop_end(
        &self,
        stop_reason: &str,
        turn_id: &str,
        session_id: &str,
    ) -> Option<Dispatch> {
        if stop_reason != "no_tool_calls" {
            return None;
        }
        let given = self.mode_for(LOOP_PACK, session_id);
        if !given.on() {
            return None;
        }
        let pack = theseus_judge::pack::by_name(LOOP_PACK)?;
        sampled(turn_id, self.cfg.sample_of(LOOP_PACK, pack.sample))
            .then(|| Dispatch::new(LOOP_PACK, "loop_end", given.judge_mode()))
    }

    /// Mark the turn's trace with `loop.v1`'s dispatch, when it judges the
    /// turn: at its end, before its last frame, which carries the trace. The
    /// judgment is spawned after that frame (`after_turn`), with this id.
    pub fn mark_turn_end(
        &self,
        trace: &mut crate::trace::Trace,
        res: &theseus_protocol::TurnSubmitResult,
        task: bool,
    ) {
        if let Some(d) = self.plan_loop_end(&res.stop_reason, &res.turn_id, &res.session_id) {
            let class = loop_end::class(task, res.tool_calls);
            d.mark(
                trace,
                json!({"loop": res.loops.saturating_sub(1), "class": class}),
            );
        }
    }

    /// A turn that ended: one the baseline ended with no tool calls goes to
    /// `loop.v1` (`at_loop_end`), with the id its trace's mark names, and a
    /// conversation's to `categorize.v1`'s decision (`at_exchange_end`, 28b);
    /// any other is not judged.
    pub fn after_turn(&self, res: &theseus_protocol::TurnSubmitResult, task: bool) {
        if res.stop_reason != "no_tool_calls" {
            return;
        }
        self.at_exchange_end(
            categorize::ExchangeEnd {
                session_id: res.session_id.clone(),
                execution_id: res.execution_id.clone().unwrap_or_default(),
                turn_id: res.turn_id.clone(),
            },
            task,
        );
        let marked = res
            .trace
            .as_ref()
            .map(mark::marks)
            .unwrap_or_default()
            .into_iter()
            .find(|d| d.pack == LOOP_PACK)
            .map(|d| d.id);
        self.at_loop_end(
            LoopEnd {
                session_id: res.session_id.clone(),
                execution_id: res.execution_id.clone().unwrap_or_default(),
                turn_id: res.turn_id.clone(),
                task,
                output: res.output.clone(),
                loops: res.loops,
                cost_usd: res.cost_usd,
                tool_calls: res.tool_calls,
            },
            marked,
        );
    }

    /// A turn the baseline ended with no tool calls: `loop.v1` judges it in
    /// shadow, in a task of its own, as `id` when the turn's trace marked one
    /// (else a fresh id). Returns at once, whatever Jev does.
    pub fn at_loop_end(&self, end: LoopEnd, id: Option<String>) {
        let Some(d) = self.plan_loop_end("no_tool_calls", &end.turn_id, &end.session_id) else {
            return;
        };
        let Some(pack) = theseus_judge::pack::by_name(LOOP_PACK) else {
            return;
        };
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        rt.spawn(judge_loop(self.me.clone(), pack, end, id.unwrap_or(d.id)));
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn pending_insert(&self, ids: impl IntoIterator<Item = String>) {
        self.pending().extend(ids);
    }

    fn pending_has(&self, id: &str) -> bool {
        self.pending().contains(id)
    }

    fn pending_remove(&self, ids: &[String]) {
        let mut p = self.pending();
        for id in ids {
            p.remove(id);
        }
    }

    /// The blocking half before the call: the transcript's read, the state's
    /// build and blob, and the reservation. `None`: nothing to send.
    fn prepare_loop(
        &self,
        pack: Arc<Pack>,
        end: &LoopEnd,
        id: String,
        today: &str,
    ) -> Option<Prepared> {
        let (state, blob) = self.loop_state(&pack, end)?;
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut context = json!({
            "session": end.session_id, "execution": end.execution_id, "turn": end.turn_id,
            "loops": end.loops, "baseline": "until_no_tool_calls", "decision": "no_tool_calls",
            "class": end.class(), "blob": blob, "on_path_ms": 0,
        });
        let mode = self.ask_mode(&pack.name(), &mut context);
        let mut ask = Ask::new(pack, &state, mode, context);
        ask.id = Some(id);
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        self.reserve(today, need)
            .then_some(Prepared { built, ask, need })
    }

    /// `loop.v1`'s state from the session's nodes, scrubbed and capped, and
    /// its blob, written before any row names it.
    fn loop_state(&self, pack: &Pack, end: &LoopEnd) -> Option<(theseus_judge::Prepared, String)> {
        let nodes: Vec<_> = self
            .store
            .session_nodes(&end.session_id)
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the session's nodes were not read"))
            .ok()?
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        let input = loop_end::input(&nodes, end, theseus_protocol::now_unix_ms());
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(pack, &Input::Loop(input), &scrub).ok()?;
        let blob = self
            .store
            .blobs()
            .put(state.state.json.as_bytes())
            .map_err(|e| tracing::warn!(error = %e, "judge: the state's blob was not written; not judged"))
            .ok()?;
        Some((state, blob))
    }

    /// Reserve `need` from the shadow budget, writing what the reservation
    /// asks for (a new block, a crash's booked rest, the day's pause) first.
    /// False: paused at the limit, or the frame was not written.
    fn reserve(&self, today: &str, need: theseus_judge::price::Micros) -> bool {
        let (granted, records, said) = match self.budget.reserve(&self.store, today, need) {
            Reserve::Granted(r, s) => (true, r, s),
            Reserve::Paused(r, s) => (false, r, s),
        };
        if records.is_empty() {
            return granted;
        }
        match self.store.append(&records) {
            Ok(_) => {
                if let Some(n) = self.narrator.get() {
                    let rec = self.rec(n, None, None);
                    said.iter().for_each(|s| s.announce(&rec));
                }
                granted
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "judge: the budget's frame was not written");
                if granted {
                    self.budget.settle(today, need, 0, false, false);
                }
                false
            }
        }
    }

    /// Health's `judge` block: what the config says, the breaker, and
    /// today's counts. A read at most; nothing is built or written.
    pub fn health(&self) -> JudgeHealth {
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        // Off, nothing is counted and the store is not read.
        let t = match self.cfg.enabled {
            true => self.budget.today(&self.store, &today),
            false => spend::Today {
                day: today,
                ..spend::Today::default()
            },
        };
        let key = match self.secrets.states().get(&self.cfg.key_secret) {
            Some(crate::secrets::SecretState::Ready(_)) => "ready".to_string(),
            Some(crate::secrets::SecretState::Resolving) => "resolving".to_string(),
            Some(crate::secrets::SecretState::Failed(why)) => format!("failed: {why}"),
            None => "not configured".to_string(),
        };
        let shed = self
            .built
            .get()
            .map_or(0, |b| b.judge.inner().client().shed_total());
        let words = |s: Status| match s {
            Status::Closed { .. } => "closed".to_string(),
            Status::Open { secs_left } => format!("open ({secs_left}s left)"),
            Status::HalfOpen => "half_open".to_string(),
        };
        let (breaker, breakers, in_flight) = match self.built.get() {
            Some(b) => {
                let j = b.judge.inner();
                let own = j
                    .own_breakers()
                    .into_iter()
                    .map(|(name, s)| format!("{name}: {}", words(s)))
                    .collect();
                (
                    words(j.breaker_status()),
                    own,
                    j.client().in_flight() as u64,
                )
            }
            None => (
                "idle".to_string(),
                vec![format!("{}: idle", rerank::BREAKER)],
                0,
            ),
        };
        let notices = self.notices_state();
        JudgeHealth {
            enabled: self.cfg.enabled,
            max_mode: self.cfg.max_mode.as_str().into(),
            packs: self.pack_lines(),
            breaker,
            breakers,
            in_flight,
            day: t.day,
            calls_today: t.calls,
            failed_today: t.failed,
            skipped_today: t.skipped,
            spend_today_usd: theseus_judge::price::micros_to_usd(t.spent_micros),
            shadow_limit_usd: theseus_judge::price::micros_to_usd(self.budget.limit_micros()),
            paused: t.paused,
            shed,
            key,
            notices,
        }
    }
}

impl JudgeService {
    /// Health's line per wired pack (26a): its mode, share and why
    /// (`route.v1: live (owner: decision of 2026-10-04)`), or what the
    /// config caps it at. Before the ladder's first read, each pack's wired
    /// line under the config: health never reads the ladder itself.
    pub fn pack_lines(&self) -> Vec<String> {
        let loaded = self.cfg.enabled && self.ladder.is_loaded();
        self.ladder
            .wired_packs()
            .iter()
            .map(|(p, wired)| {
                if !loaded {
                    return format!("{p}: {}", self.cfg.mode_of(p, *wired).as_str());
                }
                let s = self.ladder.standing(p);
                let acts = self.cfg.mode_of(p, s.rung.acts_as());
                if s.why == ladder::WIRED_WHY {
                    // No row: the wired line under the config, as before 26a.
                    format!("{p}: {}", acts.as_str())
                } else if acts == s.rung.acts_as() {
                    format!("{p}: {}", s.words())
                } else {
                    format!(
                        "{p}: {} (the config's ceiling; on the ladder: {})",
                        acts.as_str(),
                        s.words()
                    )
                }
            })
            .collect()
    }
}

/// A judgment ready to send.
struct Prepared {
    built: Arc<Built>,
    ask: Ask,
    need: theseus_judge::price::Micros,
}

/// One `loop.v1` judgment, in its own task. The service is held only
/// around the blocking halves, never across the call: a stop never waits on
/// Jev to let the store go.
async fn judge_loop(me: Weak<JudgeService>, pack: Arc<Pack>, end: LoopEnd, id: String) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let day = today.clone();
    let prepared = tokio::task::spawn_blocking(move || svc.prepare_loop(pack, &end, id, &day))
        .await
        .ok()
        .flatten();
    let Some(Prepared { built, ask, need }) = prepared else {
        return;
    };
    let t0 = Instant::now();
    let judgments = built
        .judge
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency: Urgency::Shadow,
        })
        .await;
    tracing::debug!(
        ms = t0.elapsed().as_millis() as u64,
        "judge: loop.v1 judged"
    );
    let Some(svc) = me.upgrade() else { return };
    for j in &judgments {
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        // A call whose usage is unknown (a timeout after the send) is booked
        // at its reservation, conservatively.
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        svc.budget.settle(&today, need, spent, called, failed);
    }
}

/// Whether a turn is in the pack's shadow sample: the first 8 bytes of the
/// SHA-256 of its id, read as a fraction, under `share`.
pub(crate) fn sampled(turn_id: &str, share: f64) -> bool {
    if share >= 1.0 {
        return true;
    }
    use sha2::Digest;
    let h = sha2::Sha256::digest(turn_id.as_bytes());
    let x = u64::from_be_bytes(h[..8].try_into().expect("8 bytes")) as f64 / u64::MAX as f64;
    x < share
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_samples_about_its_part_and_none_samples_nothing() {
        let ids: Vec<String> = (0..2000).map(|i| format!("turn_{i}")).collect();
        let n = |s: f64| ids.iter().filter(|i| sampled(i, s)).count();
        assert_eq!(n(1.0), 2000);
        assert_eq!(n(0.0), 0);
        let half = n(0.5);
        assert!((850..1150).contains(&half), "{half}");
        assert!(
            ids.iter().all(|i| sampled(i, 0.5) == sampled(i, 0.5)),
            "sticky"
        );
    }
}
