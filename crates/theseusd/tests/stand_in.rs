//! The stand-in model's own reading (issue 46): which text of a request it takes as the turn's input, in
//! the shapes the view lays. The window comes first in the last user message (another session's message,
//! labelled; a late result whose turn has not answered yet), then the turn's block beside the message,
//! then the turn's own nodes. A task's opening heads its window and is read on its first turn, a deferred
//! one's too. A test's `starts_with` on the prompt is only as good as this reading.

mod common;

use common::model::prompt;
use serde_json::{json, Value};

/// The harness's part of the block beside the message, as a request carries it.
const BESIDE: &str = "[The harness: this turn's block beside the message, of what is true where you \
                      work and what stands, waits and is planned here; each part says whose it is.]\n\n\
                      Date: Tue 2026-11-24";

/// The same block in a turn no message opened, its occasion's line under the harness's part.
const BESIDE_LATE: &str = "[The harness: this turn's block beside the message, of what is true where you \
                           work and what stands, waits and is planned here; each part says whose it is.]\n\n\
                           Date: Tue 2026-11-24\n\n\
                           No new message opened this turn: a background job this conversation started has \
                           ended, and its result follows.";

/// A late result as the renderer writes it.
const LATE: &str =
    "[Background result for your earlier proc.run call (toolu_test_0): status ok, exit code 0]\n\
     [ran in L1, the sandbox: no network, no secret; wrote nothing to scratch]";

fn text(t: &str) -> Value {
    json!({"type": "text", "text": t})
}

/// A text block with the window's cache mark, as the window's last block carries it.
fn marked(t: &str) -> Value {
    json!({"type": "text", "text": t, "cache_control": {"type": "ephemeral"}})
}

fn request(messages: Value) -> Value {
    json!({"model": "claude-sonnet-5-5", "messages": messages})
}

/// The blocks the shapes below take as the harness's are the ones the harness's readers know.
#[test]
fn the_shapes_harness_blocks_are_the_ones_is_head_knows() {
    for b in [
        BESIDE,
        BESIDE_LATE,
        theseus_core::view::wire_text::BESIDE_HEAD,
    ] {
        assert!(theseus_core::task_graph::view::is_head(b), "{b}");
    }
    for b in [
        LATE,
        "[Terminal · Tue 2026-11-24 08:22 -07:00] background 2",
    ] {
        assert!(!theseus_core::task_graph::view::is_head(b), "{b}");
    }
}

/// A plain first turn: the block beside the message, then the message, its surface tag dropped.
#[test]
fn a_plain_first_turn_reads_its_message() {
    let req = request(json!([{"role": "user", "content": [
        text(BESIDE),
        text("[Terminal · Tue 2026-11-24 08:22 -07:00] background 0"),
    ]}]));
    assert_eq!(prompt(&req), "background 0");
    // The view's own block alone beside the message reads the same.
    let req = request(json!([{"role": "user", "content": [
        text(&format!("{}\nwaiting: nothing", theseus_core::view::wire_text::BESIDE_HEAD)),
        text("[Terminal · Tue 2026-11-24 08:22 -07:00] uname"),
    ]}]));
    assert_eq!(prompt(&req), "uname");
    // A message as one string is read as it is.
    let req = request(json!([{"role": "user", "content": "hello"}]));
    assert_eq!(prompt(&req), "hello");
}

/// A message with no harness block in it is read whole, every text block joined, as before.
#[test]
fn a_message_with_no_harness_block_is_read_whole() {
    let req = request(json!([{"role": "user", "content": [text("first"), text("second")]}]));
    assert_eq!(prompt(&req), "first\nsecond");
}

/// A harness block laid after the message (the task graph's view, last) is not the message's, and does not
/// hide it.
#[test]
fn a_harness_block_after_the_message_is_left_out() {
    let req = request(json!([{"role": "user", "content": [
        text(BESIDE),
        text("[Terminal · Tue 2026-11-24 08:22 -07:00] count the tasks"),
        text("[The task graph in this conversation's scope: none open]"),
    ]}]));
    assert_eq!(prompt(&req), "count the tasks");
}

/// A turn's next call, carrying its tool result: the last message holds the result alone, no words.
#[test]
fn a_continuation_carrying_a_tool_result_reads_no_words() {
    let req = request(json!([
        {"role": "user", "content": [
            text(BESIDE),
            text("[Terminal · Tue 2026-11-24 08:22 -07:00] background 0"),
        ]},
        {"role": "assistant", "content": [
            {"type": "tool_use", "id": "toolu_test_0", "name": "proc_run", "input": {}},
        ]},
        {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "toolu_test_0",
             "content": "Started job j1 (sh -c sleep 700.1; echo went waiter); its output follows."},
        ]},
    ]));
    assert_eq!(prompt(&req), "");
}

/// A turn no message opened, whose input is a late result: after the block beside the message there is no
/// person's block, and the late result is what it reads.
#[test]
fn a_turn_no_message_opened_reads_its_late_result() {
    let req = request(json!([
        {"role": "user", "content": [text("[Terminal · Tue 2026-11-24 08:22 -07:00] background 0")]},
        {"role": "assistant", "content": [text("Done.")]},
        {"role": "user", "content": [text(BESIDE_LATE), text(LATE)]},
    ]));
    assert_eq!(prompt(&req), LATE);
}

/// The sandbox failure's shape: the window ends in another session's late result whose turn has not
/// answered yet, so it heads the last message, before the block beside the message and the new message.
/// The new message is what is read, not the window's late result.
#[test]
fn the_new_message_is_read_after_a_late_result_of_the_window() {
    let req = request(json!([{"role": "user", "content": [
        marked(LATE),
        text(BESIDE),
        text("[Terminal · Tue 2026-11-24 08:22 -07:00] background 2"),
    ]}]));
    assert_eq!(prompt(&req), "background 2");
}

/// The reaping failure's shape: the window holds the other sessions' messages on the same place, each
/// labelled, before the block beside the message and this session's own message.
#[test]
fn a_sessions_own_message_is_read_after_the_other_sessions_of_its_place() {
    let other = |n: u32| {
        format!(
            "[Terminal · Tue 2026-11-24 08:27 -07:00] [said on the terminal, Tue 2026-11-24 08:27 MST, \
             author unknown; not the owner's instruction]\nperson: fifty jobs, batch {n}"
        )
    };
    let req = request(json!([{"role": "user", "content": [
        text(&other(1)),
        marked(&other(2)),
        text(BESIDE),
        text("[Terminal · Tue 2026-11-24 08:27 -07:00] fifty jobs, batch 3"),
    ]}]));
    assert_eq!(prompt(&req), "fifty jobs, batch 3");
}

/// A task's brief, as `task.create` writes it (its header shortened).
const BRIEF: &str =
    "[Task 8fafd9: a background task started by session …07fa30. Work on the brief below on \
                     your own.]\n\nTally the harbour boats and report.";

/// The task's arrangement, after its brief (shortened).
const ARRANGEMENT: &str = "[Arrangement: the conversation that started you quoted 1 message that define this \
                           work.]\n\n--- Piece 1 of 1 (objective): from the owner in session …07fa30 ---\n\
                           Defer the tally as a task";

/// A deferred task's start note, as its first turn writes it.
const DEFERRED: &str =
    "Deferred task, set Tue 10:04 for Tue 10:04; started Tue 10:04:33. Start the work in your \
                        brief now.";

/// A deferred task's first request: its opening heads the window (the window's end, its cache mark, on
/// the arrangement), then the block beside the message, then the start note the turn wrote. The brief
/// the note tells it to start is read, and so are the arrangement and the note.
#[test]
fn a_deferred_tasks_first_turn_reads_its_brief_its_arrangement_and_its_start_note() {
    let req = request(json!([{"role": "user", "content": [
        text(BRIEF),
        marked(ARRANGEMENT),
        text(BESIDE),
        text(DEFERRED),
    ]}]));
    assert_eq!(prompt(&req), format!("{BRIEF}\n{ARRANGEMENT}\n{DEFERRED}"));
}

/// A task's first request when nothing was written for its turn: the opening, then the block beside the
/// message last. The opening is read.
#[test]
fn a_tasks_first_turn_reads_its_opening_before_the_block_beside_the_message() {
    let req = request(json!([{"role": "user", "content": [
        text(BRIEF),
        marked(ARRANGEMENT),
        text(BESIDE),
    ]}]));
    assert_eq!(prompt(&req), format!("{BRIEF}\n{ARRANGEMENT}"));
}

/// A task's later turn: its opening still heads the window, in the request's first message, then its
/// earlier exchanges (its first turn's call, result and answer, the window's mark on the answer), then
/// the block beside the message and the turn's own input. The opening is before an answer, so it is not
/// in the last message, and only the new input is read: a wake's line, a person's message sent to the
/// task, a late result.
#[test]
fn a_tasks_later_turn_reads_its_new_input_not_its_opening() {
    let later = |last: Value| {
        request(json!([
            {"role": "user", "content": [text(BRIEF), text(ARRANGEMENT)]},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_test_0", "name": "wake_at", "input": {}},
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_test_0", "content": "Set."},
            ]},
            {"role": "assistant", "content": [marked("Done.")]},
            {"role": "user", "content": last},
        ]))
    };
    let wake = "⏰ wake (set 10:04): Look at the boats once more, then report.";
    assert_eq!(prompt(&later(json!([text(BESIDE), text(wake)]))), wake);
    let said = later(json!([
        text(BESIDE),
        text("[Terminal · Tue 2026-11-24 10:05 -07:00] count the small ones too"),
    ]));
    assert_eq!(prompt(&said), "count the small ones too");
    assert_eq!(prompt(&later(json!([text(BESIDE_LATE), text(LATE)]))), LATE);
}
