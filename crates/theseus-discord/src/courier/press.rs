//! A press on a fold, at its place's lane (issue 56). The lane is the one
//! writer of a fold's message, for a press as for a live update: a press
//! asks the lane first, and only when the lane will write is the fold's new
//! state made, by the place, and written as any live state is, after
//! whatever the lane already holds of it. So a press and the turn's own
//! updates reach Discord in the place's order, and the place's fold and
//! Discord's message never disagree.
//!
//! The lane answers a press by what it knows: while it waits out Discord, it
//! refuses (the presser is told; nothing changes); the press is honoured only
//! on the message the lane wrote that turn's fold to, from its map or its
//! durable rows (`recall.rs`); a press on any other message (a card, another
//! turn's fold) is refused, and a turn it never folded is too old.

use super::Lane;

/// What a press came to, for its presser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Told {
    /// Shown as asked: nothing to say.
    Shown,
    /// The lane waits out Discord: nothing changed.
    Away,
    /// The pressed message is not the fold the press names.
    NotHere,
    /// No fold of that turn is known here.
    TooOld,
    /// The place shows neither its tool lines nor its thinking now.
    Hidden,
    /// No answer in time.
    Busy,
}

/// A press on the fold under `key`, for the lane to check before the place
/// shows it.
pub(crate) struct Press {
    pub key: String,
    /// Where it was pressed: the channel and the message.
    pub at: (u64, u64),
    /// Called once: `Ok` when the lane will write the fold (the place then
    /// makes its new state and sends it on), or why not.
    pub then: Box<dyn FnOnce(Result<(), Told>) + Send + Sync>,
}

/// A press the lane holds until its earlier writes are done.
pub(super) struct Pending(Box<Press>);

impl Pending {
    pub(super) fn of(p: Box<Press>) -> Self {
        Self(p)
    }

    fn refuse(self, why: Told) {
        (self.0.then)(Err(why));
    }
}

impl Lane {
    /// Whether the lane waits out Discord now.
    fn waits_out(&self) -> bool {
        self.retry_at
            .is_some_and(|at| tokio::time::Instant::now() < at)
    }

    /// A press come to the lane: refused at once while it waits out
    /// Discord; else kept, to be answered after the writes it holds.
    pub(super) fn pressed(&mut self, p: Pending) {
        if self.waits_out() {
            p.refuse(Told::Away);
        } else {
            self.presses.push(p);
        }
    }

    /// Discord went away: every press kept is refused, unchanged.
    pub(super) fn refuse_presses(&mut self) {
        for p in std::mem::take(&mut self.presses) {
            p.refuse(Told::Away);
        }
    }

    /// Each kept press, in order.
    pub(super) async fn presses(&mut self) {
        for p in std::mem::take(&mut self.presses) {
            if self.waits_out() {
                p.refuse(Told::Away);
                continue;
            }
            let Pending(p) = p;
            let r = self.fold_at(&p.key, p.at).await;
            (p.then)(r);
        }
    }

    /// Whether `at` is the message this lane wrote the fold `key` to.
    async fn fold_at(&mut self, key: &str, at: (u64, u64)) -> Result<(), Told> {
        let known = match self.msgs.get(key).copied() {
            Some(m) => Some(m),
            None => self.recall(key).await,
        };
        match known {
            None => Err(Told::TooOld),
            Some(m) if m == at => Ok(()),
            Some(_) => Err(Told::NotHere),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests_bound::{lane, upsert};
    use super::super::{LaneMsg, SendErr};
    use super::*;
    use crate::render::Op;

    /// While the lane waits out Discord, a fold's latest state is kept to be
    /// written after, as the board's is, where a loop's tool message is
    /// dropped; and a press is refused at once, and kept nowhere.
    #[tokio::test]
    async fn a_fold_waits_out_discord_and_a_press_meanwhile_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let fake = theseus_sim::fake_discord::FakeDiscord::start();
        let mut lane = lane(d.path(), &fake);
        lane.take(upsert("turn_a:fold", "-# 1 step: 1 read"));
        lane.take(upsert("turn_a:L0:tools", "-# ✅ `fs.read` a"));
        lane.away(&SendErr {
            away: true,
            unsure: false,
            gone: false,
            message: "away".into(),
        });
        let live = |l: &Lane| -> Vec<(String, String)> {
            l.live
                .iter()
                .filter_map(|o| match o {
                    Op::Upsert { key, content, .. } => Some((key.clone(), content.clone())),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            live(&lane),
            [("turn_a:fold".to_string(), "-# 1 step: 1 read".to_string())]
        );
        lane.take(upsert("turn_a:fold", "-# 2 steps: 2 reads"));
        lane.take(upsert("turn_a:L1:tools", "-# ✅ `fs.read` b"));
        assert_eq!(
            live(&lane),
            [("turn_a:fold".to_string(), "-# 2 steps: 2 reads".to_string())]
        );
        let (tx, rx) = tokio::sync::oneshot::channel();
        lane.take(LaneMsg::Press(Box::new(Press {
            key: "turn_a:fold".into(),
            at: (1, 2),
            then: Box::new(move |r| {
                let _ = tx.send(r);
            }),
        })));
        assert_eq!(rx.await.unwrap(), Err(Told::Away));
        assert!(lane.presses.is_empty());
    }
}
