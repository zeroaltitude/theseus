//! Topics per exchange: `onto.v1` already rides the request sent for every
//! message a person sends and files the session under its categories. This
//! reads the same answer once more, where the sink writes the judgment's
//! frame, and writes a touch from each category it chose at the act band to
//! the exchange of that message. No judge call is added and the request is
//! not changed: only a reader of its answer.
//!
//! A touch's root is the category's own id (`topic:rust-harness`), as
//! `Root::Thing`: a category id is `<kind>:<local>`, as an index term is
//! `<type>:<value>`, and no kind of the ontology is a type of the index's.
//! The touch takes its exchange's time, so a topic's timeline is in the
//! order the messages were said, whenever the judge answered. A category
//! the session already holds touches too: the filing writes nothing then,
//! but the exchange was about it.

use std::collections::BTreeSet;

use theseus_judge::Judgment;
use theseus_ontology::{CategoryId, Ontology};
use theseus_store::NewRecord;

use crate::exchange::{self, touch, Root};
use crate::judge::onto::decide::{self, Plan, Step};
use crate::store::Store;

/// The categories `j` chose at the act band `act`, each once and in the
/// order of the pack's kinds: a listed candidate the ontology `o` still
/// holds, and one the judgment's `plans` minted. A project is a task, not a
/// category, and is left to the project's own records.
pub fn chosen(j: &Judgment, o: &Ontology, act: f64, plans: &[Plan]) -> Vec<CategoryId> {
    let ctx = &j.context["onto"];
    let Some(kinds) = ctx.get("kinds").and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for kind in theseus_judge::builders::ONTO_KINDS {
        let Some(candidates) = kinds.get(kind).filter(|_| kind != "project") else {
            continue;
        };
        let Some((answer, confidence)) = decide::choice(j, kind) else {
            continue;
        };
        if confidence < act {
            continue;
        }
        let id = match answer.as_str() {
            "none" => None,
            "new" => minted(plans, &j.id, kind),
            _ => decide::nth(&decide::strings(candidates), &answer, 'c')
                .and_then(|s| CategoryId::parse(s).ok())
                .filter(|id| o.category(id).is_some_and(|c| !c.is_gone())),
        };
        out.extend(id.filter(|id| !out.contains(id)));
    }
    out
}

/// The category a plan of judgment `judgment` minted for `kind`.
fn minted(plans: &[Plan], judgment: &str, kind: &str) -> Option<CategoryId> {
    plans
        .iter()
        .filter(|p| p.judgment == judgment)
        .flat_map(|p| &p.steps)
        .find_map(|s| match s {
            Step::Filed {
                kind: k,
                id,
                minted: true,
                ..
            } if k == kind => Some(id.clone()),
            _ => None,
        })
}

/// Where a judgment's touches go: its exchange's turn, session and start;
/// or, when the sink writes beside a turn whose exchange is not in the store
/// yet, the judgment's turn and session at its message's time. None for a
/// judgment that names no turn, or whose message is not in the store.
pub(super) fn landing(store: &Store, j: &Judgment) -> Option<(String, String, u64)> {
    let s = |k: &str| j.context[k].as_str().filter(|v| !v.is_empty());
    let turn = s("turn")?;
    match exchange::exchange(store, turn) {
        Ok(Some(ex)) => return Some((ex.turn_id, ex.session_id, ex.start_ms)),
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(turn, error = %format!("{e:#}"), "names: an exchange was not read");
            return None;
        }
    }
    let (_, node) = store.get_node(s("node")?).ok().flatten()?;
    Some((turn.into(), s("session")?.into(), node.created_at_ms))
}

/// The touches the judgments of `batch` come to: for each one the ontology
/// may act on, a touch from every category it [`chosen`] to its message's
/// exchange ([`landing`]). The caller holds them back while the ledger is
/// off; a task's turn is never judged by `onto.v1`.
pub fn touches(
    store: &Store,
    o: &Ontology,
    (batch, act, plans): (&[&Judgment], f64, &[Plan]),
) -> Vec<NewRecord> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for j in batch {
        let ids = chosen(j, o, act, plans);
        if ids.is_empty() {
            continue;
        }
        let Some((turn, session, at)) = landing(store, j) else {
            continue;
        };
        for id in ids {
            if seen.insert((turn.clone(), id.clone())) {
                out.extend(touch(Root::Thing(id.as_str()), &turn, &session, at));
            }
        }
    }
    out
}

/// [`touches`] for the sink's frame: the batch's judgments the ontology may
/// act on, at `[ontology] act`, against the snapshot, with the plans the
/// frame's `filing` came to.
pub(crate) fn of_batch(
    core: &crate::rpc::Core,
    batch: &[Judgment],
    filing: Option<&crate::judge::onto::stage::Pending<'_>>,
) -> Vec<NewRecord> {
    let wanted: Vec<&Judgment> = batch
        .iter()
        .filter(|j| crate::judge::onto::stage::acts(j))
        .collect();
    if wanted.is_empty() {
        return Vec::new();
    }
    let o = match core.runner.ontology.snapshot(&core.store) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "names: the ontology was not read");
            return Vec::new();
        }
    };
    let plans = filing.map_or(&[][..], |f| f.plans());
    touches(&core.store, &o, (&wanted, core.cfg.ontology.act, plans))
}

/// A topic's timeline: the newest `limit` exchanges that touched category
/// `id`, newest first.
pub fn timeline(
    store: &Store,
    id: &CategoryId,
    limit: usize,
) -> anyhow::Result<Vec<exchange::Touch>> {
    exchange::newest(store, Root::Thing(id.as_str()), limit)
}
