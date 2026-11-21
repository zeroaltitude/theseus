//! A reply stays with its conversation, and a turn is given what was just
//! said (issue 43), through the core: the fault played with a stand-in Jev on
//! the threadbench's clock. The place's own conversation answers; a follow-up
//! four minutes later that names nothing, which Jev puts in another thread at
//! 0.66, runs at home; sent to that thread anyway, its turn is given the
//! exchange in one marked block after the cache mark, and can read further
//! back with `memory_lookup`; a sealed thread's words are never carried; and
//! a thread's turn that follows its own thread's exchange is given nothing.

use super::*;

/// The fault's first message.
const ASKED: &str = "what does the harbour tide model assume about the moon";
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

/// The fault: both messages run in the place's own session, and the second's
/// row says the credit that kept it.
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
        (RouteHow::Home, None, true)
    );
    assert_eq!(
        texts_of(c, &dm),
        [ASKED, "can you say more about the second point"]
    );
    let routed = rows(c, "thread.routed");
    let credit = routed[1].data["home_follow"].as_f64().unwrap();
    assert!(
        credit > 0.5 && (t.confidence - credit).abs() < 1e-9,
        "{credit}"
    );
    assert!(
        block_of(&r.fake.requests().pop().unwrap()).is_none(),
        "home holds it already"
    );
}
