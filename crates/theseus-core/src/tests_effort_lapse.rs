//! The effort refusal mark on a virtual clock (theseus-2ix1k). `turn/effort_step.rs` marks a
//! model that refused per-message effort and holds the mark for `REFUSAL_HOLDS_MS` of the
//! kernel's clock, so a core built on a `VirtualClock` drives the hour without waiting for it.
//! The rig and the scripts are `tests_turn_effort`'s.

use theseus_judge::fake::FakeJev;
use theseus_kernel::{Clock, VirtualClock};

use crate::provider::{ProviderError, ProviderRequest, Scripted};
use crate::tests_route::{mode, rig_on_clock, try_turn_attached, turn, Rig};
use crate::tests_turn_effort::effort;

const HOUR_MS: u64 = 3_600_000;

fn carries(q: &ProviderRequest) -> bool {
    q.messages.iter().any(|m| m["role"] == "system")
}

fn refused_rows(r: &Rig) -> usize {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = r.core.store.ledger_tail(2000).unwrap();
    rows.iter()
        .filter(|(_, l)| l.kind == "provider.effort_refused")
        .count()
}

fn bad_request(message: &str) -> Scripted {
    Scripted::Fail(ProviderError::InvalidRequest {
        status: 400,
        message: message.into(),
    })
}

fn rig_at(clock: std::sync::Arc<VirtualClock>) -> (Rig, FakeJev) {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    effort(&jev, "low", 0.95);
    let r = rig_on_clock(Some(&jev), 0, |c| c.model.live = "fable".into(), clock);
    (r, jev)
}

/// A 400 naming `output_config` marks the model; its next request carries no effort message
/// until the hour has passed, to the millisecond.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refusal_mark_holds_an_hour_by_the_kernels_clock() {
    let clock = VirtualClock::new(1_800_000_000_000);
    let (r, _jev) = rig_at(clock.clone());
    let marked_at = clock.now_ms();
    {
        let mut s = r.claude.script.lock().unwrap();
        s.push_back(bad_request(
            "invalid_request_error: messages.0.output_config: Extra inputs are not permitted",
        ));
        for t in [
            "By the weir.",
            "Nor in winter.",
            "Under the alders.",
            "At dusk.",
        ] {
            s.push_back(Scripted::text(t));
        }
    }
    let one = turn(&r.core, None, "Where do the herons nest?", None).await;
    assert_eq!(one.output, "By the weir.");
    let reqs = r.claude.requests();
    assert_eq!(reqs.len(), 2, "asked again once");
    assert!(carries(&reqs[0]) && !carries(&reqs[1]));
    assert_eq!(refused_rows(&r), 1, "one mark");

    clock.set(marked_at + HOUR_MS - 1);
    turn(&r.core, Some(&one.session_id), "And in winter?", None).await;
    let held = r.claude.requests().pop().unwrap();
    assert!(!carries(&held), "{:?}", held.messages);
    assert_eq!(r.claude.requests().len(), 3, "no further failed call");

    clock.set(marked_at + HOUR_MS);
    turn(&r.core, Some(&one.session_id), "Where else?", None).await;
    let lapsed = r.claude.requests().pop().unwrap();
    assert!(carries(&lapsed), "an hour on, the messages are tried again");
}

/// A 400 that does not name `output_config` is no refusal of effort: the turn does not ask
/// again, the model is not marked, and the next request still carries the effort message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_400_that_does_not_name_output_config_marks_nothing() {
    let clock = VirtualClock::new(1_800_000_000_000);
    let (r, _jev) = rig_at(clock);
    {
        let mut s = r.claude.script.lock().unwrap();
        s.push_back(bad_request(
            "invalid_request_error: messages.0.content: Input should be a valid list",
        ));
        s.push_back(Scripted::text("By the weir."));
    }
    let failed =
        try_turn_attached(&r.core, None, "Where do the herons nest?", None, Vec::new()).await;
    assert!(failed.is_err(), "the 400 ends the turn");
    let reqs = r.claude.requests();
    assert_eq!(reqs.len(), 1, "not asked again without the effort message");
    assert!(carries(&reqs[0]));
    assert_eq!(refused_rows(&r), 0, "nothing marked");

    turn(&r.core, None, "And in winter?", None).await;
    let next = r.claude.requests().pop().unwrap();
    assert!(carries(&next), "{:?}", next.messages);
}
