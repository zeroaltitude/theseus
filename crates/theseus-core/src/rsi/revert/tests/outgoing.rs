//! A revert's push is cleared before it leaves (round 4 of issue 53; `Core::revert_cleared`): one whose tree is a
//! tree main already had sends nothing new and is cleared without reading what any session read; one whose tree is
//! new (main moved on after the join) is cleared as a join is, and a run of the owner's words it would send is
//! refused before main is pushed, held by the revert's own stop with its card.

use std::sync::atomic::Ordering;

use super::*;

/// Words of the owner's in another conversation: thirteen.
const SAID: &str = "kelp oars drift past the third bollard where the heron waits each grey morning";

/// A session of item `item`'s build that looked back and was shown [`SAID`], named by a row of its build alone.
fn read_by_its_build(r: &Rig, item: &str) {
    use crate::node::{Body, Node, ResultStatus};
    let s = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    r.core.store.put_session(&s.session_id, &s).unwrap();
    let turn = crate::new_id("trn");
    let shown = Node::tool_result(
        &s.session_id,
        Some(&turn),
        Some(0),
        Body::ToolResult {
            tool_use_id: "tu_look".into(),
            tool: crate::view::history::HISTORY_TOOL.into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: format!("An opened exchange:\n> {SAID}"),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: Some(5),
            late: false,
            meta: serde_json::json!({}),
            image: None,
            external: None,
            payload: None,
            parts: Vec::new(),
        },
    );
    let row = crate::fact::row(
        &crate::fact::rsi_pr::SelfBranchBuilt {
            item,
            issue: "theseus-mast1",
            branch: &format!("self/20261014-{item}"),
            head: Some("abc"),
            gate: "green",
            report: None,
            cost_usd: 0.0,
            stopped_at: None,
            why: "trim",
            numbers: serde_json::json!({}),
        },
        Some(&s.session_id),
        None,
    )
    .unwrap();
    r.core
        .store
        .append(&[shown.record().unwrap(), row])
        .unwrap();
}

/// A revert of main's newest merge: its tree is the tree main had before it, so it is cleared by the scrub alone,
/// with no read of what the item's sessions read, though one of them read the owner's words.
#[tokio::test]
async fn a_pure_revert_is_cleared_by_its_tree_alone() {
    let r = rig(SelfMode::Act);
    let good = r.land("mend-sails", &[("sails.txt", "mended\n")]);
    let bad = r.land("trim-mast", &[("broken.txt", "oops\n")]);
    r.joined("trim-mast", &bad);
    read_by_its_build(&r, "trim-mast");
    r.red("gate", &bad);
    let before = crate::rsi::outgoing::read::tests_hooks::READS.load(Ordering::SeqCst);
    let pass = r.pass().await;
    assert_eq!(pass.reverted.len(), 1, "{pass:?}");
    let now = r.upstream_main();
    let tree = |c: &str| git(&r.f.upstream, &["rev-parse", &format!("{c}^{{tree}}")]);
    assert_eq!(
        tree(&now),
        tree(&good),
        "the revert's tree is main's before the join"
    );
    let read = crate::rsi::outgoing::read::tests_hooks::READS.load(Ordering::SeqCst) - before;
    assert_eq!(read, 0, "no session's reads were gathered");
}

/// Main moved on after the join, so the revert's tree is new: it is cleared as a join is. The merge it undoes had
/// replaced a file holding the owner's words, which the revert would put back, and the item's builder had read
/// them: refused before main is pushed, held with the revert's card, which holds none of them.
#[tokio::test]
async fn a_revert_with_a_new_tree_is_checked_and_refused_before_main_is_pushed() {
    let r = rig(SelfMode::Act);
    r.land("notes", &[("notes.txt", &format!("{SAID}\n"))]);
    let bad = r.land(
        "trim-mast",
        &[("broken.txt", "oops\n"), ("notes.txt", "tide tables\n")],
    );
    r.joined("trim-mast", &bad);
    read_by_its_build(&r, "trim-mast");
    let later = r.land("later", &[("later.txt", "later\n")]);
    r.red("gate", &bad);
    let pass = r.pass().await;
    assert!(pass.reverted.is_empty(), "{pass:?}");
    assert_eq!(r.upstream_main(), later, "main was not pushed");
    let stops = r.rows("self.join.stopped");
    let held = stops
        .iter()
        .find(|s| s["phase"] == "push")
        .expect("the revert's stop");
    assert_eq!(held["numbers"]["held"], true, "{held}");
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    for w in ["bollard", "heron", "kelp"] {
        assert!(!posts[0].contains(w), "{w}: {}", posts[0]);
        assert!(!held.to_string().contains(w), "{w}: {held}");
    }
}
