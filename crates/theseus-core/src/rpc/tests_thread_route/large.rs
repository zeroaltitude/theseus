//! A guess into a thread that holds a large context (the router's guard on
//! context size): the message runs at home, movable, and the row says which
//! thread was spared and how large it was.

use super::*;

use crate::node::{Body, Node};
use theseus_protocol::Usage;

/// Give `slug`'s session one answer whose prompt the provider counted at
/// `tokens`, as a conversation that long leaves behind.
fn hold_context(c: &Core, slug: &str, tokens: u64) {
    let sid = c.thread_session_open(slug).unwrap().session_id;
    let a = Node::assistant(
        &sid,
        "turn_big",
        0,
        Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": "Done."})],
            model: "claude-sonnet-5-5".into(),
            provider: "anthropic".into(),
            stop_reason: Some("end_turn".into()),
            usage: Usage {
                input_tokens: 1_000,
                output_tokens: 300,
                cache_read_input_tokens: tokens - 1_000,
                cache_creation_input_tokens: 0,
                cache_creation_1h_input_tokens: 0,
            },
            cost_usd: Some(0.5),
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    c.store.append(&[a.record().unwrap()]).unwrap();
}

/// A rig whose Jev answers too late, so the router's own pick stands as a
/// guess (the same rig as the timeout's test).
fn late_jev_rig(jev: &FakeJev) -> Rig {
    jev.set_latency(Duration::from_millis(1500), Duration::from_millis(1500));
    thread_answer(jev, "security-fix", 0.95);
    rig_on(texts(4), Some(jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 60;
    })
}

/// The router's guess, `tidewatch builds slowly`, is the thread `pr-round`
/// (it names that repo). With that thread at 137k the message runs at home,
/// tagged a guess and movable (a person sees the ordinary move offer, not the
/// reason: only the row says which thread was spared and how large it was); the
/// thread's own session took no turn. A message that names the thread by
/// its slug still goes there: the owner chose it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guess_into_a_thread_at_137k_runs_at_home_movable_and_the_row_names_the_thread() {
    let jev = FakeJev::start().unwrap();
    let r = late_jev_rig(&jev);
    let c = &r.core;
    two_threads(c);
    hold_context(c, "pr-round", 137_000);
    let dm = home(c, None);
    let a = submit(c, &dm, "tidewatch builds slowly").await;
    let t = tag(&a);
    assert_eq!(a.session_id, dm, "the message ran at home");
    assert_eq!(
        (t.slug.as_deref(), t.how, t.movable),
        (None, RouteHow::Guess, true)
    );
    assert!(
        texts_of(c, &session_of(c, "pr-round")).is_empty(),
        "the large thread took no turn"
    );
    let row = &rows(c, "thread.routed")[0];
    assert_eq!(row.data["to"], "home");
    assert_eq!(
        row.data["spared"],
        json!({"thread": "pr-round", "context_tokens": 137_000})
    );
    let named = submit(c, &dm, "#pr-round does tidewatch still build slowly?").await;
    assert_eq!(
        named.session_id,
        session_of(c, "pr-round"),
        "an address is the owner's choice"
    );
    assert_eq!(tag(&named).how, RouteHow::Addressed);
}

/// The same guess into a thread below the line is placed there as before,
/// and its row carries no `spared`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guess_into_a_thread_under_the_line_is_placed_there_as_before() {
    let jev = FakeJev::start().unwrap();
    let r = late_jev_rig(&jev);
    let c = &r.core;
    two_threads(c);
    hold_context(c, "pr-round", 60_000);
    let dm = home(c, None);
    let a = submit(c, &dm, "tidewatch builds slowly").await;
    assert_eq!(a.session_id, session_of(c, "pr-round"));
    assert_eq!(tag(&a).how, RouteHow::Guess);
    assert!(rows(c, "thread.routed")[0].data.get("spared").is_none());
}
