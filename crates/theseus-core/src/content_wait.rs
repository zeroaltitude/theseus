//! Waiting for a session's items to be judged (theseus-9ymz0, design §4.3).
//!
//! A turn bound for a model of restricted origin may go only when every item in
//! its session has been judged (`ContentFlags::judged_through`), so it waits
//! for the verdict, at most `[content_based_security] wait_ms`, and goes to Claude when the
//! verdict does not come: the turn is never blocked and never fails for want of
//! one. A turn bound for Claude never waits.
//!
//! This module is the turn's side of that, and two things only:
//! - [`unjudged`], pure: whether the transcript holds an item past what was
//!   judged, which the chokepoint (`data_gate.rs`) and the turn's floor
//!   (`turn/data_step.rs`) both read;
//! - [`Verdicts`], the handle a turn waits on. The judge service's per-session
//!   notify (`judge/content.rs`) is what stands behind it once the pack's lane
//!   joins; [`Ready`] is the stand-in until then, and answers at once, so a
//!   turn that finds items unjudged goes to Claude without waiting.

use std::future::Future;
use std::pin::Pin;

use crate::content::ContentFlags;
#[cfg(test)]
use crate::node::Body;
use crate::stub::Stub;

/// What a turn waits on: the next time the judge may have moved a session's
/// `judged_through`. The future may end early or never end; the caller bounds
/// it with `[content_based_security] wait_ms` and reads the record again afterwards, so a
/// wake-up that judged nothing costs one read.
///
/// **A wake-up that comes after this call must end the future, even if it
/// comes before the future's first poll** (a `Notify`'s `notified()` enabled
/// at once, or a permit): the turn takes the future, reads the record, and only
/// then waits, so a verdict that lands in between is not slept through.
pub trait Verdicts: Send + Sync {
    fn settled(&self, session: &str) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// The stand-in: nothing is being judged, so there is nothing to wait for.
#[derive(Debug, Default, Clone, Copy)]
pub struct Ready;

impl Verdicts for Ready {
    fn settled(&self, _session: &str) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(std::future::ready(()))
    }
}

/// The runner's handle on the verdicts, set once the judge that makes them
/// exists, and [`Ready`] until then (and in every test that judges nothing).
pub struct Handle(std::sync::RwLock<std::sync::Arc<dyn Verdicts>>);

impl Default for Handle {
    fn default() -> Self {
        Self(std::sync::RwLock::new(std::sync::Arc::new(Ready)))
    }
}

impl Handle {
    /// Stand `v` behind the handle.
    pub fn set(&self, v: std::sync::Arc<dyn Verdicts>) {
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = v;
    }

    pub fn settled(&self, session: &str) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let v = self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        v.settled(session)
    }
}

/// The session's content labels as the store holds them, or none: a session
/// with no record, or with no labels, reads as none. One record read.
pub fn flags_of(store: &crate::store::Store, session: &str) -> Option<ContentFlags> {
    let rec = store
        .get_session::<crate::session::SessionRecord>(session)
        .ok()??;
    rec.data.map(|b| *b)
}

/// Whether `node` is an item: what the judge classifies when it enters a
/// session. An operator's message, a brief or a report, and a tool result; not
/// the acknowledgement of a tool whose answer is Theseus's own fixed string,
/// and not a line the harness wrote in its own words (the turn clock, the
/// progress line, the end note).
///
/// **It states nothing itself**: `content::item_text` is the one definition,
/// and the submitter is built on the same call (theseus-sh1n3). A second
/// statement of the rule here is how the two sets came apart, and how a clock
/// node or a `task.create` result could wedge a session off every model of
/// restricted origin for good.
pub(crate) fn is_item(node: &Stub) -> bool {
    crate::content::item_text(node).is_some()
}

/// Whether `nodes` (a session's transcript, in order) hold an item the judge
/// has not reached: any item after the node `flags.judged_through` names. With
/// nothing judged yet every item counts; a mark the transcript does not hold
/// (a forked or compacted-away node) counts as nothing judged, which fails
/// closed. An empty transcript has no item.
pub fn unjudged(nodes: &[(u64, Stub)], flags: Option<&ContentFlags>) -> bool {
    let through = flags
        .and_then(|f| f.judged_through.as_deref())
        .and_then(|id| nodes.iter().position(|(_, n)| n.id == id));
    let after = match through {
        Some(at) => &nodes[at + 1..],
        None => nodes,
    };
    after.iter().any(|(_, n)| is_item(n))
}

/// A judge for tests: it lands a verdict when a turn waits for one, as the
/// real judge's notify does between a turn's read and its wait. Every item
/// past the session's `judged_through` is read by `rule`, which names the
/// flags its kind and text raise, and the mark moves to the last node. Attach it to a
/// core's store, then stand it behind `core.runner.verdicts`.
#[cfg(test)]
pub(crate) mod judging {
    use super::*;
    use crate::content::{Flag, ItemKind, Label};

    type Rule = Box<dyn Fn(ItemKind, &str) -> Vec<Flag> + Send + Sync>;

    pub(crate) struct Judging {
        store: std::sync::OnceLock<crate::store::Store>,
        rule: Rule,
        /// Set: judge only the nodes a hook site actually submitted, and stop
        /// the mark at the first item nobody did (theseus-sh1n3).
        real: std::sync::OnceLock<std::sync::Arc<crate::judge::JudgeService>>,
        /// How many times a turn waited on it, by session.
        pub waits: std::sync::Mutex<Vec<String>>,
    }

    impl Judging {
        /// A judge that finds nothing in anything.
        pub fn clean() -> std::sync::Arc<Self> {
            Self::with(|_, _| Vec::new())
        }

        pub fn with(
            rule: impl Fn(ItemKind, &str) -> Vec<Flag> + Send + Sync + 'static,
        ) -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                store: Default::default(),
                rule: Box::new(rule),
                real: Default::default(),
                waits: Default::default(),
            })
        }

        /// The store it judges in, once the core that owns it is built.
        pub fn attach(&self, store: &crate::store::Store) {
            let _ = self.store.set(store.clone());
        }

        /// Judge only what the hook sites submitted to `judge`, and never
        /// carry the mark past an item none of them did (theseus-sh1n3).
        ///
        /// **Without this a test's judge is more generous than the running
        /// system**, because it judges straight from the transcript: it lands
        /// a verdict for a node nothing ever offered, and so cannot see the
        /// defect that took every cn session down — a node the floor counts
        /// and no site submits. Every test of the floor that does not ask for
        /// this is testing the floor, not the wiring.
        ///
        /// It is **still more generous than production**, in one way worth
        /// knowing: it moves the mark node by node, where the real landing
        /// moves it only over a span from the mark that one batch covered
        /// whole (`ContentFlags::judged_run`). So a test can pass under this
        /// that the real `move_mark` would leave stuck. Do not read a pass
        /// here as a claim about the mark's arithmetic.
        pub fn faithful(&self, judge: &std::sync::Arc<crate::judge::JudgeService>) {
            let _ = self.real.set(judge.clone());
        }

        /// Judge `session`'s items now.
        pub fn land(&self, session: &str) {
            let Some(store) = self.store.get() else {
                return;
            };
            let Ok(nodes) = store.transcript(session) else {
                return;
            };
            if nodes.is_empty() {
                return;
            }
            let submitted = self.real.get().map(|j| j.content_submitted());
            let _ = store.update_session(session, |rec| {
                let mut flags = rec.data.take().map(|b| *b).unwrap_or_default();
                let from = flags
                    .judged_through
                    .as_deref()
                    .and_then(|id| nodes.iter().position(|(_, n)| n.id == id))
                    .map_or(0, |at| at + 1);
                let mut mark = None;
                for (_, n) in &nodes[from..] {
                    // Faithful: the mark stops at the first item no hook site
                    // offered, as the real judge's does — it never hears of it.
                    if let Some(done) = &submitted {
                        if is_item(n) && !done.contains(&n.id) {
                            break;
                        }
                    }
                    mark = Some(n.id.clone());
                    let (kind, text) = match &n.body {
                        Body::UserMessage { text, .. } => (ItemKind::OperatorMessage, text.clone()),
                        Body::ToolResult { content, .. } => (ItemKind::ToolResult, content.clone()),
                        _ => continue,
                    };
                    for flag in (self.rule)(kind, &text) {
                        flags.raise(Label {
                            flag,
                            node: Some(n.id.clone()),
                            kind,
                            source: "a test's judge".into(),
                            confidence: 0.9,
                            band: flag.bar(),
                            at_ms: 1,
                        });
                    }
                }
                if let Some(mark) = mark {
                    flags.judged_through = Some(mark);
                }
                rec.data = Some(Box::new(flags));
                Ok(vec![])
            });
        }
    }

    /// A judge that never answers: a turn that waits on it waits its whole
    /// `[content_based_security] wait_ms`. It counts the waits, by session.
    #[derive(Default)]
    pub(crate) struct Silent {
        pub waits: std::sync::Mutex<Vec<String>>,
    }

    impl Verdicts for Silent {
        fn settled(&self, session: &str) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            self.waits.lock().unwrap().push(session.to_string());
            Box::pin(std::future::pending())
        }
    }

    impl Verdicts for Judging {
        fn settled(&self, session: &str) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            self.waits.lock().unwrap().push(session.to_string());
            self.land(session);
            Box::pin(std::future::ready(()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn stubs(nodes: Vec<Node>) -> Vec<(u64, Stub)> {
        nodes
            .into_iter()
            .enumerate()
            .map(|(i, n)| (i as u64 + 1, n.into()))
            .collect()
    }

    fn through(id: &str) -> ContentFlags {
        let mut f = ContentFlags::default();
        f.judged_through = Some(id.into());
        f
    }

    fn user(text: &str) -> Node {
        Node::user("ses_1", Some("trn_1"), "cli", text)
    }

    /// Items after the mark are unjudged, the mark's own and everything before
    /// it are judged, and a model's own words are never items.
    #[test]
    fn only_an_item_past_the_mark_is_unjudged() {
        let (a, b) = (user("one"), user("two"));
        let flags = through(&a.id);
        let nodes = stubs(vec![a.clone(), b.clone()]);
        assert!(unjudged(&nodes, Some(&flags)), "b is past the mark");
        let flags = through(&b.id);
        assert!(!unjudged(&nodes, Some(&flags)), "both are judged");
        let reply = Node::recall("ses_1", "trn_1", "rcl_1", "baseline", vec![]);
        let nodes = stubs(vec![a, reply]);
        let flags = through(&nodes[0].1.id);
        assert!(!unjudged(&nodes, Some(&flags)), "a recall note is no item");
    }

    /// Nothing judged means every item counts; a mark the transcript lacks
    /// is nothing judged; an empty transcript holds no item.
    #[test]
    fn nothing_judged_counts_every_item() {
        let nodes = stubs(vec![user("one")]);
        assert!(unjudged(&nodes, None));
        assert!(unjudged(&nodes, Some(&ContentFlags::default())));
        let lost = through("nod_gone");
        assert!(unjudged(&nodes, Some(&lost)), "fail closed");
        assert!(!unjudged(&[], None));
    }

    /// The stand-in answers at once.
    #[tokio::test]
    async fn the_stand_in_is_ready() {
        Ready.settled("ses_1").await;
    }

    /// **Blocker and submitter are the same set** (theseus-sh1n3): for every
    /// node kind, what [`is_item`] counts against a model of restricted origin
    /// is exactly what `ContentItem` builds to be judged. A node in the gap —
    /// counted and never built — can never be judged, so `judged_through`
    /// never reaches it and every request of that session to such a model is
    /// refused for the rest of the session's life, with no label to point at.
    ///
    /// The turn clock's node and a `task.create` result were both in that gap
    /// until this was written: the first made `theseus ask --deadline 840`
    /// refuse where the same ask without the flag answered.
    #[test]
    fn the_blocker_and_the_submitter_are_one_set() {
        use crate::content::ItemKind;
        use crate::judge::content::ContentItem;
        let clock = Node::relayed(
            "ses_1",
            Some("trn_1"),
            crate::node::Origin::Harness,
            crate::turn::clock_step::CLOCK_AUTHOR,
            "[The harness: 13 minutes left of this turn's 14.]",
        );
        let report = Node::relayed(
            "ses_1",
            Some("trn_1"),
            crate::node::Origin::Harness,
            "task:ab12",
            "the child's findings",
        );
        let nodes = vec![
            user("the owner's words"),
            user("   "),
            clock,
            report,
            Node::recall("ses_1", "trn_1", "rcl_1", "baseline", vec![]),
        ];
        for n in &nodes {
            let built = ContentItem::of_message(n, ItemKind::PersonMessage, "message")
                .or_else(|| ContentItem::of_result(n));
            let stub: Stub = n.clone().into();
            assert_eq!(
                is_item(&stub),
                built.is_some(),
                "{} ({:?}): blocked {} but built {}",
                n.id,
                n.author,
                is_item(&stub),
                built.is_some(),
            );
        }
    }
}
