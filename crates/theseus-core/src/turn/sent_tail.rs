//! What a request sent after the conversation, sent again (issue 20, `docs/design/thinking-binding.md`).
//!
//! Every request ends with a tail of the harness's own blocks (the date and
//! the git line, the task view, the standing records, a hint), glued to its
//! last message. They are new at each step, so the message that carried them
//! stops carrying them the moment the answer to it is appended: its content
//! changes under the thinking block that answer holds. A thinking block is
//! bound to every block before it, so the next request replays a block whose
//! prefix is no longer the one it was written in. Older models report it;
//! `claude-fable-5-2` refuses it.
//!
//! So a tail is kept with the message it rode, by what that message rendered
//! as, and put back in place on the requests after it; what is new goes
//! after, as it always did. Within one turn a block the latest tail already
//! holds, unchanged, is not sent a second time: the model has it a step back,
//! and a turn of many steps grows by what changed and nothing else. Each new
//! turn's request carries its tail whole, as it always did, and the turns
//! before it keep theirs. A kept tail whose message
//! no longer renders as it did (a recompile, a ring's cut, a compaction)
//! is let go, and so is the lot when a recompile strips the thinking, which
//! is what needed them.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::compiler::Compiled;
use crate::provider::binding::unmarked;

/// How many sessions keep their tails; the least recently used goes first.
const SESSIONS: usize = 256;
/// How many tails a session keeps.
const PER_SESSION: usize = 2048;

/// One request's tail: the message it rode, what that message rendered as
/// without it, and its blocks.
#[derive(Debug, Clone)]
struct Sent {
    turn: String,
    /// Where it was sent: the place, the surface, where the session and the turn's reply speak
    /// (`compile_step`). A tail is put back only on a request for the same.
    key: String,
    anchor: String,
    blocks: Vec<Value>,
}

#[derive(Default)]
struct Held {
    by_session: HashMap<String, Vec<Sent>>,
    recent: VecDeque<String>,
}

/// Each session's kept tails.
#[derive(Default)]
pub struct SentTails(Mutex<Held>);

/// A request's last message before its tail: where the tail begins, and the
/// tails of earlier requests put back.
pub(super) struct Begun {
    turn: String,
    key: String,
    /// The latest tail put back on this request, in `kept`.
    here: Option<usize>,
    blocks: usize,
    anchor: String,
    kept: Vec<Sent>,
}

fn anchor(message: &Value) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(&unmarked(message)).unwrap_or_default(),
    ))
}

/// A message's content as blocks, a string content turned into one text block.
fn blocks_of(message: &mut Value) -> Option<&mut Vec<Value>> {
    let content = &mut message["content"];
    if let Value::String(s) = content {
        *content = json!([{"type": "text", "text": s.clone()}]);
    }
    content.as_array_mut()
}

fn bytes(blocks: &[Value]) -> u64 {
    blocks
        .iter()
        .map(|b| b["text"].as_str().map_or(0, str::len) as u64)
        .sum()
}

/// `n` more bytes of text in the request: its estimate, as `attach_hint` counts a tail block.
fn counted(compiled: &mut Compiled, n: u64) {
    let n = n.div_ceil(crate::task_graph::view::BYTES_PER_TOKEN);
    compiled.est_tokens += n;
    compiled.estimate.tokens += n;
    compiled.estimate.estimated += n;
    compiled.estimate.upper += n + n * crate::compiler::MARGIN_PERCENT / 100;
}

/// Put the kept tails `held` back on `compiled`'s messages, in message order: each is found by what
/// its message rendered as, not by its place (routing may have moved a message). Only the tails
/// sent to `key` go back (`None`: the newest tail's own key, for a request no turn is making yet);
/// the rest stay in the list as they were sent. Nothing goes back on a request a recompile
/// stripped the thinking from. Returns the list and the place in it of the latest tail put back.
/// The one function a turn's request and every other request of a conversation share.
fn put_back(
    compiled: &mut Compiled,
    held: Vec<Sent>,
    key: Option<&str>,
    stripped: bool,
) -> (Vec<Sent>, Option<usize>) {
    let Some(last) = compiled.request.messages.len().checked_sub(1) else {
        return (held, None);
    };
    let key = key
        .or_else(|| held.last().map(|s| s.key.as_str()))
        .map(str::to_string);
    let mut kept = Vec::new();
    let anchors: Vec<String> = match held.is_empty() || stripped {
        true => Vec::new(),
        false => compiled.request.messages[..last]
            .iter()
            .map(anchor)
            .collect(),
    };
    let (mut from, mut here) = (0, None);
    for s in held.into_iter().filter(|_| !stripped) {
        // What was said to another place or surface is not said here: it stays as it was sent,
        // for a request that comes back to where it was sent.
        if Some(&s.key) != key.as_ref() {
            kept.push(s);
            continue;
        }
        let Some(i) = (from..anchors.len()).find(|&i| anchors[i] == s.anchor) else {
            continue;
        };
        if let Some(b) = blocks_of(&mut compiled.request.messages[i]) {
            b.extend(s.blocks.iter().cloned());
            counted(compiled, bytes(&s.blocks));
            from = i + 1;
            kept.push(s);
            here = Some(kept.len() - 1);
        }
    }
    (kept, here)
}

impl SentTails {
    /// The tails a conversation's earlier requests sent, put back on a request made for it
    /// outside a turn (the keep-warm read, the census), so that it is the prefix the next turn
    /// sends: byte for byte what `begin` puts back, for the key the newest tail was sent to.
    /// Nothing is kept or changed here, and a request for a model of restricted origin gets none.
    pub(crate) fn warm(&self, session: &str, restricted: bool, compiled: &mut Compiled) {
        if restricted {
            return;
        }
        let held = {
            let h = self.0.lock().unwrap_or_else(|e| e.into_inner());
            h.by_session.get(session).cloned().unwrap_or_default()
        };
        let stripped = compiled.new_compilation && compiled.compilation.manifest.strip_thinking;
        put_back(compiled, held, None, stripped);
    }

    /// Put back the tails the session's earlier requests sent, on the
    /// messages that carried them, before this request's own is attached.
    pub(super) fn begin(
        &self,
        session: &str,
        (turn, key): (&str, &str),
        restricted: bool,
        compiled: &mut Compiled,
    ) -> Option<Begun> {
        // A request for a model of restricted origin carries none of what another turn's request
        // carried after the conversation (a task view, a record, a note drawn from other
        // sessions): they stay with the requests that had them, kept, and this one is as it was.
        if restricted {
            return None;
        }
        let last = compiled.request.messages.len().checked_sub(1)?;
        let stripped = compiled.new_compilation && compiled.compilation.manifest.strip_thinking;
        let held = {
            let mut h = self.0.lock().unwrap_or_else(|e| e.into_inner());
            h.recent.retain(|s| s != session);
            h.recent.push_back(session.to_string());
            h.by_session.get(session).cloned().unwrap_or_default()
        };
        let (kept, here) = put_back(compiled, held, Some(key), stripped);
        let m = &mut compiled.request.messages[last];
        let anchor = anchor(m);
        let blocks = blocks_of(m).map_or(0, |b| b.len());
        Some(Begun {
            turn: turn.to_string(),
            key: key.to_string(),
            here,
            blocks,
            anchor,
            kept,
        })
    }

    /// Keep this request's tail with the last message, less what the latest
    /// kept tail already says, which is left off the request.
    pub(super) fn finish(&self, session: &str, compiled: &mut Compiled, begun: Option<Begun>) {
        let Some(Begun {
            turn,
            key,
            here,
            blocks,
            anchor,
            mut kept,
        }) = begun
        else {
            return;
        };
        // The tail is on the request's last message, wherever routing has since put it.
        let at = compiled.request.messages.len().saturating_sub(1);
        if let Some(b) = compiled
            .request
            .messages
            .get_mut(at)
            .and_then(|m| blocks_of(m))
        {
            // What the same turn's latest tail said: the model has it, and is not told again.
            let said: Vec<Value> = here
                .and_then(|i| kept.get(i))
                .filter(|s| s.turn == turn)
                .map_or(vec![], |s| s.blocks.clone());
            let tail: Vec<Value> = b.split_off(blocks.min(b.len()));
            let (fresh, repeated): (Vec<Value>, Vec<Value>) =
                tail.into_iter().partition(|t| !said.contains(t));
            b.extend(fresh.iter().cloned());
            if !repeated.is_empty() {
                // They were counted when they were attached; the model has them already.
                let n = bytes(&repeated).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN);
                compiled.est_tokens = compiled.est_tokens.saturating_sub(n);
                compiled.estimate.tokens = compiled.estimate.tokens.saturating_sub(n);
                compiled.estimate.estimated = compiled.estimate.estimated.saturating_sub(n);
            }
            if !fresh.is_empty() {
                kept.push(Sent {
                    turn,
                    key,
                    anchor,
                    blocks: fresh,
                });
            }
        }
        // A session that moves between places keeps the tails of each, but not without end.
        let over = kept.len().saturating_sub(PER_SESSION);
        kept.drain(..over);
        let mut h = self.0.lock().unwrap_or_else(|e| e.into_inner());
        h.recent.retain(|s| s != session);
        h.recent.push_back(session.to_string());
        h.by_session.insert(session.to_string(), kept);
        while h.recent.len() > SESSIONS {
            if let Some(old) = h.recent.pop_front() {
                h.by_session.remove(&old);
            }
        }
    }
}
