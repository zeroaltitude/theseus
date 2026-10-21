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
//!   and, once it has paid, it still outranks the asking thread's own best
//!   hit of the query (`own_top`; `beats_own`: a note of another stream
//!   that is no better than what the thread already holds is not worth
//!   its room, whatever the question's shape shares with it). Else it is
//!   dropped as `thread`. Naming the thread (`waived`: its slug, its
//!   title, or one of its working set's entities) waives all of it.
//!   A candidate that holds concrete entities (a path, a ticket, a host)
//!   and shares none with the message (`needs_entity`; its [`Tie`] is
//!   `Apart`) is about something else and is dropped the same way: the
//!   question's shape alone is no reason to pull another stream's note.
//! - **Whose a node is.** A node's own thread (`of_node`: a range a split
//!   moved) outranks its session's. A node of a session no thread serves
//!   (the place's own conversation, where each stream's first messages
//!   run before its thread exists) is another stream's to a threaded asker
//!   when `base` is on: it pays the same distance and is never waived by a
//!   name. Every candidate of an unthreaded session's recall is as before.

use std::collections::{BTreeMap, BTreeSet};

/// The thread a session belongs to, as the core read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionThread {
    pub slug: String,
    /// Sealed, or a seal proposed and not yet answered.
    pub held: bool,
}

/// What a candidate's chunk shares with the message by concrete entity (the
/// index's exact terms: paths, files, tickets, commits, hosts, crates).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tie {
    /// The chunk holds entities and the message names one of them.
    Shared,
    /// The chunk holds entities and the message names none of them.
    Apart,
    /// The chunk holds none, or the index does not say: no evidence.
    #[default]
    Unknown,
}

impl Tie {
    /// From the chunk's entities and the message's that it holds.
    pub fn of(entities: &[String], matched: &[String]) -> Tie {
        match (entities.is_empty(), matched.is_empty()) {
            (_, false) => Tie::Shared,
            (false, true) => Tie::Apart,
            (true, true) => Tie::Unknown,
        }
    }
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
    /// The thread of each candidate node that has one of its own (a node a
    /// split moved to a thread): it outranks the node's session's.
    pub of_node: BTreeMap<String, SessionThread>,
    /// `[threads] base_distance`: a candidate no thread claims (by node or
    /// by session) is another stream's to a threaded asker.
    pub base: bool,
    /// The threads the message names: no distance for these.
    pub waived: BTreeSet<String>,
    /// The index's best fused score of the query: what a candidate's is
    /// weighed against.
    pub top_fused: f64,
    /// The best fused score among the candidates the asking thread owns:
    /// what another stream's hit must outrank once it has paid.
    pub own_top: f64,
    /// `[threads] cross_recall_beats_own`: whether another stream's hit
    /// must outrank `own_top` after its penalty.
    pub beats_own: bool,
    /// `[threads] cross_recall_needs_entity`: another stream's hit whose
    /// chunk holds entities that the message shares none of is dropped.
    pub needs_entity: bool,
    /// `[threads] cross_recall_penalty`: the share of its score another
    /// thread's candidate pays.
    pub penalty: f64,
    /// `[threads] cross_recall_bar`: the share of the best score a candidate
    /// of another thread must be above.
    pub bar: f64,
}

/// Which of the distance's rules dropped a candidate (theseus-98bz.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cause {
    /// Its share of the index's best is at or under `bar`.
    Bar,
    /// Once it has paid, it does not outrank the asking thread's own best
    /// hit (`beats_own`).
    Own,
    /// Its chunk holds entities the message names none of (`needs_entity`).
    Entity,
    /// No thread claims it: the place's own conversation (`base`). It wins
    /// over the other three.
    Base,
}

/// A drop by the distance: its rule and the numbers it weighed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distant {
    pub cause: Cause,
    /// The asking thread's own best hit (`ThreadLens::own_top`).
    pub own_top: f64,
    /// Its fused score's share of the index's best.
    pub share: f64,
    /// Its fused score once it has paid the penalty.
    pub paid: f64,
}

/// What the lens makes of one candidate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    /// Unchanged.
    Pass,
    /// A thread's own seal keeps it out.
    Sealed,
    /// Another stream's, and one of the distance's rules keeps it out.
    Distant(Distant),
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

    /// The thread of the candidate `node` of `session`: its own (a moved
    /// node), else its session's.
    pub fn thread_of_node(&self, session: &str, node: &str) -> Option<&SessionThread> {
        self.of_node
            .get(node)
            .or_else(|| self.of_session.get(session))
    }

    /// What the lens does to the candidate `node` of `session` with `fused`.
    pub fn judge(&self, session: &str, node: &str, fused: f64, tie: Tie) -> Verdict {
        let slug = match self.thread_of_node(session, node) {
            Some(t) => {
                if self.here.as_deref() == Some(t.slug.as_str()) {
                    return Verdict::Pass;
                }
                if t.held {
                    return Verdict::Sealed;
                }
                Some(t.slug.as_str())
            }
            // No thread claims it: the place's own conversation, another
            // stream's to a threaded asker when `base` is on.
            None if self.base && self.here.is_some() => None,
            None => return Verdict::Pass,
        };
        if !self.distance || slug.is_some_and(|s| self.waived.contains(s)) {
            return Verdict::Pass;
        }
        let share = if self.top_fused > 0.0 {
            fused / self.top_fused
        } else {
            1.0
        };
        let keep = (1.0 - self.penalty).clamp(0.0, 1.0);
        let paid = fused * keep;
        let cause = if self.needs_entity && tie == Tie::Apart {
            Some(Cause::Entity)
        } else if share <= self.bar {
            Some(Cause::Bar)
        } else if self.beats_own && paid <= self.own_top {
            Some(Cause::Own)
        } else {
            None
        };
        match cause {
            None => Verdict::Costs(keep),
            Some(c) => Verdict::Distant(Distant {
                cause: if slug.is_none() { Cause::Base } else { c },
                own_top: self.own_top,
                share,
                paid,
            }),
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
            ..ThreadLens::default()
        }
    }

    fn cause(v: Verdict) -> Option<Cause> {
        match v {
            Verdict::Distant(d) => Some(d.cause),
            _ => None,
        }
    }

    #[test]
    fn another_threads_candidate_pays_and_needs_the_bar() {
        let l = lens(Some("here"));
        assert_eq!(
            l.judge("ses_pr", "n", 0.04, Tie::Unknown),
            Verdict::Costs(0.65)
        );
        assert_eq!(
            l.judge("ses_pr", "n", 0.031, Tie::Unknown),
            Verdict::Costs(0.65)
        );
        assert_eq!(
            cause(l.judge("ses_pr", "n", 0.03, Tie::Unknown)),
            Some(Cause::Bar),
            "at the bar is not above it"
        );
        assert_eq!(
            cause(l.judge("ses_pr", "n", 0.01, Tie::Unknown)),
            Some(Cause::Bar)
        );
        assert_eq!(
            l.judge("ses_unthreaded", "n", 0.001, Tie::Unknown),
            Verdict::Pass
        );
    }

    #[test]
    fn naming_the_thread_waives_both() {
        let mut l = lens(Some("here"));
        l.waived.insert("pr-round".into());
        assert_eq!(l.judge("ses_pr", "n", 0.001, Tie::Unknown), Verdict::Pass);
    }

    #[test]
    fn an_unthreaded_session_pays_no_distance_but_a_seal_holds() {
        let l = lens(None);
        assert_eq!(l.judge("ses_pr", "n", 0.001, Tie::Unknown), Verdict::Pass);
        assert_eq!(l.judge("ses_fix", "n", 0.04, Tie::Unknown), Verdict::Sealed);
    }

    #[test]
    fn a_sealed_thread_reads_itself_and_no_one_else() {
        assert_eq!(
            lens(Some("fix")).judge("ses_fix", "n", 0.04, Tie::Unknown),
            Verdict::Pass
        );
        assert_eq!(
            lens(Some("pr-round")).judge("ses_fix", "n", 0.04, Tie::Unknown),
            Verdict::Sealed
        );
        let mut l = lens(Some("pr-round"));
        l.waived.insert("fix".into());
        assert_eq!(
            l.judge("ses_fix", "n", 0.04, Tie::Unknown),
            Verdict::Sealed,
            "naming never opens a seal"
        );
    }
    /// Each rule names itself, with the numbers it weighed (theseus-98bz.2):
    /// `bar`, `own`, `entity`, and `base`, which wins over the other three.
    #[test]
    fn each_rule_of_the_distance_says_it_dropped_the_candidate() {
        let mut l = lens(Some("here"));
        l.beats_own = true;
        l.needs_entity = true;
        l.own_top = 0.024;
        let at = |l: &ThreadLens, s: &str, f: f64, t: Tie| match l.judge(s, "n", f, t) {
            Verdict::Distant(d) => Some(d),
            _ => None,
        };
        let bar = at(&l, "ses_pr", 0.02, Tie::Unknown).unwrap();
        assert_eq!(bar.cause, Cause::Bar);
        assert!((bar.share - 0.5).abs() < 1e-9 && (bar.paid - 0.013).abs() < 1e-9);
        assert_eq!(bar.own_top, 0.024);
        // Above the bar (0.9 of the best), but 0.036 x 0.65 = 0.0234 is no better
        // than the thread's own 0.024.
        let own = at(&l, "ses_pr", 0.036, Tie::Unknown).unwrap();
        assert_eq!(own.cause, Cause::Own);
        assert!((own.paid - 0.0234).abs() < 1e-9, "{own:?}");
        assert_eq!(
            l.judge("ses_pr", "n", 0.04, Tie::Unknown),
            Verdict::Costs(0.65)
        );
        assert_eq!(
            at(&l, "ses_pr", 0.04, Tie::Apart).map(|d| d.cause),
            Some(Cause::Entity)
        );
        assert_eq!(
            l.judge("ses_pr", "n", 0.04, Tie::Shared),
            Verdict::Costs(0.65)
        );
        // The place's own conversation: base, whichever rule took it.
        l.base = true;
        for (f, t) in [
            (0.02, Tie::Unknown),
            (0.036, Tie::Unknown),
            (0.04, Tie::Apart),
        ] {
            assert_eq!(
                at(&l, "ses_home", f, t).map(|d| d.cause),
                Some(Cause::Base),
                "{f} {t:?}"
            );
        }
        assert_eq!(
            l.judge("ses_home", "n", 0.04, Tie::Shared),
            Verdict::Costs(0.65)
        );
    }
}
