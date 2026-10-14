//! The thread distance (theseus-f2g0; the owner, 2026-10-09: separable
//! streams that do not burden or contaminate each other). Pure: the core
//! reads which thread each candidate's session belongs to and builds a
//! [`ThreadLens`]; this decides what the lens does to the pipeline.
//!
//! - **Sealed** (the owner seals; Jev only recommends): a candidate of a
//!   sealed thread, or of one whose seal is proposed and not yet answered
//!   (`held`), is dropped as `sealed`, whoever asks, unless the asking turn
//!   is in that very thread. A tightening only: the filter never admits.
//! - **The distance**: in a threaded session, a candidate of *another*
//!   thread pays `penalty` of its score and is admitted only when its score,
//!   against the index's best hit of the query (`top_fused`), is above `bar`
//!   (else it is dropped as `thread`), unless the message names that thread
//!   (`waived`: its slug, its title, or one of its working set's entities),
//!   which waives both. An unthreaded candidate, and every candidate of an
//!   unthreaded session's recall, is as before.

use std::collections::{BTreeMap, BTreeSet};

/// The thread a session belongs to, as the core read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionThread {
    pub slug: String,
    /// Sealed, or a seal proposed and not yet answered.
    pub held: bool,
}

/// What the threads do to one recall.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadLens {
    /// The asking turn's own thread; none for an unthreaded session.
    pub here: Option<String>,
    /// Whether another thread's candidate pays the distance: an ambient
    /// recall in a threaded session. A model's own lookup, and an unthreaded
    /// session's recall, ask with it off; the seal still holds.
    pub distance: bool,
    /// The thread of each candidate session that has one.
    pub of_session: BTreeMap<String, SessionThread>,
    /// The threads the message names: no distance for these.
    pub waived: BTreeSet<String>,
    /// The index's best fused score of the query: what a candidate's is
    /// weighed against.
    pub top_fused: f64,
    /// `[threads] cross_recall_penalty`: the share of its score another
    /// thread's candidate pays.
    pub penalty: f64,
    /// `[threads] cross_recall_bar`: the share of the best score a candidate
    /// of another thread must be above.
    pub bar: f64,
}

/// What the lens makes of one candidate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    /// Unchanged.
    Pass,
    /// A thread's own seal keeps it out.
    Sealed,
    /// Another thread's, at or under the bar.
    Distant,
    /// Another thread's, above the bar: its score is multiplied by this.
    Costs(f64),
}

impl ThreadLens {
    /// The thread of `session`, when it has one.
    pub fn thread_of(&self, session: &str) -> Option<&SessionThread> {
        self.of_session.get(session)
    }

    /// Whether the lens changes nothing: no thread is known.
    pub fn is_empty(&self) -> bool {
        self.of_session.is_empty() && self.here.is_none()
    }

    /// What the lens does to a candidate of `session` with `fused`.
    pub fn judge(&self, session: &str, fused: f64) -> Verdict {
        let Some(t) = self.of_session.get(session) else {
            return Verdict::Pass;
        };
        if self.here.as_deref() == Some(t.slug.as_str()) {
            return Verdict::Pass;
        }
        if t.held {
            return Verdict::Sealed;
        }
        if !self.distance || self.waived.contains(&t.slug) {
            return Verdict::Pass;
        }
        let share = if self.top_fused > 0.0 {
            fused / self.top_fused
        } else {
            1.0
        };
        if share > self.bar {
            Verdict::Costs((1.0 - self.penalty).clamp(0.0, 1.0))
        } else {
            Verdict::Distant
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lens(here: Option<&str>) -> ThreadLens {
        ThreadLens {
            here: here.map(str::to_string),
            distance: here.is_some(),
            of_session: BTreeMap::from([
                (
                    "ses_pr".into(),
                    SessionThread {
                        slug: "pr-round".into(),
                        held: false,
                    },
                ),
                (
                    "ses_fix".into(),
                    SessionThread {
                        slug: "fix".into(),
                        held: true,
                    },
                ),
            ]),
            waived: BTreeSet::new(),
            top_fused: 0.04,
            penalty: 0.35,
            bar: 0.75,
        }
    }

    #[test]
    fn another_threads_candidate_pays_and_needs_the_bar() {
        let l = lens(Some("here"));
        assert_eq!(l.judge("ses_pr", 0.04), Verdict::Costs(0.65));
        assert_eq!(l.judge("ses_pr", 0.031), Verdict::Costs(0.65));
        assert_eq!(
            l.judge("ses_pr", 0.03),
            Verdict::Distant,
            "at the bar is not above it"
        );
        assert_eq!(l.judge("ses_pr", 0.01), Verdict::Distant);
        assert_eq!(l.judge("ses_unthreaded", 0.001), Verdict::Pass);
    }

    #[test]
    fn naming_the_thread_waives_both() {
        let mut l = lens(Some("here"));
        l.waived.insert("pr-round".into());
        assert_eq!(l.judge("ses_pr", 0.001), Verdict::Pass);
    }

    #[test]
    fn an_unthreaded_session_pays_no_distance_but_a_seal_holds() {
        let l = lens(None);
        assert_eq!(l.judge("ses_pr", 0.001), Verdict::Pass);
        assert_eq!(l.judge("ses_fix", 0.04), Verdict::Sealed);
    }

    #[test]
    fn a_sealed_thread_reads_itself_and_no_one_else() {
        assert_eq!(lens(Some("fix")).judge("ses_fix", 0.04), Verdict::Pass);
        assert_eq!(
            lens(Some("pr-round")).judge("ses_fix", 0.04),
            Verdict::Sealed
        );
        let mut l = lens(Some("pr-round"));
        l.waived.insert("fix".into());
        assert_eq!(
            l.judge("ses_fix", 0.04),
            Verdict::Sealed,
            "naming never opens a seal"
        );
    }
}
