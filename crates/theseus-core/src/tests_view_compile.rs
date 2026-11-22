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
    (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && last_user(q).contains(what))
        .collect()
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
            .filter(|q| last_user(q).contains("deferred: which tools?"))
            .last()
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
