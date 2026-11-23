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

/// KEY: a directory named like an install's tree, which the loop did not open, holds the one slot,
/// and the hold has lasted four hours: `health` and the morning notice say so, with the tree's name,
/// what it is (not the loop's) and no command: "look before removing it".
#[tokio::test]
async fn a_long_hold_on_the_slots_is_told_with_the_tree_and_what_it_is() {
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
        "install-72af5c3fd1ff: not opened by the loop: look before removing it",
    ] {
        assert!(line.contains(want), "{want:?} not in {line}");
    }
    for never in ["restart", "systemctl", "--force", "drop"] {
        assert!(!line.contains(never), "{never:?} in {line}");
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
    assert!(
        line.contains("join-trim: not opened by the loop: look before removing it"),
        "{line}"
    );
    assert!(!line.contains("--force"), "{line}");
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

/// A live install past three hours (the loop's own tree, opened by this daemon, its install under way) is said as
/// live and working, with no act at all.
#[tokio::test]
async fn a_live_install_past_three_hours_is_said_as_working_with_no_act() {
    let r = rig_with(1, 30.0);
    let place = &r.f.place;
    let merge = place
        .git_ok(&place.repo, &["rev-parse", "refs/remotes/origin/main"])
        .unwrap();
    let t = crate::rsi::join::git::detached(place, "install", &merge[..12], &merge).unwrap();
    *r.core.rsi.pipeline.installing.lock().unwrap() = Some(super::Installing {
        item: "tsk_sails".into(),
        title: "Trim the sails".into(),
        branch: "self/trim-the-sails".into(),
        merge,
        since_ms: theseus_protocol::now_unix_ms() - 4 * HOUR,
        unit: None,
        root_helper: false,
    });
    let now = theseus_protocol::now_unix_ms();
    held_at(
        &r,
        "self.pipeline.held",
        now - 4 * HOUR,
        json!({"hold": "slots"}),
    );
    let line = r.core.self_held_at(now + HOUR).expect("told");
    for want in [
        "started nothing for 5 h",
        "the loop is installing",
        "it is working",
        &format!("{}: the loop's install tree, opened by this daemon", t.slug),
        "still in use: it is working, nothing to do",
    ] {
        assert!(line.contains(want), "{want:?} not in {line}");
    }
    for never in [
        "restart the daemon",
        "systemctl",
        "--force",
        "drop",
        "look before removing",
    ] {
        assert!(!line.contains(never), "{never:?} in {line}");
    }
}
