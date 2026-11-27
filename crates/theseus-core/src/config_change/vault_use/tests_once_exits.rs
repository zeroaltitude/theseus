//! What leaves after `vault.use`'s run form gave a program a key it printed (issue 64): the
//! record keeps the job's output as the job wrote it, and the exits mask the value because the
//! board knows it. Theseus remembers which items it gave, by name, and at every start reads them
//! again and holds their values, so the requests the model is sent, the courier's and the push
//! line's text carry no value: from a read of a running job, an earlier step, a capped output's
//! kept file, and a result built after a restart. Each case is checked before and after a
//! restart. An item that cannot be read at a start is passed over; a value too short to be
//! masked is not given.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::tests_once::{
    counted, held, rig_once_with, run_call, script_run, settings, Once, VALUE,
};
use super::*;
use crate::config_change::tests::Rig;
use crate::content::OutsideMode;
use crate::node::{Body, ResultStatus};
use crate::secrets::look::tests::{logged, stub};

/// Prints its key, and goes on past the call's wait.
const PRINTS_AND_WAITS: &str =
    "printf '%s\\n' \"${#LANTERN_TOKEN}\" >> got.txt\necho \"token=$LANTERN_TOKEN\"\nsleep 2\n";

/// Every copy that leaves for `sid`: each request the model was sent (after the exits), and
/// each of the session's nodes and of the owner's posts as the courier (`Exits::text`) and the
/// push line (the board's scrubber) would send it.
fn leaves(r: &Rig, sid: &str) -> String {
    let mut out: Vec<String> = r
        .sent()
        .iter()
        .map(|q| serde_json::to_string(&q.messages).unwrap())
        .collect();
    let push = crate::scrub::Scrubber::from_board(r.core.secrets.clone());
    let nodes = r.core.store.session_nodes(sid).unwrap();
    let posts = r.core.outbox.open_for(crate::outbox::OPERATOR_TARGET);
    let texts = nodes
        .iter()
        .map(|(_, n)| format!("{:?}", n.body))
        .chain(posts.iter().map(|a| crate::outbox::body_of(a).to_string()));
    for t in texts {
        out.push(r.core.exits.text(&t).0.into_owned());
        out.push(push.scrub(&t).0);
    }
    out.join("\n")
}

/// The settings these cases run under: a job's wait of one second.
fn quick(c: &mut crate::Config) {
    c.tools.proc_sync_secs = 1;
}

fn rig(body: &str) -> Once {
    rig_once_with(OutsideMode::default(), body, quick)
}

/// The daemon stopped and started on the same store, its vault the stub in `mode`, and its
/// start's step for the items the run form gave.
async fn restarted(o: Once, mode: &str) -> Once {
    let Once {
        r,
        vault,
        work,
        _d,
        _w,
    } = o;
    let at = work.clone();
    let r = r.restart(move |c, _| settings(c, &at, OutsideMode::default(), &quick));
    r.core.tools.look.set_vault(Arc::new(stub(&vault, mode)));
    r.core.hold_vault_used().await;
    r.owner_types();
    Once {
        r,
        vault,
        work,
        _d,
        _w,
    }
}

/// An owner's turn that answers in words: the model is sent the whole history.
async fn a_word(r: &Rig, sid: &str) {
    r.says("anything new?");
    r.run_turn(sid).await;
}

/// Wait until the script has counted `n` runs and has ended, and take its completion as the
/// daemon's heartbeat does (`Core::drain_spool`).
async fn ran(o: &Once, n: usize) {
    for _ in 0..100 {
        if counted(o).len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(2500)).await;
    o.r.core.drain_spool();
}

/// The text of the latest result of `tool` in `sid`.
fn latest(r: &Rig, sid: &str, tool: &str) -> (ResultStatus, String) {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .rev()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool: t,
                status,
                content,
                ..
            } if t == tool => Some((status, content)),
            _ => None,
        })
        .expect("a result of the tool")
}

fn clean(r: &Rig, sid: &str, when: &str) {
    let out = leaves(r, sid);
    assert!(!out.contains(VALUE), "the value left {when}");
}

/// (a) A script that prints its key and goes on past the wait; `job.read` reads it; restart.
#[tokio::test]
async fn a_read_of_a_running_job_leaves_masked_before_and_after_a_restart() {
    let o = rig(PRINTS_AND_WAITS);
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![
            ("vault_use", run_call("./call_lantern.sh")),
            script_run(),
            ("job_read", json!({"job": "j1"})),
        ],
    )
    .await;
    let (_, text) = o.r.result_of(&sid, "p3");
    assert!(text.contains("token="), "the read has the output: {text}");
    a_word(&o.r, &sid).await;
    clean(&o.r, &sid, "before a restart");
    ran(&o, 1).await;
    let o = restarted(o, "ok").await;
    a_word(&o.r, &sid).await;
    assert!(!o.r.sent().is_empty());
    clean(&o.r, &sid, "after a restart");
}

/// (b) A first step prints the key, a second follows.
#[tokio::test]
async fn an_earlier_steps_output_leaves_masked_before_and_after_a_restart() {
    let o = rig(super::tests_once::PRINTS);
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![
            ("vault_use", run_call("./call_lantern.sh")),
            (
                "proc_run",
                json!({"steps": [{"argv": ["./call_lantern.sh"]}, {"argv": ["true"]}]}),
            ),
        ],
    )
    .await;
    assert_eq!(counted(&o), [VALUE.len().to_string()], "the step had it");
    a_word(&o.r, &sid).await;
    clean(&o.r, &sid, "before a restart");
    let o = restarted(o, "ok").await;
    a_word(&o.r, &sid).await;
    clean(&o.r, &sid, "after a restart");
}

/// (c) Output past the result's cap, the key in the part kept only in the file; the file read
/// back after a restart.
#[tokio::test]
async fn a_capped_outputs_kept_file_leaves_masked_after_a_restart() {
    let body = "printf '%s\\n' \"${#LANTERN_TOKEN}\" >> got.txt\n\
                i=0; while [ $i -lt 100 ]; do echo 'filler filler filler filler filler filler'; i=$((i+1)); done\n\
                echo \"token=$LANTERN_TOKEN\"\n\
                i=0; while [ $i -lt 100 ]; do echo 'filler filler filler filler filler filler'; i=$((i+1)); done\n";
    let o = rig_once_with(OutsideMode::default(), body, |c| {
        c.tools.result_max_chars = 2_000;
    });
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let kept =
        o.r.core
            .store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match n.body {
                Body::ToolResult {
                    tool_use_id,
                    full_ref,
                    ..
                } if tool_use_id == "p2" => full_ref,
                _ => None,
            })
            .expect("the whole output is kept in a file");
    let Once {
        r,
        vault,
        work,
        _d,
        _w,
    } = o;
    let at = work.clone();
    let r = r.restart(move |c, _| {
        settings(c, &at, OutsideMode::default(), &|c: &mut crate::Config| {
            c.tools.result_max_chars = 2_000;
        });
    });
    r.core.tools.look.set_vault(Arc::new(stub(&vault, "ok")));
    r.core.hold_vault_used().await;
    r.owner_types();
    r.steps(
        &sid,
        vec![("fs_read", json!({"path": kept, "offset": 95, "limit": 12}))],
    )
    .await;
    let (status, text) = latest(&r, &sid, "fs.read");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("token="), "the kept line was read: {text}");
    a_word(&r, &sid).await;
    clean(&r, &sid, "after a restart");
    drop((vault, work, _d, _w));
}

/// (d) A restart while the job runs, and a restart after its output is written but before its
/// result is taken: the result built afterwards reaches the model masked.
#[tokio::test]
async fn a_result_built_after_a_restart_leaves_masked() {
    for done_first in [false, true] {
        let o = rig(PRINTS_AND_WAITS);
        let sid = o.r.session();
        o.r.steps(
            &sid,
            vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
        )
        .await;
        if done_first {
            ran(&o, 1).await;
        }
        let o = restarted(o, "ok").await;
        ran(&o, 1).await;
        a_word(&o.r, &sid).await;
        let read =
            o.r.sent()
                .iter()
                .map(|q| serde_json::to_string(&q.messages).unwrap())
                .collect::<String>();
        assert!(
            read.contains("token="),
            "{done_first}: the result reached the model"
        );
        clean(
            &o.r,
            &sid,
            &format!("after a restart (written first: {done_first})"),
        );
    }
}

/// (e) An item taken out of the vault before a start: passed over, and the start goes on.
#[tokio::test]
async fn an_item_that_cannot_be_read_at_a_start_is_passed_over() {
    let o = rig(super::tests_once::COUNTS);
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    // The start's own reads, apart from the run's.
    std::fs::remove_file(o.vault.join("op.argv")).unwrap();
    let o = restarted(o, "fail-read").await;
    a_word(&o.r, &sid).await;
    assert!(!o.r.sent().is_empty(), "the turn ran");
    assert!(o.r.core.secrets.is_settled(), "nothing is left resolving");
    assert!(held(&o.r).is_empty());
    let used: Vec<String> =
        o.r.core
            .secrets
            .states()
            .keys()
            .filter(|k| k.starts_with("vault-used:"))
            .cloned()
            .collect();
    assert!(used.is_empty(), "{used:?}");
    assert!(
        logged(&o.vault)
            .iter()
            .any(|l| l == "read --no-newline op://Harbor/Lantern key/credential"),
        "the start tried the item again"
    );
}

/// (f) A value shorter than the scrubber's floor could be masked at no exit: it is not given.
#[tokio::test]
async fn a_value_too_short_to_mask_is_not_given() {
    let o = rig(super::tests_once::COUNTS);
    o.r.core
        .tools
        .look
        .set_vault(Arc::new(stub(&o.vault, "short")));
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let (status, text) = o.r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("too short"), "{text}");
    assert!(held(&o.r).is_empty());
    assert_eq!(counted(&o), ["0"]);
    let none: Option<Value> = o.r.core.store.get_meta(used::KEY).unwrap();
    assert!(none.is_none(), "nothing remembered: {none:?}");
}

/// H1: a start whose vault answers slowly holds the remembered item before it serves: an old post
/// holding the value, sent again by the courier (`Exits::text`) or read by a guarded push (the
/// board's scrubber), leaves masked at once, with no turn first.
#[tokio::test]
async fn the_start_holds_the_items_before_the_courier_or_the_push_send() {
    let o = rig(super::tests_once::COUNTS);
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let old = format!("the script said token={VALUE}");
    let o = restarted(o, "slow").await;
    let courier = o.r.core.exits.text(&old).0.into_owned();
    let push = crate::scrub::Scrubber::from_board(o.r.core.secrets.clone())
        .scrub(&old)
        .0;
    assert!(!courier.contains(VALUE), "the courier's copy: {courier}");
    assert!(!push.contains(VALUE), "the push's copy: {push}");
}

/// H1: a record of the items that cannot be decoded is not an empty list: the start holds nothing
/// from it, a later run form is not given, and the record is left as it was.
#[tokio::test]
async fn a_record_that_cannot_be_read_holds_nothing_and_is_not_replaced() {
    let o = rig(super::tests_once::COUNTS);
    let r = &o.r;
    r.core
        .store
        .put_meta(used::KEY, &json!({"not": "a list"}))
        .unwrap();
    let before: Value = r.core.store.get_meta(used::KEY).unwrap().unwrap();
    r.core.hold_vault_used().await;
    let used_on_board = r
        .core
        .secrets
        .states()
        .keys()
        .any(|k| k.starts_with(used::HANDLE));
    assert!(!used_on_board, "nothing held from it");
    let sid = r.session();
    r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let (status, text) = r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("could not note"), "{text}");
    assert!(held(r).is_empty());
    assert_eq!(counted(&o), ["0"]);
    let after: Value = r.core.store.get_meta(used::KEY).unwrap().unwrap();
    assert_eq!(after, before, "the record is not replaced");
}

/// H1: an item whose read fails once at a start is tried again in the background, and held.
#[tokio::test]
async fn a_read_that_fails_once_is_held_after_the_retry() {
    let o = rig(super::tests_once::COUNTS);
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![("vault_use", run_call("./call_lantern.sh")), script_run()],
    )
    .await;
    let o = restarted(o, "fail-read-once").await;
    let first = format!("{}0", used::HANDLE);
    for _ in 0..60 {
        if o.r.core.secrets.get(&first).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        o.r.core.secrets.get(&first).is_some(),
        "held after the retry"
    );
    assert!(
        o.vault.join("read-failed-once").exists(),
        "the first read failed"
    );
}
