//! The index gap, told (theseus-lu5n). When an index write fails past its bound
//! after its frames synced, the log goes on recording and refuses nothing; the
//! index lacks those frames, so no checkpoint claims past them and the next
//! open replays them from the last good checkpoint. The owner, 2026-11-09, on
//! keeping the daemon recording and telling him rather than stopping it: "Yes,
//! we never want invisibility." So the gap is in health (`ledger_gap`), in the
//! store's one warning, and here, in one post to the owner: the rule that
//! refused, the figure that tripped it, and what is still true. Once per gap,
//! as `rule_notice` speaks once per scope.
//!
//! The gap opens on the store's writer, which cannot post from where it
//! stands (a post is a frame, and the writer is what takes frames), so a
//! thread of its own looks once a second and holds the core by `Weak`, as
//! `keep_day_total` does.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::LedgerGap;

use crate::outbox::Outbox;
use crate::owner_note::{day, note, Kind};
use crate::rpc::Core;

/// The store's rule the post names: what it refuses, by what it does.
pub const RULE: &str = "no checkpoint past a failed index write";

/// How often the watcher looks: a gap is a long condition, and the owner is
/// told within a second of it.
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// The store's gap, as health carries it.
pub fn gap_of(g: theseus_store::IndexGap) -> LedgerGap {
    LedgerGap {
        since_ms: g.since_ms,
        batches_since: g.batches,
        reason: g.reason,
    }
}

/// The owner's post: the rule, when it tripped and why, the batches since,
/// that nothing was refused, and what the next open does.
pub fn notice_text(g: &LedgerGap) -> String {
    let l = crate::wake::local(g.since_ms);
    let n = g.batches_since;
    note(
        Kind::Alert,
        &format!("Rule \"{RULE}\" refused a checkpoint"),
        &format!(
            "An index write failed at {} {} after its frames synced, and {n} batch{} {} reached the log since: {}",
            day(g.since_ms),
            l.hm(),
            if n == 1 { "" } else { "es" },
            if n == 1 { "has" } else { "have" },
            g.reason,
        ),
        "The log is still recording, and no caller was refused: the frames are durable. The index \
         lacks them, so the checkpoint stays at the last good one, and `theseus ledger verify` can \
         report a removed row until the daemon restarts; the next open replays them from that \
         checkpoint. `theseus health` shows the gap while it stands. This tells you once.",
    )
}

/// What the watcher has told.
#[derive(Default)]
pub(crate) struct Told {
    /// The gap told, by when it opened: a gap is told once.
    since_ms: Option<u64>,
    /// A post that failed to write, said once in the log.
    failed: bool,
}

/// Tells the owner of `gap` unless `told` has. True when a post was written.
/// A post that cannot be written is logged once and tried again on the next
/// look, and never changes what the store does.
pub(crate) fn tell(outbox: &Outbox, told: &mut Told, gap: Option<&LedgerGap>) -> bool {
    let Some(g) = gap else { return false };
    if told.since_ms == Some(g.since_ms) {
        return false;
    }
    match outbox.to_operator(None, json!({"kind": "notice", "text": notice_text(g)})) {
        Ok(_) => {
            told.since_ms = Some(g.since_ms);
            true
        }
        Err(e) => {
            if !std::mem::replace(&mut told.failed, true) {
                tracing::warn!(error = %format!("{e:#}"), "ledger gap: the owner's notice was not written");
            }
            false
        }
    }
}

impl Core {
    /// The open gap, for health and the notice.
    pub(crate) fn ledger_gap(&self) -> Option<LedgerGap> {
        self.store.inner().ledger_gap().map(gap_of)
    }

    /// Tells the owner of a gap once, from a thread of its own.
    pub fn tell_ledger_gap(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let spawned = std::thread::Builder::new()
            .name("ledger-gap".into())
            .spawn(move || {
                let _ = theseus_store::pressure::idle_this_thread();
                let mut told = Told::default();
                loop {
                    std::thread::sleep(LOOK_EVERY);
                    let Some(core) = weak.upgrade() else { return };
                    tell(&core.outbox, &mut told, core.ledger_gap().as_ref());
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "ledger gap: the watcher was not started");
        }
    }
}

#[cfg(test)]
mod tests {
    use theseus_kernel::Kernel;

    use super::*;
    use crate::store::Store;

    fn outbox(dir: &tempfile::TempDir) -> (Store, Outbox) {
        let store = Store::open(&dir.path().join("store")).unwrap();
        let kernel = Arc::new(Kernel::new(
            store.shared(),
            Arc::new(theseus_kernel::RealClock),
            Default::default(),
        ));
        (store.clone(), Outbox::new(store, kernel))
    }

    fn gap(batches_since: u64) -> LedgerGap {
        LedgerGap {
            since_ms: 1_790_000_000_000,
            batches_since,
            reason: "the index write failed 3 times: disk full".into(),
        }
    }

    #[test]
    fn the_store_gap_is_health_as_it_stands() {
        let g = gap_of(theseus_store::IndexGap {
            position: 7,
            since_ms: 1_790_000_000_000,
            batches: 4,
            reason: "why".into(),
        });
        assert_eq!(
            (g.since_ms, g.batches_since, g.reason.as_str()),
            (1_790_000_000_000, 4, "why")
        );
    }

    #[test]
    fn the_notice_names_the_rule_the_batches_and_that_nothing_was_refused() {
        let t = notice_text(&gap(3));
        assert!(crate::owner_note::is_headed(&t), "{t}");
        assert!(t.contains(RULE), "{t}");
        assert!(t.contains("3 batches have reached the log"), "{t}");
        assert!(t.contains("disk full"), "{t}");
        assert!(t.contains("no caller was refused"), "{t}");
        assert!(t.contains("replays them from that checkpoint"), "{t}");
        assert!(notice_text(&gap(1)).contains("1 batch has reached the log"));
    }

    /// One post for a gap, however many looks see it, and none before one
    /// opens; a gap that opened later is a gap of its own.
    #[test]
    fn a_gap_is_told_once_however_often_it_is_seen() {
        let dir = tempfile::tempdir().unwrap();
        let (store, o) = outbox(&dir);
        let mut told = Told::default();
        assert!(!tell(&o, &mut told, None), "no gap, no post");
        let before = store.last_position();
        assert!(tell(&o, &mut told, Some(&gap(1))));
        let after = store.last_position();
        assert!(after > before, "the post is a frame");
        for batches in [2, 3, 50] {
            assert!(!tell(&o, &mut told, Some(&gap(batches))), "{batches}");
        }
        assert_eq!(store.last_position(), after, "no second post");
        let mut later = gap(1);
        later.since_ms += 1;
        assert!(tell(&o, &mut told, Some(&later)), "another gap is told");
    }
}
