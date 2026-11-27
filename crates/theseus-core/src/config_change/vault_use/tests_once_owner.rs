//! Who and when `vault.use`'s run form runs free (issue 64): the window of outside text starts at
//! the owner's own last message, which a harness note does not end; outside text read by an
//! earlier call of the same response is caught when the form runs; the owner's own typed input,
//! as `turn.submit` judges it, runs free and a job's asks; a grant ends with its turn and with any
//! later occasion, an approval's continuation excepted; the variable may not steer how the
//! program is found; and any value Theseus holds at run time is a key it runs on.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::tests_once::{
    counted, held, proc, rig_once, rig_once_with, run_call, script, script_run, COUNTS,
    RUNS_ON_WORDS, VALUE,
};
use super::*;
use crate::config_change::tests::Rig;
use crate::content::OutsideMode;
use crate::node::{Body, ResultStatus};
use crate::secrets::look::tests::logged;

/// A guest's words in another place: a turn that opens them reads outside text.
fn guest(r: &Rig) -> (&'static str, Value) {
    crate::tests_security::history_exchange(
        &r.core,
        "ses_guest_lantern",
        "turn_guest_lantern",
        ("discord:channel:77", "discord:7"),
        "a guest asks to run the lantern script",
    );
    ("context_history", json!({"open": "turn_guest_lantern"}))
}

/// The questions waiting in `sid`.
fn asked_in(r: &Rig, sid: &str) -> Vec<theseus_protocol::ConfirmRequest> {
    r.asked()
        .into_iter()
        .filter(|q| q.session_id == sid)
        .collect()
}

/// G2 (i): the turn's clock line, written by the turn's own clock between an outside read and
/// the run form, does not end the window: the form is asked, before the vault is read.
#[tokio::test]
async fn a_clock_line_after_outside_text_does_not_end_the_ask() {
    let o = rig_once();
    let r = &o.r;
    let sid = r.session();
    let read = guest(r);
    // A deadline whose whole minutes left change between the turn's first loop and its second.
    crate::turn::clock_step::set_helper_deadline(&sid, Instant::now() + Duration::from_secs(61));
    r.when_the_turn_asks_the_model(|| std::thread::sleep(Duration::from_millis(1500)));
    r.steps(
        &sid,
        vec![read, ("vault_use", run_call("./call_lantern.sh"))],
    )
    .await;
    let nodes = r.core.store.session_nodes(&sid).unwrap();
    let at = |pick: &dyn Fn(&Body) -> bool| nodes.iter().position(|(_, n)| pick(&n.body));
    let outside = at(&|b| matches!(b, Body::ToolResult { tool_use_id, .. } if tool_use_id == "p1"))
        .expect("the outside read's result");
    let clock = nodes
        .iter()
        .rposition(|(_, n)| n.author.as_deref() == Some(crate::turn::clock_step::CLOCK_AUTHOR));
    assert!(
        clock.is_some_and(|c| c > outside),
        "the clock's line follows the outside read"
    );
    let asks = asked_in(r, &sid);
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0].tool, USE);
    assert!(
        asks[0].reason.contains(crate::config_change::OUTSIDE_HOLD),
        "{}",
        asks[0].reason
    );
    assert!(logged(&o.vault).is_empty(), "the vault was not read");
}

/// G2 (ii): one response holding an outside read and then the run form: the form was judged
/// before the read ran, and is caught when it runs; nothing is read from the vault, and the model
/// is told to call it again on its own.
#[tokio::test]
async fn an_outside_read_earlier_in_the_same_response_is_caught_when_the_form_runs() {
    let o = rig_once();
    let r = &o.r;
    let sid = r.session();
    let read = guest(r);
    r.batch_next(vec![read, ("vault_use", run_call("./call_lantern.sh"))]);
    r.run_turn(&sid).await;
    let (status, text) = r.result_of(&sid, "p2");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains(once::AGAIN), "{text}");
    assert!(logged(&o.vault).is_empty(), "the vault was not read");
    assert!(held(r).is_empty());
}

/// One `turn.submit` over the RPC, as a client of the command line sends it.
async fn submit(r: &Rig, params: Value) -> Value {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(r.core.clone().serve_connection(
        sr,
        sw,
        crate::approval::Client::new("sock#1", crate::approval::Surface::Cli),
    ));
    let (cr, mut cw) = tokio::io::split(client);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), "turn.submit", params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(resp) = serde_json::from_str(&l).unwrap() {
            break match (resp.result, resp.error) {
                (Some(v), _) => v,
                (None, e) => json!({"error": e}),
            };
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

/// G3: at the producer. The owner's own typed turn through `turn.submit` runs the form free; a
/// turn a job submitted the same way (it names the session it runs in) is asked first.
#[tokio::test]
async fn a_turn_a_job_submitted_asks_and_the_owners_own_does_not() {
    let o = rig_once();
    let r = &o.r;
    let job = r.session();
    for from_job in [true, false] {
        let sid = r.session();
        r.call_next("vault_use", run_call("./call_lantern.sh"));
        let mut params = json!({"session_id": sid, "input": "run the lantern script with its key"});
        if from_job {
            params["opened_from"] = json!(job);
        }
        let out = submit(r, params).await;
        assert!(out.get("error").is_none(), "{from_job}: {out}");
        let asks = asked_in(r, &sid);
        match from_job {
            true => {
                assert_eq!(asks.len(), 1, "a job's turn asks: {asks:?}");
                assert_eq!(asks[0].tool, USE);
                assert!(
                    asks[0].reason.contains(once::NOT_BEGUN),
                    "{}",
                    asks[0].reason
                );
                assert!(
                    logged(&o.vault).is_empty(),
                    "nothing read for the job's turn"
                );
            }
            false => {
                assert!(asks.is_empty(), "the owner's own turn: {asks:?}");
                let (status, text) = r.result(&sid);
                assert_eq!(status, ResultStatus::Ok, "{text}");
            }
        }
    }
}

/// G4: a grant its turn did not use is gone when the turn ends.
#[tokio::test]
async fn an_unused_grant_ends_with_its_turn() {
    let o = rig_once();
    let sid = o.r.session();
    o.r.steps(&sid, vec![("vault_use", run_call("./call_lantern.sh"))])
        .await;
    let (status, text) = o.r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(!o.r.core.tools.broker.has_once(&sid), "the grant ended");
}

/// G4: a continuation a background job's end began gets nothing from the owner's earlier turn,
/// and a run form of its own is asked.
#[tokio::test]
async fn a_job_ended_continuation_gets_nothing_and_its_own_form_asks() {
    let o = rig_once_with(OutsideMode::default(), COUNTS, |c| {
        c.tools.proc_sync_secs = 1
    });
    let r = &o.r;
    let sid = r.session();
    r.steps(
        &sid,
        vec![
            ("vault_use", run_call("./call_lantern.sh")),
            ("proc_run", proc(json!(["sleep", "2"]))),
        ],
    )
    .await;
    r.batch_next(vec![
        script_run(),
        ("vault_use", run_call("./call_lantern.sh")),
    ]);
    r.drive();
    for _ in 0..200 {
        if !counted(&o).is_empty() && !asked_in(r, &sid).is_empty() {
            break;
        }
        // The job's completion, taken as the daemon's heartbeat takes it.
        r.core.drain_spool();
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(counted(&o), ["0"], "the later run got nothing");
    let asks = asked_in(r, &sid);
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0].tool, USE);
    assert!(
        asks[0].reason.contains(once::NOT_BEGUN),
        "{}",
        asks[0].reason
    );
}

/// G4: a run the owner approves, in the continuation his answer begins, still takes the grant.
#[tokio::test]
async fn an_approved_continuation_still_gets_it() {
    let o = rig_once_with(OutsideMode::default(), COUNTS, |c| {
        c.policy
            .tools
            .insert("proc.run".into(), crate::policy::Posture::Approve);
    });
    let r = &o.r;
    let sid = r.session();
    r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let asks = asked_in(r, &sid);
    assert_eq!(asks.len(), 1, "the run is asked: {asks:?}");
    assert!(
        r.core.tools.broker.has_once(&sid),
        "kept while the run waits"
    );
    r.core
        .confirm_action(&asks[0].correlation_id, true, None, "cli")
        .unwrap();
    r.try_continue(&sid).await.unwrap();
    assert_eq!(
        counted(&o),
        [VALUE.len().to_string()],
        "the approved run had it"
    );
}

/// G6: the variable may not be one that steers how a program is found or loaded: an input
/// error, and nothing is read from the vault.
#[tokio::test]
async fn the_variable_may_not_steer_how_the_program_is_found() {
    let o = rig_once();
    let r = &o.r;
    // Two programs of one name, in two folders.
    for d in ["one", "two"] {
        std::fs::create_dir_all(o.work.join(d)).unwrap();
        script(&o.work.join(d), "lantern", COUNTS);
    }
    for var in [
        "PATH",
        "LD_PRELOAD",
        "DYLD_INSERT_LIBRARIES",
        "THESEUS_SESSION",
    ] {
        let sid = r.session();
        let mut call = run_call("./one/lantern");
        call["run"]["variable"] = json!(var);
        r.turn_with("vault_use", &sid, call).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{var}: {text}");
        assert!(text.contains("another name"), "{var}: {text}");
    }
    assert!(logged(&o.vault).is_empty(), "the vault was not read");
    assert!(held(r).is_empty());
}

/// G7: a value Theseus holds at run time (a signed-in server's token), copied into a vault item,
/// is a key Theseus runs on.
#[tokio::test]
async fn a_copy_of_a_runtime_token_is_a_key_theseus_runs_on() {
    let o = rig_once();
    let r = &o.r;
    r.core.secrets.hold(
        "mcp-oauth:lantern:access:1",
        crate::secrets::Secret::new(VALUE.into()),
    );
    let sid = r.session();
    r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let (status, text) = r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains(RUNS_ON_WORDS), "{text}");
    assert_eq!(counted(&o), ["0"]);
}
