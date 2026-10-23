//! A hold under `[policy] external_text = "notify"` tells the owner once
//! (theseus-nsqqo): its first call that acts runs with the notice, the later
//! ones under the same hold run quietly, across a restart too, and a hold
//! begun after a trust tells once more. Under `ask` every call still waits.

use super::*;
use crate::external::Mode;

/// A fetch, then `n` runs: the model's loop for "read <url>, run <n>".
fn read_then_runs(req: &ProviderRequest) -> Scripted {
    let (said, results) = asked(req);
    let n: usize = said
        .split_whitespace()
        .find_map(|w| w.strip_prefix("run").and_then(|n| n.parse().ok()))
        .unwrap_or(0);
    let url = said.split_whitespace().last().unwrap_or("").to_string();
    match results {
        0 if said.starts_with("read") => fetch("f1", &url),
        0 if n > 0 => run(&format!("r{}-0", said.len()), "x"),
        i if i <= n && said.starts_with("read") => run(&format!("r{}-{i}", said.len()), "x"),
        i if i < n => run(&format!("r{}-{i}", said.len()), "x"),
        _ => Scripted::text("Done."),
    }
}

/// The hold's notices: `proc.run` rows whose setting is the hold's.
fn told(core: &Core) -> usize {
    ledgered(core, "tool.notified")
        .into_iter()
        .filter(|n| {
            n["tool"] == "proc.run"
                && n["setting"]
                    .as_str()
                    .is_some_and(|s| s.ends_with("external_text = notify"))
        })
        .count()
}

fn notify_core(dir: &Path, port: u16) -> Arc<Core> {
    core_mode(
        dir,
        port,
        read_then_runs,
        false,
        (Mode::Notify, Posture::Open),
    )
}

#[tokio::test]
async fn a_notify_hold_tells_the_owner_once() {
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let core = notify_core(dir.path(), server.port);
    let sid = session(&core);
    let res = turn(&core, &sid, &format!("read, run3 {}", page(server.port))).await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    let runs = core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter(|(_, n)| matches!(&n.body, Body::ToolResult { tool, .. } if tool == "proc.run"))
        .count();
    assert_eq!(runs, 3, "all three acting calls ran");
    assert_eq!(told(&core), 1, "the hold tells once");
}

#[tokio::test]
async fn a_notify_hold_tells_once_across_a_restart() {
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let url = page(server.port);
    let sid = {
        let core = notify_core(dir.path(), server.port);
        let sid = session(&core);
        turn(&core, &sid, &format!("read, run1 {url}")).await;
        assert_eq!(told(&core), 1);
        sid
    };
    let core = notify_core(dir.path(), server.port);
    assert!(hold(&core, &sid).is_some(), "still held");
    turn(&core, &sid, "run2 more").await;
    assert_eq!(told(&core), 1, "the restarted daemon does not tell again");
}

#[tokio::test]
async fn a_trusted_session_that_reads_again_is_told_again() {
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let url = page(server.port);
    let core = notify_core(dir.path(), server.port);
    let sid = session(&core);
    turn(&core, &sid, &format!("read, run1 {url}")).await;
    assert_eq!(told(&core), 1);
    core.trust_session(&sid, "test").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    turn(&core, &sid, &format!("read, run2 {url}")).await;
    assert_eq!(told(&core), 2, "the new hold tells once more, not twice");
}

#[tokio::test]
async fn an_ask_hold_still_waits_on_every_call() {
    let server = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_mode(
        dir.path(),
        server.port,
        read_then_runs,
        false,
        (Mode::Ask, Posture::Open),
    );
    let sid = session(&core);
    let res = turn(&core, &sid, &format!("read, run1 {}", page(server.port))).await;
    let corr = res.awaiting_confirm.clone().expect("the first run waits");
    core.confirm_action(&corr, false, Some("not now"), "test")
        .unwrap();
    core.continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap();
    let res = turn(&core, &sid, "run1 again").await;
    assert!(res.awaiting_confirm.is_some(), "the second run waits too");
    assert_eq!(ledgered(&core, "tool.confirm_requested").len(), 2);
    assert_eq!(told(&core), 0);
}
