//! Jev judges the near-miss cross candidates (theseus-98bz.10): after the
//! thread lens, the notes it dropped as another stream's (`thread`) whose
//! score is above the bar's share of the query's best go to Jev, best first,
//! at most `[threads] cross_recall_judge_k`, in one request
//! (`judge::cross`). The lens cannot tell "this other stream's note answers
//! the question" from "this other stream's note is the same kind of fact";
//! Jev, already in the loop, can.
//!
//! - **A yes is admitted with the penalty**: the lens's `admitted` names it,
//!   and the pack runs again with the same filters and budget
//!   (`Memory::refill`, the order as it stood). Its header names its thread
//!   (`from thread «title»`), as every other stream's note's does.
//! - **Bounded**: the turn waits at most `[threads] route_wait_ms`; a timeout
//!   or a failure leaves the drops as they were.
//! - **Never a held thread's note**: a sealed or seal-pending thread's
//!   candidate is dropped `sealed` before this, is never sent, and the lens
//!   never admits it.
//! - **Not with a digest**: a turn whose request carries the place digest
//!   asks nothing.

use std::collections::BTreeSet;
use std::time::Duration;

use theseus_judge::builders::CrossNote;
use theseus_memory::recall::{Candidate, Link, Place, Reason};
use theseus_memory::MemoryScience;
use theseus_protocol::memory::RecallManifest;

use super::{Turn, TurnRunner};
use crate::judge::cross::Cross;
use crate::recall::{Ranks, Scene};
use crate::thread_recall::ThreadScene;

/// Whether a drop is the thread distance's: the one line the drop causes'
/// split changes.
fn is_thread_drop(r: Reason) -> bool {
    matches!(r, Reason::Thread(_))
}

/// A recall's scene, owned: what a pack again needs once the turn is
/// borrowed again.
pub(super) struct Parts {
    pub place: Place,
    pub in_context: BTreeSet<String>,
    pub labeled: BTreeSet<String>,
    pub budget_tokens: Option<u64>,
    pub science: std::sync::Arc<dyn MemoryScience>,
    pub threads: Option<ThreadScene>,
}

impl Parts {
    /// A scene's owned parts.
    pub fn of(s: Scene<'_>) -> Self {
        Parts {
            place: s.place,
            in_context: s.in_context,
            labeled: s.labeled,
            budget_tokens: s.budget_tokens,
            science: s.science,
            threads: s.threads,
        }
    }

    /// What the threads do to a recall over `candidates`, as `Scene::lens`.
    pub fn lens(&self, candidates: &[Candidate]) -> Option<theseus_memory::ThreadLens> {
        let t = self.threads.as_ref()?;
        Some(
            t.lens(
                candidates
                    .iter()
                    .map(|c| (c.session_id.as_str(), c.node_id.as_str(), c.fused)),
                &self.in_context,
            ),
        )
    }

    pub fn scene<'a>(&self, mode: &'a str, session: &'a str, turn: &'a str) -> Scene<'a> {
        Scene {
            mode,
            session_id: Some(session),
            turn_id: Some(turn),
            place: self.place.clone(),
            in_context: self.in_context.clone(),
            labeled: self.labeled.clone(),
            budget_tokens: self.budget_tokens,
            science: self.science.clone(),
            activation: None,
            threads: self.threads.clone(),
            persona: None,
            category: None,
        }
    }
}

impl TurnRunner {
    /// The near misses Jev reads, and what its yes admits, into `m`.
    pub(super) async fn cross_judged(
        &self,
        t: &mut Turn<'_>,
        mode: &str,
        parts: &mut Parts,
        m: &mut RecallManifest,
        (candidates, links, ranks): (&[Candidate], &[Link], &Ranks),
        message: &str,
    ) {
        let cfg = &self.cfg.threads;
        if !cfg.cross_recall_judge || t.digest.fired() {
            return;
        }
        let (Some(threads), Some(lens)) = (parts.threads.as_ref(), parts.lens(candidates)) else {
            return;
        };
        if !lens.distance || lens.top_fused <= 0.0 {
            return;
        }
        let mut near: Vec<_> = m
            .dropped
            .iter()
            .filter(|d| {
                Reason::ALL
                    .iter()
                    .find(|r| r.as_str() == d.reason)
                    .is_some_and(|r| is_thread_drop(*r))
            })
            .filter(|d| d.fused / lens.top_fused > lens.bar)
            .filter(|d| {
                lens.thread_of_node(&d.session_id, &d.node_id)
                    .is_none_or(|th| !th.held)
            })
            .collect();
        near.sort_by(|a, b| b.fused.total_cmp(&a.fused));
        let mut seen = BTreeSet::new();
        near.retain(|d| seen.insert(d.node_id.clone()));
        near.truncate(cfg.cross_recall_judge_k);
        let notes: Vec<CrossNote> = near
            .iter()
            .filter_map(|d| {
                let c = candidates.iter().find(|c| c.node_id == d.node_id)?;
                Some(CrossNote {
                    key: d.node_id.clone(),
                    thread: threads
                        .other_title(&d.session_id)
                        .unwrap_or_else(|| "the place's own conversation".into()),
                    text: c.text.clone(),
                })
            })
            .collect();
        if notes.is_empty() {
            return;
        }
        let ask = Cross {
            session_id: t.tc.session_id.to_string(),
            turn_id: t.tc.turn_id.to_string(),
            recall_id: m.recall_id.clone(),
            message: message.to_string(),
            notes,
        };
        let wait = Duration::from_millis(cfg.route_wait_ms);
        let Some(yes) = self.judge.cross_live(&mut t.trace, ask, wait).await else {
            return;
        };
        if yes.is_empty() {
            return;
        }
        // The threads Jev's yes drew on, once each, for a link to be offered (theseus-98bz.11).
        let drawn: BTreeSet<String> = yes
            .iter()
            .filter_map(|n| {
                let c = candidates.iter().find(|c| c.node_id == *n)?;
                lens.thread_of_node(&c.session_id, n)
            })
            .filter(|t| !t.held)
            .map(|t| t.slug.clone())
            .collect();
        if let Some(th) = parts.threads.as_mut() {
            th.admit(yes);
            self.link_seen(th.link_pairs(&drawn));
        }
        let order: Vec<String> = m
            .admitted
            .iter()
            .map(|a| format!("{}#{}", a.node_id, a.chunk))
            .collect();
        let scene = parts.scene(mode, t.tc.session_id, t.tc.turn_id);
        self.memory
            .refill(&scene, m, candidates.to_vec(), links, ranks.clone(), order);
    }
}
