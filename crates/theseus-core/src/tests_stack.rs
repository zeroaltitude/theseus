//! The turn's stack margin (theseus-b4sf). A turn's future at opt-level 0
//! needs far more stack than a release build's, and a debug daemon polls it
//! on a tokio worker with std's 2 MiB stack. The turn's futures are boxed
//! (`TurnRunner::run` and the loop's largest calls), so a thread's stack holds
//! only their poll frames; this holds the margin those boxes bought: against a
//! large local across an await in the loop's poll frames, and against the loop's
//! boxes lost. It does not hold each box: `TurnRunner::run` as an `async fn` again
//! (the loop's boxes kept) needs about 896 KiB, inside the bound, so the entry
//! box has a test of its own, by its size.
//!
//! The golden's conversation (every scenario but the budget's: tool calls,
//! a background job's late result, a wake) runs on a thread with an explicit
//! 1.5 MiB stack, its future on the heap as a spawned task's is, so the thread
//! holds what a worker holds. Its size is set here, never by the environment,
//! so the test holds under any `RUST_MIN_STACK` and under nextest. Bisected,
//! the conversation needed between 512 and 576 KiB when this landed (it
//! needed 1,856 to 1,920 KiB before the boxes); a step that adds a large
//! local across an await in the turn loop, or unboxes the loop, overflows here,
//! and an overflow aborts the test's process.

use super::tests_output::conversation;

/// Three quarters of a debug daemon's worker stack.
const STACK: usize = 1536 * 1024;

#[test]
fn the_golden_conversation_runs_on_a_one_and_a_half_mib_stack() {
    let out = std::thread::Builder::new()
        .name("stack-margin".into())
        .stack_size(STACK)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let mut out = String::new();
            rt.block_on(Box::pin(conversation(&mut out)));
            out
        })
        .unwrap()
        .join()
        .expect("the conversation panicked");
    assert!(
        out.starts_with("== a conversation\n") && out.lines().count() > 100,
        "the conversation ran short:\n{out}"
    );
}

/// `TurnRunner::run` returns a box, never the turn's future: at opt-level 0
/// a caller's poll frame keeps a slot the size of each future it builds
/// (theseus-2kyc). The stack test above can't tell it: an `async fn run`
/// with the loop's boxes kept runs inside its bound. So the future's size is
/// the box's: a fat pointer, where an `async fn` makes it about 8 KiB.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_turns_entry_is_a_box_not_the_turns_future() {
    let r = crate::tests_route::rig(None, 1, |_| {});
    let rec = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = crate::bus::EventSink::new(r.core.bus.clone(), &rec.session_id, None);
    let req = crate::turn::TurnRequest {
        prompt: None,
        session: rec,
        input: Some("hello".into()),
        target,
        sink,
        author: "test".into(),
        recompile: None,
        attachments: vec![],
        arrived: None,
        reply_to: None,
    };
    let fut = r.core.runner.run(req);
    type Boxed<'a> = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = anyhow::Result<theseus_protocol::TurnSubmitResult>>
                + Send
                + 'a,
        >,
    >;
    assert_eq!(
        std::mem::size_of_val(&fut),
        std::mem::size_of::<Boxed<'static>>()
    );
    drop(fut);
}
