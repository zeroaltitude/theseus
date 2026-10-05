//! Recall's facts (M6 steps 30a, 30b): a turn's recall in shadow, as its
//! `recall.shadow` row (the manifest, scoped `recall:<session>` so
//! `memory.recalls` reads a session's alone), its `recall` span under the
//! loop with the index's stages inside it, and its narrative line; the same
//! for a recall in front of the model, as `recall.ran`; a session's sticky
//! arm (`memory.arm`, scoped with its recalls); and an operator's label
//! (`memory.label`, scoped `memory`). No notification: the rows and the
//! lines are how a surface sees them.

use serde_json::{json, Value};
use theseus_protocol::memory::RecallManifest;
use theseus_protocol::{LedgerKind, NarrativePart::Context, Span};

use super::{Fact, Say};
use crate::narrative;
use crate::trace::Trace;

/// The scope a session's recall rows are kept under.
pub fn scope(session_id: &str) -> String {
    format!("recall:{session_id}")
}

/// A turn's recall, in shadow: what would have been admitted, and why each
/// other candidate was dropped. Nothing of it reached the model.
pub struct RecallShadow<'a> {
    pub manifest: &'a RecallManifest,
    /// When the recall began and ended, on the turn's clock.
    pub t0: u64,
    pub t1: u64,
}

impl Fact for RecallShadow<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::RecallShadow);

    fn row(&self) -> Value {
        serde_json::to_value(self.manifest).unwrap_or(Value::Null)
    }

    fn span(&self, trace: &mut Trace) {
        span(self.manifest, self.t0, self.t1, trace);
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Context, words(self.manifest));
    }
}

/// A turn's recall in front of the model (30b, canary or live): what it
/// admitted, the `Recall` node that carries it, and why each other
/// candidate was dropped.
pub struct RecallRan<'a> {
    pub manifest: &'a RecallManifest,
    pub t0: u64,
    pub t1: u64,
}

impl Fact for RecallRan<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::RecallRan);

    fn row(&self) -> Value {
        serde_json::to_value(self.manifest).unwrap_or(Value::Null)
    }

    fn span(&self, trace: &mut Trace) {
        span(self.manifest, self.t0, self.t1, trace);
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Context, words(self.manifest));
    }
}

/// A session's arm, assigned once (30b; §2.9's canary): sticky, from a
/// hash of the session and the experiment, so M5's ladder can take it over.
pub struct ArmAssigned<'a> {
    pub mode: &'a str,
    pub arm: &'a str,
    pub live: bool,
    pub experiment: &'a str,
    pub science: &'a str,
}

impl Fact for ArmAssigned<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::MemoryArm);

    fn row(&self) -> Value {
        json!({"mode": self.mode, "arm": self.arm, "live": self.live,
               "experiment": self.experiment, "science": self.science,
               "assignment": "hash"})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let how = if self.live {
            "recall in front of the model"
        } else {
            "the control: no recall in front of the model, baseline in shadow"
        };
        say.line(
            Context,
            format!(
                "Memory ({}) put this session on arm {} of {}: {how}.",
                self.mode, self.arm, self.experiment
            ),
        );
    }
}

/// An operator's label on a node (30b, `memory.label`).
pub struct Labeled<'a> {
    pub node_id: &'a str,
    pub label: &'a str,
    pub recall_id: Option<&'a str>,
    pub note: Option<&'a str>,
    pub who: &'a str,
    pub via: &'a str,
    pub excluded: bool,
}

impl Fact for Labeled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::MemoryLabel);

    fn row(&self) -> Value {
        json!({"node_id": self.node_id, "label": self.label, "recall_id": self.recall_id,
               "note": self.note, "who": self.who, "via": self.via, "excluded": self.excluded})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let then = if self.excluded {
            "recall leaves it out from now on"
        } else {
            "recall may offer it"
        };
        say.line(
            Context,
            format!(
                "{} labeled {} {} through {}: {then}.",
                self.who, self.node_id, self.label, self.via
            ),
        );
    }
}

/// A recall's span under the loop, with the index's stages inside it.
fn span(m: &RecallManifest, t0: u64, t1: u64, trace: &mut Trace) {
    let mut children = Vec::new();
    if let Some(ix) = &m.timings.index {
        // The index's stages, laid end to end from the ask: it reports
        // their lengths, not their starts.
        let mut at = t0;
        for (name, ms) in [
            ("bm25", ix.bm25_ms),
            ("entity", ix.entity_ms),
            ("embed", ix.embed_ms),
            ("scan", ix.vector_ms),
            ("fuse", ix.fuse_ms),
        ] {
            if ms <= 0.0 {
                continue;
            }
            let us = (ms * 1000.0) as u64;
            children.push(Span {
                name: format!("recall.{name}"),
                kind: "recall".into(),
                start_us: at,
                end_us: Some(at + us),
                attrs: Value::Null,
                children: Vec::new(),
            });
            at += us;
        }
    }
    trace.push(Span {
        name: "recall".into(),
        kind: "recall".into(),
        start_us: t0,
        end_us: Some(t1),
        attrs: json!({
            "recall_id": m.recall_id, "mode": m.mode, "science": m.science, "arm": m.arm,
            "outcome": m.outcome, "candidates": m.candidates,
            "admitted": m.admitted.len(), "tokens": m.used_tokens, "drops": m.drops,
            "index_ms": m.timings.index_ms, "deadline_ms": m.timings.deadline_ms,
        }),
        children,
    });
}

/// The narrative's line: "Recall (shadow) found 12 candidates in 34 ms
/// (bm25 8, entity 3, vector 9) and would admit 3 (1,140 tokens) from 2
/// sessions; dropped 2 for the budget, 1 for its place." Or why it found
/// nothing.
pub fn words(m: &RecallManifest) -> String {
    let ms = m.timings.index_ms.round() as u64;
    if m.outcome == "deadline" {
        let deadline = m.timings.deadline_ms;
        return format!(
            "Recall ({}) had no answer from the index within {deadline} ms; the turn went on without it.",
            m.mode
        );
    }
    if m.outcome == "paused" {
        return format!(
            "Recall ({}) paused: the session's recall notes reached their cap; it resumes at the \
             next recompile.",
            m.mode
        );
    }
    if m.outcome == "unavailable" {
        let why = m.why.as_deref().unwrap_or("it is down");
        return format!("Recall ({}) found no index to ask: {why}.", m.mode);
    }
    if m.outcome == "detoured" {
        return format!(
            "Recall ({}) admitted {} ({}), but the turn took a trivial detour, whose request \
             carries no recall: none reached the model.",
            m.mode,
            narrative::count(m.admitted.len() as u64, "note", "notes"),
            narrative::count(m.used_tokens, "token", "tokens"),
        );
    }
    let sources = if m.sources.is_empty() {
        String::new()
    } else {
        let s: Vec<String> = m.sources.iter().map(|(k, n)| format!("{k} {n}")).collect();
        format!(" ({})", s.join(", "))
    };
    let sessions = m
        .admitted
        .iter()
        .map(|a| a.session_id.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u64;
    let admit = if m.mode == "shadow" || m.mode == "search" {
        "would admit"
    } else {
        "admitted"
    };
    let mut line = format!(
        "Recall ({}) found {} in {ms} ms{sources} and {admit} {} ({}) from {}",
        m.mode,
        narrative::count(m.candidates, "candidate", "candidates"),
        m.admitted.len(),
        narrative::count(m.used_tokens, "token", "tokens"),
        narrative::count(sessions, "session", "sessions"),
    );
    if !m.drops.is_empty() {
        let d: Vec<String> = m
            .drops
            .iter()
            .map(|(reason, n)| format!("{n} {}", reason_words(reason)))
            .collect();
        line.push_str(&format!("; dropped {}", d.join(", ")));
    }
    line.push('.');
    line
}

fn reason_words(reason: &str) -> &'static str {
    match reason {
        "place" => "for its place",
        "in_context" => "already in context",
        "untrusted" => "as external text",
        "labeled_wrong" => "as labeled wrong",
        "recursion" => "as a harness line",
        "superseded" => "as corrected by a newer note",
        "duplicate" => "as a newer note's duplicate",
        "threshold" => "below the threshold",
        "budget" => "for the budget",
        _ => "for another reason",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use theseus_protocol::memory::{RecallItem, RecallTimings};

    #[test]
    fn the_line_says_what_recall_found_and_dropped() {
        let item = |s: &str| RecallItem {
            session_id: s.into(),
            ..RecallItem::default()
        };
        let m = RecallManifest {
            mode: "shadow".into(),
            outcome: "ran".into(),
            candidates: 12,
            sources: BTreeMap::from([("bm25".into(), 8), ("entity".into(), 3)]),
            admitted: vec![item("a"), item("b"), item("a")],
            drops: BTreeMap::from([("budget".into(), 2), ("place".into(), 1)]),
            used_tokens: 1140,
            timings: RecallTimings {
                index_ms: 34.2,
                ..RecallTimings::default()
            },
            ..RecallManifest::default()
        };
        assert_eq!(
            words(&m),
            "Recall (shadow) found 12 candidates in 34 ms (bm25 8, entity 3) and would admit 3 \
             (1,140 tokens) from 2 sessions; dropped 2 for the budget, 1 for its place."
        );
        let late = RecallManifest {
            outcome: "deadline".into(),
            timings: RecallTimings {
                deadline_ms: 250,
                ..RecallTimings::default()
            },
            ..m
        };
        assert!(words(&late).contains("within 250 ms; the turn went on"));
        // A trivial detour's (theseus-n7nc): admitted, and sent nowhere.
        let detoured = RecallManifest {
            mode: "live".into(),
            outcome: "detoured".into(),
            ..late
        };
        assert_eq!(
            words(&detoured),
            "Recall (live) admitted 3 notes (1,140 tokens), but the turn took a trivial detour, \
             whose request carries no recall: none reached the model."
        );
    }
}
