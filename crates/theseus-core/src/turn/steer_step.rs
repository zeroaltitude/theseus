//! The steer (theseus-qu1lc): a person's message sent while a turn runs is
//! heard by that turn at once, and joins it at its next loop boundary
//! instead of waiting for the turn to end and running as a turn of its own.
//! The queue and the signal are `toolrun/waits/steer.rs`; this file is the
//! two ends of it in the turn.
//!
//! - The message's own turn, queued behind the running one (`steered`):
//!   before it waits in the admission queue it offers its input to the
//!   running turn. If the turn takes it, this request is answered with that
//!   turn's result, its input written once, by the turn, and never run
//!   again; if the turn ends without it, the message queues as it always
//!   did.
//! - The running turn (`steer_in`), after a loop's tool results are written
//!   and before the next model call: it takes the messages offered, in
//!   arrival order, and writes each as the node its own turn would have
//!   written, in one frame with the `turn.steered` row. The judge's inbound
//!   point runs for each. The turn's route stays the running one: a verdict
//!   or a correction in the words applies from the next turn, as a message
//!   that arrived after the turn's route was settled always did. A message
//!   that comes during a model call waits for that call's boundary (the
//!   call is never cut), and one that comes during the turn's last call
//!   (no tool calls) runs as the next turn, as before.
//!
//! Only a person's typed input is offered: a wake, a continuation and a job's
//! completion queue as they did, and so does a task's turn, unless its input is
//! the owner's (`EventSink::owner`): the owner joins a running task through
//! the same offer (theseus-slmj1; `join_step.rs`). Nothing is matched in the
//! words: "cancel it" reaches the model, which calls `job_stop`.

use super::admission::Class;
use super::*;
use crate::toolrun::waits::steer::{Settled, Steered};

impl TurnRunner {
    /// A person's input that found its session's turn held: offered to that
    /// turn. `Some` is the request's answer (the running turn's result, or
    /// why it has none); `None`, it queues as before.
    pub(crate) async fn steered(
        &self,
        req: &TurnRequest,
        class: Class,
        arrived: Instant,
    ) -> Option<Result<TurnSubmitResult>> {
        // A person's typed input, or the owner's into a task (theseus-slmj1):
        // anyone else's queues as it did.
        let steerable = class.is_typed(req.sink.owner);
        let text = req.input.as_ref().filter(|_| steerable)?;
        if req.prompt.is_some() {
            return None;
        }
        let sid = &req.session.session_id;
        let offer = self.tools.job_waits.steers.offer(
            sid,
            Steered {
                text: text.clone(),
                attachments: req.attachments.clone(),
                author: req.author.clone(),
                owner: req.sink.owner,
                surface: req.sink.surface.clone(),
                reply_to: req.reply_to.clone(),
                arrived,
            },
        )?;
        let Settled::Taken(answer) = offer.settle().await else {
            return None;
        };
        // Stored: the next request on its client's lane may run (theseus-klo2).
        crate::rpc::ordered::applied();
        Some(answer.answer().await.map_err(|why| {
            turn_error(
                "steered",
                sid,
                "",
                arrived.elapsed().as_millis() as u64,
                anyhow::anyhow!(why),
            )
        }))
    }

    /// The loop boundary: the messages offered to this turn become its
    /// context. Nothing offered anywhere is one atomic load.
    pub(super) fn steer_in(&self, t: &mut Turn<'_>) -> Result<()> {
        let steers = &self.tools.job_waits.steers;
        if !steers.any() {
            return Ok(());
        }
        let Some(steer) = steers.get(t.tc.session_id) else {
            return Ok(());
        };
        let taken = steer.take();
        if taken.is_empty() {
            return Ok(());
        }
        let (sid, turn_id) = (t.tc.session_id, t.tc.turn_id);
        let mut records = Vec::new();
        let mut nodes = Vec::new();
        let mut waited_ms = 0;
        for m in taken.messages() {
            let files = self.accept_files(t, m.attachments.clone());
            let mut node = Node::user_with(sid, Some(turn_id), &m.author, &m.text, files);
            node.surface.clone_from(&m.surface);
            records.push(node.record()?);
            if let Some(id) = &m.reply_to {
                records.extend(crate::recall::cite::origin_record(&node.id, "discord", id));
                t.reply_to = Some(id.clone());
            }
            waited_ms = waited_ms.max(m.arrived.elapsed().as_millis() as u64);
            nodes.push((node, m.author.clone(), m.owner));
        }
        let ids: Vec<String> = nodes.iter().map(|(n, _, _)| n.id.clone()).collect();
        // The row and the nodes are one frame: written together or not at
        // all, so a crash leaves each message delivered once or not yet.
        let fact = fact::steer::TurnSteered {
            nodes: &ids,
            loop_index: t.loops,
            waited_ms,
        };
        records.push(t.tc.rec().row(&fact)?);
        t.tc.store.append(&records)?;
        t.announce_fact(&fact);
        taken.written(&steer);
        if t.tc.task.is_some() {
            steers.join(sid, u32::try_from(nodes.len()).unwrap_or(u32::MAX));
        }
        for (node, author, owner) in &nodes {
            t.tc.node_written(node);
            self.inbound_point(t, node, author, true);
            let kind = self.message_kind(sid, *owner);
            self.content_message(sid, node, kind, "operator steer");
        }
        Ok(())
    }
}
