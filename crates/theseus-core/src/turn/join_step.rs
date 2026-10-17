//! The owner joins a task (theseus-slmj1): a task is a child session that
//! works alone and reports once, and the owner's typed input to it is a steer
//! (`steer_step.rs`), heard at the task's next loop boundary.
//!
//! - **Steering in.** `steered` accepts a task's input when it is the owner's
//!   (`EventSink::owner`, settled by `Core::owner_input` from the connection
//!   and the place rule). A guest's or a job's input queues as before.
//! - **Parking instead of reporting** (`owner_waits`). A message that comes
//!   during the task's last call, which made no tool calls, is offered and
//!   not taken. The task's turn would end it and report, and the message would
//!   be refused as "complete"; so the turn parks on input, the message wakes it
//!   (`WokenByInput`), and that turn's last message is the report. One atomic
//!   load, read only at the end of a task's turn.
//! - **Parking on the owner's hold** (`paused_at`, `task.pause`): at a loop
//!   boundary a held task's turn ends waiting on input, and `task_end` keeps
//!   that wait, so it parks and never reports until `task.resume` wakes it.
//! - **The count** (`join`, `joined`): the messages the task took from the
//!   owner, in memory by session. The frame that ends the task writes it as
//!   the META record `task::joined_key` (no store format change), and the
//!   parent's report node says `(you joined: 2 messages)` (`Report::node_text`).

use super::admission::Class;
use super::*;

impl TurnRunner {
    /// Whether the owner's message is offered to this task's turn and not
    /// taken: the turn parks on input instead of reporting.
    pub(super) fn owner_waits(&self, session_id: &str) -> bool {
        let steers = &self.tools.job_waits.steers;
        steers.any() && steers.get(session_id).is_some_and(|s| s.pending())
    }

    /// The loop boundary of a task the owner holds: the turn ends here, waiting
    /// on input (`task_end` keeps the wait). One atomic load while none is held.
    pub(super) fn paused_at(&self, t: &mut Turn<'_>) -> bool {
        let held = t.tc.task.is_some() && self.task_paused(t.tc.session_id);
        if held {
            t.stop_reason = "paused".into();
        }
        held
    }

    /// A task's turn that the owner's input began is a message it took.
    pub(super) fn note_owner_turn(&self, req: &TurnRequest, class: Class) {
        if class == Class::Task && req.sink.owner && req.input.is_some() && req.prompt.is_none() {
            self.tools.job_waits.steers.join(&req.session.session_id, 1);
        }
    }

    /// A task's turn that would wait on input ends it (its last message is
    /// its report) unless something is left for it: results its model has not
    /// read, a pending wake (37b), or the owner's message (above).
    pub(super) fn task_end(
        &self,
        (kernel, execution_id, session_id): (&theseus_kernel::Kernel, &str, &str),
        end: TurnEnd,
        is_task: bool,
        again: &mut bool,
        stop: &str,
    ) -> TurnEnd {
        let wait = TurnEnd::Wait { wake: Wake::Input };
        match end {
            // The owner holds the task (`task.pause`): it parks, never reports.
            TurnEnd::Wait { wake: Wake::Input } if is_task && self.task_paused(session_id) => {
                *again = false;
                TurnEnd::Wait {
                    wake: Wake::DueAt {
                        at_ms: theseus_protocol::task_pause::PAUSED_AT_MS,
                    },
                }
            }
            TurnEnd::Wait { wake: Wake::Input } if is_task => {
                if *again || stop == "max_loops" {
                    *again = true;
                    wait
                } else if crate::task::parks_on_wake(kernel, execution_id)
                    || self.owner_waits(session_id)
                {
                    wait
                } else {
                    TurnEnd::Complete {
                        reason: "reported".into(),
                    }
                }
            }
            end => end,
        }
    }
}
