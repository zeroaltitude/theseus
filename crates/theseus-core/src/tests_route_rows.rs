//! A routed turn's rows (theseus-d13v): one `context.compiled` and one
//! `loop.started` a loop, whether routing kept the first compile, switched,
//! or detoured, and every `context.compiled` row names a compilation that
//! reads back. A detour's loop records `loop.started` alone: its compilation
//! is never stored, so no row names it. The rig is `tests_route`'s.

use theseus_judge::fake::FakeJev;
use theseus_protocol::TurnSubmitResult;

use crate::tests_judge::kinds;
use crate::tests_route::{mode, rig, turn, Rig};

/// The turn's rows of `kind`, by their loop index.
fn loops(r: &Rig, res: &TurnSubmitResult, kind: &str) -> Vec<u64> {
    kinds(&r.core.store, kind)
        .into_iter()
        .filter(|row| row.turn_id.as_deref() == Some(res.turn_id.as_str()))
        .map(|row| row.data["loop"].as_u64().unwrap())
        .collect()
}

/// One `context.compiled` and one `loop.started` for loop 0, and the
/// compilation the row names is stored.
fn one_of_each(r: &Rig, res: &TurnSubmitResult, what: &str) {
    assert_eq!(loops(r, res, "loop.started"), [0], "{what}: loop.started");
    assert_eq!(
        loops(r, res, "context.compiled"),
        [0],
        "{what}: context.compiled"
    );
    for row in kinds(&r.core.store, "context.compiled") {
        let id = row.data["compilation_id"].as_str().unwrap();
        assert!(
            r.core.store.get_compilation(id).unwrap().is_some(),
            "{what}: {id} does not read back"
        );
    }
}

/// A switch (a hard question to Opus) and a verdict that keeps the first
/// compile (a chat on the routed profile) each record one row of each kind,
/// and each row's compilation reads back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_switched_turn_records_one_compile_and_one_loop() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 3, |c| c.routing.max_wait_ms = 5_000);
    let one = turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe write-ahead log.",
        None,
    )
    .await;
    assert_eq!(one.route.as_ref().unwrap().reason, "verdict");
    assert_eq!(one.profile, "opus");
    one_of_each(&r, &one, "the switch");
    mode(&jev, "chat", 0.95);
    let two = turn(&r.core, Some(&one.session_id), "What is a frame?", None).await;
    assert_eq!(two.profile, "opus");
    one_of_each(&r, &two, "the first compile kept");
}

/// A detour's loop records one `loop.started`, and no `context.compiled`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detour_records_one_loop_and_no_compile() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 3, |c| c.routing.max_wait_ms = 5_000);
    let first = turn(&r.core, None, "What does the store's manifest hold?", None).await;
    mode(&jev, "trivial", 0.95);
    let thanks = turn(&r.core, Some(&first.session_id), "thank you!", None).await;
    assert_eq!(thanks.route.as_ref().unwrap().reason, "detour");
    assert_eq!(loops(&r, &thanks, "loop.started"), [0]);
    assert!(loops(&r, &thanks, "context.compiled").is_empty());
}
