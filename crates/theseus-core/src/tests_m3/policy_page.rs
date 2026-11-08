//! The Policy page's read stays true after a tightening made elsewhere
//! (theseus-n9wa). The cockpit connects with `executions.watch` and watches
//! no session, so what it hears of a tightening is what the push gives every
//! all-session watcher. A child of `tests_m3` for its rig and helpers.

use super::*;
use crate::approval::Surface::Cli;
use theseus_protocol::{method, notify, PolicyExplainParams, PolicyExplainResult};

/// What `policy.explain` says of `proc.run` in the CLI's place: its result and
/// its tightening layer's words.
fn proc_run(core: &Core) -> (String, String, bool) {
    let r: PolicyExplainResult = core
        .policy_explain(PolicyExplainParams {
            session_id: None,
            tool: Some("proc.run".into()),
        })
        .unwrap();
    let place = r.places.iter().find(|p| p.place == "cli").expect("the CLI");
    assert_eq!(place.tools.len(), 1, "the one tool asked about");
    let t = &place.tools[0];
    let l = t
        .layers
        .iter()
        .find(|l| l.layer == "tightening")
        .expect("a tightening layer");
    (t.result.clone(), l.says.clone(), l.raised)
}

/// A page that watches no session hears a tightening and its undo from
/// another surface, and the read it makes then states the posture in force.
#[tokio::test]
async fn a_page_watching_no_session_hears_a_tightening_made_elsewhere() {
    let r = rig_with(vec![], |cfg| cfg.policy.enforcement = Posture::Notify);
    let (tx, mut page) = tokio::sync::mpsc::unbounded_channel();
    r.core.bus.watch_all("page", tx);
    assert_eq!(r.core.bus.watchers("none"), 0, "it watches no session");

    let before = proc_run(&r.core);
    assert_eq!(
        before,
        ("notify".to_string(), "not tightened".to_string(), false)
    );

    // The tightening comes from the CLI, not from the page.
    rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::POLICY_TIGHTEN,
        json!({"tool": "proc.run"}),
    )
    .await
    .unwrap();
    let heard = sent(&mut page, notify::POLICY_TIGHTENED);
    assert_eq!(heard.len(), 1, "the page hears it once: {heard:?}");
    assert_eq!(heard[0]["tool"], "proc.run");
    assert_eq!(heard[0]["posture"], "approve");
    let after = proc_run(&r.core);
    assert_eq!(after.0, "approve", "the result is the tightened posture");
    assert!(
        after.1.starts_with("tightened by the CLI"),
        "the layer names who: {}",
        after.1
    );
    assert!(after.2, "the tightening raised it");

    // The undo from the CLI loosens it again, and the page hears that too.
    rpc_as(
        &r.core,
        surface("sock#2", Cli),
        method::POLICY_UNTIGHTEN,
        json!({"tool": "proc.run"}),
    )
    .await
    .unwrap();
    let undone = sent(&mut page, notify::POLICY_UNTIGHTENED);
    assert_eq!(undone.len(), 1, "{undone:?}");
    assert_eq!(undone[0]["tool"], "proc.run");
    assert_eq!(proc_run(&r.core), before, "back to what the config says");
}

/// A tightening reaches each connection once: one that watches a session and
/// every session, one that watches only a session, and one that watches only
/// every session.
#[tokio::test]
async fn a_tightening_reaches_each_connection_once() {
    let r = rig_with(vec![], |cfg| cfg.policy.enforcement = Posture::Notify);
    let (tx_both, mut both) = tokio::sync::mpsc::unbounded_channel();
    let (tx_all, mut all) = tokio::sync::mpsc::unbounded_channel();
    let (sid, mut one) = watched_session(&r);
    r.core.bus.watch(&sid, "both", tx_both.clone());
    r.core.bus.watch_all("both", tx_both);
    r.core.bus.watch_all("page", tx_all);
    rpc_as(
        &r.core,
        surface("sock#1", Cli),
        method::POLICY_TIGHTEN,
        json!({"tool": "proc.run"}),
    )
    .await
    .unwrap();
    for (who, rx) in [
        ("both", &mut both),
        ("page", &mut all),
        ("session", &mut one),
    ] {
        assert_eq!(
            sent(rx, notify::POLICY_TIGHTENED).len(),
            1,
            "{who} hears it once"
        );
    }
}
