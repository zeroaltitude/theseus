//! A turn whose input is an MCP server's prompt (M7 step 36c): each message
//! is a user-role node of origin `mcp`, authored `prompt:<server>/<name>`,
//! and all of them are one frame.

use anyhow::Result;
use theseus_store::NewRecord;

use super::{title_from, Turn, TurnRunner};
use crate::mcp::prompts::PromptInput;
use crate::node::Node;
use crate::session::{SessionRecord, TargetRef};

impl TurnRunner {
    /// Write the prompt's messages as the turn's input nodes: one frame, with
    /// the hold and a moved target riding in it under the session's lock.
    pub(super) fn write_prompt_input(
        &self,
        t: &mut Turn<'_>,
        session: &mut SessionRecord,
        prompt: PromptInput,
        moved: Option<&TargetRef>,
    ) -> Result<()> {
        let (sid, turn_id) = (t.tc.session_id, t.tc.turn_id);
        let nodes: Vec<Node> = prompt
            .messages
            .iter()
            .map(|m| {
                let files = self.accept_files(t, m.files.clone());
                Node::prompt_message(sid, Some(turn_id), &prompt.author, &m.text, files)
            })
            .collect();
        let title = match title_from(&prompt.text()) {
            t if t.is_empty() => prompt.author,
            t => t,
        };
        super::title_step::retitle(session, title, self.cfg.sessions.retitle_within_turns);
        let records = nodes
            .iter()
            .map(Node::record)
            .collect::<Result<Vec<NewRecord>>>()?;
        let framed = t.tc.store.with_session(sid, |mut rec| {
            let mut frame = records.clone();
            if let Some(m) = moved {
                rec.last_target = Some(m.clone());
            }
            if moved.is_some() {
                frame.push(NewRecord::json(
                    theseus_store::kinds::SESSION,
                    Some(sid),
                    &rec,
                )?);
            }
            t.tc.store.append(&frame)?;
            Ok(())
        })?;
        if framed.is_none() {
            t.tc.store.append(&records)?;
        }
        for n in &nodes {
            t.tc.node_written(n);
        }
        // An MCP server's prompt is text from outside, an item like a result.
        self.content_nodes(
            sid,
            &nodes,
            crate::content::ItemKind::ToolResult,
            "mcp.prompt",
        );
        Ok(())
    }
}
