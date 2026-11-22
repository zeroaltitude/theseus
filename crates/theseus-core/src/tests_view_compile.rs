//! The view in the compiler (issue 44; `compiler::view`, `turn::view_step`): every conversation's turn
//! sends its window as real turns, read across sessions, frozen within a turn, with a front that holds from
//! one turn to the next.

use serde_json::Value;

use crate::provider::binding::unmarked;
use crate::provider::ProviderRequest;
use crate::tests_places::{last_user, rig_with, turn, Rig};
use crate::tests_private_view::{body, owners_rig, session_in, with_person, ASKED};

/// `tests/golden/view_requests.txt`: a small conversation's requests in a place set to `view`.
/// `THESEUS_GOLDEN=write` writes it again.
const VIEW_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/view_requests.txt"
);

pub(crate) fn view_rig() -> Rig {
    rig_with(|_| {})
}

/// The conversation's requests whose newest user message says `what`, in the order they were sent: a
/// request with tools (the ladder's own model calls, a did line's, offer none).
pub(crate) fn asked(r: &Rig, what: &str) -> Vec<ProviderRequest> {
    let got: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && last_user(q).contains(what))
        .collect();
    got.iter().for_each(legal);
    got
}

/// The provider's rules on an unscrubbed request: it opens with the person; roles alternate; every call
/// has its result in the very next message and every result answers a call of the message before it, each
/// once; no message is empty; at most four cache breakpoints.
pub(crate) fn legal(q: &ProviderRequest) {
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

pub(crate) fn texts(q: &ProviderRequest) -> String {
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
/// earlier one ran.
#[tokio::test]
async fn a_view_window_reads_the_place_across_its_sessions() {
    let r = view_rig();
    let c = &r.core;
    let before = session_in(c, None, &r.root);
    turn(c, &before, "alpha: the earlier words").await;
    let after = session_in(c, None, &r.root);
    turn(c, &after, "beta: what came before").await;
    let q = asked(&r, "beta: what came before").pop().unwrap();
    assert_eq!(
        texts(&q).matches("alpha: the earlier words").count(),
        1,
        "{}",
        texts(&q)
    );
}

/// The fault of 2026-11-21, replayed in `view` with no judge in the way: a reply minutes after an answer,
/// naming nothing, is sent with the answer before it, as a real turn of the window.
#[tokio::test]
async fn a_reply_that_names_nothing_sees_the_answer_before_it() {
    let r = crate::tests_places::rig_scripted(
        |_| {},
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

/// A place in company: its request carries its own exchanges.
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
pub(crate) async fn turn_as(
    core: &std::sync::Arc<crate::Core>,
    sid: &str,
    input: &str,
    in_person: bool,
) -> theseus_protocol::TurnSubmitResult {
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
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap()
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
/// defers them) a view turn offers the resolved set the transcript's turn does, not the raw catalog. The
/// transcript's turn is one with the ledger off, which only a test can set.
#[tokio::test]
async fn a_view_turn_offers_the_resolved_deferred_tools() {
    let mut offered = Vec::new();
    for view in [true, false] {
        let r = crate::tests_places::rig_deferring(
            |c| c.context.ledger = (!view).then_some(false),
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
    let (r, _) = owners_rig(|_| {});
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    with_person(c, &sid);
    turn(c, &sid, ASKED).await;
    let q = r.requests().pop().unwrap();
    let system = crate::tests_places::system_text(&q);
    assert!(system.contains("is the owner's accountant"), "{system}");
}

/// A thread `kelp`, bound to its own session, sealed as `seal` says.
pub(crate) fn kelp(store: &crate::store::Store, seal: bool) -> String {
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
pub(crate) fn seal_kelp(c: &crate::Core) {
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
pub(crate) fn fallbacks(c: &crate::Core) -> Vec<String> {
    (crate::test_support::rows(c, "context.view_fallback").into_iter())
        .filter_map(|r| r["why"].as_str().map(str::to_string))
        .collect()
}

/// A turn of `sid` submitted as the driver does, its outcome returned (a turn may fail on its own).
pub(crate) async fn submit(
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
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// Hand-back (c) answered: an established session whose earlier conversation the ledger holds no exchange
/// for is the view's turn all the same. The unledgered turn is out of the view's reach: nothing of it is
/// sent, and no fallback row is written.
#[tokio::test]
async fn an_established_session_without_its_exchanges_is_the_views_turn() {
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
    assert!(
        texts(&q).contains("apply the second option"),
        "{}",
        texts(&q)
    );
    assert!(!texts(&q).contains("the blue one"), "{}", texts(&q));
    let strategies: Vec<String> = (crate::test_support::rows(c, "context.compiled").into_iter())
        .filter_map(|r| r["strategy"].as_str().map(str::to_string))
        .collect();
    assert_eq!(strategies, vec!["view".to_string()]);
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// A view that cannot fit even its floor in its window (a 12,000-token working window under the reply's
/// room) is never handed back: the turn fails before any call on the core overage, once, with no fallback.
#[tokio::test]
async fn a_view_over_its_budget_fails_on_its_overage_and_is_never_handed_back() {
    let r = rig_with(|c| {
        c.context.working_window_tokens = Some(12_000);
    });
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let err = submit(c, &sid, "small window: hello")
        .await
        .expect_err("past the window");
    let te = err.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(te.class, crate::turn::overage::OVERAGE_CLASS);
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
    assert_eq!(crate::test_support::rows(c, "context.overage").len(), 1);
    assert!(
        r.requests().iter().all(|q| q.tools.is_empty()),
        "no call of the turn was sent"
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

/// The view's first call is refused by the provider for its size, with nothing of the turn's own to clear:
/// it is never handed back. The retry is compiled under the provider's stated window, its floor cannot fit
/// that, and the turn fails before a second call on the core overage, once, with no fallback
/// (`room::tests` has a refusal the view makes room for).
#[tokio::test]
async fn a_view_call_refused_for_size_with_nothing_to_clear_fails_plainly() {
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.memory.mode = crate::config::MemoryMode::Live;
        },
        |_| {},
        move |q| {
            if q.tools.is_empty() {
                return crate::provider::Scripted::text("aside");
            }
            crate::provider::Scripted::Fail(crate::provider::ProviderError::InvalidRequest {
                status: 400,
                message:
                    "invalid_request_error: prompt is too long: 250000 tokens > 200000 maximum"
                        .into(),
            })
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
    let err = submit(c, &sid, ASKED).await.expect_err("past the window");
    let te = err.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(te.class, crate::turn::overage::OVERAGE_CLASS);
    let asked: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && texts(q).contains(ASKED))
        .collect();
    assert_eq!(asked.len(), 1, "the refused call, and no retry loop");
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
    assert_eq!(crate::test_support::rows(c, "context.overage").len(), 1);
}

/// Item 4 of round 2, under the cut: a floor too large for the view's budget (one enormous message) is never
/// a small view, and never handed back: the turn fails before any call on the core overage, once, saying the
/// window and the estimate.
#[tokio::test]
async fn a_floor_too_large_for_the_view_fails_on_its_overage() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    // Larger than the model's whole window: the floor alone cannot be sent.
    let huge = "enormous ".repeat(1_500_000);
    let err = submit(c, &sid, &huge).await.expect_err("past the window");
    let te = err.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(te.class, crate::turn::overage::OVERAGE_CLASS);
    let said = format!("{err:#}");
    assert!(
        said.contains("does not fit") && said.contains("Nothing was sent"),
        "{said}"
    );
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
    assert_eq!(crate::test_support::rows(c, "context.overage").len(), 1);
    assert!(
        r.requests().iter().all(|q| q.tools.is_empty()),
        "no call of the turn was sent"
    );
    for q in r.requests() {
        assert!(!texts(&q).contains("could be laid out"), "no small view");
    }
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

/// How many turns the late-seal tests run before theirs: one more than the window's floor of three exchanges
/// (`view::select::here::FLOOR_EXCHANGES`), so the view has an exchange to cut when it makes room after a
/// refusal.
const EARLIER: u32 = 4;

/// [`EARLIER`] turns of `sid`, each a long message answered "Hello.".
async fn said_before(c: &std::sync::Arc<crate::Core>, sid: &str) {
    for k in 0..EARLIER {
        let long = "the keeper's long account of the tide at the north light. ".repeat(20);
        turn(c, sid, &format!("the tide table, earlier ({k}): {long}")).await;
    }
}

/// What the late-seal tests' model does at the first call of the turn that asks `question`, after the
/// gate's reading was changed (`act`): refuse it for its size (the view makes room and calls again,
/// reassembled under the gate of now, `view_step::room`), or call a tool (the ordinary next call). Every
/// other request is answered in words.
fn late_seal_script(
    question: &'static str,
    refusal: bool,
    act: impl Fn() + Send + Sync + 'static,
) -> impl Fn(&ProviderRequest) -> crate::provider::Scripted + Send + Sync + 'static {
    use std::sync::atomic::{AtomicU32, Ordering};
    let asked = AtomicU32::new(0);
    move |q| {
        if q.tools.is_empty() || !last_user(q).contains(question) {
            return crate::provider::Scripted::text("Hello.");
        }
        if asked.fetch_add(1, Ordering::SeqCst) > 0 {
            return crate::provider::Scripted::text("Done.");
        }
        act();
        match refusal {
            true => {
                crate::provider::Scripted::Fail(crate::provider::ProviderError::InvalidRequest {
                    status: 400,
                    message: "invalid_request_error: prompt is too long".into(),
                })
            }
            false => crate::provider::Scripted::tools(
                "Looking.",
                &[("r1", "fs_list", serde_json::json!({"path": "."}))],
            ),
        }
    }
}

/// Round 2 fix, item 1, under the cut: a recalled source the gate admitted on the view's first call (a real
/// turn's message, with its exchange, in a thread) is sealed before the next call, which is a retry after a
/// size refusal or an ordinary tool call: the next request holds nothing of it, neither its text nor its
/// header (its thread's title).
#[tokio::test]
async fn a_late_decline_readmits_the_kept_recall_item_by_item() {
    use std::sync::{Arc, OnceLock};
    const SECRET: &str = "the kelp vault code is 5519";
    const ASKS: &str = "what is the vault code?";
    for refusal in [true, false] {
        let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
        let held = core.clone();
        let thread = std::cell::RefCell::new(String::new());
        let r = crate::tests_places::rig_setup_scripted(
            |c| {
                c.memory.mode = crate::config::MemoryMode::Live;
                c.threads.enabled = true;
            },
            |s| *thread.borrow_mut() = kelp(s, false),
            late_seal_script(ASKS, refusal, move || seal_kelp(held.get().unwrap())),
        );
        let _ = core.set(r.core.clone());
        let c = &r.core;
        let thread = thread.into_inner();
        turn_as(c, &thread, SECRET, true).await;
        (c.runner.memory).set_ask(crate::tests_private_view::index_of(c, vec![thread.clone()]));
        let sid = session_in(c, None, &r.root);
        said_before(c, &sid).await;
        turn_as(c, &sid, ASKS, true).await;
        let asked: Vec<ProviderRequest> = (r.requests().into_iter())
            .filter(|q| !q.tools.is_empty() && texts(q).contains(ASKS))
            .collect();
        assert!(asked.len() >= 2, "refusal {refusal}: {}", asked.len());
        assert!(
            texts(&asked[0]).contains("5519"),
            "refusal {refusal}: the first call laid the source: {}",
            texts(&asked[0])
        );
        for q in &asked[1..] {
            assert!(!texts(q).contains("5519"), "{}", texts(q));
            assert!(
                !texts(q).contains("kelp"),
                "no header names the sealed source: {}",
                texts(q)
            );
        }
        // By the owner's decision of 2026-11-22 (A): the Recall note written at the first dispatch stays, a
        // record of what was sent. (1) No request after the seal lays its item: not this turn's later calls
        // (above), nor the session's next turn.
        turn_as(c, &sid, "and the tide?", true).await;
        let next = (r.requests().into_iter())
            .rfind(|q| !q.tools.is_empty() && texts(q).contains("and the tide?"))
            .expect("the next turn's request");
        assert!(
            !texts(&next).contains("5519") && !texts(&next).contains("kelp"),
            "refusal {refusal}: the next turn lays nothing of the sealed source: {}",
            texts(&next)
        );
        // (2) The seal's own report of what had already crossed out of the thread names the asking session:
        // what a person sealing the thread is told (opened again here, then sealed by the owner).
        let crossed = resealed_crossed(c);
        assert!(
            crossed.iter().any(|x| x.session_id == sid),
            "refusal {refusal}: the seal reports the note that crossed: {crossed:?}"
        );
    }
}

/// The report a person gets when sealing `kelp`: what had already crossed out of the thread. The test seal
/// (`seal_kelp`) is lifted first, so the owner's `thread.seal` is the one that reports.
fn resealed_crossed(c: &crate::Core) -> Vec<theseus_protocol::threads::ThreadCrossed> {
    c.runner
        .threads
        .write(
            &c.store,
            |b| {
                let mut t = b.get("kelp").expect("kelp").clone();
                t.sealed = None;
                Ok(vec![t])
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
    let sealed = c
        .thread_seal(
            &theseus_protocol::threads::ThreadSealParams {
                slug: "kelp".into(),
                reason: Some("embargo".into()),
                ..Default::default()
            },
            crate::tests_task_brief::cli(),
        )
        .unwrap();
    sealed.crossed
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

/// Absence is not proof (item 2): a shown exchange whose record is gone between two calls of one turn
/// revokes the frozen view, and the second request holds nothing of it.
#[tokio::test]
async fn a_shown_exchange_removed_between_two_calls_is_held() {
    use std::sync::{Arc, OnceLock};
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
    let held = core.clone();
    let r = crate::tests_places::rig_setup_scripted(
        |_| {},
        |_| {},
        move |q| {
            let last = last_user(q);
            if last.contains("drop it now") && !crate::tests_places::answers_a_call(q) {
                let c = held.get().expect("the core");
                for s in (c.store.list_sessions::<crate::session::SessionRecord>()).unwrap() {
                    for (_, n) in c.store.session_nodes(&s.session_id).unwrap() {
                        let said = matches!(&n.body, crate::node::Body::UserMessage { text, .. }
                            if text.contains("7741"));
                        if let (true, Some(t)) = (said, n.turn_id.as_deref()) {
                            let key = format!("{}{t}", crate::exchange::EXCHANGE_PREFIX);
                            c.store
                                .append(&[theseus_store::NewRecord::delete_meta(&key)])
                                .unwrap();
                        }
                    }
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
    turn_as(c, &desk, "desk: the figure is 7741", true).await;
    turn_as(c, &desk, "desk: drop it now", true).await;
    let calls: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| texts(q).contains("desk: drop it now"))
        .collect();
    assert_eq!(calls.len(), 2, "the turn made two calls");
    assert!(
        texts(&calls[0]).contains("7741"),
        "before: {}",
        texts(&calls[0])
    );
    assert!(
        !texts(&calls[1]).contains("7741"),
        "after: {}",
        texts(&calls[1])
    );
}
