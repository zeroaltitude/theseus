//! A split on the view (issue 44, `fork::split`): after `thread.split` the home's next request does not lay
//! the moved turns and lays the split's one line instead; the thread's next request lays them, in their order
//! (an existing thread's at its tail); a split from a thread leaves that thread's window too; an unsplit puts
//! both back. Real splits through the RPC, real turns, the requests the model was sent read back.

use serde_json::{json, Value};
use theseus_protocol::threads::{ThreadNewParams, ThreadSplitParams, ThreadUnsplitParams};
use theseus_protocol::SessionOpenParams;

use crate::approval::{Answerer, Surface};
use crate::config::MemoryMode;
use crate::node::{Body, Node};
use crate::provider::Scripted;
use crate::tests_recall::{rig_with, turn, Rig};
use crate::tests_view_compile::texts;
use crate::Core;

const PLACE: &str = "dm:owner-split";

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn rig(n: usize) -> Rig {
    let r = rig_with(MemoryMode::Off, |_| {});
    (r.model.script.lock().unwrap()).extend((0..n).map(|i| Scripted::text(&format!("reply {i}"))));
    r
}

/// Turn `n` as the loop writes it: the message and the reply, each carrying `marker-<n>`.
fn add_turn(c: &Core, sid: &str, n: usize) {
    let turn_id = format!("turn_s{n:04}");
    let q = Node::user(
        sid,
        Some(&turn_id),
        "owner",
        &format!("Question {n} about marker-{n}"),
    );
    let a = Node::assistant(
        sid,
        &turn_id,
        0,
        Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": format!("Answer {n} marker-{n}")})],
            model: "fake".into(),
            provider: "fake".into(),
            stop_reason: Some("end_turn".into()),
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    c.store
        .append(&[q.record().unwrap(), a.record().unwrap()])
        .unwrap();
}

/// A home bound to a place, with turns `from..to`, then one real turn that makes their exchanges.
async fn home_with(r: &Rig, from: usize, to: usize) -> String {
    let c = &r.core;
    let sid = c
        .open_session(SessionOpenParams::default())
        .unwrap()
        .session_id;
    c.outbox.bind_place(PLACE, &sid).unwrap();
    for i in from..to {
        add_turn(c, &sid, i);
    }
    turn(c, &sid, "And what is the plan for the neap week?").await;
    sid
}

fn thread(c: &Core, slug: &str) -> String {
    let p = ThreadNewParams {
        slug: slug.into(),
        title: Some(format!("Title of {slug}")),
        ..Default::default()
    };
    c.thread_new(&p, cli()).unwrap();
    c.thread_session(slug, cli()).unwrap().session_id
}

fn pos(c: &Core, sid: &str, needle: &str) -> u64 {
    (c.store.transcript(sid).unwrap().iter())
        .find(|(_, n)| crate::arrangement::text_of(n).is_some_and(|t| t.contains(needle)))
        .unwrap_or_else(|| panic!("no node with {needle:?}"))
        .0
}

fn span(c: &Core, sid: &str, a: usize, b: usize) -> [u64; 2] {
    [
        pos(c, sid, &format!("Question {a} about")),
        pos(c, sid, &format!("Answer {b} marker")),
    ]
}

/// The text of the request the model was sent for `text`'s turn (its first call).
async fn asked(r: &Rig, sid: &str, text: &str) -> String {
    let before = r.model.requests().len();
    turn(&r.core, sid, text).await;
    let q = (r.model.requests().into_iter().skip(before))
        .find(|q| texts(q).contains(text))
        .unwrap_or_else(|| panic!("no request for {text:?}"));
    texts(&q)
}

fn split(c: &Core, p: ThreadSplitParams) -> theseus_protocol::threads::ThreadSplitResult {
    c.thread_split(&p, cli()).map_err(|e| e.message).unwrap()
}

fn once(t: &str, n: usize) -> bool {
    t.matches(&format!("Question {n} about marker-{n}")).count() == 1
}

/// The census of a session's next request: its messages as text.
fn census(c: &Core, sid: &str) -> String {
    let rec = c.store.get_session(sid).unwrap().unwrap();
    let (live, _) = c.live_profile();
    let next = c.runner.census_request(Some(&rec), "dm", &live).unwrap();
    serde_json::to_string::<Vec<Value>>(&next.compiled.request.messages).unwrap()
}

#[tokio::test]
async fn a_homes_moved_range_leaves_its_next_request_for_one_line_and_the_thread_lays_it() {
    let r = rig(8);
    let c = &r.core;
    let sid = home_with(&r, 0, 9).await;
    let range = span(c, &sid, 3, 6);
    let res = split(
        c,
        ThreadSplitParams {
            from: format!("home@{PLACE}"),
            slug: "gauges".into(),
            range: Some(range),
            gist: Some("the neap week's gauge readings".into()),
            ..Default::default()
        },
    );
    assert!(res.noted, "{res:?}");
    let home = asked(&r, &sid, "What is left for the home?").await;
    for n in 3..=6 {
        assert!(
            !home.contains(&format!("marker-{n}")),
            "marker-{n} left the home's request"
        );
    }
    for n in (0..3).chain(7..9) {
        assert!(once(&home, n), "marker-{n} stays, once");
    }
    let line = "moved to thread «gauges»: the neap week's gauge readings";
    assert_eq!(
        home.matches(line).count(),
        1,
        "one line where the range was: {home}"
    );
    let at = home.find(line).unwrap();
    assert!(
        home.find("Answer 2 marker-2").unwrap() < at && at < home.find("Question 7 about").unwrap()
    );

    let tsid = c
        .threads()
        .get("gauges")
        .unwrap()
        .session_id
        .clone()
        .unwrap();
    let theirs = asked(&r, &tsid, "Where do the gauges stand?").await;
    for n in 3..=6 {
        assert!(
            once(&theirs, n),
            "marker-{n} is in the thread's request once: {theirs}"
        );
    }
    let at = |n: usize| theirs.find(&format!("Question {n} about")).unwrap();
    assert!(
        at(3) < at(4) && at(4) < at(5) && at(5) < at(6),
        "in their order"
    );
    assert!(!theirs.contains("marker-2\"") && !theirs.contains("Question 7 about"));
}

#[tokio::test]
async fn a_split_into_an_existing_thread_lays_the_range_at_its_tail() {
    let r = rig(8);
    let c = &r.core;
    let sid = home_with(&r, 0, 7).await;
    let dev = thread(c, "dev");
    turn(c, &dev, "How is the harness built?").await;
    let range = span(c, &sid, 2, 4);
    let res = split(
        c,
        ThreadSplitParams {
            from: sid.clone(),
            into: Some("dev".into()),
            range: Some(range),
            ..Default::default()
        },
    );
    assert!(res.into, "{res:?}");
    let theirs = asked(&r, &dev, "And the build after that?").await;
    let at = |s: &str| theirs.find(s).unwrap_or_else(|| panic!("{s} in {theirs}"));
    assert!(
        at("How is the harness built?") < at("Question 2 about"),
        "after the thread's own turn"
    );
    assert!(
        at("Question 2 about") < at("Question 3 about")
            && at("Question 3 about") < at("Question 4 about")
    );
    for n in 2..=4 {
        assert!(once(&theirs, n), "marker-{n} once");
    }
}

#[tokio::test]
async fn a_split_from_a_thread_leaves_the_threads_request_too() {
    let r = rig(8);
    let c = &r.core;
    let _home = home_with(&r, 0, 2).await;
    let dev = thread(c, "dev");
    for i in 10..15 {
        add_turn(c, &dev, i);
    }
    turn(c, &dev, "Where are we on the build?").await;
    let range = span(c, &dev, 11, 12);
    split(
        c,
        ThreadSplitParams {
            from: dev.clone(),
            slug: "deeper".into(),
            range: Some(range),
            gist: Some("the linker detour".into()),
            ..Default::default()
        },
    );
    let theirs = asked(&r, &dev, "What is next on the build?").await;
    for n in 11..=12 {
        assert!(
            !theirs.contains(&format!("marker-{n}")),
            "marker-{n} left the thread's request"
        );
    }
    for n in [10, 13, 14] {
        assert!(once(&theirs, n), "marker-{n} stays");
    }
    assert_eq!(
        theirs
            .matches("moved to thread «deeper»: the linker detour")
            .count(),
        1
    );
    let deeper = c
        .threads()
        .get("deeper")
        .unwrap()
        .session_id
        .clone()
        .unwrap();
    let held = asked(&r, &deeper, "Tell me about the detour").await;
    for n in 11..=12 {
        assert!(once(&held, n), "marker-{n} in the new thread's request");
    }
}

#[tokio::test]
async fn an_unsplit_puts_both_requests_back() {
    let r = rig(8);
    let c = &r.core;
    let sid = home_with(&r, 0, 7).await;
    let dev = thread(c, "dev");
    turn(c, &dev, "How is the harness built?").await;
    let (home_before, dev_before) = (census(c, &sid), census(c, &dev));
    let res = split(
        c,
        ThreadSplitParams {
            from: sid.clone(),
            into: Some("dev".into()),
            range: Some(span(c, &sid, 1, 3)),
            ..Default::default()
        },
    );
    assert!(
        !census(c, &sid).contains("marker-2"),
        "the split took the range out"
    );
    assert!(census(c, &dev).contains("marker-2"), "and into the thread");
    let p = ThreadUnsplitParams {
        split: res.split.clone(),
        ..Default::default()
    };
    c.thread_unsplit(&p, cli()).unwrap();
    assert_eq!(
        census(c, &sid),
        home_before,
        "the home's next request is back exactly"
    );
    assert_eq!(census(c, &dev), dev_before, "the thread's is back exactly");
    let home = asked(&r, &sid, "And now?").await;
    for n in 0..7 {
        assert!(once(&home, n), "marker-{n} is back in the home");
    }
    assert!(
        !home.contains("moved to thread"),
        "no line after an unsplit"
    );
}
