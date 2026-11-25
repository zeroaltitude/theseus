//! `told.undo` once-only across a crash (issue 57): an undo whose effect was
//! written and whose receipt was not is found done on the next press, marked,
//! and never run again; an undo that fails says so plainly; two presses at
//! once undo once.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{
    method, Message, Request, Response, StandingAddParams, StandingKind, StandingRevokeParams,
    StandingScope,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::{ToldUndoResult, FAULT_RECEIPT, PROBED};
use crate::approval::{Answerer, Client, Surface};
use crate::owner_note::told::{self, Undo};
use crate::rpc::Core;

/// The owner at the terminal: the general rule counts it.
fn owner() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A standing order and its note: the order's id and the note's post id.
fn ordered(core: &Core) -> (String, String) {
    let id = core
        .standing_add(
            &StandingAddParams {
                kind: StandingKind::Order,
                text: "read the tide table in metres".into(),
                scope: StandingScope::Surface("terminal".into()),
                ..Default::default()
            },
            owner(),
        )
        .unwrap()
        .record
        .id;
    let (a, frame) = told::stage(
        &core.outbox,
        None,
        "Theseus kept a standing order",
        "In “Tides”, without asking you",
        "",
        &Undo::StandingRevoke(id.clone()),
    )
    .unwrap();
    core.store.append(&frame).unwrap();
    core.outbox.posted(&a);
    (id, a.correlation_id)
}

/// How many revokes ended the order.
fn ends(core: &Core, id: &str) -> usize {
    let all = core.runner.standing.all(&core.store).unwrap();
    all.iter().filter(|x| x.ends.as_deref() == Some(id)).count()
}

fn marked(core: &Core, post: &str) -> bool {
    told::undone(&core.store, post).unwrap().is_some()
}

/// The state a crash between effect and receipt leaves: the order revoked
/// by its own method, the note not marked. A press finds the revoke, marks
/// the note, says so, and revokes nothing again.
#[tokio::test]
async fn a_press_after_an_effect_whose_receipt_was_lost_marks_and_runs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let core = crate::tests_unlabel::core_in(dir.path());
    let (id, post) = ordered(&core);
    let p = StandingRevokeParams {
        id: id.clone(),
        ..Default::default()
    };
    core.standing_revoke(&p, owner()).unwrap();
    assert!(!marked(&core, &post));
    let done = core.told_undo(&post, &owner()).await.unwrap();
    assert!(
        done.text
            .ends_with("\n↩️ Nothing left to undo: the standing order is revoked."),
        "{}",
        done.text
    );
    assert!(!done.text.contains("Undo: `"), "{}", done.text);
    assert_eq!(ends(&core, &id), 1, "revoked once");
    assert!(marked(&core, &post));
}

/// A fault planted between the effect and the receipt: the press says the
/// undo was done and its record was not kept; the next press finds it done,
/// marks the note, and runs nothing; a third is refused.
#[tokio::test]
async fn a_fault_between_effect_and_receipt_then_a_second_press_undoes_once() {
    let dir = tempfile::tempdir().unwrap();
    let core = crate::tests_unlabel::core_in(dir.path());
    let (id, post) = ordered(&core);
    FAULT_RECEIPT.store(true, Ordering::SeqCst);
    let e = core.told_undo(&post, &owner()).await.unwrap_err();
    assert!(e.message.starts_with("The undo was done"), "{}", e.message);
    assert_eq!(e.data["done"], true);
    assert_eq!(ends(&core, &id), 1, "the effect was written");
    assert!(!marked(&core, &post), "the receipt was not");
    let done = core.told_undo(&post, &owner()).await.unwrap();
    assert!(done.found, "{}", done.text);
    assert!(done.text.contains("Nothing left to undo"), "{}", done.text);
    assert_eq!(ends(&core, &id), 1, "nothing ran twice");
    assert!(marked(&core, &post));
    let again = core.told_undo(&post, &owner()).await.unwrap_err();
    assert_eq!(again.data["undone_by"], "the CLI");
    assert_eq!(ends(&core, &id), 1);
}

/// An undo whose own call fails says it did not finish, never that nothing
/// was undone, and leaves the note unmarked for a press to finish it.
#[tokio::test]
async fn a_failing_undo_says_it_did_not_finish_and_marks_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let core = crate::tests_unlabel::core_in(dir.path());
    let (a, frame) = told::stage(
        &core.outbox,
        None,
        "Theseus kept a standing order",
        "In “Tides”, without asking you",
        "",
        &Undo::StandingRevoke("std_no_such_order".into()),
    )
    .unwrap();
    core.store.append(&frame).unwrap();
    core.outbox.posted(&a);
    let post = a.correlation_id;
    let e = core.told_undo(&post, &owner()).await.unwrap_err();
    assert!(
        e.message.starts_with("The undo did not finish: "),
        "{}",
        e.message
    );
    assert!(!e.message.contains("Nothing was undone"), "{}", e.message);
    assert!(e
        .message
        .contains("`theseus standing revoke std_no_such_order`"));
    assert!(!marked(&core, &post));
}

/// Two presses at once: one undoes, the other is refused as already undone;
/// the order is revoked once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_presses_at_once_undo_once() {
    let dir = tempfile::tempdir().unwrap();
    let core: Arc<Core> = crate::tests_unlabel::core_in(dir.path());
    let (id, post) = ordered(&core);
    let press = |core: Arc<Core>, post: String| {
        tokio::spawn(async move { core.told_undo(&post, &owner()).await })
    };
    let (a, b) = tokio::join!(
        press(core.clone(), post.clone()),
        press(core.clone(), post.clone())
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    let undone = [&a, &b].iter().filter(|r| r.is_ok()).count();
    assert_eq!(undone, 1, "{a:?} {b:?}");
    let refused = [a, b].into_iter().find_map(Result::err).unwrap();
    assert_eq!(refused.data["undone_by"], "the CLI");
    assert_eq!(ends(&core, &id), 1);
}

/// A note of the tests' undo with no method of its own: its post id.
fn probe_note(core: &Core) -> String {
    let (a, frame) = told::stage(
        &core.outbox,
        None,
        "Theseus took something back",
        "In “Tides”, without asking you",
        "",
        &Undo::Probe("thing_1".into()),
    )
    .unwrap();
    core.store.append(&frame).unwrap();
    core.outbox.posted(&a);
    a.correlation_id
}

/// One request on a connection of `client`'s surface, its whole response.
async fn call(core: &Arc<Core>, client: Client, m: &str, params: Value) -> Response {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), m, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

/// A note of an undo with no method of its own names the door, with its own
/// post id: "Undo: `theseus told undo <post>`", filled where the note is
/// read. The stored words keep the mark the id is filled into.
#[tokio::test]
async fn a_method_less_notes_undo_line_names_the_verb_with_its_own_post_id() {
    let dir = tempfile::tempdir().unwrap();
    let core = crate::tests_unlabel::core_in(dir.path());
    let post = probe_note(&core);
    let (text, undo) = told::of(&core.kernel, &post).unwrap().unwrap();
    assert_eq!(undo, Undo::Probe("thing_1".into()));
    assert!(
        text.ends_with(&format!("\nUndo: `theseus told undo {post}`")),
        "{text}"
    );
    let a = core.kernel.outbox_action(&post).unwrap().unwrap();
    let stored = crate::outbox::body_of(&a)["text"].clone();
    assert!(stored.as_str().unwrap().contains(told::THIS_POST), "{stored}");
}

/// `theseus told undo` from a terminal: `told.undo` on a CLI connection,
/// naming the note by the end of its post id, undoes once; a second is
/// refused by the core and runs nothing.
#[tokio::test]
async fn told_undo_from_the_cli_surface_undoes_once() {
    let dir = tempfile::tempdir().unwrap();
    let core = crate::tests_unlabel::core_in(dir.path());
    let post = probe_note(&core);
    let end = json!({"post": &post[post.len() - 6..]});
    let cli = || Client::new("test", Surface::Cli);
    let r = call(&core, cli(), method::TOLD_UNDO, end.clone()).await;
    assert!(r.error.is_none(), "{:?}", r.error);
    let done: ToldUndoResult = serde_json::from_value(r.result.unwrap()).unwrap();
    assert!(done.text.contains("\n↩️ Undone by "), "{}", done.text);
    assert!(!done.text.contains("Undo: `"), "{}", done.text);
    assert_eq!(PROBED.load(Ordering::SeqCst), 1);
    assert!(marked(&core, &post));
    let r = call(&core, cli(), method::TOLD_UNDO, end).await;
    let e = r.error.expect("a second undo is refused");
    assert!(e.data.get("undone_by").is_some(), "{e:?}");
    assert_eq!(PROBED.load(Ordering::SeqCst), 1, "it ran once");
}
