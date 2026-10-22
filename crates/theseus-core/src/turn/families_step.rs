//! Jev judges each hidden tool family against the prompt in focus
//! (theseus-xlri5, `judge::families`): one classification a prompt, started at
//! the inbound point beside the turn's other work, taken, bounded, before each
//! `defer_tools`. The families it finds relevant join the named rule's
//! (`tools_defer::want::extra`), after them, and may raise the first request
//! past the place's budget under `[tools.defer] admit_ceiling`.
//!
//! - **Only where it can matter**: the session's own model has no tool search,
//!   or routing acts live and may detour to one that has none. A model with
//!   tool search never waits for it and loads nothing by it.
//! - **Bounded from its start**: the turn waits until `[tools.defer]
//!   judge_wait_ms` after the inbound point at most, and not again once it has
//!   the answer; a late answer is dropped (`answered: false` in the `judge`
//!   span), as is a failure: the named rule then works alone.
//! - **Cached per prompt**: the turn keeps the set for its later loops and its
//!   routed recompile. The defer board keeps the conversation's last set by the
//!   prompt's digest, in memory: the same prompt again asks nothing, and a turn
//!   with no new message (a continuation, a task's later turn) reuses it.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_judge::builders::{FamilyItem, FamilyTool, FAMILIES_MAX, FAMILY_TOOLS};
use tokio::sync::oneshot;
use tokio::time::Instant;

use super::{Turn, TurnRunner};
use crate::config::PackMode;
use crate::judge::families::{Families, Relevant, FAMILIES_PACK};
use crate::tools_defer::{family, PLAN_FAMILY};

/// A turn's families judgment: the answer it waits for, until its deadline, and
/// the set once taken.
#[derive(Default)]
pub(super) struct Judged {
    pending: Option<(oneshot::Receiver<Relevant>, Instant, u64)>,
    /// The families Jev judged relevant, once taken (empty: none, or no answer).
    pub(super) relevant: Option<Relevant>,
}

/// The digest a prompt's judgment is kept by.
fn digest_of(ask: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ask.hash(&mut h);
    h.finish()
}

impl TurnRunner {
    fn tool_search(&self, model: &str) -> bool {
        self.catalog.get(model).is_some_and(|e| e.tool_search)
    }

    /// The inbound point's part: start the judgment of the turn's message
    /// (`t.route.ask`), unless the conversation's last one was of this prompt.
    pub(super) fn families_start(&self, t: &mut Turn<'_>, route: PackMode) {
        let cfg = &self.cfg.tools.defer;
        if !cfg.enabled || !cfg.judge_families || t.route.ask.trim().is_empty() {
            return;
        }
        if self.tool_search(&t.target.model) && route < PackMode::Canary {
            return;
        }
        let sid = t.tc.session_id;
        let digest = digest_of(&t.route.ask);
        if let Some((d, set)) = self.tools.defer.judged(sid) {
            if d == digest {
                t.route.families.relevant = Some(set);
                return;
            }
        }
        let started = Instant::now();
        let ask = Families {
            session_id: sid.to_string(),
            turn_id: t.tc.turn_id.to_string(),
            message: t.route.ask.clone(),
            families: self.hidden_families(t),
        };
        if let Some(rx) = self.judge.families_start(&mut t.trace, ask) {
            let deadline = started + Duration::from_millis(cfg.judge_wait_ms);
            t.route.families.pending = Some((rx, deadline, digest));
        }
    }

    /// Take the judgment before the turn settles its tools on `model`: wait for
    /// it until its deadline at most, once; nothing for a model with tool search.
    pub(super) async fn families_ready(&self, t: &mut Turn<'_>, model: &str) {
        let cfg = &self.cfg.tools.defer;
        if !cfg.enabled || !cfg.judge_families || self.tool_search(model) {
            return;
        }
        if t.route.families.relevant.is_some() {
            return;
        }
        let sid = t.tc.session_id;
        let Some((rx, deadline, digest)) = t.route.families.pending.take() else {
            // No new message: the conversation's last set; a message not asked about: none.
            let last = match t.route.ask.trim().is_empty() {
                true => self.tools.defer.judged(sid).map(|(_, set)| set),
                false => None,
            };
            t.route.families.relevant = Some(last.unwrap_or_default());
            return;
        };
        let t0 = t.trace.now_us();
        let got = tokio::time::timeout_at(deadline, rx)
            .await
            .ok()
            .and_then(Result::ok);
        t.trace.record(
            "judge",
            "wait",
            t0,
            t.trace.now_us(),
            json!({"pack": FAMILIES_PACK, "point": "inbound", "mode": "live",
                   "answered": got.is_some(), "wait_ms": cfg.judge_wait_ms}),
        );
        if let Some(set) = &got {
            self.tools.defer.set_judged(sid, digest, set.clone());
        }
        t.route.families.relevant = Some(got.unwrap_or_default());
    }

    /// `route_to`, once the families judgment is taken for the routed model.
    pub(super) async fn routed_to<'a>(
        &self,
        t: &mut Turn<'a>,
        session: &mut crate::session::SessionRecord,
        spec: &mut crate::compiler::RequestSpec,
        slot: &'a std::sync::OnceLock<super::Target>,
        target: super::Target,
        d: &crate::routing::Decision,
    ) -> anyhow::Result<()> {
        self.families_ready(t, &target.model).await;
        self.route_to(t, session, spec, slot, target, d)
    }

    /// The families the turn's place may hide, in the table's order, with what
    /// each holds and some of its tools: not the always-loaded ones, the place
    /// kind's defaults, or the plan's (its mode loads it).
    fn hidden_families(&self, t: &Turn<'_>) -> Vec<FamilyItem> {
        let cfg = &self.cfg.tools.defer;
        let kind = self.place_kind(t.tc.session_id, t.tc.task.is_some());
        let tools = self.tools.definitions_for(t.tc.place());
        let mut by: BTreeMap<String, Vec<FamilyTool>> = BTreeMap::new();
        for d in tools.iter() {
            let Some(name) = d.get("name").and_then(Value::as_str) else {
                continue;
            };
            let fam = family::of_wire(name);
            let skip = !family::hideable(&fam)
                || fam == PLAN_FAMILY
                || cfg.always_loaded.contains(&fam)
                || kind.defaults().contains(&fam.as_str());
            if skip {
                continue;
            }
            let desc = d.get("description").and_then(Value::as_str).unwrap_or("");
            let first = desc.split_inclusive(". ").next().unwrap_or(desc).trim();
            let list = by.entry(fam).or_default();
            if list.len() < FAMILY_TOOLS {
                list.push(FamilyTool {
                    name: name.to_string(),
                    does: first.to_string(),
                });
            }
        }
        let rank = |f: &str| {
            family::FAMILIES
                .iter()
                .position(|(n, _)| *n == f)
                .unwrap_or(family::FAMILIES.len())
        };
        let mut out: Vec<FamilyItem> = by
            .into_iter()
            .map(|(key, tools)| FamilyItem {
                holds: family::summary(&key).unwrap_or_default(),
                key,
                tools,
            })
            .collect();
        out.sort_by(|a, b| rank(&a.key).cmp(&rank(&b.key)).then(a.key.cmp(&b.key)));
        out.truncate(FAMILIES_MAX);
        out
    }
}
