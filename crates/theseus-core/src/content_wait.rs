//! Waiting for a session's outstanding content asks (theseus-9ymz0, design
//! §4.3; a counter since theseus-gyjyo.6).
//!
//! A turn bound for a model of restricted origin waits, at most
//! `[content_based_security] wait_ms`, for the asks still in flight for its
//! session to land, and then **goes either way**: it reads the session's
//! labels once more and is sent or kept off by them. Nothing is proved: an ask
//! that never lands leaves a session with the labels it has. The turn is never
//! blocked and never fails for want of a verdict, and a turn bound for Claude
//! never waits.
//!
//! What it waits on is a **number**, how many asks are out for the session
//! ([`Verdicts::outstanding`]), not a position in its transcript, so none of
//! the arithmetic of a mark comes with it. [`Handle::drained`] is the wait.

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use crate::content::ContentFlags;
#[cfg(test)]
use crate::node::Body;

/// What a turn waits on: the asks out for a session, and the next landing.
/// The judge service's per-session notify (`judge/content.rs`) stands behind
/// it; [`Ready`] is the stand-in until it exists.
pub trait Verdicts: Send + Sync {
    /// How many content asks are in flight for `session` now: queued or being
    /// judged, not yet landed.
    fn outstanding(&self, session: &str) -> usize;

    /// The next time the count may have moved. The future may end early or
    /// never end; the caller bounds it and reads the count again afterwards,
    /// so a wake-up that changed nothing costs one read.
    ///
    /// **A wake-up that comes after this call must end the future, even if it
    /// comes before the future's first poll** (a `Notify`'s `notified()` enabled
    /// at once, or a permit): the turn takes the future, reads the count, and only
    /// then waits, so a verdict that lands in between is not slept through.
    fn settled(&self, session: &str) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// The stand-in: nothing is being judged, so there is nothing to wait for.
#[derive(Debug, Default, Clone, Copy)]
pub struct Ready;

impl Verdicts for Ready {
    fn outstanding(&self, _session: &str) -> usize {
        0
    }

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

    fn get(&self) -> std::sync::Arc<dyn Verdicts> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// How many asks are out for `session`.
    pub fn outstanding(&self, session: &str) -> usize {
        self.get().outstanding(session)
    }

    /// Wait at most `max` for `session`'s outstanding asks to land; whether
    /// none is out now. Zero never waits. Each read of the count is taken
    /// **after** the wait is registered, so a landing between the two is not
    /// slept through, and no single sleep is longer than [`SLICE`], so a
    /// wake-up a stand-in lost costs a slice and not the wait.
    pub async fn drained(&self, session: &str, max: Duration) -> bool {
        let v = self.get();
        let deadline = Instant::now() + max;
        loop {
            if v.outstanding(session) == 0 {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let woken = v.settled(session);
            if v.outstanding(session) == 0 {
                return true;
            }
            let _ = tokio::time::timeout(left.min(SLICE), woken).await;
        }
    }
}

/// The longest one sleep of [`Handle::drained`] is.
const SLICE: Duration = Duration::from_millis(250);

/// The session's content labels as the store holds them, or none: a session
/// with no record, or with no labels, reads as none. One record read.
pub fn flags_of(store: &crate::store::Store, session: &str) -> Option<ContentFlags> {
    let rec = store
        .get_session::<crate::session::SessionRecord>(session)
        .ok()??;
    rec.data.map(|b| *b)
}

/// A judge for tests: it lands a verdict when a turn waits for one, as the
/// real judge's notify does between a turn's read and its wait. Every item it
/// has not read yet is read by `rule`, which names the flags its kind and text
/// raise; what it has read it remembers in memory, as the count of asks out
/// for a session. Attach it to a core's store, then stand it behind
/// `core.runner.verdicts`.
#[cfg(test)]
pub(crate) mod judging {
    use super::*;
    use crate::content::{item_text, Flag, ItemKind, Label};

    type Rule = Box<dyn Fn(ItemKind, &str) -> Vec<Flag> + Send + Sync>;

    pub(crate) struct Judging {
        store: std::sync::OnceLock<crate::store::Store>,
        rule: Rule,
        /// Set: judge only the nodes a hook site actually submitted
        /// (theseus-sh1n3).
        real: std::sync::OnceLock<std::sync::Arc<crate::judge::JudgeService>>,
        /// The nodes it has read, by id: what is no longer an ask out.
        read: std::sync::Mutex<std::collections::HashSet<String>>,
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
                read: Default::default(),
                waits: Default::default(),
            })
        }

        /// The store it judges in, once the core that owns it is built.
        pub fn attach(&self, store: &crate::store::Store) {
            let _ = self.store.set(store.clone());
        }

        /// Judge only what the hook sites submitted to `judge` (theseus-sh1n3).
        ///
        /// **Without this a test's judge is more generous than the running
        /// system**, because it judges straight from the transcript: it lands
        /// a verdict for a node nothing ever offered. Every test of the floor
        /// that does not ask for this is testing the floor, not the wiring.
        pub fn faithful(&self, judge: &std::sync::Arc<crate::judge::JudgeService>) {
            let _ = self.real.set(judge.clone());
        }

        /// The items of `session` it has not read: the asks a real judge would
        /// have out for them (of the nodes a hook site submitted, when
        /// `faithful`).
        fn unread(&self, session: &str) -> Vec<(String, ItemKind, String)> {
            let Some(store) = self.store.get() else {
                return vec![];
            };
            let Ok(nodes) = store.transcript(session) else {
                return vec![];
            };
            let submitted = self.real.get().map(|j| j.content_submitted());
            let read = self.read.lock().unwrap();
            nodes
                .iter()
                .filter(|(_, n)| item_text(n).is_some() && !read.contains(&n.id))
                .filter(|(_, n)| submitted.as_ref().is_none_or(|s| s.contains(&n.id)))
                .filter_map(|(_, n)| match &n.body {
                    Body::UserMessage { text, .. } => {
                        Some((n.id.clone(), ItemKind::OperatorMessage, text.clone()))
                    }
                    Body::ToolResult { content, .. } => {
                        Some((n.id.clone(), ItemKind::ToolResult, content.clone()))
                    }
                    _ => None,
                })
                .collect()
        }

        /// Judge `session`'s items now.
        pub fn land(&self, session: &str) {
            let Some(store) = self.store.get() else {
                return;
            };
            let items = self.unread(session);
            if items.is_empty() {
                return;
            }
            let _ = store.update_session(session, |rec| {
                let mut flags = rec.data.take().map(|b| *b).unwrap_or_default();
                for (id, kind, text) in &items {
                    for flag in (self.rule)(*kind, text) {
                        flags.raise(Label {
                            flag,
                            node: Some(id.clone()),
                            kind: *kind,
                            source: "a test's judge".into(),
                            confidence: 0.9,
                            band: flag.bar(),
                            at_ms: 1,
                            judgment: String::new(),
                        });
                    }
                }
                rec.data =
                    (flags != crate::content::ContentFlags::default()).then(|| Box::new(flags));
                Ok(vec![])
            });
            self.read
                .lock()
                .unwrap()
                .extend(items.into_iter().map(|(id, _, _)| id));
        }
    }

    /// A judge that never answers: a turn that waits on it waits its whole
    /// `[content_based_security] wait_ms`. It counts the waits, by session.
    #[derive(Default)]
    pub(crate) struct Silent {
        pub waits: std::sync::Mutex<Vec<String>>,
    }

    impl Verdicts for Silent {
        /// One ask that never lands.
        fn outstanding(&self, _session: &str) -> usize {
            1
        }

        fn settled(&self, session: &str) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            self.waits.lock().unwrap().push(session.to_string());
            Box::pin(std::future::pending())
        }
    }

    impl Verdicts for Judging {
        /// The items it has not read: the asks a real judge would have out.
        fn outstanding(&self, session: &str) -> usize {
            self.unread(session).len()
        }

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

    /// The stand-in answers at once.
    #[tokio::test]
    async fn the_stand_in_is_ready() {
        Ready.settled("ses_1").await;
    }

    /// A counter a test moves by hand: a stand-in for the judge's asks, with
    /// the wake-up the real one gives, or without it.
    struct Counted {
        out: std::sync::atomic::AtomicUsize,
        wake: std::sync::Arc<tokio::sync::Notify>,
    }

    impl Counted {
        fn with(n: usize) -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                out: n.into(),
                wake: Default::default(),
            })
        }

        fn set(&self, n: usize) {
            self.out.store(n, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl Verdicts for Counted {
        fn outstanding(&self, _session: &str) -> usize {
            self.out.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn settled(&self, _session: &str) -> Pin<Box<dyn Future<Output = ()> + Send>> {
            let wake = self.wake.clone();
            Box::pin(async move { wake.notified().await })
        }
    }

    /// With nothing outstanding the wait is not even begun, and `Ready` (the
    /// stand-in) never has anything outstanding.
    #[tokio::test]
    async fn a_session_with_no_ask_out_is_not_waited_for() {
        let handle = Handle::default();
        assert_eq!(handle.outstanding("ses_1"), 0);
        let t = Instant::now();
        assert!(handle.drained("ses_1", Duration::from_secs(30)).await);
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    /// An ask that never lands is waited for as long as `max` and no longer,
    /// and then the answer is `false`: the caller goes on its labels. Zero
    /// never waits and never asks the judge to hurry (no waiter is counted).
    #[tokio::test]
    async fn an_ask_that_never_lands_is_waited_for_max_and_no_longer() {
        let handle = Handle::default();
        let silent = std::sync::Arc::new(judging::Silent::default());
        handle.set(silent.clone());
        assert_eq!(handle.outstanding("ses_1"), 1);
        assert!(!handle.drained("ses_1", Duration::ZERO).await);
        assert!(silent.waits.lock().unwrap().is_empty(), "zero never waits");
        let t = Instant::now();
        assert!(!handle.drained("ses_1", Duration::from_millis(300)).await);
        let took = t.elapsed();
        assert!(took >= Duration::from_millis(300), "{took:?}");
        assert!(took < Duration::from_secs(10), "{took:?}");
        assert!(!silent.waits.lock().unwrap().is_empty());
    }

    /// The count coming down ends the wait early, with the wake-up or, if a
    /// stand-in loses it, within one slice rather than the whole `max`.
    #[tokio::test]
    async fn the_count_reaching_zero_ends_the_wait() {
        for woken in [true, false] {
            let handle = Handle::default();
            let counted = Counted::with(2);
            handle.set(counted.clone());
            let c = counted.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                c.set(0);
                if woken {
                    c.wake.notify_waiters();
                }
            });
            let t = Instant::now();
            assert!(handle.drained("ses_1", Duration::from_secs(60)).await);
            assert!(t.elapsed() < Duration::from_secs(5), "woken {woken}");
        }
    }
}
