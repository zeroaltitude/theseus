//! The exchange-end observer (theseus-6f8t0), with a fake model that counts
//! its requests. Every name here is invented.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::{CardObservation, StandingAddParams, StandingKind, StandingScope};

use super::extract::{self, filter, parse, Held, OwnerLine, Shown};
use super::{Seen, SWEEP_WINDOW_MS};
use crate::card::state;
use crate::judge::categorize::{Mark, EVERY};
use crate::node::{Body, Node};
use crate::places::BoundPlace;
use crate::provider::Scripted;
use crate::rpc::Core;
use crate::session::{SessionRecord, TaskOf};
use crate::tests_categorize::{rig, session, turn, Rig, OWNER};
use crate::tests_judge::kinds;

const LAB: &str = "314159265358979323";
const PRIVATE_LAB: &str = "271828182845904523";
const M1: &str = "Fix the build. I'm on the M1 now, by the way.";

/// The DM, as `session` binds it (the outbox prefixes `discord:`).
fn dm() -> String {
    format!("dm:{OWNER}")
}

/// A rig with the DM, a shared channel and a guild channel bound private.
fn rig_with(turns: usize, tweak: impl FnOnce(&mut crate::Config)) -> Rig {
    let r = rig(None, turns, tweak);
    r.core.bind_places(vec![
        BoundPlace {
            target: format!("discord:{}", dm()),
            name: "DM @wren".into(),
            private: false,
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: false,
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{PRIVATE_LAB}"),
            name: "#den".into(),
            private: true,
            ..Default::default()
        },
    ]);
    r
}

/// [`rig_with`] for a test that counts the store's frames: the memory pass
/// writes a frame of its own for the turns' nodes, `QUIET` after the last
/// turn, so on a slow run it lands between two counts and passes for the
/// observer's (theseus-n2ig3). Off, the observer's frames are the only ones.
fn rig_counting(turns: usize) -> Rig {
    rig_with(turns, |c| c.memory.mode = crate::config::MemoryMode::Off)
}

/// What the observer's tool answers with `items`.
fn answers(items: Value) -> Scripted {
    Scripted::tools(
        "",
        &[("tu_1", extract::TOOL, json!({ "observations": items }))],
    )
}

fn item(kind: &str, text: &str, evidence: &[&str]) -> Value {
    json!({"kind": kind, "text": text, "evidence": evidence, "restates": "",
           "contradicts": "", "lasting": true})
}

fn script(r: &Rig, s: Scripted) {
    r.fake.script.lock().unwrap().push_back(s);
}

/// The requests the observer made.
fn asked(r: &Rig) -> Vec<crate::provider::ProviderRequest> {
    r.fake
        .requests()
        .into_iter()
        .filter(|q| q.tools.iter().any(|t| t["name"] == extract::TOOL))
        .collect()
}

/// The owner says `text` in `sid`: the note `turn.submit` makes, then the turn.
async fn says(core: &Arc<Core>, sid: &str, text: &str) {
    core.runner.standing.expected.note_owner(sid, text);
    turn(core, sid, text).await;
    // An exchange end that finds the last one's decision still running makes
    // none (as `people.v1`'s): the next waits for the last.
    settled(core).await;
}

/// `n` owner lines, the one at `at` being `M1`.
async fn exchange(core: &Arc<Core>, sid: &str, n: usize, at: usize) {
    for i in 0..n {
        let t = if i == at {
            M1.to_string()
        } else {
            format!("Mooring line {i}: is it chafing at low tide?")
        };
        says(core, sid, &t).await;
    }
}

fn marks(core: &Core, sid: &str) -> Option<Mark> {
    core.store
        .get_meta(&format!(
            "{}{sid}",
            theseus_protocol::CARD_OBSERVE_MARK_PREFIX
        ))
        .unwrap()
}

fn observed(core: &Core) -> Vec<CardObservation> {
    state::observations(&core.store, None).unwrap()
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The decision of every session has ended.
async fn settled(core: &Core) {
    until("the observer's decisions", || {
        core.runner
            .judge
            .observing
            .deciding
            .lock()
            .unwrap()
            .is_empty()
    })
    .await;
}

fn node_ids(core: &Core, sid: &str) -> Vec<(String, String)> {
    core.store
        .scope_after(sid, 0)
        .unwrap()
        .iter()
        .filter(|r| r.kind == theseus_store::kinds::NODE)
        .filter_map(|r| r.decode::<Node>().ok())
        .filter_map(|n| match n.body {
            Body::UserMessage { text, .. } => Some((n.id, text)),
            _ => None,
        })
        .collect()
}

fn line(n: usize, text: &str) -> OwnerLine {
    OwnerLine {
        node: format!("nod_{n}"),
        text: text.into(),
    }
}

fn shown(id: &str, text: &str) -> Shown {
    Shown {
        id: id.into(),
        kind: "fact",
        scope: "everywhere".into(),
        text: text.into(),
    }
}

fn call(items: Value) -> Vec<Value> {
    vec![json!({"type": "tool_use", "id": "t", "name": extract::TOOL,
                "input": {"observations": items}})]
}

// 1. A fact said in passing inside a work request is kept, citing its node.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fact_said_in_a_work_request_is_observed_at_the_due_point() {
    let r = rig_with(EVERY, |_| {});
    let sid = session(&r.core, Some(&dm()));
    script(
        &r,
        answers(json!([item("fact", "Is on the M1 machine now", &["L3"])])),
    );
    exchange(&r.core, &sid, EVERY, 2).await;
    until("an observation", || !observed(&r.core).is_empty()).await;
    settled(&r.core).await;

    let obs = observed(&r.core);
    assert_eq!(obs.len(), 1, "{obs:?}");
    let m1 = node_ids(&r.core, &sid)
        .into_iter()
        .find(|(_, t)| t == M1)
        .unwrap()
        .0;
    assert_eq!(obs[0].nodes, vec![m1]);
    assert_eq!(obs[0].kind, theseus_protocol::ObservationKind::Fact);
    assert_eq!(obs[0].session, sid);
    assert_eq!(obs[0].state, theseus_protocol::ObservationState::Open);
    // One call, with the line in it and the one sentence as the system.
    let q = asked(&r);
    assert_eq!(q.len(), 1);
    let sent = serde_json::to_string(&q[0]).unwrap();
    assert!(sent.contains("I'm on the M1 now"), "{sent}");
    assert_eq!(q[0].system[0]["text"], extract::INSTRUCTIONS);
    // The row, the mark.
    let row = kinds(&r.core.store, "card.observed");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0].data["purpose"], "live");
    assert_eq!(row[0].data["kept"], 1);
    assert!(marks(&r.core, &sid).is_some());
}

// 2. No request in a shared channel, a guild channel bound private, a task,
// nor for a job's, the MCP server's or a prompt's message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_place_or_sender_that_is_not_the_owner_alone_gives_no_request() {
    let r = rig_with(4 * EVERY, |_| {});
    for target in [format!("channel:{LAB}"), format!("channel:{PRIVATE_LAB}")] {
        let sid = session(&r.core, Some(&target));
        script(&r, answers(json!([item("fact", "Is on the M1", &["L3"])])));
        exchange(&r.core, &sid, EVERY, 2).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(asked(&r).is_empty(), "{target}: a request was made");
        assert!(marks(&r.core, &sid).is_none());
    }
    // A task's session: the hook is told it is a task's. The turns' own ends
    // are held off (the session's decision taken meanwhile), so the lines are
    // due and unread when the task's end comes: only the flag stops it.
    let sid = session(&r.core, Some(&dm()));
    let deciding = || r.core.runner.judge.observing.deciding.lock().unwrap();
    deciding().insert(sid.clone());
    for i in 0..EVERY {
        let t = format!("Mooring line {i}: is it chafing at low tide?");
        r.core.runner.standing.expected.note_owner(&sid, &t);
        turn(&r.core, &sid, &t).await;
    }
    deciding().remove(&sid);
    let before = asked(&r).len();
    let end = crate::judge::categorize::ExchangeEnd {
        session_id: sid.clone(),
        execution_id: String::new(),
        turn_id: "trn_x".into(),
    };
    r.core.runner.judge.observe_at_exchange_end(&end, true);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(asked(&r).len(), before, "a task's end made a request");
    assert!(deciding().is_empty(), "nothing was begun");
    assert!(marks(&r.core, &sid).is_none());
    // The same end as a conversation's is read: the lines were due.
    script(&r, answers(json!([])));
    r.core.runner.judge.observe_at_exchange_end(&end, false);
    settled(&r.core).await;
    assert_eq!(
        asked(&r).len(),
        before + 1,
        "the conversation's end is read"
    );
}

// 2b. The rule turn.submit notes by: a job's message (`opened_from`), the MCP
// server's, an unnamed surface's, a prompt's and one with files are not the
// owner's own; the CLI's is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turn_submit_notes_only_the_owners_own_messages() {
    use crate::approval::Surface;
    let r = crate::tests_route::rig(None, 8, |_| {});
    let sid = {
        let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        r.core.store.put_session(&rec.session_id, &rec).unwrap();
        rec.session_id
    };
    let sent = |surface, extra: Value, text: &str| {
        let core = r.core.clone();
        let sid = sid.clone();
        let text = text.to_string();
        async move {
            let mut p = json!({"session_id": sid, "input": text});
            p.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            crate::tests_capture::call_on(&core, surface, "turn.submit", p).await
        }
    };
    let own = |text: &str| {
        node_ids(&r.core, &sid)
            .into_iter()
            .find(|(_, t)| t == text)
            .map(|(id, _)| r.core.runner.standing.expected.is_owner(&id))
    };
    sent(Surface::Cli, json!({}), "cli words").await;
    let job = sent(Surface::Cli, json!({"opened_from": sid}), "job words").await;
    assert!(job.error.is_none(), "{:?}", job.error);
    let unnamed = sent(Surface::Unnamed, json!({}), "unnamed words").await;
    assert!(unnamed.error.is_none(), "{:?}", unnamed.error);
    assert_eq!(own("cli words"), Some(true));
    // The messages were written, and none is taken for the owner's. (The MCP
    // server's refusal of a conversation it did not open leaves no node to
    // test; `owner_message` names that surface with the unnamed one.)
    for t in ["job words", "unnamed words"] {
        assert_eq!(own(t), Some(false), "{t} was taken for the owner's");
    }
}

// 3. A restatement of a live record writes no observation and counts as said again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restatement_of_a_live_record_is_counted_and_writes_nothing() {
    let r = rig_with(EVERY, |_| {});
    let rec = r
        .core
        .standing_add(
            &StandingAddParams {
                kind: StandingKind::Fact,
                text: "Works from the north jetty office".into(),
                scope: StandingScope::Everywhere,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    let id = rec.record.id.clone();
    let sid = session(&r.core, Some(&dm()));
    let mut again = item("fact", "Works from the north jetty office", &["L2"]);
    again["restates"] = json!("R1");
    script(&r, answers(json!([again])));
    exchange(&r.core, &sid, EVERY, 1).await;
    until("the row", || {
        !kinds(&r.core.store, "card.observed").is_empty()
    })
    .await;
    settled(&r.core).await;
    assert!(observed(&r.core).is_empty());
    let row = &kinds(&r.core.store, "card.observed")[0].data;
    assert_eq!(
        (
            row["returned"].clone(),
            row["kept"].clone(),
            row["restated"].clone()
        ),
        (json!(1), json!(0), json!(1))
    );
    let now = theseus_protocol::now_unix_ms();
    let held = r.core.runner.standing.capture.peek(&r.core.store).unwrap();
    assert_eq!(held.week_of(&id, now), 1);
    assert_eq!(kinds(&r.core.store, "standing.repeated").len(), 1);
    // The model was shown the record.
    assert!(serde_json::to_string(&asked(&r)[0])
        .unwrap()
        .contains("north jetty"));
}

// 2c. A due point with none of the owner's lines writes no frame: the mark
// moves in memory alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_due_point_with_none_of_the_owners_lines_writes_no_frame() {
    let r = rig_counting(EVERY);
    let sid = session(&r.core, Some(&dm()));
    // Ten messages no one noted as the owner's (a job's, say).
    for i in 0..EVERY {
        turn(&r.core, &sid, &format!("Mooring line {i}: is it chafing?")).await;
    }
    settled(&r.core).await;
    let frames = || r.core.store.stats().unwrap().frames_appended;
    let before = frames();
    assert_eq!(
        r.core.observe_session(&sid, "live").await.unwrap(),
        Seen::Idle
    );
    assert_eq!(frames(), before);
    assert!(asked(&r).is_empty() && marks(&r.core, &sid).is_none());
}

// 4. A China-origin profile is refused at the chokepoint: no request reaches it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_china_origin_profile_is_refused_at_the_chokepoint() {
    use crate::approval::Surface;
    // The load refuses a restricted `[card] profile`, so it goes in after the
    // check: the chokepoint still holds.
    let r = crate::tests_route::rig_parts(
        None,
        EVERY,
        |_| {},
        |p| p.cfg.card.profile = Some("glm".into()),
    );
    // The session carries a label: only a label keeps a session off a model of
    // restricted origin (theseus-gyjyo.6).
    let mut flags = crate::content::ContentFlags::default();
    flags.raise(crate::content::deterministic::label(
        crate::content::Flag::Confidential,
        "nod_earlier",
        crate::content::ItemKind::ToolResult,
        "name: Examplecorp".into(),
        1,
    ));
    let mut rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id;
    for i in 0..EVERY {
        let p = json!({"session_id": sid, "input": format!("Mooring line {i}: is it chafing?")});
        crate::tests_capture::call_on(&r.core, Surface::Cli, "turn.submit", p).await;
    }
    until("the refusal", || {
        !kinds(&r.core.store, "data_gate.blocked").is_empty()
    })
    .await;
    settled(&r.core).await;
    assert!(
        r.zai
            .requests()
            .iter()
            .all(|q| !q.tools.iter().any(|t| t["name"] == extract::TOOL)),
        "an observe request reached the China-origin provider"
    );
    assert!(observed(&r.core).is_empty());
    assert!(
        marks(&r.core, &sid).is_none(),
        "a refused call moves nothing"
    );
}

// 5. Off, or no money, no call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_day_line_or_observe_off_gives_no_call() {
    for tweak in [
        (|c: &mut crate::Config| c.card.day_usd = 0.0) as fn(&mut crate::Config),
        |c| c.card.observe = false,
    ] {
        let r = rig_with(EVERY, tweak);
        let sid = session(&r.core, Some(&dm()));
        script(&r, answers(json!([item("fact", "Is on the M1", &["L3"])])));
        exchange(&r.core, &sid, EVERY, 2).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(asked(&r).is_empty());
        assert!(observed(&r.core).is_empty());
        assert!(marks(&r.core, &sid).is_none());
    }
}

// 6. A failed call moves nothing; a good one writes the observations, the
// mark and the row in one frame.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_call_writes_nothing_and_a_good_one_writes_one_frame() {
    let r = rig_counting(EVERY);
    let sid = session(&r.core, Some(&dm()));
    script(
        &r,
        Scripted::Fail(crate::provider::ProviderError::Network {
            message: "the line dropped".into(),
        }),
    );
    exchange(&r.core, &sid, EVERY, 2).await;
    until("the failed call", || !asked(&r).is_empty()).await;
    settled(&r.core).await;
    assert!(observed(&r.core).is_empty());
    assert!(marks(&r.core, &sid).is_none(), "the mark stayed");
    assert!(kinds(&r.core.store, "card.observed").is_empty());

    // The retry at the next due point succeeds; its records are one frame.
    script(
        &r,
        answers(json!([item("fact", "Is on the M1 machine now", &["L3"])])),
    );
    let frames = || r.core.store.stats().unwrap().frames_appended;
    let before = frames();
    let Seen::Done(report) = r.core.observe_session(&sid, "live").await.unwrap() else {
        panic!("a due session is observed");
    };
    assert_eq!(
        frames() - before,
        1,
        "the observation, mark and row are one frame"
    );
    assert_eq!((report.returned, report.kept), (1, 1));
    // The observation, the moved mark and the row sit in one frame: their
    // positions are within the four records it holds (those three and the
    // card's state).
    use theseus_store::Store as _;
    let first = |prefix: &str| {
        r.core
            .store
            .inner()
            .latest_with_prefix(theseus_store::kinds::META, prefix)
            .unwrap()[0]
            .position
    };
    let row_at = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .find(|(_, row)| row.kind == "card.observed")
        .map(|(p, _)| p)
        .unwrap();
    let at = [first("card.obs."), first("card.observe."), row_at];
    let (lo, hi) = (at.iter().min().unwrap(), at.iter().max().unwrap());
    assert!(hi - lo <= 3, "not one frame: {at:?}");
    assert_eq!(kinds(&r.core.store, "card.observed").len(), 1);
    assert_eq!(observed(&r.core)[0].nodes.len(), 1);
    assert!(marks(&r.core, &sid).unwrap().through > 0);
}

// 7. An observation reaches no prompt, index, recall or census.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_observation_is_in_no_prompt_and_moves_no_census() {
    use theseus_protocol::census::ContextCensusParams;
    let r = rig_with(EVERY, |_| {});
    let sid = session(&r.core, Some(&dm()));
    // The census's own clock readings differ between two reads of one request.
    let census = |core: &Core| {
        let (c, req, _) = core
            .context_census(&ContextCensusParams {
                session_id: Some(sid.clone()),
                place: None,
                exact: false,
            })
            .unwrap();
        let mut c = serde_json::to_value(c).unwrap();
        for k in ["ms", "use_since_ms"] {
            c.as_object_mut().unwrap().remove(k);
        }
        (c, serde_json::to_string(&req).unwrap())
    };
    exchange(&r.core, &sid, EVERY - 1, 2).await;
    let before = census(&r.core);
    // An observation written now moves neither the census nor the request.
    let o = CardObservation {
        id: "cob_houseboat".into(),
        session: sid.clone(),
        nodes: vec![node_ids(&r.core, &sid)[2].0.clone()],
        kind: theseus_protocol::ObservationKind::Fact,
        text: "Lives aboard a houseboat".into(),
        at_ms: theseus_protocol::now_unix_ms(),
        contradicts: None,
        state: Default::default(),
        proposal: None,
    };
    r.core
        .store
        .append(&[state::obs_record(&o).unwrap()])
        .unwrap();
    assert_eq!(census(&r.core), before, "the census moved");
    // The observer's own: the tenth line is due, the call is made, and the
    // next request holds none of what it kept.
    script(
        &r,
        answers(json!([item(
            "fact",
            "Keeps a pet heron named Quill",
            &["L3"]
        )])),
    );
    says(&r.core, &sid, "One more line about moorings.").await;
    until("the observer's observation", || {
        observed(&r.core).len() == 2
    })
    .await;
    settled(&r.core).await;
    let after = census(&r.core);
    for word in ["heron", "houseboat"] {
        assert!(!after.1.contains(word), "{}", after.1);
        assert!(!after.0.to_string().contains(word));
    }
    // The text is in no search: the index and recall never read card.obs.
    let hits = r
        .core
        .store
        .scope_after(&sid, 0)
        .unwrap()
        .iter()
        .any(|x| String::from_utf8_lossy(&x.payload).contains("heron"));
    assert!(!hits, "the observation is in the session's records");
    for q in r.fake.requests() {
        if !q.tools.iter().any(|t| t["name"] == extract::TOOL) {
            assert!(!serde_json::to_string(&q).unwrap().contains("heron"));
        }
    }
}

// 8. parse drops what is not a lasting, cited, specific, short, own-worded fact.
#[test]
fn parse_drops_what_is_not_a_lasting_cited_specific_own_worded_fact() {
    let lines = [
        line(1, "I moved the office to the north jetty last spring."),
        line(2, "Fix the flaky test in this repo before lunch."),
        line(
            3,
            "Please always use the metric units for anything about the tides and the harbour moorings.",
        ),
    ];
    let long = "x".repeat(161);
    let ok = item("fact", "Works from the north jetty office", &["L1"]);
    let mut not_lasting = item("preference", "Likes short answers", &["L1"]);
    not_lasting["lasting"] = json!(false);
    let items = json!([
        ok,
        item("trait", "Is a careful person", &["L1"]),
        item("behaviour", "Rushes the build", &["L1"]),
        item("fact", "Works from a pier", &["L9"]),
        item("fact", "Works from a pier", &[]),
        not_lasting,
        item("fact", "Fixes the flaky test", &["L2"]),
        item("preference", "Wants the flaky test fixed in this repo", &["L1"]),
        item(
            "preference",
            "Please always use the metric units for anything about the tides and the harbour moorings.",
            &["L3"]
        ),
        item("fact", &long, &["L1"]),
        item("fact", "", &["L1"]),
        item("decision", "Uses the north jetty for the office", &["l1", "L1"]),
    ]);
    let got = parse(&call(items), &lines, &[]).unwrap();
    assert_eq!(got.returned, 12);
    let texts: Vec<&str> = got.items.iter().map(|i| i.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Works from the north jetty office",
            "Uses the north jetty for the office"
        ],
        "{got:?}"
    );
    assert_eq!(got.items[1].nodes, vec!["nod_1"], "a node once");
    // No tool call: nothing parsed.
    assert!(parse(&[json!({"type": "text", "text": "None."})], &lines, &[]).is_none());
    // An unknown R label drops the field, not the item.
    let mut c = item("fact", "Works from the north jetty office", &["L1"]);
    c["contradicts"] = json!("R7");
    let got = parse(
        &call(json!([c])),
        &lines,
        &[shown("std_1", "Works at the quay")],
    )
    .unwrap();
    assert_eq!((got.items.len(), got.items[0].contradicts), (1, None));
}

// 8b. filter: a restated record is counted, a contradiction maps to its record.
#[test]
fn filter_counts_a_restatement_and_keeps_a_contradiction() {
    let lines = [line(
        1,
        "I work from the east quay these days, not the north jetty.",
    )];
    let records = [shown("std_1", "Works from the north jetty office")];
    let mut contra = item("correction", "Works from the east quay office", &["L1"]);
    contra["contradicts"] = json!("R1");
    let mut said = item("fact", "Works at the north jetty office", &["L1"]);
    said["restates"] = json!("");
    let parsed = parse(&call(json!([contra, said])), &lines, &records).unwrap();
    let f = filter(parsed, &records, &Held::default());
    assert_eq!(f.restated, vec![(0, "nod_1".to_string())]);
    assert_eq!(f.keep.len(), 1);
    assert_eq!(f.keep[0].contradicts, Some(0));
    // An item the model names as both a restatement and a contradiction of one
    // record is the contradiction: kept, and not counted as said again.
    let mut both = item("correction", "Works from the east quay office", &["L1"]);
    both["restates"] = json!("R1");
    both["contradicts"] = json!("R1");
    let f = filter(
        parse(&call(json!([both])), &lines, &records).unwrap(),
        &records,
        &Held::default(),
    );
    assert!(f.restated.is_empty(), "{:?}", f.restated);
    assert_eq!(f.keep.len(), 1);
    assert_eq!(f.keep[0].contradicts, Some(0));
    // Capped at eight; an ask capture holds is not kept again.
    let things = [
        "Keeps a spare anchor aboard",
        "Reads the barometer every morning",
        "Prefers a compass to a phone for bearings",
        "Sails a grey dinghy called Quill",
        "Flies a green ensign",
        "Stores flares under the port bunk",
        "Dislikes the cutter's gaff rig",
        "Hoists the halyard before dawn",
        "Never reefs the jib early",
        "Scrapes the keel each winter",
        "Lights the lantern at six",
        "Paints the mast in April",
    ];
    let many: Vec<Value> = things.iter().map(|t| item("fact", t, &["L1"])).collect();
    let f = filter(
        parse(&call(json!(many)), &lines, &[]).unwrap(),
        &[],
        &Held::default(),
    );
    assert_eq!(f.keep.len(), extract::MAX_KEPT);
    let asked = ["Keeps a spare anchor aboard".to_string()];
    let f = filter(
        parse(
            &call(json!([item(
                "fact",
                "Keeps a spare anchor aboard",
                &["L1"]
            )])),
            &lines,
            &[],
        )
        .unwrap(),
        &[],
        &Held {
            asked: &asked,
            earlier: &[],
        },
    );
    assert!(f.keep.is_empty());
}

// 9. The sweep reads a session that never reached a due point, and skips a
// task's and a shared place's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sweep_reads_what_never_came_due_and_skips_tasks_and_shared_places() {
    let r = rig_with(4, |_| {});
    let mine = session(&r.core, Some(&dm()));
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    let task = {
        let mut rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        rec.task = Some(TaskOf {
            parent_session: mine.clone(),
            parent_execution: "exe_x".into(),
            by: "tool_x".into(),
            target: None,
            arrangement: None,
            check: None,
            notify_done: false,
        });
        r.core.store.put_session(&rec.session_id, &rec).unwrap();
        r.core.outbox.bind_place(&dm(), &rec.session_id).ok();
        rec.session_id
    };
    exchange(&r.core, &mine, 2, 1).await;
    exchange(&r.core, &shared, 2, 1).await;
    for text in ["Mooring line 0: is it chafing?", M1] {
        let n = Node::user_with(&task, None, "op", text, vec![]);
        r.core.runner.standing.expected.mark_owner(&n.id);
        let rec = theseus_store::NewRecord::json(theseus_store::kinds::NODE, Some(&n.id), &n)
            .unwrap()
            .scoped(&task);
        r.core.store.append(&[rec]).unwrap();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(asked(&r).is_empty(), "no live pass: nothing was due");
    script(
        &r,
        answers(json!([item("fact", "Is on the M1 machine now", &["L2"])])),
    );
    let now = theseus_protocol::now_unix_ms();
    let read = r.core.observe_sweep(now).await.unwrap();
    assert_eq!(read, 1);
    assert_eq!(asked(&r).len(), 1, "only the owner's own session");
    let obs = observed(&r.core);
    assert_eq!((obs.len(), obs[0].session.as_str()), (1, mine.as_str()));
    let row = kinds(&r.core.store, "card.observed");
    assert_eq!(row[0].data["purpose"], "sweep");
    // A second sweep finds nothing past the mark.
    assert_eq!(r.core.observe_sweep(now).await.unwrap(), 0);
    assert_eq!(asked(&r).len(), 1);
    // Past the window nothing is read.
    let _ = SWEEP_WINDOW_MS;
}

// 10. The join (p2k-obs + p2k-live): the card's live pass sweeps first, so a
// conversation that never reached a due point is read the same night and its
// fact is a question at once. (Without the wiring the pass's sweep was a no-op
// and found no observation.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_live_pass_sweeps_first_and_asks_of_what_it_read() {
    let r = rig_with(2, |_| {});
    let mine = session(&r.core, Some(&dm()));
    exchange(&r.core, &mine, 2, 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(asked(&r).is_empty(), "nothing was due");
    script(
        &r,
        answers(json!([item("fact", "Is on the M1 machine now", &["L2"])])),
    );
    let mut out = theseus_protocol::CardProposeResult::default();
    let made = r.core.card_live_run(false, &mut out).await.unwrap();
    assert_eq!(asked(&r).len(), 1, "the pass swept the owner's session");
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(made[0].ask, theseus_protocol::CardAsk::Add);
    assert_eq!(made[0].text, "Is on the M1 machine now");
    let obs = observed(&r.core);
    assert_eq!(obs.len(), 1);
    assert_eq!(obs[0].session, mine);
    assert_eq!(obs[0].state, theseus_protocol::ObservationState::Used);
    let row = kinds(&r.core.store, "card.observed");
    assert_eq!(row[0].data["purpose"], "sweep");
}
