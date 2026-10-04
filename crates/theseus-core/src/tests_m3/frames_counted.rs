//! Each turn counts its own frames on its trace (theseus-wz4y): the root
//! span's `frames`, its admission's and its handle's, and its last, which
//! carries the trace. They agree with the store's own count of the same
//! turn. A child of `tests_m3` for its rig and helpers.

use super::*;

/// The root span's `frames`, from the turn's result and from its
/// `turn.trace` row, which must agree.
fn traced_frames(core: &Core, res: &TurnSubmitResult) -> u64 {
    let on_result = res.trace.as_ref().expect("a trace").attrs["frames"]
        .as_u64()
        .expect("a count");
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(200).unwrap();
    let row = rows
        .iter()
        .rev()
        .find(|(_, r)| r.kind == "turn.trace" && r.turn_id.as_deref() == Some(&res.turn_id))
        .expect("its turn.trace row");
    assert_eq!(
        row.1.data["attrs"]["frames"], on_result,
        "the row says the same"
    );
    on_result
}

/// A session's first turn (which opens its execution: 8 frames), a plain turn, and a
/// turn with a tool call each read their own count, and it is the store's
/// count of frames from before the turn to after it. The plain turn's is 5,
/// the budget.
#[tokio::test]
async fn each_turn_reads_its_own_frames_and_the_store_agrees() {
    let r = rig(vec![
        Scripted::text("first"),
        Scripted::text("hello"),
        Scripted::tools(
            "Reading it.",
            &[("t1", "fs_read", json!({"path": "hello.txt"}))],
        ),
        Scripted::text("It says hi."),
    ]);
    std::fs::write(r.root.join("hello.txt"), "hi\n").unwrap();
    let frames = || r.core.store.stats().unwrap().frames_appended;
    // The session exists before its first turn, as `session.open` makes it.
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let mut counted = Vec::new();
    for input in ["warm up", "hi", "read hello.txt"] {
        let f0 = frames();
        let res = turn(&r.core, Some(&rec.session_id), input).await;
        let written = frames() - f0;
        assert_eq!(
            traced_frames(&r.core, &res),
            written,
            "{input}: the store wrote {written}"
        );
        counted.push(written);
    }
    assert_eq!(counted[0], 8, "a first turn: {counted:?}");
    assert_eq!(counted[1], 5, "a plain turn: {counted:?}");
    assert_eq!(
        counted[2],
        counted[1] + 4,
        "a loop with a tool call adds 4: {counted:?}"
    );
}
