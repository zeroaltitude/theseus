//! The judge (M5, step 23a; design `m5-judgment.md` §2.1): where the core
//! asks Jev its typed questions, through `theseus-judge`, and records every
//! answer. In 23a one pack judges, `loop.v1`, in shadow: at each turn the
//! baseline ended with no tool calls, after the turn, and nothing acts on it.
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

pub mod loop_end;
pub mod rerank;
pub mod sink;
pub mod spend;

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, Weak};
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

pub use loop_end::LoopEnd;
use spend::{Reserve, ShadowBudget};

/// The packs this build wires in, and the mode the ladder gives each (step
/// 26a brings the ladder; until then every pack is in shadow).
pub const WIRED: &[(&str, PackMode)] = &[
    (LOOP_PACK, PackMode::Shadow),
    (rerank::RERANK_PACK, PackMode::Shadow),
];

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

pub struct JudgeService {
    cfg: JudgeConfig,
    store: Store,
    secrets: Arc<SecretBoard>,
    scrubber: Arc<Scrubber>,
    budget: ShadowBudget,
    built: OnceLock<Arc<Built>>,
    flush: Duration,
    prices: BTreeMap<String, JevPrice>,
    rerank_deadline: rerank::RerankDeadline,
    me: Weak<JudgeService>,
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
        Arc::new_cyclic(|me| Self {
            cfg,
            store,
            secrets,
            scrubber,
            budget,
            built: OnceLock::new(),
            flush,
            prices,
            rerank_deadline: rerank::RerankDeadline::default(),
            me: me.clone(),
        })
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
        let judge = JevJudge::new(client, self.prices.clone(), BreakerConfig::default());
        let (channel, rx) = sink::Channel::new();
        let b = Arc::new(Built {
            judge: Recording::new(judge, channel),
        });
        if self.built.set(b.clone()).is_ok() {
            tokio::spawn(sink::run(rx, self.me.clone(), self.flush));
        }
        Ok(self.built.get().cloned().unwrap_or(b))
    }

    /// A turn that ended: one the baseline ended with no tool calls goes to
    /// `loop.v1` (`at_loop_end`); any other is not judged in 23a.
    pub fn after_turn(&self, res: &theseus_protocol::TurnSubmitResult, task: bool) {
        if res.stop_reason != "no_tool_calls" {
            return;
        }
        self.at_loop_end(LoopEnd {
            session_id: res.session_id.clone(),
            execution_id: res.execution_id.clone().unwrap_or_default(),
            turn_id: res.turn_id.clone(),
            task,
            output: res.output.clone(),
            loops: res.loops,
            cost_usd: res.cost_usd,
            tool_calls: res.tool_calls,
        });
    }

    /// A turn the baseline ended with no tool calls: `loop.v1` judges it in
    /// shadow, in a task of its own. Returns at once, whatever Jev does.
    pub fn at_loop_end(&self, end: LoopEnd) {
        if self.cfg.mode_of(LOOP_PACK, PackMode::Shadow) == PackMode::Off {
            return;
        }
        let Some(pack) = theseus_judge::pack::by_name(LOOP_PACK) else {
            return;
        };
        if !sampled(&end.turn_id, self.cfg.sample_of(LOOP_PACK, pack.sample)) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        rt.spawn(judge_loop(self.me.clone(), pack, end));
    }

    /// The blocking half before the call: the transcript's read, the state's
    /// build and blob, and the reservation. `None`: nothing to send.
    fn prepare_loop(&self, pack: Arc<Pack>, end: &LoopEnd, today: &str) -> Option<Prepared> {
        let (state, blob) = self.loop_state(&pack, end)?;
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let context = json!({
            "session": end.session_id, "execution": end.execution_id, "turn": end.turn_id,
            "loops": end.loops, "baseline": "until_no_tool_calls", "decision": "no_tool_calls",
            "class": end.class(), "blob": blob, "on_path_ms": 0,
        });
        let ask = Ask::new(pack, &state, Mode::Shadow, context);
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
        let (granted, records) = match self.budget.reserve(&self.store, today, need) {
            Reserve::Granted(r) => (true, r),
            Reserve::Paused(r) => (false, r),
        };
        if records.is_empty() {
            return granted;
        }
        match self.store.append(&records) {
            Ok(_) => granted,
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
        let (breaker, in_flight) = match self.built.get() {
            Some(b) => {
                let j = b.judge.inner();
                let s = match j.breaker_status() {
                    Status::Closed { .. } => "closed".to_string(),
                    Status::Open { secs_left } => format!("open ({secs_left}s left)"),
                    Status::HalfOpen => "half_open".to_string(),
                };
                (s, j.client().in_flight() as u64)
            }
            None => ("idle".to_string(), 0),
        };
        JudgeHealth {
            enabled: self.cfg.enabled,
            max_mode: self.cfg.max_mode.as_str().into(),
            packs: WIRED
                .iter()
                .map(|(p, given)| format!("{p}: {}", self.cfg.mode_of(p, *given).as_str()))
                .collect(),
            breaker,
            in_flight,
            day: t.day,
            calls_today: t.calls,
            failed_today: t.failed,
            skipped_today: t.skipped,
            spend_today_usd: theseus_judge::price::micros_to_usd(t.spent_micros),
            shadow_limit_usd: theseus_judge::price::micros_to_usd(self.budget.limit_micros()),
            paused: t.paused,
        }
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
async fn judge_loop(me: Weak<JudgeService>, pack: Arc<Pack>, end: LoopEnd) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let day = today.clone();
    let prepared = tokio::task::spawn_blocking(move || svc.prepare_loop(pack, &end, &day))
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
fn sampled(turn_id: &str, share: f64) -> bool {
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
