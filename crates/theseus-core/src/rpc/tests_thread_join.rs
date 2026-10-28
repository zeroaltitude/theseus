//! Merging threads and splitting one (theseus-vt9k, T8) through the core: a
//! merge unions and retires in one frame and deletes nothing; recall from the
//! merged thread reaches the retired thread's facts under their own place; a
//! seal on either survives it; an unmerge puts the thread back; a split opens
//! a thread whose first compile quotes exactly the chosen turns and leaves
//! its source's bytes alone; and the model's `thread.split` waits for the
//! owner's tap.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::threads::{
    Seal, ThreadEditParams, ThreadMergeParams, ThreadNewParams, ThreadSplitParams, ThreadState,
    WorkingSet,
};

use super::Core;
use crate::approval::{Answerer, Surface};
use crate::config::MemoryMode;
use crate::node::Body;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::tests_recall::{admitted, index_of, recalls, rig_with, turn, Rig};

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn rig(script: Vec<Scripted>) -> Rig {
    let r = rig_with(MemoryMode::Off, |_| {});
    r.model.script.lock().unwrap().extend(script);
    r
}

fn new(c: &Core, slug: &str) {
    c.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(format!("Title of {slug}")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
}

fn edit(c: &Core, p: ThreadEditParams) {
    c.thread_edit(&p, cli()).unwrap();
}

/// A thread with its session open.
fn thread(c: &Core, slug: &str) -> String {
    new(c, slug);
    c.thread_session(slug, cli()).unwrap().session_id
}

fn merge(
    c: &Core,
    from: &str,
    into: &str,
) -> Result<theseus_protocol::threads::ThreadMergeResult, String> {
    c.thread_merge(
        &ThreadMergeParams {
            from: from.into(),
            into: into.into(),
            ..Default::default()
        },
        cli(),
    )
    .map_err(|e| e.message)
}

fn seal(c: &Core, slug: &str) {
    c.runner
        .threads
        .write(
            &c.store,
            |b| {
                let mut t = b.get(slug).unwrap().clone();
                t.sealed = Some(Seal {
                    by: "owner".into(),
                    at_ms: 1,
                    reason: "private".into(),
                });
                Ok(vec![t])
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
}

fn rows(c: &Core, kind: &str) -> Vec<crate::ledger::LedgerRow> {
    crate::tests_day_ceiling::rows(c, kind)
}

fn frames<T>(f: impl FnOnce() -> T) -> (u64, T) {
    let before = theseus_store::frames_written_here();
    let out = f();
    (theseus_store::frames_written_here() - before, out)
}

fn board(c: &Core, slug: &str) -> theseus_protocol::threads::ThreadInfo {
    c.threads().get(slug).unwrap().clone()
}

/// `(position, id, text)` of the nodes of a session that have text.
fn said(c: &Core, sid: &str) -> Vec<(u64, String, String)> {
    c.store
        .transcript(sid)
        .unwrap()
        .iter()
        .filter_map(|(p, n)| crate::arrangement::text_of(n).map(|t| (*p, n.id.clone(), t)))
        .collect()
}

fn pos_of(c: &Core, sid: &str, needle: &str) -> u64 {
    said(c, sid)
        .into_iter()
        .find(|(_, _, t)| t.contains(needle))
        .unwrap_or_else(|| panic!("no message with {needle:?}"))
        .0
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..400 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("never: {what}");
}

fn ws(files: &[&str], hosts: &[&str]) -> WorkingSet {
    WorkingSet {
        files: files.iter().map(|s| s.to_string()).collect(),
        hosts: hosts.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

/// A merge unions what two threads touch, retires the first and writes it
/// all in one frame with its row (the retired thread has no session here, so
/// nothing follows the frame).
#[test]
fn a_merge_unions_and_retires_in_one_frame() {
    let r = rig(vec![]);
    let c = &r.core;
    new(c, "io-pressure");
    new(c, "tide-tables");
    edit(
        c,
        ThreadEditParams {
            slug: "io-pressure".into(),
            topics: Some(vec!["t1".into(), "t2".into()]),
            working_set: Some(ws(&["gauge.log", "rotate.sh"], &["metrics.example.org"])),
            card: Some(vec![
                "the log disk was at 91%".into(),
                "rotate weekly".into(),
            ]),
            ..Default::default()
        },
    );
    edit(
        c,
        ThreadEditParams {
            slug: "tide-tables".into(),
            topics: Some(vec!["t3".into(), "t4".into()]),
            working_set: Some(ws(&["rotate.sh", "tides.csv"], &[])),
            card: Some(vec!["Friday's high tide is at 06:12".into()]),
            ..Default::default()
        },
    );
    let (n, res) = frames(|| merge(c, "io-pressure", "tide-tables"));
    let res = res.unwrap();
    assert_eq!(n, 1, "the merge is one frame");

    let (a, b) = (&res.from.thread, &res.into.thread);
    assert_eq!(
        (a.state, a.merged_into.as_deref()),
        (ThreadState::Done, Some("tide-tables"))
    );
    assert_eq!(b.merged_into, None);
    assert_eq!(b.state, ThreadState::Active);
    assert_eq!(b.topics, ["t3", "t4", "t1"], "B's first, three at most");
    assert_eq!(
        res.topics_left_out,
        ["t2"],
        "the owner picks among the rest"
    );
    assert_eq!(b.working_set.files, ["rotate.sh", "tides.csv", "gauge.log"]);
    assert_eq!(b.working_set.hosts, ["metrics.example.org"]);
    assert_eq!(b.card[0], "Friday's high tide is at 06:12");
    assert!(
        b.card[1].starts_with("merged from «io-pressure», "),
        "{:?}",
        b.card
    );
    assert_eq!(
        b.card[2..],
        ["> the log disk was at 91%", "> rotate weekly"]
    );
    // A keeps its card and its record whole; the board links the two.
    assert_eq!(board(c, "io-pressure").card.len(), 2);
    let shown = c.thread_show("tide-tables", cli()).unwrap();
    assert_eq!(shown.merged_from, ["io-pressure"]);
    let merged = rows(c, "thread.merged");
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].data["from"], "io-pressure");
    assert_eq!(merged[0].data["into"], "tide-tables");
    assert_eq!(
        merged[0].data["card"],
        json!(["the log disk was at 91%", "rotate weekly"])
    );
    // A restart reads the same board.
    let again = {
        let model = Arc::new(crate::provider::FakeProvider::default());
        super::Core::build(super::Parts::for_tests(
            (*c.cfg).clone(),
            model,
            c.store.clone(),
        ))
        .unwrap()
    };
    assert_eq!(
        again
            .threads()
            .get("io-pressure")
            .unwrap()
            .merged_into
            .as_deref(),
        Some("tide-tables")
    );
}

#[test]
fn a_merge_that_cannot_be_is_refused_and_writes_nothing() {
    let r = rig(vec![]);
    let c = &r.core;
    for s in ["a", "b", "c"] {
        new(c, s);
    }
    let (n, e) = frames(|| merge(c, "a", "a"));
    assert!(e.unwrap_err().contains("into itself"));
    assert!(merge(c, "a", "nope")
        .unwrap_err()
        .contains("no thread nope"));
    assert!(merge(c, "nope", "a")
        .unwrap_err()
        .contains("no thread nope"));
    c.thread_done("c", cli()).unwrap();
    assert!(merge(c, "a", "c").unwrap_err().contains("is done"));
    assert_eq!(n, 0);
    merge(c, "a", "b").unwrap();
    // A is merged: it cannot be merged again, and nothing merges into it.
    assert!(merge(c, "a", "b")
        .unwrap_err()
        .contains("merged into b already"));
    assert!(merge(c, "b", "a")
        .unwrap_err()
        .contains("merge into that one"));
    assert_eq!(rows(c, "thread.merged").len(), 1);
}

/// Nothing is deleted: the retired thread's nodes are the same and one note
/// is added at its tail; its session is retired, and a thread cannot take a
/// message once it is merged away.
#[tokio::test]
async fn a_merge_deletes_nothing_of_the_retired_thread() {
    let r = rig(vec![
        Scripted::text("The pump house key is under the blue stone."),
        Scripted::text("Rotate the logs weekly."),
    ]);
    let c = &r.core;
    let a = thread(c, "io-pressure");
    let b = thread(c, "tide-tables");
    turn(c, &a, "Where is the pump house key?").await;
    turn(c, &b, "How often do we rotate the logs?").await;
    let before = c.store.transcript(&a).unwrap();
    let ids_before: Vec<String> = before.iter().map(|(_, n)| n.id.clone()).collect();
    let texts_before = said(c, &a);

    merge(c, "io-pressure", "tide-tables").unwrap();

    let after = c.store.transcript(&a).unwrap();
    let ids_after: Vec<String> = after.iter().map(|(_, n)| n.id.clone()).collect();
    assert_eq!(
        &ids_after[..ids_before.len()],
        &ids_before[..],
        "every node is there"
    );
    assert_eq!(
        ids_after.len(),
        ids_before.len() + 1,
        "and one note at the tail"
    );
    let texts_after = said(c, &a);
    assert_eq!(&texts_after[..texts_before.len()], &texts_before[..]);
    assert!(texts_after
        .last()
        .unwrap()
        .2
        .contains("merged into tide-tables"));
    let rec: SessionRecord = c.store.get_session(&a).unwrap().unwrap();
    assert!(rec.retired.is_some(), "its session is retired");
    // B was untouched mid-warm: its session is as it was.
    let b_rec: SessionRecord = c.store.get_session(&b).unwrap().unwrap();
    assert!(b_rec.retired.is_none());
    // The merged thread takes no message.
    let e = c.thread_session("io-pressure", cli()).unwrap_err();
    assert!(e.message.contains("done"), "{}", e.message);
}

/// Recall in the thread that took the merge reaches the retired thread's
/// facts, each under its own place: the nodes stay the retired thread's.
#[tokio::test]
async fn after_a_merge_recall_from_b_finds_as_facts_under_as_provenance() {
    let r = rig_with(MemoryMode::Live, |_| {});
    let c = &r.core;
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text("The key is under the blue stone."));
    let a = thread(c, "io-pressure");
    let b = thread(c, "tide-tables");
    turn(c, &a, "Where is the pump house key kept?").await;
    merge(c, "io-pressure", "tide-tables").unwrap();
    c.runner.memory.set_ask(index_of(c, vec![a.clone()]));
    let res = turn(c, &b, "Remind me where the pump house key is").await;
    assert!(res.recalled >= 1, "recall reached the model");
    let rows = recalls(c, &b);
    assert!(
        admitted(&rows[0]).contains(&a.as_str()),
        "{:?}",
        admitted(&rows[0])
    );
    let sent =
        serde_json::to_string(&r.model.requests.lock().unwrap().last().unwrap().messages).unwrap();
    assert!(sent.contains("under the blue stone"), "{sent}");
    assert!(
        sent.contains(&a),
        "A's provenance, its session, is named: {sent}"
    );
    // The distance rule's reader: A's live thread is B.
    let live = c.threads();
    assert_eq!(live.live_of("io-pressure").unwrap().slug, "tide-tables");
    assert_eq!(live.live_of("tide-tables").unwrap().slug, "tide-tables");
}

/// A seal on either thread is on the merged one; a split of a sealed thread
/// is sealed too.
#[tokio::test]
async fn a_merge_with_a_sealed_thread_is_sealed_and_so_is_a_split_of_one() {
    let r = rig(vec![Scripted::text("one"), Scripted::text("two")]);
    let c = &r.core;
    for s in ["a", "b", "c", "d"] {
        new(c, s);
    }
    seal(c, "a");
    merge(c, "a", "b").unwrap();
    assert!(
        board(c, "b").sealed.is_some(),
        "the retired thread's seal moves"
    );
    seal(c, "c");
    merge(c, "d", "c").unwrap();
    assert!(
        board(c, "c").sealed.is_some(),
        "the receiving thread's stays"
    );

    let s = thread(c, "e");
    turn(c, &s, "A question about the weir.").await;
    seal(c, "e");
    let first = pos_of(c, &s, "A question");
    let last = said(c, &s).last().unwrap().0;
    let res = c
        .thread_split(
            &ThreadSplitParams {
                from: "e".into(),
                slug: "e-part".into(),
                range: Some([first, last]),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(
        res.thread.thread.sealed.is_some(),
        "a split of a sealed thread is sealed"
    );
}

/// An unmerge puts the retired thread back as it was, opens its session
/// again, and leaves the other thread a line saying so.
#[tokio::test]
async fn an_unmerge_restores_the_retired_thread_and_notes_the_other() {
    let r = rig(vec![Scripted::text("noted")]);
    let c = &r.core;
    let a = thread(c, "io-pressure");
    new(c, "tide-tables");
    turn(c, &a, "Where is the pump house key?").await;
    c.thread_pause("io-pressure", cli()).unwrap();
    edit(
        c,
        ThreadEditParams {
            slug: "io-pressure".into(),
            card: Some(vec!["key under the blue stone".into()]),
            ..Default::default()
        },
    );
    let was = board(c, "io-pressure");
    assert_eq!(was.state, ThreadState::Paused);
    merge(c, "io-pressure", "tide-tables").unwrap();
    let rec: SessionRecord = c.store.get_session(&a).unwrap().unwrap();
    assert!(rec.retired.is_some());

    let (n, res) = frames(|| c.thread_unmerge("io-pressure", cli()));
    let res = res.unwrap();
    assert_eq!(
        n,
        1 + 1,
        "one frame for the undo, one for the session reopened"
    );
    let now = board(c, "io-pressure");
    assert_eq!(now.state, ThreadState::Paused, "as before the merge");
    assert_eq!(now.merged_into, None);
    assert_eq!(now.card, was.card);
    let rec: SessionRecord = c.store.get_session(&a).unwrap().unwrap();
    assert!(rec.retired.is_none(), "its session is open again");
    let b = board(c, "tide-tables");
    assert!(
        b.card.last().unwrap().contains("«io-pressure» unmerged"),
        "{:?}",
        b.card
    );
    assert!(
        b.card.iter().any(|l| l.starts_with("> key under")),
        "what B took stays"
    );
    assert_eq!(res.from.thread.slug, "io-pressure");
    assert_eq!(rows(c, "thread.unmerged").len(), 1);
    // Nothing to undo twice, and it can be merged again.
    assert!(c
        .thread_unmerge("io-pressure", cli())
        .unwrap_err()
        .message
        .contains("was not merged"));
    merge(c, "io-pressure", "tide-tables").unwrap();
    assert_eq!(rows(c, "thread.merged").len(), 2);
    c.thread_unmerge("io-pressure", cli()).unwrap();
}

/// A thread with ten turns across two concerns, split by position: the new
/// thread's first compile quotes exactly the chosen turns, in order, and the
/// source's request bytes are what they were with one note more.
#[tokio::test]
async fn a_split_quotes_exactly_the_chosen_turns_and_moves_nothing() {
    let mut script = Vec::new();
    for i in 0..6 {
        script.push(Scripted::text(&format!(
            "Answer {i}: {}",
            ["tide", "tide", "tide", "disk", "disk", "disk"][i]
        )));
    }
    // The model's answer in the new thread, and the source's next one.
    script.push(Scripted::text("The disk is the log volume."));
    script.push(Scripted::text("Back to tides."));
    let r = rig(script);
    let c = &r.core;
    let a = thread(c, "harbour");
    let asks = [
        "What is the tide table for Friday?",
        "And for Saturday?",
        "And the neap week?",
        "Why is the log disk filling?",
        "Which file is the biggest?",
        "Rotate it weekly, as in #41.",
    ];
    for q in asks {
        turn(c, &a, q).await;
    }
    let before = c.store.transcript(&a).unwrap();
    let last_before = r.model.requests.lock().unwrap().last().unwrap().clone();

    let (first, last) = (
        pos_of(c, &a, "Why is the log disk"),
        pos_of(c, &a, "Answer 5"),
    );
    let (_, res) = frames(|| {
        c.thread_split(
            &ThreadSplitParams {
                from: "harbour".into(),
                slug: "disk".into(),
                title: Some("The log disk".into()),
                range: Some([first, last]),
                ..Default::default()
            },
            cli(),
        )
    });
    let res = res.unwrap();
    assert_eq!(
        (res.quoted, res.first_pos, res.last_pos, res.noted),
        (6, first, last, true)
    );
    let c_row = &res.thread.thread;
    assert_eq!(c_row.split_from.as_deref(), Some("harbour"));
    assert_eq!(c_row.state, ThreadState::Active);
    assert_eq!(c_row.working_set.refs, ["#41"]);
    assert_eq!(res.thread.merged_from, Vec::<String>::new());
    assert_eq!(
        c.thread_show("harbour", cli()).unwrap().split_to,
        ["disk"],
        "the source shows where it was split to"
    );
    assert_eq!(rows(c, "thread.split").len(), 1);

    // The new session holds its brief and one arrangement of exactly those
    // messages, verbatim and in order.
    let sid = c_row.session_id.clone().expect("opened with its session");
    let nodes = c.store.transcript(&sid).unwrap();
    assert_eq!(
        nodes.len(),
        2,
        "a brief and an arrangement, nothing else yet"
    );
    let Body::Arrangement { pieces, .. } = &nodes[1].1.body else {
        panic!("{:?}", nodes[1].1.body)
    };
    let want: Vec<(String, String)> = said(c, &a)
        .into_iter()
        .filter(|(p, _, _)| (first..=last).contains(p))
        .map(|(_, id, t)| (id, t))
        .collect();
    let got: Vec<(String, String)> = pieces
        .iter()
        .map(|p| (p.node.clone(), p.text.clone().unwrap()))
        .collect();
    assert_eq!(got, want);
    assert!(got[0].1.contains("Why is the log disk") && got[5].1.contains("Answer 5"));

    // The source: every node it had, one record more (the moved range's),
    // and a compile that leaves the turns out.
    let after = c.store.transcript(&a).unwrap();
    assert_eq!(
        before
            .iter()
            .map(|(p, n)| (*p, n.id.clone()))
            .collect::<Vec<_>>(),
        after
            .iter()
            .take(before.len())
            .map(|(p, n)| (*p, n.id.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(after.len(), before.len() + 1);
    let Body::Moved { thread, nodes, .. } = &after.last().unwrap().1.body else {
        panic!("{:?}", after.last().unwrap().1.body)
    };
    assert_eq!(thread, "disk");
    assert!(
        nodes.len() >= want.len(),
        "every quoted message left the compile"
    );

    both_go_on(&r, &sid, &a, &want, &last_before).await;
}

/// C answers from the quoted turns, and A goes on with its bytes.
async fn both_go_on(
    r: &Rig,
    sid: &str,
    a: &str,
    want: &[(String, String)],
    last_before: &crate::provider::ProviderRequest,
) {
    let c = &r.core;
    let requests_before = 0usize;
    // C answers from the quoted turns: its first request holds them, and
    // none of the tide talk.
    turn(c, sid, "What was the biggest file?").await;
    let req = r.model.requests.lock().unwrap().last().unwrap().clone();
    let seen = serde_json::to_string(&req.messages).unwrap();
    for (_, t) in want {
        assert!(
            seen.contains(&t.replace('"', "\\\"")),
            "{t} is in C's first request"
        );
    }
    assert!(
        !seen.contains("tide table for Friday"),
        "the other concern stayed behind"
    );
    assert!(seen.contains("split out of the thread harbour"), "{seen}");
    // And A goes on: its request lacks the moved turns, holds the moved line
    // where they began, and keeps the rest.
    turn(c, a, "Back to the tides, please.").await;
    let reqs = r.model.requests.lock().unwrap().clone();
    assert!(reqs.len() > requests_before);
    let next = reqs.last().unwrap();
    let old = serde_json::to_string(&last_before.messages).unwrap();
    let new = serde_json::to_string(&next.messages).unwrap();
    assert!(old.contains("Why is the log disk"));
    assert_eq!(
        new.matches("Why is the log disk").count(),
        1,
        "only in the line's gist"
    );
    assert!(!new.contains("Which file is the biggest") && new.contains("tide table for Friday"));
    assert!(new.contains("moved to thread «disk»"));
}

#[test]
fn a_split_that_cannot_be_is_refused_before_anything_is_opened() {
    let r = rig(vec![]);
    let c = &r.core;
    new(c, "harbour");
    new(c, "other");
    let split = |from: &str, slug: &str, range: Option<[u64; 2]>, positions: Option<Vec<u64>>| {
        c.thread_split(
            &ThreadSplitParams {
                from: from.into(),
                slug: slug.into(),
                range,
                positions,
                ..Default::default()
            },
            cli(),
        )
        .map_err(|e| e.message)
    };
    assert!(split("harbour", "x", Some([1, 2]), None)
        .unwrap_err()
        .contains("no turns yet"));
    assert!(split("nope", "x", Some([1, 2]), None)
        .unwrap_err()
        .contains("no thread nope"));
    assert!(split("harbour", "x", None, None)
        .unwrap_err()
        .contains("name the turns"));
    assert!(split("harbour", "x", Some([1, 2]), Some(vec![3]))
        .unwrap_err()
        .contains("name the turns"));
    assert!(split("harbour", "Bad Slug", Some([1, 2]), None)
        .unwrap_err()
        .contains("not a slug"));
    assert!(rows(c, "thread.split").is_empty());
}

#[tokio::test]
async fn a_split_cannot_take_a_name_in_use_and_is_not_made_from_a_merged_thread() {
    let r = rig(vec![Scripted::text("ok")]);
    let c = &r.core;
    let a = thread(c, "harbour");
    new(c, "taken");
    turn(c, &a, "Where is the weir?").await;
    let p = pos_of(c, &a, "Where is the weir");
    let sessions_before = c.store.live_sessions::<SessionRecord>().unwrap().len();
    let ask = |from: &str, slug: &str| {
        c.thread_split(
            &ThreadSplitParams {
                from: from.into(),
                slug: slug.into(),
                range: Some([p, p]),
                ..Default::default()
            },
            cli(),
        )
        .map_err(|e| e.message)
    };
    assert!(ask("harbour", "taken").unwrap_err().contains("exists"));
    assert_eq!(
        c.store.live_sessions::<SessionRecord>().unwrap().len(),
        sessions_before,
        "no session was opened for a split that was refused"
    );
    merge(c, "harbour", "taken").unwrap();
    assert!(ask("harbour", "fresh")
        .unwrap_err()
        .contains("merged into taken"));
}

/// The model's tool: offered in a thread's session alone, it asks for the
/// owner's tap at every posture, and on the yes opens the thread from the
/// quoted messages; its result is the note the source keeps.
#[tokio::test]
async fn the_tool_waits_for_the_owners_tap_and_then_splits_by_quotes() {
    let call = Scripted::tools(
        "",
        &[(
            "s1",
            "thread_split",
            json!({"slug": "disk", "title": "The log disk",
                   "from": "Why is the log disk filling?"}),
        )],
    );
    let r = rig(vec![
        Scripted::text("Friday's high tide is at 06:12."),
        Scripted::text("The log volume is at 91%."),
        Scripted::text("plain"),
    ]);
    let c = &r.core;
    tokio::spawn(crate::harness::drive(c.clone()));
    let a = thread(c, "harbour");
    turn(c, &a, "What is the tide table for Friday?").await;
    turn(c, &a, "Why is the log disk filling?").await;
    let names = |q: &crate::provider::ProviderRequest| {
        q.tools
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert!(names(&r.model.requests.lock().unwrap()[0]).contains(&"thread_split".to_string()));
    let plain = crate::tests_recall::session(c, None, &[]);
    turn(c, &plain, "hello").await;
    let last = r.model.requests.lock().unwrap().last().unwrap().clone();
    assert!(
        !names(&last).contains(&"thread_split".to_string()),
        "a thread's tool only"
    );
    r.model
        .script
        .lock()
        .unwrap()
        .extend([call, Scripted::text("It is its own thread now.")]);

    turn(c, &a, "Move the disk part into its own thread.").await;
    let asked = c.confirm_list().unwrap();
    assert_eq!(
        asked.len(),
        1,
        "{asked:?} {:?}",
        r.model
            .requests
            .lock()
            .unwrap()
            .last()
            .map(|q| serde_json::to_string(&q.messages).unwrap())
    );
    assert_eq!(asked[0].tool, "thread.split");
    assert!(asked[0].reason.contains("layer 1"), "{}", asked[0].reason);
    assert!(c.threads().get("disk").is_none(), "nothing before the tap");
    assert!(rows(c, "thread.split").is_empty());

    c.confirm_action(&asked[0].correlation_id, true, None, "cli")
        .unwrap();
    until("the split thread", || c.threads().get("disk").is_some()).await;
    let t = board(c, "disk");
    assert_eq!(t.split_from.as_deref(), Some("harbour"));
    let rows = rows(c, "thread.split");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].data["who"], "model");
    let sid = t.session_id.clone().unwrap();
    let nodes = c.store.transcript(&sid).unwrap();
    let Body::Arrangement { pieces, .. } = &nodes[1].1.body else {
        panic!("{:?}", nodes[1].1.body)
    };
    // From the quoted message through the end of the thread before the call:
    // the question and its answer; the request to split is not among them.
    let texts: Vec<&str> = pieces.iter().map(|p| p.text.as_deref().unwrap()).collect();
    assert_eq!(
        texts,
        ["Why is the log disk filling?", "The log volume is at 91%."]
    );
    until("the call's result", || {
        said(c, &a)
            .iter()
            .any(|(_, _, t)| t.contains("Split to «disk»"))
    })
    .await;
}

/// A surface the retired thread was shown on is archived by the merge (its
/// row names it, so a surface can point it at the thread that took over) and
/// comes back with an unmerge.
#[test]
fn a_merge_archives_the_retired_threads_surfaces_and_an_unmerge_restores_them() {
    let r = rig(vec![]);
    let c = &r.core;
    new(c, "io-pressure");
    new(c, "tide-tables");
    c.runner
        .threads
        .write(
            &c.store,
            |b| {
                let mut t = b.get("io-pressure").unwrap().clone();
                t.surfaces = vec!["discord:channel:42".into()];
                Ok(vec![t])
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
    merge(c, "io-pressure", "tide-tables").unwrap();
    assert!(board(c, "io-pressure").surfaces.is_empty());
    assert_eq!(
        rows(c, "thread.merged")[0].data["archived_surfaces"],
        json!(["discord:channel:42"])
    );
    c.thread_unmerge("io-pressure", cli()).unwrap();
    assert_eq!(board(c, "io-pressure").surfaces, ["discord:channel:42"]);
}

/// The thread that took a merge is not rewritten while it is warm: its next
/// request carries the card it carried, and a recompile shows the merge.
#[tokio::test]
async fn the_receiving_thread_is_rewritten_at_its_next_recompile_not_before() {
    let r = rig(vec![
        Scripted::text("one"),
        Scripted::text("two"),
        Scripted::text("three"),
        Scripted::text("four"),
    ]);
    let c = &r.core;
    let b = thread(c, "tide-tables");
    new(c, "io-pressure");
    edit(
        c,
        ThreadEditParams {
            slug: "io-pressure".into(),
            card: Some(vec!["the log disk was at 91%".into()]),
            ..Default::default()
        },
    );
    turn(c, &b, "first").await;
    merge(c, "io-pressure", "tide-tables").unwrap();
    turn(c, &b, "second").await;
    let card = |q: &crate::provider::ProviderRequest| {
        q.messages[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let reqs = r.model.requests.lock().unwrap().clone();
    assert_eq!(card(&reqs[0]), card(&reqs[1]), "not mid-warm");
    assert!(!card(&reqs[1]).contains("merged from"));
    // The next rewrite takes the card as it stands.
    let rec: SessionRecord = c.store.get_session(&b).unwrap().unwrap();
    let (live, _) = c.live_profile();
    let target = c.runner.resolve_target(&live, None, None, None).unwrap();
    c.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            session: rec,
            input: Some("third".into()),
            target,
            sink: crate::bus::EventSink::new(c.bus.clone(), &b, None),
            author: "test".into(),
            recompile: Some(crate::compiler::Recompile::Transcript),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
            surface: None,
        })
        .await
        .unwrap();
    let reqs = r.model.requests.lock().unwrap().clone();
    let c3 = card(reqs.last().unwrap());
    assert!(
        c3.contains("merged from «io-pressure»") && c3.contains("> the log disk was at 91%"),
        "{c3}"
    );
}

/// A split goes on from its source, and a session opened for a split that did
/// not happen is retired, not left looking like a conversation.
#[tokio::test]
async fn a_split_leaves_its_source_going_and_an_unused_session_is_retired() {
    let r = rig(vec![Scripted::text("ok")]);
    let c = &r.core;
    let a = thread(c, "harbour");
    turn(c, &a, "Where is the weir?").await;
    let p = pos_of(c, &a, "Where is the weir");
    let last = said(c, &a).last().unwrap().0;
    let res = c
        .thread_split(
            &ThreadSplitParams {
                from: "harbour".into(),
                slug: "weir".into(),
                range: Some([p, last]),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(
        res.thread.thread.session_id.is_some(),
        "the split has its session"
    );
    let src: SessionRecord = c.store.get_session(&a).unwrap().unwrap();
    assert!(src.retired.is_none(), "the source goes on");

    let orphan = c.open_session(Default::default()).unwrap().session_id;
    c.drop_orphan(&orphan);
    let rec: SessionRecord = c.store.get_session(&orphan).unwrap().unwrap();
    assert!(rec.retired.is_some());
}

/// A merge, a split and an unmerge are the owner's, from a private place: a
/// person in a shared channel is refused and nothing changes (a review's
/// planted revert: the merge's judgement removed, every other test passed).
#[test]
fn a_merge_split_or_unmerge_from_a_shared_place_is_refused() {
    use theseus_protocol::error_code::REFUSED;
    let r = rig(vec![]);
    let c = &r.core;
    for s in ["a", "b"] {
        new(c, s);
    }
    let stranger = Answerer {
        label: "discord:777".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: "314159".into(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    };
    let p = ThreadMergeParams {
        from: "a".into(),
        into: "b".into(),
        ..Default::default()
    };
    let e = c.thread_merge(&p, stranger.clone()).unwrap_err();
    assert_eq!(e.code, REFUSED, "{}", e.message);
    assert_eq!(board(c, "a").state, ThreadState::Active);
    assert!(rows(c, "thread.merged").is_empty());
    let split = ThreadSplitParams {
        from: "a".into(),
        slug: "c".into(),
        range: Some([1, 2]),
        ..Default::default()
    };
    let e = c.thread_split(&split, stranger.clone()).unwrap_err();
    assert_eq!(e.code, REFUSED, "{}", e.message);
    assert!(c.threads().get("c").is_none());
    merge(c, "a", "b").unwrap();
    let e = c.thread_unmerge("a", stranger).unwrap_err();
    assert_eq!(e.code, REFUSED, "{}", e.message);
    assert_eq!(board(c, "a").merged_into.as_deref(), Some("b"));
}
