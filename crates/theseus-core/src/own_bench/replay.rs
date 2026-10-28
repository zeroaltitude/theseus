//! `turn.replay` (own-benchmark): a past turn's first request, rebuilt
//! byte for byte (`rebuild.rs`), sent to the named profile with no tool run,
//! the reply and its price returned, and judged against the original's by
//! `compare.v1`. It writes a `turn.replayed` row and nothing else: no node,
//! no session, no execution. The owner's run (`judge_act(Act::JudgeRun)`):
//! it spends money and sends a session's text to a profile.
//!
//! `scope: "whole_turn"` runs the turn's tools too, on a scratch copy of its
//! directory (`whole/`); the first request, its price and its judgment are
//! the same.
//!
//! A replay never leaves what a turn's place allows: a shared place's turn is
//! refused, and so is one that would send a sealed thread's, a veiled
//! recall's or an attachment's bytes to a provider other than the original's
//! (`crossing`).

use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Context};
use serde_json::Value;
use theseus_judge::builders::{CompareInput, CompareReply};
use theseus_protocol::own_bench::{ReplayVerdict, TurnReplayParams, TurnReplayResult};

use super::rebuild::{Original, Said};
use super::{parse_effort, BY_OWNER, REPLAY_MAX_TOKENS};
use crate::compiler::render_request;
use crate::fact::own_bench::TurnReplayed;
use crate::node::Body;
use crate::places::PlaceClass;
use crate::rpc::Core;

/// The only scope: the turn's first request, no tool run.
pub const FIRST_LOOP: &str = "first_loop";

/// A replay the caller's cap would not pay for: its reservation against what
/// was left, in micros.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapReached {
    pub need: u64,
    pub left: u64,
}

impl std::fmt::Display for CapReached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the next replay would reserve {} micro-dollars with {} left under the cap",
            self.need, self.left
        )
    }
}

impl std::error::Error for CapReached {}

/// A replay refused because it would send what a place or a label keeps to a
/// provider other than the original's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crossing(pub String);

impl std::fmt::Display for Crossing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Crossing {}

/// A replay done: the result, and what it cost in micros, for the sampler's
/// books.
pub struct Replayed {
    pub result: TurnReplayResult,
    pub from_effort: String,
    pub replay_micros: u64,
}

/// A model's answer as the replay's reply: its text, the calls it asked for
/// by name, its tokens, and what the call cost in micros.
pub(super) fn said_of_response(resp: &crate::provider::ModelResponse, cost: u64) -> Said {
    Said {
        model: resp.model.clone(),
        text: resp.text.clone(),
        tool_calls: resp
            .content
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
            .filter_map(|b| b.get("name").and_then(Value::as_str).map(str::to_string))
            .collect(),
        input_tokens: resp.usage.input_tokens
            + resp.usage.cache_read_input_tokens
            + resp.usage.cache_creation_input_tokens,
        output_tokens: resp.usage.output_tokens,
        cost_usd: theseus_kernel::micros_to_usd(cost),
    }
}

/// The effort the original turn ran at, as a word; empty: the profile's own.
fn from_effort(original: &Original) -> String {
    original
        .spec
        .turn_effort
        .or(original.spec.effort)
        .map(super::effort_word)
        .unwrap_or_default()
}

/// Whether a request's messages carry a file: an image or a document block,
/// at any depth (a tool result's too).
pub fn carries_attachment(v: &Value) -> bool {
    match v {
        Value::Object(m) => {
            matches!(
                m.get("type").and_then(Value::as_str),
                Some("image" | "document")
            ) || m.values().any(carries_attachment)
        }
        Value::Array(a) => a.iter().any(carries_attachment),
        _ => false,
    }
}

impl Core {
    /// `turn.replay`: the owner's run, from a private place.
    pub async fn turn_replay(
        self: &Arc<Self>,
        p: TurnReplayParams,
        who: impl Into<crate::approval::Answerer>,
    ) -> anyhow::Result<TurnReplayResult> {
        let who = who.into();
        self.judge_act(
            &who,
            crate::rpc::Act::JudgeRun {
                method: theseus_protocol::method::TURN_REPLAY,
                what: "a replay of a past turn",
            },
        )?;
        let mut spent = 0;
        Ok(self
            .replay_turn(&p, BY_OWNER, None, &mut spent)
            .await?
            .result)
    }

    /// Why replaying `original` on a provider other than its own would send
    /// what must not leave it, or `None`. How it is checked: the turn's
    /// session in a sealed thread; a recall in front of the model whose
    /// source session is an imported episode labelled `personal` or
    /// `partner-confidential`; any image or document block in the rebuilt
    /// messages. The same provider as the original's sends nothing new.
    pub(crate) fn crossing(&self, original: &Original, to_provider: &str) -> Option<String> {
        if original.compilation.manifest.provider == to_provider {
            return None;
        }
        let sid = original.session.session_id.as_str();
        if self
            .threads()
            .of_session(sid)
            .is_some_and(|t| t.sealed.is_some())
        {
            return Some("its thread is sealed".into());
        }
        for (_, n) in &original.nodes {
            let Body::Recall { items, .. } = &n.body else {
                continue;
            };
            for it in items {
                let imported = self
                    .store
                    .get_session::<crate::session::SessionRecord>(&it.session_id)
                    .ok()
                    .flatten()
                    .and_then(|r| r.imported);
                if let Some(i) = imported {
                    if crate::memory_lookup::VEILED.contains(&i.labels.sensitivity.as_str()) {
                        return Some(format!(
                            "its recall carries a {} source",
                            i.labels.sensitivity
                        ));
                    }
                }
            }
        }
        if original.request.messages.iter().any(carries_attachment) {
            return Some("its request carries an attachment".into());
        }
        None
    }

    /// One turn's first loop replayed on `p.profile`, judged, and written as
    /// a `turn.replayed` row. No owner check: the owner's method and the
    /// sampler's own caps are the callers'.
    ///
    /// `left` is what a caller's cap still allows, in micros (the owner's run
    /// has none): the call's reservation is checked against it BEFORE the call,
    /// and `CapReached` is the refusal. `spent` is set to what the call cost,
    /// reservation if it failed, even when the replay then fails.
    pub(crate) async fn replay_turn(
        self: &Arc<Self>,
        p: &TurnReplayParams,
        by: &str,
        left: Option<u64>,
        spent: &mut u64,
    ) -> anyhow::Result<Replayed> {
        self.replay_turn_with(p, by, left, spent, None).await
    }

    /// A replay carries no pass for a session's text (theseus-a1wvq.10): a
    /// profile of restricted origin is refused, in words, before the rebuild.
    pub(crate) fn refuse_restricted(&self, target: &crate::turn::Target) -> Result<(), Crossing> {
        let origin = self
            .cfg
            .profile_origin(&target.profile, &self.runner.catalog);
        if !origin.restricted() {
            return Ok(());
        }
        self.runner
            .judge
            .record_data_gate("replay", "restricted_origin");
        Err(Crossing(format!(
            "not replayed on `{}`: `{}` is a model of {} origin (a restricted origin), and a \
             replay carries no pass for a session's text",
            target.profile,
            target.model,
            format!("{origin:?}").to_lowercase()
        )))
    }

    /// `replay_turn` with a minted flow loaded in the replayed model's tool
    /// set (a whole-turn replay's arm B): the mint step's A/B replay.
    pub(crate) async fn replay_turn_with(
        self: &Arc<Self>,
        p: &TurnReplayParams,
        by: &str,
        left: Option<u64>,
        spent: &mut u64,
        minted: Option<super::whole::hands::Minted>,
    ) -> anyhow::Result<Replayed> {
        let t0 = Instant::now();
        let scope = p.scope.as_deref().unwrap_or(FIRST_LOOP);
        if scope != FIRST_LOOP && scope != super::whole::WHOLE_TURN {
            bail!(
                "the scope {scope:?} is not one: {FIRST_LOOP:?} or {:?} replays",
                super::whole::WHOLE_TURN
            );
        }
        let whole = scope == super::whole::WHOLE_TURN;
        if self.runner.class_of(&p.session) == PlaceClass::Shared {
            bail!("a shared place's turn is never replayed");
        }
        let (live, _) = self.live_profile();
        let target = self
            .runner
            .resolve_target(&live, Some(&p.profile), None, None)?;
        self.refuse_restricted(&target)?;
        let effort = match p.effort.as_deref() {
            Some(e) => Some(parse_effort(e).with_context(|| format!("{e:?} is not an effort"))?),
            None => target.effort,
        };
        let me = self.clone();
        let (session, turn) = (p.session.clone(), p.turn.clone());
        let original = tokio::task::spawn_blocking(move || me.rebuild_first(&session, &turn))
            .await
            .context("the rebuild did not finish")??;
        if let Some(why) = self.crossing(&original, &target.provider) {
            return Err(Crossing(format!(
                "not replayed on {} ({}): {why}, and replaying would send it to a provider \
                 other than the original's ({})",
                target.profile, target.provider, original.compilation.manifest.provider
            ))
            .into());
        }
        let provider = self
            .runner
            .providers
            .get(&target.provider)
            .cloned()
            .with_context(|| format!("the provider {:?} is not configured", target.provider))?;
        // The replay sends the turn's own request again: its session's labels
        // hold it.
        let provider = crate::data_gate::bound(provider, &p.session);
        let request = self.replay_request(p, &original, &target, effort);
        let (replay, cost, whole_done, original_said) = if whole {
            let done = self
                .replay_whole(
                    p,
                    &original,
                    (provider.as_ref(), &request),
                    &target,
                    (left, minted),
                    spent,
                )
                .await?;
            (done.replay, done.micros, Some(done.whole), done.original)
        } else {
            let (replay, cost) = self
                .call_replay(provider.as_ref(), &request, &target, left, spent)
                .await?;
            (replay, cost, None, original.said.clone())
        };
        let mut result = TurnReplayResult {
            session: p.session.clone(),
            turn: p.turn.clone(),
            profile: target.profile.clone(),
            effort: effort.map(super::effort_word),
            scope: scope.to_string(),
            request_digest: original.digest.clone(),
            original: original_said.clone(),
            replay: replay.clone(),
            verdict: None,
            unjudged: None,
            cost_usd: replay.cost_usd,
            elapsed_ms: 0,
            whole: whole_done,
        };
        if p.compare.unwrap_or(true) {
            self.compare_replay(p, (&original.ask, &original_said), &replay, &mut result)
                .await;
        }
        if result.whole.is_some() {
            self.label_whole(p, &target.profile, &mut result);
        }
        result.elapsed_ms = t0.elapsed().as_millis() as u64;
        let from_effort = from_effort(&original);
        self.rec(Some(&p.session))
            .in_turn(Some(&p.turn))
            .record(&TurnReplayed {
                by,
                result: &result,
                from_model: &original.compilation.manifest.model,
                from_effort: &from_effort,
            });
        Ok(Replayed {
            result,
            from_effort,
            replay_micros: cost,
        })
    }

    /// The replay's one model call: its reservation checked against what
    /// the caller's cap leaves BEFORE the call, held on the day ceiling, and
    /// settled at its price. Returns the reply and its cost in micros.
    async fn call_replay(
        &self,
        provider: &dyn crate::provider::Provider,
        request: &crate::provider::ProviderRequest,
        target: &crate::turn::Target,
        left: Option<u64>,
        spent: &mut u64,
    ) -> anyhow::Result<(Said, u64)> {
        let (resp, cost) = self
            .call_model(provider, request, target, left, spent)
            .await?;
        Ok((said_of_response(&resp, cost), cost))
    }

    /// The replay's one model call, as the model answered it.
    pub(super) async fn call_model(
        &self,
        provider: &dyn crate::provider::Provider,
        request: &crate::provider::ProviderRequest,
        target: &crate::turn::Target,
        left: Option<u64>,
        spent: &mut u64,
    ) -> anyhow::Result<(crate::provider::ModelResponse, u64)> {
        let need = self
            .audit_reserve(request)
            .context("the model has no price in the catalog: a replay needs one to reserve")?;
        if let Some(l) = left.filter(|l| need > *l) {
            return Err(CapReached { need, left: l }.into());
        }
        let hold = self
            .runner
            .day_hold(need, "a replay")
            .map_err(|why| anyhow::anyhow!("{why}"))?;
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let answered = provider.stream_message(request, &mut quiet).await;
        let cost = answered
            .as_ref()
            .map_or(need, |r| self.runner.priced(r, &target.model, need));
        hold.settle(cost);
        *spent = cost;
        let resp = answered.with_context(|| format!("{} did not answer", target.profile))?;
        Ok((resp, cost))
    }

    /// The same request for the other model: the original's system, tools
    /// and messages, that model's thinking and effort.
    pub(super) fn replay_request(
        &self,
        p: &TurnReplayParams,
        original: &Original,
        target: &crate::turn::Target,
        effort: Option<crate::config::Effort>,
    ) -> crate::provider::ProviderRequest {
        let mut spec = self
            .runner
            .built_parts(
                &p.session,
                target,
                original.session.kind,
                Some(original.compilation.manifest.memberships.as_slice()),
            )
            .spec;
        let kept = &original.spec;
        spec.system_text.clone_from(&kept.system_text);
        spec.context_text.clone_from(&kept.context_text);
        spec.session_text.clone_from(&kept.session_text);
        spec.tools.clone_from(&kept.tools);
        spec.context_files.clone_from(&kept.context_files);
        spec.guidance.clone_from(&kept.guidance);
        spec.memberships.clone_from(&kept.memberships);
        spec.effort = effort;
        spec.turn_effort = None;
        spec.max_tokens = target.max_tokens.min(REPLAY_MAX_TOKENS);
        let request = render_request(
            &spec,
            &self.catalog,
            &original.compilation,
            &original.nodes,
            (
                Some(self.store.blobs()),
                &original.session.not_shown,
                None,
                &original.sources,
            ),
        )
        .request;
        request
    }

    /// `compare.v1` on the two replies, into the result.
    async fn compare_replay(
        &self,
        p: &TurnReplayParams,
        (ask, original): (&str, &Said),
        replay: &Said,
        result: &mut TurnReplayResult,
    ) {
        let input = CompareInput {
            request: ask.to_string(),
            a: CompareReply {
                text: original.text.clone(),
                calls: original.tool_calls.clone(),
            },
            b: CompareReply {
                text: replay.text.clone(),
                calls: replay.tool_calls.clone(),
            },
        };
        match self
            .runner
            .judge
            .compare_replies(&p.session, &p.turn, input)
            .await
        {
            Ok(c) => {
                result.cost_usd += c.cost_usd;
                result.verdict = Some(ReplayVerdict {
                    verdict: c.verdict,
                    p: c.p,
                    same_or_better: c.same_or_better,
                    judgment: c.judgment,
                    mode: c.mode,
                });
            }
            Err(why) => result.unjudged = Some(why),
        }
    }
}
