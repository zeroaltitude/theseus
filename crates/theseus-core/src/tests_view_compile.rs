//! `[context] assembly` in the compiler (issue 44; `compiler::view`, `turn::view_step`): the transcript's
//! requests are the golden's whatever the setting says of other places, and a place set to `view` sends
//! its window as real turns, read across sessions, frozen within a turn, with a front that holds from one
//! turn to the next.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::config::assembly::{Assembly, AssemblyByPlace, AssemblySetting};
use crate::provider::binding::unmarked;
use crate::provider::ProviderRequest;
use crate::tests_places::{last_user, rig_with, turn, Rig};
use crate::tests_private_view::{body, owners_rig, said_earlier, session_in, with_person, ASKED};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/private_requests.txt"
);
/// `tests/golden/view_requests.txt`: a small conversation's requests in a place set to `view`.
/// `THESEUS_GOLDEN=write` writes it again.
const VIEW_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/view_requests.txt"
);

/// The golden's two requests (`tests_private_view`), made under `setting`.
async fn golden_under(setting: Option<AssemblySetting>) -> String {
    let (r, ids) = owners_rig(|c| c.context.assembly = setting);
    let c = &r.core;
    let tmp = r.root.parent().unwrap().to_path_buf();
    let mut got = String::new();
    for (name, place) in [
        ("cli", None),
        ("dm", Some(format!("dm:{}", crate::tests_places::OWNER))),
    ] {
        let sid = session_in(c, place.as_deref(), &r.root);
        with_person(c, &sid);
        said_earlier(c, &sid);
        turn(c, &sid, ASKED).await;
        let req = r.requests().pop().unwrap();
        got.push_str(&format!(
            "== {name}\n{}",
            body(&req, &tmp, &[&[sid], &ids[..]].concat())
        ));
    }
    got
}

/// With `transcript`, said outright or as the default of a table that sets another place to `view`, every
/// golden request is byte for byte what it is with no setting at all.
#[tokio::test]
async fn the_transcript_assembly_sends_the_golden_requests_byte_for_byte() {
    let want = std::fs::read_to_string(GOLDEN).unwrap();
    let elsewhere = AssemblyByPlace {
        default: Assembly::Transcript,
        places: BTreeMap::from([("discord:elsewhere".to_string(), Assembly::View)]),
    };
    for setting in [
        None,
        Some(AssemblySetting::All(Assembly::Transcript)),
        Some(AssemblySetting::ByPlace(elsewhere)),
    ] {
        let got = golden_under(setting.clone()).await;
        assert!(got == want, "{setting:?} moved the golden requests:\n{got}");
    }
}

fn view_rig() -> Rig {
    rig_with(|c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)))
}

/// The conversation's requests whose newest user message says `what`, in the order they were sent: a
/// request with tools (the ladder's own model calls, a did line's, offer none).
fn asked(r: &Rig, what: &str) -> Vec<ProviderRequest> {
    let got: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && last_user(q).contains(what))
        .collect();
    got.iter().for_each(legal);
    got
}

/// The provider's rules on an unscrubbed request: it opens with the person; roles alternate; every call
/// has its result in the very next message and every result answers a call of the message before it, each
/// once; no message is empty; at most four cache breakpoints.
fn legal(q: &ProviderRequest) {
    let ids = |m: &Value, kind: &str, key: &str| -> Vec<String> {
        (m["content"].as_array().into_iter().flatten())
            .filter(|b| b["type"] == kind)
            .filter_map(|b| b[key].as_str().map(str::to_string))
            .collect()
    };
    assert_eq!(
        q.messages.first().map(|m| &m["role"]),
        Some(&Value::from("user"))
    );
    for (i, m) in q.messages.iter().enumerate() {
        let content = m["content"].as_array().map_or(0, Vec::len);
        assert!(
            content > 0 || m["content"].is_string(),
            "message {i} is empty"
        );
        if i > 0 {
            assert_ne!(
                m["role"],
                q.messages[i - 1]["role"],
                "roles alternate at {i}"
            );
        }
        let uses = ids(m, "tool_use", "id");
        let answered = q
            .messages
            .get(i + 1)
            .map_or(vec![], |n| ids(n, "tool_result", "tool_use_id"));
        for u in &uses {
            assert_eq!(
                answered.iter().filter(|a| *a == u).count(),
                1,
                "call {u} answered once"
            );
        }
        let results = ids(m, "tool_result", "tool_use_id");
        let asked = (i.checked_sub(1)).map_or(vec![], |p| ids(&q.messages[p], "tool_use", "id"));
        for r in &results {
            assert!(
                asked.contains(r),
                "result {r} answers a call of the message before"
            );
        }
    }
    assert!(breakpoints(q) <= 4, "{} breakpoints", breakpoints(q));
}

/// A request's messages without their cache marks: what the provider's prefix is compared by.
fn plain(q: &ProviderRequest) -> Vec<Value> {
    q.messages.iter().map(unmarked).collect()
}

fn texts(q: &ProviderRequest) -> String {
    serde_json::to_string(&q.messages).unwrap()
}

/// Every call has its result in the message after it, and the roles alternate.
fn paired(q: &ProviderRequest) {
    for (i, m) in q.messages.iter().enumerate() {
        if i > 0 {
            assert_ne!(
                m["role"],
                q.messages[i - 1]["role"],
                "roles alternate at {i}"
            );
        }
        let uses: Vec<&str> = (m["content"].as_array().into_iter().flatten())
            .filter(|b| b["type"] == "tool_use")
            .filter_map(|b| b["id"].as_str())
            .collect();
        for id in uses {
            let next = &q.messages[i + 1]["content"];
            let answered = (next.as_array().into_iter().flatten())
                .any(|b| b["type"] == "tool_result" && b["tool_use_id"] == id);
            assert!(answered, "call {id} has its result after it");
        }
    }
}

/// How many cache breakpoints a request carries, the top-level one included.
fn breakpoints(q: &ProviderRequest) -> usize {
    let marked = |v: &Value| v.get("cache_control").is_some();
    let blocks = (q.messages.iter())
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter(marked)
        .count();
    q.system.iter().filter(|b| marked(b)).count() + blocks + usize::from(q.cache_control.is_some())
}

/// A place set to `view`: the window is the place's earlier exchanges as real turns, calls paired with
/// their results; each call of a turn begins with the bytes of the call before it; and the next turn's
/// request begins with this turn's up to the end of its window.
#[tokio::test]
async fn a_view_place_sends_its_window_as_real_turns_frozen_within_a_turn() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "first: the tide").await;
    turn(c, &sid, "look around").await;
    turn(c, &sid, "third: and now").await;

    // The conversation's requests from the tool turn on: its two calls, then the third turn's.
    let calls: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && texts(q).contains("look around"))
        .collect();
    assert_eq!(
        calls.len(),
        3,
        "the tool turn made two calls, the next turn one"
    );
    let (second, third) = (&calls[..2], &calls[2]);
    assert!(last_user(third).contains("third: and now"));
    let all = texts(third);
    for said in [
        "first: the tide",
        "Hello.",
        "look around",
        "Looking.",
        "Done.",
    ] {
        assert!(all.contains(said), "the window carries {said:?}: {all}");
    }
    paired(third);
    calls.iter().for_each(legal);
    // Frozen within a turn: the second call's request begins with the first's messages, byte for byte.
    let (a, b) = (plain(&second[0]), plain(&second[1]));
    assert_eq!(
        b[..a.len()],
        a[..],
        "the turn's second call repeats its first"
    );
    assert_eq!(second[0].system, second[1].system);
    // The fixed front: turn 2's window (turn 1's exchange) heads turn 3's request unchanged.
    let window = a.len() - 1;
    assert_eq!(
        plain(third)[..window],
        a[..window],
        "the front holds across turns"
    );
    assert_eq!(third.system, second[0].system);
    // The window's end carries the mark, within the provider's four.
    if third.cache_control.is_some() {
        let end = &third.messages[third.messages.len() - 2]["content"];
        let last = end.as_array().and_then(|b| b.last()).unwrap();
        assert!(
            last.get("cache_control").is_some(),
            "the window's end is marked"
        );
    }
    assert!(
        breakpoints(third) <= 4,
        "{} breakpoints",
        breakpoints(third)
    );
}

/// The window is the place's, not the session's: a new session in the same place sees the exchange the
/// earlier one ran; the transcript's does not.
#[tokio::test]
async fn a_view_window_reads_the_place_across_its_sessions() {
    for (view, sees) in [(true, true), (false, false)] {
        let r = match view {
            true => view_rig(),
            false => rig_with(|_| {}),
        };
        let c = &r.core;
        let before = session_in(c, None, &r.root);
        turn(c, &before, "alpha: the earlier words").await;
        let after = session_in(c, None, &r.root);
        turn(c, &after, "beta: what came before").await;
        let q = asked(&r, "beta: what came before").pop().unwrap();
        assert_eq!(
            texts(&q).contains("alpha: the earlier words"),
            sees,
            "view {view}: {}",
            texts(&q)
        );
    }
}

/// The fault of 2026-11-21, replayed in `view` with no judge in the way: a reply minutes after an answer,
/// naming nothing, is sent with the answer before it, as a real turn of the window.
#[tokio::test]
async fn a_reply_that_names_nothing_sees_the_answer_before_it() {
    let r = crate::tests_places::rig_scripted(
        |c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)),
        |q| match last_user(q).contains("which door") {
            true => crate::provider::Scripted::text("The north door, code 4417."),
            false => crate::provider::Scripted::text("Noted."),
        },
    );
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "which door do I use tonight?").await;
    turn(c, &sid, "ok and after that?").await;
    let q = asked(&r, "ok and after that?").pop().unwrap();
    let at = |what: &str| {
        (q.messages.iter())
            .position(|m| serde_json::to_string(m).unwrap().contains(what))
            .unwrap_or_else(|| panic!("the request carries {what:?}: {}", texts(&q)))
    };
    assert_eq!(q.messages[at("north door, code 4417")]["role"], "assistant");
    assert!(at("which door") < at("north door") && at("north door") < at("ok and after that?"));
    // No note of recall is written into the session in a view place.
    let notes = (c.store.session_nodes(&sid).unwrap().into_iter())
        .filter(|(_, n)| matches!(n.body, crate::node::Body::Recall { .. }))
        .count();
    assert_eq!(notes, 0, "no recall note in the transcript");
}

/// The next morning: the window is counted in exchanges, so eleven hours on it is the same window.
#[tokio::test]
async fn eleven_hours_later_the_window_is_unchanged() {
    use crate::ladder::view::StoreLadder;
    use crate::view::fold::Ladder;
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "evening: the plan").await;
    turn(c, &sid, "evening: the second step").await;
    // The place as the ledger wrote it for these turns.
    let turn_id = (c.store.session_nodes(&sid).unwrap().into_iter())
        .find_map(|(_, n)| n.turn_id)
        .unwrap();
    let place = crate::exchange::exchange(&c.store, &turn_id)
        .unwrap()
        .expect("the turn's exchange")
        .place;
    let ladder = StoreLadder(&c.store);
    let tonight = ladder
        .read(&place, crate::view::fold::WINDOW_N, None)
        .unwrap();
    assert_eq!(tonight.window.len(), 2, "{tonight:?}");
    // The window is counted in exchanges and reads no clock: the same read eleven hours on.
    let snap = crate::view::wire::StoreSnapshot {
        now_ms: theseus_protocol::now_unix_ms() + 11 * 3_600_000,
        timeline: crate::view::wire::StoreTimeline(&c.store),
        at_hand: None,
    };
    let pov = crate::view::PointOfView::compute(
        &crate::view::Arrival {
            anchor: crate::view::Anchor::Place(place.clone()),
            person: None,
            owner_marked: false,
            audience: crate::view::Audience::Private,
        },
        &snap,
    );
    let renderer = crate::view::wire_text::TextRenderer {
        store: &c.store,
        kernel: None,
        sessions: None,
        zone: &crate::recall::stamp::system,
    };
    let parts = crate::view::wire::Parts {
        selectors: &[],
        gate: &crate::view::permit::Permit::of(crate::view_gate::Verified::for_tests(
            theseus_memory::gate::PointOfView {
                anchor: place,
                place: theseus_memory::recall::Place::Private,
                present: theseus_memory::gate::Present::OwnerAlone,
            },
        )),
        renderer: &renderer,
        ladder: &ladder,
        since: None,
    };
    let budget = crate::view::budget::Budget::new(200_000, 8_000);
    let morning = crate::view::wire::assemble(&pov, &snap, &parts, &budget).unwrap();
    assert_eq!(
        morning.ladder, tonight,
        "the next morning's window is the evening's"
    );
}

/// A place in company: its request carries its own exchanges and nothing said elsewhere.
#[tokio::test]
async fn a_place_in_company_gets_nothing_from_elsewhere() {
    let r = view_rig();
    let c = &r.core;
    let private = session_in(c, None, &r.root);
    turn(c, &private, "private: the owner's own words").await;
    let lab = format!("channel:{}", crate::tests_places::LAB);
    let shared = crate::tests_places::session(c, Some(&lab));
    turn(c, &shared, "shared: first here").await;
    turn(c, &shared, "shared: second here").await;
    let q = asked(&r, "shared: second here").pop().unwrap();
    let all = texts(&q);
    assert!(all.contains("shared: first here"), "its own window: {all}");
    assert!(
        !all.contains("the owner's own words"),
        "nothing from elsewhere: {all}"
    );
}

/// Keep-warm is off for a place in `view`: its next request is built afresh, so there is no transcript
/// prefix to read; the same conversation under the transcript is kept warm.
#[tokio::test]
async fn a_view_place_is_not_kept_warm() {
    use crate::keep_warm::Outcome;
    use crate::tests_keep_warm::{build, one_hour, reads, session, turn};
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour(24);
    c.context.assembly = Some(AssemblySetting::All(Assembly::View));
    let (core, fake) = build(dir.path(), c);
    let live = core.live_profile().0;
    let sid = session(&core, theseus_protocol::SessionKind::Conversation, None);
    turn(&core, &sid, "a conversation in a view place").await;
    let got = core.runner.keep_warm_once(&sid, &live).await;
    assert_eq!(got, Outcome::Forgotten("its place is set to view".into()));
    assert!(reads(&fake).is_empty());
}

/// A small conversation in a place set to `view`, byte for byte: three turns, the second with three calls
/// and their results; each request as the model is sent it.
#[tokio::test]
async fn a_small_view_conversations_requests_are_the_golden() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    for said in ["first: the tide", "look around", "third: and now"] {
        turn(c, &sid, said).await;
    }
    let tmp = r.root.parent().unwrap().to_path_buf();
    let mut got = String::new();
    let calls = (r.requests().into_iter()).filter(|q| !q.tools.is_empty());
    for (i, q) in calls.enumerate() {
        got.push_str(&format!(
            "== call {i}\n{}",
            body(&q, &tmp, std::slice::from_ref(&sid))
        ));
    }
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        let to = std::env::var("THESEUS_GOLDEN_TO").unwrap_or_else(|_| VIEW_GOLDEN.to_string());
        std::fs::write(to, &got).unwrap();
    }
    let want = std::fs::read_to_string(VIEW_GOLDEN).unwrap_or_default();
    assert!(
        got == want,
        "a view conversation's requests moved from tests/golden/view_requests.txt \
         (THESEUS_GOLDEN=write writes it again):\n{got}"
    );
}

/// A turn of `sid` whose input `turn.submit` marked as typed by the owner in person, or by an agent's
/// process (the gate's own test mark, `view_gate::test_mark`).
async fn turn_as(core: &std::sync::Arc<crate::Core>, sid: &str, input: &str, in_person: bool) {
    use crate::session::SessionRecord;
    let mark = crate::view_gate::test_mark(&core.runner, sid, in_person);
    let rec = (core.store.get_session::<SessionRecord>(sid).unwrap()).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// The owner's words in his DM, then turns in other places set to view. The point of view's presence is
/// the gate's, verified from the turn's own mark and the place read on its own: an agent's process at the
/// terminal, and the owner's own marked message in a channel bound private (decision 5: a channel is never
/// alone), get nothing from the DM; the owner in person at the terminal is the owner alone.
#[tokio::test]
async fn a_view_turn_not_the_owner_alone_gets_nothing_from_another_place() {
    use crate::view_gate::Verified;
    use theseus_memory::gate::Present;
    const HIS: &str = "the plum orchard gate code is 2718";
    let r = view_rig();
    let c = &r.core;
    c.bind_places(vec![crate::places::BoundPlace {
        persona: None,
        target: format!("discord:channel:{}", crate::tests_places::LAB),
        name: "#lab".into(),
        private: true,
        guild: Some("100000000000000001".into()),
        ceiling: None,
    }]);
    let dm = crate::tests_places::session(c, Some(&format!("dm:{}", crate::tests_places::OWNER)));
    turn_as(c, &dm, HIS, true).await;

    let agent = session_in(c, None, &r.root);
    turn_as(c, &agent, "agent: what did he say?", false).await;
    let present = |sid: &str| Verified::of_session(&c.runner, sid).pov().present;
    assert_eq!(present(&agent), Present::PrivateChannel);
    let lab = format!("channel:{}", crate::tests_places::LAB);
    let channel = crate::tests_places::session(c, Some(&lab));
    turn_as(c, &channel, "lab: what did he say?", true).await;
    assert_eq!(
        present(&channel),
        Present::PrivateChannel,
        "a channel is never alone"
    );
    for asked_in in ["agent: what did he say?", "lab: what did he say?"] {
        let q = asked(&r, asked_in).pop().unwrap();
        assert!(!texts(&q).contains(HIS), "{asked_in}: {}", texts(&q));
    }
    let owner = session_in(c, None, &r.root);
    turn_as(c, &owner, "owner: what did I say?", true).await;
    assert_eq!(present(&owner), Present::OwnerAlone);
}

/// A view place's request offers the tools the transcript's does: the history tool among them.
#[tokio::test]
async fn a_view_place_offers_the_history_tool_as_the_transcript_does() {
    let mut offered = Vec::new();
    for r in [view_rig(), rig_with(|_| {})] {
        let sid = session_in(&r.core, None, &r.root);
        turn(&r.core, &sid, "tools: which?").await;
        let q = asked(&r, "tools: which?").pop().unwrap();
        offered.push(crate::tests_places::offered(&q));
    }
    assert!(
        offered[0].iter().any(|n| n == "context_history"),
        "{:?}",
        offered[0]
    );
    assert_eq!(offered[0], offered[1]);
}

/// The view runs the transcript's spec preparation: with tools deferred (the owner's direct conversation
/// defers them) a view turn offers the resolved set the transcript's turn does, not the raw catalog.
#[tokio::test]
async fn a_view_turn_offers_the_resolved_deferred_tools() {
    let mut offered = Vec::new();
    for view in [true, false] {
        let r = crate::tests_places::rig_deferring(
            |c| {
                c.context.assembly = view.then_some(AssemblySetting::All(Assembly::View));
            },
            |_| crate::provider::Scripted::text("Hello."),
        );
        let sid = session_in(&r.core, None, &r.root);
        turn(&r.core, &sid, "deferred: which tools?").await;
        let q = (r.requests().into_iter())
            .rfind(|q| last_user(q).contains("deferred: which tools?"))
            .unwrap();
        offered.push(crate::tests_places::offered(&q));
    }
    assert_eq!(
        offered[0], offered[1],
        "the view offers the transcript's resolved set"
    );
}

/// The view runs the transcript's spec preparation: a conversation whose ontology membership carries
/// guidance has that guidance in its view request's system part.
#[tokio::test]
async fn a_view_turn_carries_its_memberships_guidance() {
    let (r, _) = owners_rig(|c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)));
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    with_person(c, &sid);
    turn(c, &sid, ASKED).await;
    let q = r.requests().pop().unwrap();
    let system = crate::tests_places::system_text(&q);
    assert!(system.contains("is the owner's accountant"), "{system}");
}

/// A thread `kelp`, bound to its own session, sealed as `seal` says.
fn kelp(store: &crate::store::Store, seal: bool) -> String {
    use crate::session::SessionRecord;
    let r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    store.put_session(&r.session_id, &r).unwrap();
    let mut t = serde_json::json!({"slug": "kelp", "title": "kelp"});
    if seal {
        t["sealed"] = serde_json::json!({"by": "cli", "at_ms": 1, "reason": "embargo"});
    }
    store.put_meta("thread:kelp", &t).unwrap();
    store
        .put_meta("discord.session.thread:kelp", &r.session_id)
        .unwrap();
    r.session_id
}

/// A sealed thread's exchange in the SAME place is not in the window: the seal is the gate's own reading
/// of the exchange's session. Unsealed, the same exchange is.
#[tokio::test]
async fn a_sealed_threads_exchange_in_the_same_place_is_not_in_the_window() {
    const KELP: &str = "kelp: the embargoed figure is 7741";
    for seal in [false, true] {
        let thread = std::cell::RefCell::new(String::new());
        let r = crate::tests_places::rig_setup(
            |c| {
                c.context.assembly = Some(AssemblySetting::All(Assembly::View));
                c.threads.enabled = true;
            },
            |s| *thread.borrow_mut() = kelp(s, seal),
        );
        let c = &r.core;
        let thread = thread.into_inner();
        turn_as(c, &thread, KELP, true).await;
        let desk = crate::tests_places::session(c, None);
        turn_as(c, &desk, "desk: and now?", true).await;
        let q = asked(&r, "desk: and now?").pop().unwrap();
        assert_eq!(
            texts(&q).contains("7741"),
            !seal,
            "sealed {seal}: {}",
            texts(&q)
        );
    }
}

/// A seal applied between two calls of one turn: the turn's next call is a fresh chain without the sealed
/// exchange.
#[tokio::test]
async fn a_seal_between_two_calls_of_a_turn_ends_the_chain() {
    use std::sync::{Arc, OnceLock};
    const KELP: &str = "kelp: the embargoed figure is 7741";
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
    let held = core.clone();
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| *thread.borrow_mut() = kelp(s, false),
        move |q| {
            let last = last_user(q);
            if last.contains("seal it now") && !crate::tests_places::answers_a_call(q) {
                let c = held.get().expect("the core");
                seal_kelp(c);
                return crate::provider::Scripted::tools(
                    "Looking.",
                    &[("s1", "fs_list", serde_json::json!({"path": "."}))],
                );
            }
            crate::provider::Scripted::text("Done.")
        },
    );
    let _ = core.set(r.core.clone());
    let c = &r.core;
    let thread = thread.into_inner();
    turn_as(c, &thread, KELP, true).await;
    let desk = crate::tests_places::session(c, None);
    turn_as(c, &desk, "desk: seal it now", true).await;
    let calls: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| texts(q).contains("desk: seal it now"))
        .collect();
    assert_eq!(calls.len(), 2, "the turn made two calls");
    assert!(
        texts(&calls[0]).contains("7741"),
        "before the seal: {}",
        texts(&calls[0])
    );
    assert!(
        !texts(&calls[1]).contains("7741"),
        "after it: {}",
        texts(&calls[1])
    );
}

/// Seal the thread `kelp` through the thread board's own write.
fn seal_kelp(c: &crate::Core) {
    c.runner
        .threads
        .write(
            &c.store,
            |b| {
                let mut v = serde_json::to_value(b.get("kelp").expect("kelp")).unwrap();
                v["sealed"] = serde_json::json!({"by": "cli", "at_ms": 2, "reason": "embargo"});
                Ok(vec![serde_json::from_value(v)?])
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
}

/// One ladder read per assembly: a fold that lands between two reads of the same place does not reach the
/// assembly, so every admitted range has one representation.
#[test]
fn an_assembly_reads_the_ladder_once() {
    use crate::view::fold::{Ladder, LadderRead};
    use crate::view::timeline::ExchangeRef;
    use std::cell::Cell;
    struct Folding(Cell<u32>);
    impl Ladder for Folding {
        fn read(
            &self,
            _: &str,
            _: usize,
            _: Option<&crate::view::Pos>,
        ) -> anyhow::Result<LadderRead> {
            self.0.set(self.0.get() + 1);
            let e = |i: u32| ExchangeRef {
                turn_id: format!("t{i}"),
                session_id: "s".into(),
                at_ms: u64::from(i),
            };
            // The first read sees two exchanges; any later read would see them folded away.
            let window = match self.0.get() {
                1 => vec![e(1), e(2)],
                _ => vec![e(3)],
            };
            Ok(LadderRead {
                nodes: Vec::new(),
                window,
            })
        }
    }
    let inner = Folding(Cell::new(0));
    let once = crate::view::wire::OnceLadder::new(&inner);
    let a = once.read("local", 20, None).unwrap();
    let b = once.read("local", 20, None).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.window.len(), 2);
    assert_eq!(inner.0.get(), 1, "the store's ladder is read once");
}

/// The `context.view_fallback` rows' reasons.
fn fallbacks(c: &crate::Core) -> Vec<String> {
    (crate::test_support::rows(c, "context.view_fallback").into_iter())
        .filter_map(|r| r["why"].as_str().map(str::to_string))
        .collect()
}

/// A turn of `sid` submitted as the driver does, its outcome returned (a turn may fail on its own).
async fn submit(
    core: &std::sync::Arc<crate::Core>,
    sid: &str,
    input: &str,
) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    use crate::session::SessionRecord;
    let rec = (core.store.get_session::<SessionRecord>(sid).unwrap()).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let mark = crate::view_gate::test_mark(&core.runner, sid, true);
    let sink = crate::bus::EventSink::new(core.bus.clone(), sid, None).with_arrival(Some(mark));
    core.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// Fallback (c): an established session switched to view, whose earlier conversation the ledger holds no
/// exchange for, is the transcript's turn: "apply the second option" is sent with the options.
#[tokio::test]
async fn an_established_session_without_its_exchanges_is_the_transcripts_turn() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let earlier = crate::node::Node::user(
        &sid,
        Some("turn_old"),
        "test",
        "options: the red one, or the blue one",
    );
    c.store.append(&[earlier.record().unwrap()]).unwrap();
    turn(c, &sid, "apply the second option").await;
    let q = r.requests().pop().unwrap();
    assert!(texts(&q).contains("the blue one"), "{}", texts(&q));
    assert!(
        fallbacks(c).contains(&"unledgered_history".to_string()),
        "{:?}",
        fallbacks(c)
    );
}

/// Fallback (b): a view whose rendered request is over the budget is the transcript's turn, whose ring and
/// compaction make room; the ledger says why.
#[tokio::test]
async fn a_view_over_its_budget_is_the_transcripts_turn() {
    let r = rig_with(|c| {
        c.context.assembly = Some(AssemblySetting::All(Assembly::View));
        c.context.working_window_tokens = Some(12_000);
    });
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    // The transcript's turn may itself be over so small a window: its own overage says so.
    let _ = submit(c, &sid, "small window: hello").await;
    assert!(
        fallbacks(c).contains(&"over_budget".to_string()),
        "{:?}",
        fallbacks(c)
    );
}

/// P1-A: a steer that narrows the turn's presence (the gate's own `steered`) before its next call: the view
/// is reassembled under the new permit, so the owner's words from his DM, admitted while he was alone, are
/// not in the next request.
#[tokio::test]
async fn a_steer_that_narrows_the_presence_reassembles_the_view() {
    use std::sync::{Arc, Mutex, OnceLock};
    const HIS: &str = "the plum orchard gate code is 2718";
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
    let desk: Arc<Mutex<String>> = Arc::default();
    let (held, at) = (core.clone(), desk.clone());
    let r = crate::tests_places::rig_setup_scripted(
        |c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)),
        |_| {},
        move |q| {
            if last_user(q).contains("desk: narrow me") && !crate::tests_places::answers_a_call(q) {
                let c = held.get().expect("the core");
                crate::view_gate::steered(&c.runner, &at.lock().unwrap(), None);
                return crate::provider::Scripted::tools(
                    "Looking.",
                    &[("n1", "fs_list", serde_json::json!({"path": "."}))],
                );
            }
            crate::provider::Scripted::text("Done.")
        },
    );
    let _ = core.set(r.core.clone());
    let c = &r.core;
    let dm = crate::tests_places::session(c, Some(&format!("dm:{}", crate::tests_places::OWNER)));
    turn_as(c, &dm, HIS, true).await;
    let sid = crate::tests_places::session(c, None);
    *desk.lock().unwrap() = sid.clone();
    turn_as(c, &sid, "desk: narrow me", true).await;
    let calls: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| texts(q).contains("desk: narrow me"))
        .collect();
    assert_eq!(calls.len(), 2, "the turn made two calls");
    calls.iter().for_each(legal);
    assert!(
        !texts(&calls[1]).contains(HIS),
        "after the steer: {}",
        texts(&calls[1])
    );
}

/// P1-B: a fold node built while its thread was clear is dropped whole (its account with its words) once
/// the thread is sealed: the gate reads its constituent exchanges' sessions today.
#[tokio::test]
async fn a_fold_over_a_thread_sealed_after_it_was_built_is_dropped() {
    const KELP: &str = "kelp: the embargoed figure is 7741";
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| *thread.borrow_mut() = kelp(s, false),
    );
    let c = &r.core;
    let thread = thread.into_inner();
    turn_as(c, &thread, KELP, true).await;
    let turn_id = (c.store.session_nodes(&thread).unwrap().into_iter())
        .find_map(|(_, n)| n.turn_id)
        .unwrap();
    let ex = crate::exchange::exchange(&c.store, &turn_id)
        .unwrap()
        .unwrap();
    let node = crate::ladder::fold::FoldNode {
        id: "fold-kelp-1".into(),
        place: ex.place.clone(),
        level: 1,
        exchanges: vec![turn_id.clone()],
        first_ms: ex.start_ms,
        last_ms: ex.start_ms,
        his: vec![],
        counted: 0,
        children: vec![],
        title: "kelp".into(),
        account: "ACCOUNT: the figure 7741 was discussed".into(),
        marks: crate::view::Marks {
            place: Some(ex.place.clone()),
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        model: None,
        writer: 1,
        at_ms: ex.start_ms,
    };
    c.store
        .append(&crate::ladder::fold::records(&node).unwrap())
        .unwrap();
    let desk = crate::tests_places::session(c, None);
    turn_as(c, &desk, "desk: before the seal", true).await;
    let before = asked(&r, "desk: before the seal").pop().unwrap();
    assert!(
        texts(&before).contains("ACCOUNT: the figure"),
        "{}",
        texts(&before)
    );
    seal_kelp(c);
    turn_as(c, &desk, "desk: after the seal", true).await;
    let after = asked(&r, "desk: after the seal").pop().unwrap();
    assert!(!texts(&after).contains("7741"), "{}", texts(&after));
}

/// A view turn that declines is the transcript's turn in full: its live recall, withheld while the view
/// was tried, is carried in the same call's request as on any transcript turn. Here the decline is
/// `unledgered_history` (an earlier turn of the session has no exchange), and the recalled episode, said
/// in another session and absent from this transcript, is in the provider's request.
#[tokio::test]
async fn a_declined_view_turn_carries_its_recall() {
    let (r, _) = owners_rig(|c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)));
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let earlier = crate::node::Node::user(&sid, Some("turn_old"), "test", "an earlier word");
    c.store.append(&[earlier.record().unwrap()]).unwrap();
    turn(c, &sid, ASKED).await;
    assert!(
        fallbacks(c).contains(&"unledgered_history".to_string()),
        "{:?}",
        fallbacks(c)
    );
    let q = r.requests().pop().unwrap();
    assert!(
        texts(&q).contains(crate::tests_private_view::EPISODE),
        "the recalled episode rides the declined turn's request: {}",
        texts(&q)
    );
}

/// Item 1 of round 2: the view's first call is refused by the provider for its size; the turn declines to
/// the transcript (`over_budget`, held), whose retry carries the turn's recall, kept since the view's
/// dispatch: the recalled episode rides the retry and the call after it exactly once each, and its Recall
/// record is written once. One fallback row, three calls in all.
#[tokio::test]
async fn a_view_call_refused_for_size_declines_and_keeps_its_recall() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    let calls = Arc::new(AtomicU32::new(0));
    let seen = calls.clone();
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.memory.mode = crate::config::MemoryMode::Live;
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
        },
        |_| {},
        move |q| {
            if q.tools.is_empty() {
                return crate::provider::Scripted::text("aside");
            }
            match seen.fetch_add(1, Ordering::SeqCst) {
                0 => crate::provider::Scripted::Fail(crate::provider::ProviderError::InvalidRequest {
                    status: 400,
                    message: "invalid_request_error: prompt is too long: 250000 tokens > 200000 maximum"
                        .into(),
                }),
                1 => crate::provider::Scripted::tools(
                    "Looking.",
                    &[("r1", "fs_list", serde_json::json!({"path": "."}))],
                ),
                _ => crate::provider::Scripted::text("Done."),
            }
        },
    );
    let c = &r.core;
    let earlier = crate::tests_places::session(c, None);
    let episode = crate::tests_private_view::EPISODE;
    let mut n = crate::node::Node::user(&earlier, None, "test", episode);
    n.created_at_ms = 1_767_787_200_000;
    c.store.append(&[n.record().unwrap()]).unwrap();
    (c.runner.memory).set_ask(crate::tests_private_view::index_of(c, vec![earlier]));
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, ASKED).await;
    let asked: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && texts(q).contains(ASKED))
        .collect();
    assert_eq!(
        asked.len(),
        3,
        "the refused view call, the retry, the call after the result"
    );
    for q in &asked[1..] {
        assert_eq!(
            texts(q).matches(episode).count(),
            1,
            "the recall once: {}",
            texts(q)
        );
    }
    let notes = (c.store.session_nodes(&sid).unwrap().into_iter())
        .filter(|(_, n)| matches!(n.body, crate::node::Body::Recall { .. }))
        .count();
    assert_eq!(notes, 1, "its Recall record is written once");
    assert_eq!(fallbacks(c), vec!["over_budget".to_string()]);
}

/// Item 4 of round 2: a floor too large for the view's budget (one enormous message) declines to the
/// transcript (`over_budget`) instead of laying a "nothing could be laid out" note in a small view.
#[tokio::test]
async fn a_floor_too_large_for_the_view_declines() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let huge = "enormous ".repeat(150_000);
    let _ = submit(c, &sid, &huge).await;
    assert!(
        fallbacks(c).contains(&"over_budget".to_string()),
        "{:?}",
        fallbacks(c)
    );
    for q in r.requests() {
        assert!(
            !texts(&q).contains("could be laid out"),
            "no small view was sent"
        );
    }
    // The transcript's turn then either carries the input or ends on its own overage before any call.
    let main: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty())
        .collect();
    assert!(
        main.iter().all(|q| texts(q).contains("enormous enormous")),
        "a request the transcript sent carries the current input"
    );
}

/// Round 2 fix, item 2: a source sealed after the view's assembly and before the call is sent (a barrier in
/// `env_tail`, routing off): the request that goes out holds nothing of it; on the first call and on a
/// tool-result call.
#[tokio::test]
async fn a_seal_after_assembly_and_before_the_send_is_honoured() {
    use std::sync::{Arc, OnceLock};
    const KELP: &str = "kelp: the embargoed figure is 7741";
    for later in [false, true] {
        let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
        let desk: Arc<OnceLock<String>> = Arc::default();
        let (held, at) = (core.clone(), desk.clone());
        let thread = std::cell::RefCell::new(String::new());
        let r = crate::tests_places::rig_setup_scripted(
            |c| {
                c.context.assembly = Some(AssemblySetting::All(Assembly::View));
                c.threads.enabled = true;
            },
            |s| *thread.borrow_mut() = kelp(s, false),
            move |q| {
                if last_user(q).contains("desk: look") && !crate::tests_places::answers_a_call(q) {
                    if later {
                        // Sealed at the next call's barrier, after that call's assembly.
                        let (c, sid) = (held.get().unwrap().clone(), at.get().unwrap().clone());
                        crate::turn::view_step::env_tail_hook(
                            &sid,
                            Box::new(move || seal_kelp(&c)),
                        );
                    }
                    return crate::provider::Scripted::tools(
                        "Looking.",
                        &[("b1", "fs_list", serde_json::json!({"path": "."}))],
                    );
                }
                crate::provider::Scripted::text("Done.")
            },
        );
        let _ = core.set(r.core.clone());
        let c = &r.core;
        let thread = thread.into_inner();
        turn_as(c, &thread, KELP, true).await;
        let sid = crate::tests_places::session(c, None);
        let _ = desk.set(sid.clone());
        if !later {
            let core2 = r.core.clone();
            crate::turn::view_step::env_tail_hook(&sid, Box::new(move || seal_kelp(&core2)));
        }
        turn_as(c, &sid, "desk: look", true).await;
        let calls: Vec<ProviderRequest> = (r.requests().into_iter())
            .filter(|q| !q.tools.is_empty() && texts(q).contains("desk: look"))
            .collect();
        assert_eq!(calls.len(), 2, "later {later}");
        for q in &calls[usize::from(later)..] {
            assert!(!texts(q).contains("7741"), "later {later}: {}", texts(q));
        }
        if later {
            assert!(
                texts(&calls[0]).contains("7741"),
                "before the seal it was in view"
            );
        }
    }
}

/// Round 2 fix, item 1: the kept recall of a view call is admitted again ITEM BY ITEM at a late decline: a
/// source sealed since leaves nothing, neither its text nor its header (its thread's title).
#[tokio::test]
async fn a_late_decline_readmits_the_kept_recall_item_by_item() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, OnceLock};
    const SECRET: &str = "the kelp vault code is 5519";
    let calls = Arc::new(AtomicU32::new(0));
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
    let (seen, held) = (calls.clone(), core.clone());
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.memory.mode = crate::config::MemoryMode::Live;
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| *thread.borrow_mut() = kelp(s, false),
        move |q| {
            if q.tools.is_empty() {
                return crate::provider::Scripted::text("aside");
            }
            match seen.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    // After the view's first call: the recalled source's thread is sealed, and the call
                    // is refused for its size, so the turn declines.
                    seal_kelp(held.get().unwrap());
                    crate::provider::Scripted::Fail(crate::provider::ProviderError::InvalidRequest {
                        status: 400,
                        message: "invalid_request_error: prompt is too long: 250000 tokens > 200000 maximum"
                            .into(),
                    })
                }
                _ => crate::provider::Scripted::text("Done."),
            }
        },
    );
    let _ = core.set(r.core.clone());
    let c = &r.core;
    let thread = thread.into_inner();
    let n = crate::node::Node::user(&thread, None, "test", SECRET);
    c.store.append(&[n.record().unwrap()]).unwrap();
    (c.runner.memory).set_ask(crate::tests_private_view::index_of(c, vec![thread.clone()]));
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "what is the vault code?").await;
    let asked: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && texts(q).contains("what is the vault code?"))
        .collect();
    assert!(asked.len() >= 2, "{}", asked.len());
    for q in &asked[1..] {
        assert!(!texts(q).contains("5519"), "{}", texts(q));
        assert!(
            !texts(q).contains("kelp"),
            "no header names the sealed source: {}",
            texts(q)
        );
    }
    let written = (c.store.session_nodes(&sid).unwrap().into_iter())
        .filter(|(_, n)| matches!(n.body, crate::node::Body::Recall { .. }))
        .map(|(_, n)| serde_json::to_string(&n.body).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !written.contains("kelp") && !written.contains("5519"),
        "{written}"
    );
}

/// Every message saying `what`, in any session, moved into the thread `kelp` as a split records it.
fn move_into_kelp(c: &crate::Core, what: &str) {
    for s in (c.store.list_sessions::<crate::session::SessionRecord>()).unwrap() {
        for (_, n) in c.store.session_nodes(&s.session_id).unwrap() {
            if matches!(&n.body, crate::node::Body::UserMessage { text, .. } if text.contains(what))
            {
                let to = serde_json::json!({"thread": "kelp", "split": "spl_x"});
                c.store
                    .put_meta(&crate::moved::moved_of_key(&n.id), &to)
                    .unwrap();
            }
        }
    }
}

/// A frozen view re-checks the node seals of what it shows before each call (P1-1): a shown message moved
/// into a sealed thread between two calls of one turn, or moved into a clear thread that is sealed
/// between them, is in neither part of the second request.
#[tokio::test]
async fn a_move_or_its_threads_seal_between_two_calls_holds_the_shown_message() {
    use std::sync::{Arc, OnceLock};
    const SAID: &str = "desk: the embargoed figure is 7741";
    for seal_later in [false, true] {
        let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
        let held = core.clone();
        let r = crate::tests_places::rig_setup_scripted(
            |c| {
                c.context.assembly = Some(AssemblySetting::All(Assembly::View));
                c.threads.enabled = true;
            },
            move |s| {
                kelp(s, !seal_later);
            },
            move |q| {
                let last = last_user(q);
                if last.contains("move it now") && !crate::tests_places::answers_a_call(q) {
                    let c = held.get().expect("the core");
                    match seal_later {
                        true => seal_kelp(c),
                        false => move_into_kelp(c, "7741"),
                    }
                    return crate::provider::Scripted::tools(
                        "Looking.",
                        &[("s1", "fs_list", serde_json::json!({"path": "."}))],
                    );
                }
                crate::provider::Scripted::text("Done.")
            },
        );
        let _ = core.set(r.core.clone());
        let c = &r.core;
        let desk = crate::tests_places::session(c, None);
        turn_as(c, &desk, SAID, true).await;
        if seal_later {
            move_into_kelp(c, "7741");
        }
        turn_as(c, &desk, "desk: move it now", true).await;
        let calls: Vec<ProviderRequest> = (r.requests().into_iter())
            .filter(|q| texts(q).contains("desk: move it now"))
            .collect();
        assert_eq!(calls.len(), 2, "the turn made two calls");
        assert!(
            texts(&calls[0]).contains("7741"),
            "before: {}",
            texts(&calls[0])
        );
        assert!(
            !texts(&calls[1]).contains("7741"),
            "seal later {seal_later}: {}",
            texts(&calls[1])
        );
    }
}

/// `view show --text`'s gate (P1-3): a recorded exchange whose message moved into a sealed thread since is
/// not shown to the owner's inspection either; the owner's look lifts no seal.
#[tokio::test]
async fn the_owners_inspection_withholds_a_message_moved_into_a_sealed_thread() {
    use crate::view::record::show::Source as _;
    let r = crate::tests_places::rig_setup(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| {
            kelp(s, true);
        },
    );
    let c = &r.core;
    let desk = crate::tests_places::session(c, None);
    turn_as(c, &desk, "desk: the embargoed figure is 7741", true).await;
    let ex = (c.store.session_nodes(&desk).unwrap().into_iter())
        .find_map(|(_, n)| n.turn_id)
        .and_then(|t| crate::exchange::exchange(&c.store, &t).unwrap())
        .expect("the exchange");
    let pos = crate::view::Pos {
        at_ms: ex.start_ms,
        id: ex.turn_id.clone(),
    };
    let range = crate::view::SourceRange {
        space: crate::view::Space::Exchanges {
            place: ex.place.clone(),
        },
        first: pos.clone(),
        last: pos,
    };
    let source = crate::view::record::source::GateSource {
        runner: &c.runner,
        pov: theseus_memory::gate::PointOfView {
            anchor: ex.place,
            place: theseus_memory::recall::Place::Private,
            present: theseus_memory::gate::Present::OwnerAlone,
        },
        asker: None,
    };
    assert!(source.may_show(&range, None), "before the move");
    move_into_kelp(c, "7741");
    assert!(!source.may_show(&range, None), "after it");
}
