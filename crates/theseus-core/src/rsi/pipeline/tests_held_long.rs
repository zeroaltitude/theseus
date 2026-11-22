//! The owner is told when the loop cannot start anything (issue 41, `held_long`): a hold on the slots
//! past `TELL_AFTER` is in the morning notice and in `health` (the Ops page), with the tree and the
//! one act that clears it; a hold that is new, or that a start has ended, is told nowhere.

use serde_json::json;

use super::held_long::TELL_AFTER;
use super::tests::rig_with;
use crate::ledger::LedgerRow;

const HOUR: u64 = 3_600_000;

fn held_at(r: &super::tests::Rig, kind: &str, at: u64, data: serde_json::Value) {
    let mut row = LedgerRow::named(kind, None, None, data);
    row.at_unix_ms = at;
    r.core.store.append_ledger(&row).unwrap();
}

/// KEY: an install's tree holds the one slot, and the hold has lasted four hours: `health` and the
/// morning notice say so, with the tree's name and the one act that clears it.
#[tokio::test]
async fn a_long_hold_on_the_slots_is_told_with_the_tree_and_its_act() {
    let r = rig_with(1, 30.0);
    std::fs::create_dir_all(r.f.place.trees().join("install-72af5c3fd1ff")).unwrap();
    let now = theseus_protocol::now_unix_ms();
    held_at(
        &r,
        "self.pipeline.held",
        now - 4 * HOUR,
        json!({"hold": "slots"}),
    );
    let line = r.core.health().self_held.expect("told in health");
    for want in [
        "started nothing for 4 h",
        "install-72af5c3fd1ff",
        "restart the daemon",
    ] {
        assert!(line.contains(want), "{want:?} not in {line}");
    }
    let notice = r
        .core
        .self_notice(now - 24 * HOUR, now)
        .unwrap()
        .expect("a notice");
    assert!(notice.contains("Waits for you"), "{notice}");
    assert!(notice.contains(&line), "{notice}");
}

/// A new hold, one a start ended, and a slot no tree takes are told nowhere.
#[tokio::test]
async fn a_new_or_ended_hold_is_told_nowhere() {
    let r = rig_with(1, 30.0);
    let now = theseus_protocol::now_unix_ms();
    let trees = r.f.place.trees();
    held_at(
        &r,
        "self.pipeline.held",
        now - 4 * HOUR,
        json!({"hold": "slots"}),
    );
    assert_eq!(r.core.self_held_at(now), None, "no tree takes the slot");
    std::fs::create_dir_all(trees.join("join-trim")).unwrap();
    let line = r.core.self_held_at(now).expect("the merge's tree holds it");
    assert!(line.contains("worktree remove --force"), "{line}");
    assert!(line.contains("join-trim"), "{line}");
    let since = now - 4 * HOUR;
    let just = since + TELL_AFTER.as_millis() as u64 - 1;
    assert_eq!(r.core.self_held_at(just), None, "not yet");
    held_at(
        &r,
        "self.pipeline.started",
        now - HOUR,
        json!({"item": "tsk_a"}),
    );
    assert_eq!(r.core.self_held_at(now), None, "a start ended it");
    assert_eq!(r.core.health().self_held, None);
}
