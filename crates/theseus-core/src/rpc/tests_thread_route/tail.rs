//! A reply stays with its conversation, and a turn is given what was just
//! said (issue 43), through the core: the fault played with a stand-in Jev on
//! the threadbench's clock. The place's own conversation answers; a follow-up
//! four minutes later that names nothing, which Jev puts in another thread at
//! 0.66, runs at home (nothing reads Jev's pick to place it); addressed to that
//! thread, its turn is given the exchange in one marked block after the cache
//! mark, and can read further back with `memory_lookup`; a sealed thread's
//! words are never carried; and a thread's turn that follows its own thread's
//! exchange is given nothing.

use theseus_protocol::threads::ThreadSealParams;

use super::*;

/// The fault's first message, and a word of it no other message holds.
const ASKED: &str = "what does the harbour tide model assume about the moon";
const MARK: &str = "moon";
/// A bench minute, and the plan's first message on the bench's clock.
const MIN: u64 = 60_000;
const BASE: u64 = 1_700_000_000_000;

fn rig_at(jev: &FakeJev, n: usize) -> Rig {
    rig_on(texts(n), Some(jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 3000;
        c.threads.bench_clock = true;
    })
}

/// One `turn.submit` at `at` on the bench's clock.
async fn at(c: &Arc<Core>, session: &str, input: &str, at: u64) -> TurnSubmitResult {
    let mut w = Wire::open(c);
    let p = TurnSubmitParams {
        route_at_ms: Some(at),
        ..params(session, input, None)
    };
    let id = w.send(method::TURN_SUBMIT, p).await;
    let r = w.response(id).await;
    serde_json::from_value(r["result"].clone()).unwrap_or_else(|_| panic!("a turn result in {r}"))
}

/// The block of a request: the text after its last cache mark that says
/// what was just said, and whether the block before it carries the mark.
fn block_of(q: &crate::provider::ProviderRequest) -> Option<(String, bool)> {
    let blocks = q.messages.last()?["content"].as_array()?;
    let at = blocks.iter().position(|b| {
        b["text"]
            .as_str()
            .is_some_and(|t| t.starts_with("[What was just said in this place"))
    })?;
    let marked = at > 0 && blocks[at - 1].get("cache_control").is_some();
    Some((blocks[at]["text"].as_str()?.to_string(), marked))
}

/// The fault: both messages run in the place's own session, whatever Jev's
/// `thread` answer says, and neither is given a block.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_follow_up_of_the_places_own_reply_stays_there() {
    let jev = FakeJev::start().unwrap();
    let r = rig_at(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    thread_answer(&jev, "home", 0.65);
    let a = at(c, &dm, ASKED, BASE).await;
    assert_eq!(a.session_id, dm);
    thread_answer(&jev, "security-fix", 0.66);
    let b = at(
        c,
        &dm,
        "can you say more about the second point",
        BASE + 4 * MIN,
    )
    .await;
    assert_eq!(b.session_id, dm, "the follow-up ran where the exchange was");
    let t = tag(&b);
    assert_eq!(
        (t.how, t.slug.as_deref(), t.movable),
        (RouteHow::Home, None, false)
    );
    assert_eq!(
        texts_of(c, &dm),
        [ASKED, "can you say more about the second point"]
    );
    let routed = rows(c, "thread.routed");
    assert_eq!(routed[1].data["to"], "home");
    assert!(routed[1].data.get("tail").is_none());
    assert!(
        block_of(&r.fake.requests().pop().unwrap()).is_none(),
        "home holds it already"
    );
}

/// Addressed to the other thread, the turn there is given
/// the place's exchange after the cache mark, named as the place's own
/// conversation; the thread's record holds none of it; and the turn reads
/// further back with `memory_lookup`. Its thread's next turn, which follows
/// its own exchange, is given nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_sent_elsewhere_is_given_what_was_just_said_and_can_look_back() {
    let jev = FakeJev::start().unwrap();
    let r = rig_at(&jev, 6);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, None);
    thread_answer(&jev, "home", 0.95);
    at(c, &dm, ASKED, BASE).await;
    r.fake.when_tool(
        "memory_lookup",
        Scripted::tools(
            "Looking back.",
            &[("m1", "memory_lookup", json!({"session": dm}))],
        ),
    );
    let b = at(
        c,
        &dm,
        "#security-fix can you say more about the second point",
        BASE + 4 * MIN,
    )
    .await;
    let ran = session_of(c, "security-fix");
    assert_eq!(b.session_id, ran);
    let asks = r.fake.requests();
    let first = asks
        .iter()
        .rev()
        .find(|q| block_of(q).is_some())
        .expect("a block");
    let (text, marked) = block_of(first).unwrap();
    assert!(marked, "after the last cache mark");
    for said in [
        ASKED,
        "Done: answer 0.",
        "the place's own conversation",
        dm.as_str(),
    ] {
        assert!(text.contains(said), "{said:?} in {text}");
    }
    let results: Vec<String> = (asks.last().unwrap().messages.iter())
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|b| b["type"] == "tool_result")
        .map(|b| b["content"].to_string())
        .collect();
    assert!(
        results
            .iter()
            .any(|t| t.contains("memory.lookup: session") && t.contains(MARK)),
        "the look read the place's exchange: {results:?}"
    );
    let routed = rows(c, "thread.routed");
    assert_eq!(routed[1].data["tail"]["from"], "home");
    assert!(routed[1].data["tail"]["tokens"].as_u64().unwrap() > 0);
    assert_eq!(
        texts_of(c, &ran),
        ["can you say more about the second point"]
    );
    let kept = serde_json::to_string(&c.store.session_nodes(&ran).unwrap()).unwrap();
    assert!(
        !kept.contains("[What was just said"),
        "not written to the thread"
    );
    let n = r.fake.requests().len();
    at(
        c,
        &dm,
        "#security-fix and the vault key too",
        BASE + 5 * MIN,
    )
    .await;
    let after = &r.fake.requests()[n..];
    assert!(
        after.iter().all(|q| block_of(q).is_none()),
        "its own exchange: none"
    );
    assert!(rows(c, "thread.routed")[2].data.get("tail").is_none());
}

/// The place's last exchange ran in a sealed thread: the turn at home is given
/// nothing of it, and no word of it reaches the request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sealed_threads_words_are_never_carried() {
    let jev = FakeJev::start().unwrap();
    let r = rig_at(&jev, 4);
    let c = &r.core;
    two_threads(c);
    c.thread_seal(
        &ThreadSealParams {
            slug: "security-fix".into(),
            reason: Some("before disclosure".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let dm = home(c, None);
    let a = at(c, &dm, "#security-fix the zircon traversal patch", BASE).await;
    assert_eq!(a.session_id, session_of(c, "security-fix"));
    thread_answer(&jev, "home", 0.95);
    let n = r.fake.requests().len();
    let b = at(c, &dm, "can you say more about it", BASE + MIN).await;
    assert_eq!(b.session_id, dm);
    for q in &r.fake.requests()[n..] {
        assert!(block_of(q).is_none());
        let sent = serde_json::to_string(&q.messages).unwrap();
        assert!(!sent.contains("zircon"), "a sealed thread's word: {sent}");
    }
}
