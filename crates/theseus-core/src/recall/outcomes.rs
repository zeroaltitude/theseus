//! Recall's recent outcomes, for health's memory block (theseus-w9qv): how
//! the last [`WINDOW`] turns' recalls went, and when a whole answer last
//! arrived. In memory only, since the start: the `recall.shadow` and
//! `recall.ran` rows are the record, and this is what health reads at once,
//! so a recall that silently answers nothing shows. Each is counted twice:
//! by its outcome, and by what the index's answer held (vectors, the words
//! alone, or nothing: theseus-y9a3), since a `ran` whose vector source was
//! skipped (the model loading) carried no vectors either.

use std::collections::VecDeque;

use theseus_protocol::memory::{answered, RecallManifest, RecallOutcomes};

/// The newest recalls counted.
pub const WINDOW: usize = 50;

/// What the index made of one recall's query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ok,
    WordsOnly,
    Deadline,
    Error,
}

#[derive(Debug, Default)]
pub struct Outcomes {
    /// Each recall's outcome, and what its answer held
    /// (`RecallManifest::answered`).
    last: VecDeque<(Kind, &'static str)>,
    last_full_ms: Option<u64>,
}

impl Outcomes {
    /// A turn's recall, as its manifest says once the index's answer is
    /// read (before a pause or a detour, which say nothing of the index).
    pub fn note(&mut self, m: &RecallManifest, now_ms: u64) {
        let kind = match m.outcome.as_str() {
            "ran" => Kind::Ok,
            super::WORDS_ONLY => Kind::WordsOnly,
            "deadline" => Kind::Deadline,
            "unavailable" => Kind::Error,
            _ => return,
        };
        if kind == Kind::Ok && m.skipped.is_empty() {
            self.last_full_ms = Some(now_ms);
        }
        if self.last.len() == WINDOW {
            self.last.pop_front();
        }
        self.last.push_back((kind, m.answered()));
    }

    /// Health's counts; none before the first recall.
    pub fn health(&self) -> Option<RecallOutcomes> {
        if self.last.is_empty() {
            return None;
        }
        let n = |k: Kind| self.last.iter().filter(|(x, _)| *x == k).count() as u64;
        let held = |a: &str| self.last.iter().filter(|(_, x)| *x == a).count() as u64;
        Some(RecallOutcomes {
            window: WINDOW as u64,
            turns: self.last.len() as u64,
            ok: n(Kind::Ok),
            words_only: n(Kind::WordsOnly),
            deadline: n(Kind::Deadline),
            error: n(Kind::Error),
            with_vectors: held(answered::VECTORS),
            words_alone: held(answered::WORDS),
            empty: held(answered::EMPTY),
            last_full_ms: self.last_full_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn m(outcome: &str, skipped: &[&str]) -> RecallManifest {
        RecallManifest {
            outcome: outcome.into(),
            skipped: skipped
                .iter()
                .map(|s| (s.to_string(), "why".to_string()))
                .collect::<BTreeMap<_, _>>(),
            ..RecallManifest::default()
        }
    }

    /// `m`, with `hits` from each of `sources`.
    fn held(outcome: &str, skipped: &[&str], sources: &[(&str, u64)]) -> RecallManifest {
        RecallManifest {
            candidates: sources.iter().map(|(_, n)| n).max().copied().unwrap_or(0),
            sources: sources.iter().map(|(s, n)| (s.to_string(), *n)).collect(),
            ..m(outcome, skipped)
        }
    }

    #[test]
    fn health_counts_the_newest_recalls_by_outcome() {
        let mut o = Outcomes::default();
        assert_eq!(o.health(), None, "nothing before the first recall");
        o.note(&held("ran", &[], &[("bm25", 30), ("vector", 40)]), 1_000);
        o.note(&held("words_only", &["vector"], &[("bm25", 40)]), 2_000);
        o.note(&m("deadline", &[]), 3_000);
        o.note(&m("unavailable", &[]), 4_000);
        // A recall whose answer skipped a source is no whole answer, and
        // held the words alone (the model loading).
        o.note(&held("ran", &["vector"], &[("bm25", 40)]), 5_000);
        // Not the index's outcome: a pause is decided after it.
        o.note(&m("paused", &[]), 6_000);
        assert_eq!(
            o.health(),
            Some(RecallOutcomes {
                window: WINDOW as u64,
                turns: 5,
                ok: 2,
                words_only: 1,
                deadline: 1,
                error: 1,
                with_vectors: 1,
                words_alone: 2,
                empty: 2,
                last_full_ms: Some(1_000),
            })
        );
        for i in 0..WINDOW as u64 {
            o.note(&m("deadline", &[]), 10_000 + i);
        }
        let h = o.health().unwrap();
        assert_eq!(
            (h.turns, h.ok, h.deadline),
            (WINDOW as u64, 0, WINDOW as u64)
        );
        assert_eq!(
            (h.with_vectors, h.words_alone, h.empty),
            (0, 0, WINDOW as u64),
            "the window's answers held nothing"
        );
        assert_eq!(h.last_full_ms, Some(1_000), "kept past the window");
    }
}
