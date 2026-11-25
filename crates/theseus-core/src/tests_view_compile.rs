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
        .read(&place, crate::view::fold::FOLD_STEP, None)
        .unwrap();
    assert_eq!(tonight.window.len(), 2, "{tonight:?}");
    // The window is counted in exchanges and reads no clock: the same read eleven hours on.
    let snap = crate::view::wire::StoreSnapshot {
        now_ms: theseus_protocol::now_unix_ms() + 11 * 3_600_000,
        timeline: crate::view::wire::StoreTimeline(&c.store),
        at_hand: None,
    };
    let pov = crate::view::PointOfView {
        anchor: crate::view::Anchor::Place(place.clone()),
        person: None,
        present: crate::view::Present::Private,
        at_hand: None,
        now_ms: snap.now_ms,
    };
    let renderer = crate::view::wire_text::TextRenderer {
        store: &c.store,
        kernel: None,
        sessions: None,
        zone: &crate::recall::stamp::system,
        since: None,
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

/// Seal the thread `kelp`: its record written again sealed, as an older store holds it (nothing
/// writes a thread now, issue 44; the record keeps no session), and the board read again.
pub(crate) fn seal_kelp(c: &crate::Core) {
    let mut v = serde_json::to_value(c.threads().get("kelp").expect("kelp")).unwrap();
    v["sealed"] = serde_json::json!({"by": "cli", "at_ms": 2, "reason": "embargo"});
    if let Some(o) = v.as_object_mut() {
        o.remove("session_id");
    }
    c.store.put_meta("thread:kelp", &v).unwrap();
    c.runner.threads.forget();
    c.threads();
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
    assert_eq!(te.class, crate::turn::WINDOW_CLASS);
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
    assert_eq!(crate::test_support::rows(c, "context.overage").len(), 1);
    assert!(
        r.requests().iter().all(|q| q.tools.is_empty()),
        "no call of the turn was sent"
    );
}

/// The view's first call is refused by the provider for its size, with nothing of the turn's own to clear:
/// it is never handed back. A smaller request exists (its recall can go), so it is sent once; refused again,
/// the turn fails plainly on the window, with no third call and no fallback (`room::tests` has a refusal
/// the view makes room for).
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
    assert_eq!(te.class, crate::turn::WINDOW_CLASS);
    let asked: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && texts(q).contains(ASKED))
        .collect();
    assert_eq!(
        asked.len(),
        2,
        "the refused call and one smaller retry, and no retry loop"
    );
    // The smaller retry is smaller by what could go: the recalled episode the first request laid.
    assert!(texts(&asked[0]).contains(episode), "{}", texts(&asked[0]));
    assert!(!texts(&asked[1]).contains(episode), "{}", texts(&asked[1]));
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
    assert_eq!(crate::test_support::rows(c, "context.overflow").len(), 2);
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
    assert_eq!(te.class, crate::turn::WINDOW_CLASS);
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

/// Round 2, item 1, its size half: a size repair's recompile comes back to the size check, and the request
/// that goes out lays what the first compile laid (a recalled source).
#[tokio::test]
async fn a_size_repair_comes_back_to_the_size_check() {
    const SECRET: &str = "the kelp vault code is 5519";
    const ASKS: &str = "what is the vault code now?";
    let thread = std::cell::RefCell::new(String::new());
    let r = crate::tests_places::rig_setup_scripted(
        |c| {
            c.memory.mode = crate::config::MemoryMode::Live;
        },
        |s| *thread.borrow_mut() = kelp(s, false),
        |q| match last_user(q).contains(ASKS) && !q.tools.is_empty() {
            true => crate::provider::Scripted::text("Done."),
            false => crate::provider::Scripted::text("Hello."),
        },
    );
    let c = &r.core;
    let source = thread.into_inner();
    turn_as(c, &source, SECRET, true).await;
    (c.runner.memory).set_ask(crate::tests_private_view::index_of(c, vec![source]));
    let sid = session_in(c, None, &r.root);
    crate::turn::view_step::force_resize(&sid);
    turn_as(c, &sid, ASKS, true).await;
    let sent: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| !q.tools.is_empty() && texts(q).contains(ASKS))
        .collect();
    assert_eq!(sent.len(), 1, "one call");
    let text = texts(&sent[0]);
    assert!(text.contains("5519"), "the repair lays the source: {text}");
}

/// A fold over `turn_id`'s exchange at its place, with an account the requests can be searched for.
fn fold_over_exchange(c: &crate::Core, turn_id: &str, account: &str) {
    let ex = crate::exchange::exchange(&c.store, turn_id)
        .unwrap()
        .unwrap();
    let node = crate::ladder::fold::FoldNode {
        id: format!("fold-{turn_id}"),
        place: ex.place.clone(),
        level: 1,
        exchanges: vec![turn_id.to_string()],
        first_ms: ex.start_ms,
        last_ms: ex.start_ms,
        his: vec![],
        counted: 0,
        children: vec![],
        title: "the figure".into(),
        account: account.into(),
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
}

/// The exchange record of the turn that said `what`, in any session, removed.
fn remove_exchange_saying(c: &crate::Core, what: &str) {
    for s in (c.store.list_sessions::<crate::session::SessionRecord>()).unwrap() {
        for (_, n) in c.store.session_nodes(&s.session_id).unwrap() {
            let said = matches!(&n.body, crate::node::Body::UserMessage { text, .. }
                if text.contains(what));
            if let (true, Some(t)) = (said, n.turn_id.as_deref()) {
                let key = format!("{}{t}", crate::exchange::EXCHANGE_PREFIX);
                c.store
                    .append(&[theseus_store::NewRecord::delete_meta(&key)])
                    .unwrap();
            }
        }
    }
}

/// The turn id of the first message saying `what` in `sid`.
fn turn_saying(c: &crate::Core, sid: &str, what: &str) -> String {
    (c.store.session_nodes(sid).unwrap().into_iter())
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::UserMessage { text, .. } if text.contains(what) => n.turn_id,
            _ => None,
        })
        .unwrap()
}

/// A removed thing is not sent again (round 2, item 1): a fold whose constituent exchange was removed before
/// the assembly is not laid; with it present, the same fold is.
#[tokio::test]
async fn a_fold_whose_constituent_was_removed_before_the_assembly_is_not_laid() {
    const ACCOUNT: &str = "ACCOUNT: the figure 7741 was discussed";
    let r = view_rig();
    let c = &r.core;
    let other = crate::tests_places::session(c, None);
    turn_as(c, &other, "other: the figure is 7741", true).await;
    fold_over_exchange(c, &turn_saying(c, &other, "the figure is 7741"), ACCOUNT);
    let desk = crate::tests_places::session(c, None);
    turn_as(c, &desk, "desk: before the removal", true).await;
    let before = asked(&r, "desk: before the removal").pop().unwrap();
    assert!(texts(&before).contains(ACCOUNT), "{}", texts(&before));
    remove_exchange_saying(c, "the figure is 7741");
    turn_as(c, &desk, "desk: after the removal", true).await;
    let after = asked(&r, "desk: after the removal").pop().unwrap();
    assert!(!texts(&after).contains(ACCOUNT), "{}", texts(&after));
}

/// Round 2, item 1: a fold the frozen view laid whose constituent exchange is removed between two calls of
/// one turn is not in the second call's request (its constituents are the view's dependencies).
#[tokio::test]
async fn a_fold_whose_constituent_is_removed_between_two_calls_is_not_sent_again() {
    use std::sync::{Arc, OnceLock};
    const ACCOUNT: &str = "ACCOUNT: the figure 7741 was discussed";
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::default();
    let held = core.clone();
    let r = crate::tests_places::rig_setup_scripted(
        |_| {},
        |_| {},
        move |q| {
            if last_user(q).contains("drop it now") && !crate::tests_places::answers_a_call(q) {
                remove_exchange_saying(held.get().expect("the core"), "the figure is 7741");
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
    let other = crate::tests_places::session(c, None);
    turn_as(c, &other, "other: the figure is 7741", true).await;
    fold_over_exchange(c, &turn_saying(c, &other, "the figure is 7741"), ACCOUNT);
    let desk = crate::tests_places::session(c, None);
    turn_as(c, &desk, "desk: drop it now", true).await;
    let calls: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| texts(q).contains("desk: drop it now"))
        .collect();
    assert_eq!(calls.len(), 2, "the turn made two calls");
    assert!(
        texts(&calls[0]).contains(ACCOUNT),
        "before: {}",
        texts(&calls[0])
    );
    assert!(
        !texts(&calls[1]).contains(ACCOUNT),
        "after: {}",
        texts(&calls[1])
    );
}

/// Round 2, item 4: the removal bound. Something the view laid is removed under it after each of three
/// assemblies of one call: the call fails plainly (`ViewFailed::Gone`), and nothing is sent.
#[tokio::test]
async fn a_third_removal_in_one_call_fails_it_plainly_and_sends_nothing() {
    use std::sync::{Arc, OnceLock};
    let r = view_rig();
    let c = &r.core;
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::new(OnceLock::new());
    let _ = core.set(c.clone());
    let desk = crate::tests_places::session(c, None);
    for i in 0..3 {
        turn_as(c, &desk, &format!("desk {i}: the buoy {i} is red"), true).await;
    }
    for i in 0..3 {
        let held = core.clone();
        crate::turn::view_step::assembled_hook(
            &desk,
            Box::new(move || {
                remove_exchange_saying(held.get().unwrap(), &format!("the buoy {i} is red"))
            }),
        );
    }
    let e = submit(c, &desk, "tidy the harbour notes")
        .await
        .unwrap_err();
    let te = e.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(te.class, "context_unreadable");
    assert!(
        format!("{:#}", te.source).contains("was removed while it was being prepared"),
        "{:#}",
        te.source
    );
    let sent: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| texts(q).contains("tidy the harbour notes"))
        .collect();
    assert!(sent.is_empty(), "nothing is sent: {}", sent.len());
}

/// Round 2, item 4: a source removed during a size repair's await (after the repair compile's assembly)
/// is not in the request that goes out.
#[tokio::test]
async fn a_source_removed_during_a_size_repair_is_not_sent() {
    use std::sync::{Arc, OnceLock};
    const ASKS: &str = "what is the buoy now?";
    let r = view_rig();
    let c = &r.core;
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::new(OnceLock::new());
    let _ = core.set(c.clone());
    let desk = crate::tests_places::session(c, None);
    turn_as(c, &desk, "desk: the buoy code is 6620", true).await;
    let (held, again) = (core.clone(), desk.clone());
    crate::turn::view_step::assembled_hook(
        &desk,
        Box::new(move || {
            crate::turn::view_step::assembled_hook(
                &again,
                Box::new(move || {
                    remove_exchange_saying(held.get().unwrap(), "the buoy code is 6620")
                }),
            )
        }),
    );
    crate::turn::view_step::force_resize(&desk);
    turn_as(c, &desk, ASKS, true).await;
    let sent: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| texts(q).contains(ASKS))
        .collect();
    assert!(!sent.is_empty(), "a call went out");
    for q in &sent {
        assert!(!texts(q).contains("6620"), "{}", texts(q));
    }
}

/// A session the self loop opens, as its fixer, reviewer or approver turns do (`rsi::join::fix::turns`):
/// its execution's principal is `self`.
fn self_session(c: &crate::Core, label: &str) -> String {
    let rec = c
        .open_session_as(
            theseus_protocol::SessionOpenParams {
                label: Some(label.into()),
                ..Default::default()
            },
            theseus_kernel::Authority {
                principal: crate::rsi::SELF_PRINCIPAL.into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    rec.session_id
}

/// Round 3, the self loop's scope (the conductor's rule of 12:33): a fixer's and an approver's request hold
/// nothing of another session or another place, and none of slots C, E or F; an ordinary session at the same
/// place, with no mark either, is laid the other session's words (the widest point of view).
#[tokio::test]
async fn a_self_loop_session_lays_its_own_place_only_and_an_ordinary_one_the_widest() {
    use crate::tests_view_hand_backs::unmarked;
    // The self loop acting, its budget resumed, so its sessions' turns run (as `rpc::tests_self_view`).
    let r = rig_with(|c| c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act);
    let c = &r.core;
    c.self_resume(
        &theseus_protocol::rsi::SelfResumeParams::default(),
        &crate::approval::Answerer::from("the CLI"),
    )
    .unwrap();
    let dm = crate::tests_places::session(c, Some(&format!("dm:{}", crate::tests_places::OWNER)));
    turn_as(c, &dm, "dm: the lighthouse lamp is amber", true).await;
    let fixer = self_session(c, "self fix: kelp");
    let approver = self_session(c, "self approve: kelp");
    let ordinary = crate::tests_places::session(c, None);
    // Every one of them works at the same place as the desk's session.
    let place = |s: &str| crate::view_gate::place_name(&c.runner, s);
    let desk = crate::tests_places::session(c, None);
    assert_eq!(place(&fixer), place(&desk), "one place");
    turn_as(c, &desk, "desk: the heron is grey", true).await;
    for (sid, asks) in [
        (&fixer, "fixer: what is in front of you?"),
        (&approver, "approver: what is in front of you?"),
        (&ordinary, "ordinary: what is in front of you?"),
    ] {
        unmarked(c, sid, asks).await.unwrap();
    }
    for asks in ["fixer: what is in front", "approver: what is in front"] {
        let sent = texts(&asked(&r, asks).pop().expect("its request"));
        assert!(
            !sent.contains("the heron is grey"),
            "{asks}: another session's: {sent}"
        );
        assert!(
            !sent.contains("lighthouse lamp"),
            "{asks}: another place's: {sent}"
        );
    }
    let sent = texts(
        &asked(&r, "ordinary: what is in front")
            .pop()
            .expect("its request"),
    );
    assert!(sent.contains("the heron is grey"), "the widest: {sent}");
}

/// Round 3, the self loop's scope against an ordinary session (the brief's second test), with one
/// foreign-context fixture: the owner's DM, another session at another place, made recallable. An ordinary
/// conversation's turn is laid it (the widest point of view); a fixer's and an approver's turn asking the same
/// are laid nothing of it. An ordinary TASK is not shown here: a task's first compile admits a recall note only
/// as its compilation's own section (`compiler::situation`), and the task's execution is complete after that
/// turn, so this rig gives it no second turn to ask on.
#[tokio::test]
async fn a_fixer_and_an_approver_see_nothing_of_other_sessions_and_an_ordinary_session_does() {
    use crate::tests_view_hand_backs::unmarked;
    const SECRET: &str = "the kelp vault code is 5519";
    let r = rig_with(|c| {
        c.memory.mode = crate::config::MemoryMode::Live;
        c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
    });
    let c = &r.core;
    c.self_resume(
        &theseus_protocol::rsi::SelfResumeParams::default(),
        &crate::approval::Answerer::from("the CLI"),
    )
    .unwrap();
    let dm = crate::tests_places::session(c, Some(&format!("dm:{}", crate::tests_places::OWNER)));
    turn_as(c, &dm, SECRET, true).await;
    (c.runner.memory).set_ask(crate::tests_private_view::index_of(c, vec![dm.clone()]));
    let ordinary = crate::tests_places::session(c, None);
    let fixer = self_session(c, "self fix: kelp");
    let approver = self_session(c, "self approve: kelp");
    for (sid, asks) in [
        (&ordinary, "ordinary: what is the kelp vault code?"),
        (&fixer, "fixer: what is the kelp vault code?"),
        (&approver, "approver: what is the kelp vault code?"),
    ] {
        unmarked(c, sid, asks).await.unwrap();
    }
    let sent = |asks: &str| texts(&asked(&r, asks).pop().expect("its request"));
    let got = sent("ordinary: what is");
    assert!(
        got.contains("5519"),
        "the ordinary session, the widest: {got}"
    );
    for asks in ["fixer: what is", "approver: what is"] {
        let got = sent(asks);
        assert!(!got.contains("5519"), "{asks}: another place's: {got}");
    }
}

/// Round 3: a fork of a self-loop session keeps the run it was handed. Its turn lays the source's exchanges
/// through the fork's point (handed material, as a brief is), none the source said after the point, and nothing
/// of another session at the source's place.
#[tokio::test]
async fn a_fork_of_a_self_loop_session_lays_its_inherited_run_to_its_point_and_nothing_else_foreign(
) {
    use crate::tests_view_hand_backs::unmarked;
    let r = rig_with(|c| c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act);
    let c = &r.core;
    c.self_resume(
        &theseus_protocol::rsi::SelfResumeParams::default(),
        &crate::approval::Answerer::from("the CLI"),
    )
    .unwrap();
    let desk = crate::tests_places::session(c, None);
    turn_as(c, &desk, "desk: the heron is grey", true).await;
    let fixer = self_session(c, "self fix: kelp");
    for said in [
        "fixer: FIRST-KELP",
        "fixer: SECOND-KELP",
        "fixer: THIRD-KELP",
    ] {
        unmarked(c, &fixer, said).await.unwrap();
    }
    // The point: the answer to the second message (the fourth of the source's nodes that are messages).
    let said: Vec<String> = (c.store.session_nodes(&fixer).unwrap().into_iter())
        .filter(|(_, n)| {
            matches!(
                n.body,
                crate::node::Body::UserMessage { .. } | crate::node::Body::AssistantMessage { .. }
            )
        })
        .map(|(_, n)| n.id)
        .collect();
    assert_eq!(said.len(), 6, "three messages and their answers: {said:?}");
    let f = c
        .session_fork(
            theseus_protocol::SessionForkParams {
                session: fixer.clone(),
                at: said[3].clone(),
                ..Default::default()
            },
            crate::approval::Surface::Cli,
        )
        .unwrap();
    let source = crate::view_gate::Verified::of_session(&c.runner, &fixer);
    let forked = crate::view_gate::Verified::of_session(&c.runner, &f.session_id);
    assert!(source.own_place_only(), "the source is the self loop's");
    assert!(forked.own_place_only(), "the fork keeps its source's scope");
    unmarked(c, &f.session_id, "fork: what came before?")
        .await
        .unwrap();
    let got = texts(
        &asked(&r, "fork: what came before")
            .pop()
            .expect("its request"),
    );
    assert!(got.contains("FIRST-KELP"), "the handed run: {got}");
    assert!(
        got.contains("SECOND-KELP"),
        "the handed run, through its point: {got}"
    );
    assert!(!got.contains("THIRD-KELP"), "nothing past the point: {got}");
    assert!(
        !got.contains("the heron is grey"),
        "another session's: {got}"
    );
}

/// Round 3: a scope that cannot be read never widens. A conversation whose execution the kernel does not hold
/// reads as unread, and its census (the turn's own assembler, `view_built`) fails plainly. A turn of it does
/// not reach that read: a missing execution is given an operator one before the turn runs (`turn.rs`), so the
/// rig cannot fail a live turn's read.
#[tokio::test]
async fn a_scope_that_cannot_be_read_fails_the_assembly_and_widens_nothing() {
    let r = rig_with(|_| {});
    let c = &r.core;
    let sid = crate::tests_places::session(c, None);
    let mut rec: crate::session::SessionRecord = c.store.get_session(&sid).unwrap().unwrap();
    rec.execution_id = Some("exe_gone".into());
    c.store.put_session(&sid, &rec).unwrap();
    let v = crate::view_gate::Verified::of_preview(&c.runner, &sid, "local");
    assert!(v.scope_unread(), "an execution the kernel does not hold");
    let sent = r.requests().len();
    let err = c
        .context_census(&theseus_protocol::census::ContextCensusParams {
            session_id: Some(sid),
            place: None,
            exact: false,
        })
        .err()
        .expect("the census fails");
    assert!(
        format!("{err:#}").contains("cannot be decided"),
        "plainly: {err:#}"
    );
    assert_eq!(r.requests().len(), sent, "nothing is sent");
}

/// Round 3: a self-principal task's census is assembled under the scope its live turn gets
/// (`Verified::of_task` and `Verified::of_turn` read one discriminator), and an operator task's under none.
#[tokio::test]
async fn a_self_principal_tasks_census_has_its_live_turns_scope() {
    let r = rig_with(|c| c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act);
    let c = &r.core;
    let parent = crate::tests_places::session(c, None);
    let worker = self_session(c, "self fix: census");
    crate::tests_self_budget::make_task(c, &worker, &parent, crate::rsi::SELF_PRINCIPAL, None);
    let (ordinary, _) = crate::tests_self_budget::principal_session(c, &parent, "operator", None);
    for (sid, scoped) in [(&worker, true), (&ordinary, false)] {
        let census = crate::view_gate::Verified::of_task(&c.runner, sid).expect("a task's");
        let live = crate::view_gate::Verified::of_turn(&c.runner, sid, None);
        assert_eq!(census.own_place_only(), scoped, "{sid}: the census");
        assert_eq!(live.own_place_only(), scoped, "{sid}: its live turn");
        assert_eq!(census.pov().anchor, live.pov().anchor, "{sid}: one place");
    }
}

/// The harness's own line in a finished exchange is headed as the harness's, never as a person's words
/// (`content::harness_own`): a second session's request at the same place lays the first turn's clock line
/// under the harness's heading, and under no person's.
#[tokio::test]
async fn a_finished_exchanges_clock_line_is_headed_as_the_harnesss_and_no_persons() {
    use crate::tests_view_hand_backs::unmarked;
    const CLOCK: &str = "[The harness: 14 minutes left of this turn's 15.]";
    let r = rig_with(|_| {});
    let c = &r.core;
    let desk = crate::tests_places::session(c, None);
    turn_as(c, &desk, "desk: the heron is grey", true).await;
    let turn = (c.store.session_nodes(&desk).unwrap().into_iter())
        .find_map(|(_, n)| n.turn_id)
        .expect("the desk's turn");
    let line = crate::node::Node::relayed(
        &desk,
        Some(&turn),
        crate::node::Origin::Harness,
        crate::turn::clock_step::CLOCK_AUTHOR,
        CLOCK,
    );
    c.store.append(&[line.record().unwrap()]).unwrap();
    let other = crate::tests_places::session(c, None);
    unmarked(c, &other, "other: what was said here?")
        .await
        .unwrap();
    let sent = texts(
        &asked(&r, "other: what was said here")
            .pop()
            .expect("its request"),
    );
    assert!(
        sent.contains("the heron is grey"),
        "the first turn is laid: {sent}"
    );
    assert!(
        sent.contains(&format!("harness: {CLOCK}")),
        "as the harness's: {sent}"
    );
    assert!(
        sent.contains("the harness's own line, "),
        "under its heading: {sent}"
    );
    assert!(
        !sent.contains(&format!("person: {CLOCK}")),
        "no person's: {sent}"
    );
    let before = &sent[..sent.find(CLOCK).unwrap()];
    let heading = before
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default();
    assert!(
        !heading.contains("author unknown"),
        "not headed as a person of unknown authorship: {heading}"
    );
}

/// X190, the install's smoke on build 190: a FRESH terminal session asks one short line at a place whose
/// recent exchanges are a run of that same line (three here, five at another terminal place), with recall live.
/// The request the view compiles is printed whole (each message's role, each block's size, head and end, and
/// the block beside the message line by line). It must end with the person's line; and the harness's block
/// beside it must say it is the harness's context and not a new request, must not disown its items (some are
/// the person's own words, said elsewhere, each under its own framing), and must not head itself with an
/// agenda of topics (what is waiting, in play, said elsewhere, called), which a model read on build 190 as
/// the thing to report instead of answering the line.
#[tokio::test]
async fn x190_a_fresh_terminal_line_after_a_run_of_it_elsewhere_is_what_the_request_ends_with() {
    const ASK: &str = "Reply with the single word: ok";
    let r = rig_with(|c| c.memory.mode = crate::config::MemoryMode::Live);
    let c = &r.core;
    // Earlier smoke turns: three at this terminal place (the window), five at another terminal place (said
    // elsewhere lately), all of them in the words index (recall).
    let mut earlier = Vec::new();
    for i in 0..8 {
        let at = (i >= 3).then_some("local:/home/smoke");
        let s = crate::tests_places::session(c, at);
        turn_as(c, &s, ASK, true).await;
        earlier.push(s);
    }
    (c.runner.memory).set_ask(crate::tests_private_view::index_of(c, earlier));
    let fresh = crate::tests_places::session(c, None);
    turn_as(c, &fresh, ASK, true).await;
    let q = r
        .requests()
        .into_iter()
        .last()
        .expect("the fresh turn's request");
    let cut = |s: &str, n: usize| -> String {
        let flat = s.replace('\n', "⏎");
        match flat.chars().count() > 2 * n {
            true => format!(
                "{} … {}",
                flat.chars().take(n).collect::<String>(),
                flat.chars()
                    .rev()
                    .take(n)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<String>()
            ),
            false => flat,
        }
    };
    println!("X190 SYSTEM: {} blocks", q.system.len());
    for (i, b) in q.system.iter().enumerate() {
        let t = b["text"].as_str().unwrap_or_default();
        println!("X190 system[{i}] {} chars: {}", t.len(), cut(t, 160));
    }
    println!("X190 MESSAGES: {}", q.messages.len());
    for (i, m) in q.messages.iter().enumerate() {
        let blocks = m["content"].as_array().cloned().unwrap_or_default();
        println!(
            "X190 message[{i}] role={} blocks={}",
            m["role"],
            blocks.len()
        );
        for (j, b) in blocks.iter().enumerate() {
            let t = b["text"]
                .as_str()
                .map_or_else(|| b.to_string(), str::to_string);
            println!(
                "X190   [{i}.{j}] {} {} chars: {}",
                b["type"],
                t.len(),
                cut(&t, 220)
            );
            if t.starts_with(crate::view::wire_text::BESIDE_HEAD) {
                for line in t.lines() {
                    println!("X190 BESIDE| {line}");
                }
            }
        }
    }
    let last = q.messages.last().expect("a message");
    assert_eq!(
        last["role"], "user",
        "the request ends with the person's turn"
    );
    let blocks = last["content"].as_array().expect("blocks");
    let end = blocks
        .iter()
        .rev()
        .find_map(|b| b["text"].as_str())
        .unwrap_or_default();
    assert!(
        end.trim_end().ends_with(ASK),
        "the request's last words are the person's message, not a block of the harness's: {}",
        cut(end, 300)
    );
    let beside = (blocks.iter().filter_map(|b| b["text"].as_str()))
        .find(|t| t.starts_with(crate::view::wire_text::BESIDE_HEAD))
        .expect("the block beside the message (recall laid it)");
    let head = beside.lines().next().unwrap_or_default();
    assert!(
        head.contains("Harness-supplied context") && head.contains("not a new request"),
        "the block says whose it is and that it asks for nothing: {head}"
    );
    assert!(
        !head.contains("not the person's words"),
        "it disowns no item: some are the person's words, each under its own framing: {head}"
    );
    for agenda in ["what is waiting", "in play", "said elsewhere", "called"] {
        assert!(
            !head.contains(agenda),
            "its heading names no topic to cover ({agenda}): {head}"
        );
    }
}

/// `[context]`'s sizes at their bounds (issue 48): a view turn assembles at each, and its request lays the
/// window the HERE setting names and the history tool's page the pager setting names.
#[tokio::test]
async fn a_view_assembles_at_each_sizes_bounds() {
    use crate::config::context::{
        EVERYWHERE_EXCHANGES_RANGE as E, HERE_EXCHANGES_RANGE as H, HISTORY_PAGE_EXCHANGES_RANGE as P,
    };
    for (here, everywhere, page) in [
        (*H.start(), *E.start(), *P.start()),
        (*H.end(), *E.end(), *P.end()),
    ] {
        let r = rig_with(|c| {
            c.context.here_exchanges = Some(here);
            c.context.everywhere_exchanges = Some(everywhere);
            c.context.history_page_exchanges = Some(page);
        });
        let sid = session_in(&r.core, None, &r.root);
        turn(&r.core, &sid, "bounds: the first").await;
        turn(&r.core, &sid, "bounds: the second").await;
        let q = asked(&r, "bounds: the second").pop().expect("a request");
        assert!(texts(&q).contains("bounds: the first"), "{here}: the window carries the first");
        let system = crate::tests_places::system_text(&q);
        let window = format!("up to {here} to {} recent exchanges", here + 19);
        assert_eq!(system.matches(&window).count(), 1, "{here}: {system}");
        let history = (q.tools.iter())
            .find(|t| t["name"] == "context_history")
            .and_then(|t| t["description"].as_str())
            .expect("the history tool is offered");
        assert!(
            history.starts_with(&format!("Earlier history, {page} exchanges a page. ")),
            "{history}"
        );
    }
}
