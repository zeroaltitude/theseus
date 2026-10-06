//! The turn's stack margin (theseus-b4sf). A turn's future at opt-level 0
//! needs far more stack than a release build's, and a debug daemon polls it
//! on a tokio worker with std's 2 MiB stack. The turn's futures are boxed
//! (`TurnRunner::run` and the loop's largest calls), so a thread's stack holds
//! only their poll frames; this holds the margin those boxes bought.
//!
//! The golden's conversation (every scenario but the budget's: tool calls,
//! a background job's late result, a wake) runs on a thread with an explicit
//! 1.5 MiB stack, its future on the heap as a spawned task's is, so the thread
//! holds what a worker holds. Its size is set here, never by the environment,
//! so the test holds under any `RUST_MIN_STACK` and under nextest. Bisected,
//! the conversation needed between 512 and 576 KiB when this landed (it
//! needed 1,856 to 1,920 KiB before the boxes); a step that adds a large
//! local across an await in the turn loop, or unboxes a turn, overflows here,
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
