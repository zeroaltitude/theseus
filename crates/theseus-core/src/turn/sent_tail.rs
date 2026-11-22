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
//!
//! A kept tail is admitted again before it is sent again (issue 44, the
//! gate, `docs/design/view-gate.md`): it is put back only under the presence
//! it was made for, while no thread is held from its reader that was not
//! then (an unreadable board holds every one), and while nothing has moved
//! into a thread or changed a thread's binding since (`Store::moves`). This
//! is decided before the request is compiled ([`SentTails::fresh`]): a tail
//! the chain was built on that may not be sent again starts a new chain, its
//! thinking stripped, so the first request sent is one the provider's
//! binding accepts.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Mutex;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_memory::gate::Present;

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
    /// Who it was made for, the threads held from that reader, and the
    /// store's moves, when it was made.
    sealed: Under,
}

/// What a tail is made under, read before the request it rides is compiled:
/// where it is said (`key`), who is present (`view_gate::presence_key`), the
/// threads held from that reader (`view_gate::held_threads`; none when the
/// threads do not wholly read), and the store's moves (`Store::moves`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Under {
    pub key: String,
    pub presence: Option<(Present, String)>,
    pub held: Option<BTreeSet<String>>,
    pub moves: u64,
}

/// Whether the tail `s` may be sent again under `now`: the same presence,
/// no thread held now that was not then, threads that read, and nothing
/// moved or rebound since.
fn admissible(s: &Sent, now: &Under) -> bool {
    let then = s.sealed.held.clone().unwrap_or_default();
    s.sealed.presence == now.presence
        && s.sealed.moves == now.moves
        && now.held.as_ref().is_some_and(|h| h.is_subset(&then))
}

#[derive(Default)]
struct Held {
    by_session: HashMap<String, Vec<Sent>>,
    recent: VecDeque<String>,
    /// Sessions a tail was let go of, at a turn or by a warm read: until a
    /// stripped compilation starts their new chain, every request of theirs
    /// starts one, and no warm read is sent (issue 44).
    stale: HashSet<String>,
    /// Run once at the next `fresh`, for a test that changes what tails are
    /// admitted under between the snapshot and the put-back.
    #[cfg(test)]
    hook: Option<Box<dyn FnOnce() + Send>>,
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
    under: Under,
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
/// stripped the thinking from. A tail sent to `key` that is not admissible under `now` is let go
/// (the caller has started a new chain where one was: `SentTails::fresh`). Returns the list and
/// the place in it of the latest tail put back.
/// The one function a turn's request and every other request of a conversation share.
fn put_back(
    compiled: &mut Compiled,
    held: Vec<Sent>,
    (key, now): (Option<&str>, &Under),
    stripped: bool,
) -> (Vec<Sent>, Option<usize>) {
    let key = key
        .or_else(|| held.last().map(|s| s.key.as_str()))
        .map(str::to_string);
    let held: Vec<Sent> = held
        .into_iter()
        .filter(|s| Some(&s.key) != key.as_ref() || admissible(s, now))
        .collect();
    let Some(last) = compiled.request.messages.len().checked_sub(1) else {
        return (held, None);
    };
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
    /// Whether a request of `session` made under `now` must start a new chain: a tail it would
    /// put back (sent to `now.key`) may not be sent again. Asked before the request is compiled,
    /// so the compile strips the thinking those tails were under.
    pub(super) fn fresh(&self, session: &str, now: &Under) -> bool {
        #[cfg(test)]
        {
            let hook = self.0.lock().unwrap_or_else(|e| e.into_inner()).hook.take();
            if let Some(f) = hook {
                f();
            }
        }
        let h = self.0.lock().unwrap_or_else(|e| e.into_inner());
        h.stale.contains(session)
            || h.by_session
                .get(session)
                .is_some_and(|v| v.iter().any(|s| s.key == now.key && !admissible(s, now)))
    }

    /// Run `f` once at the next `fresh`, after a request's snapshot is read.
    #[cfg(test)]
    pub(crate) fn hook_for_tests(&self, f: impl FnOnce() + Send + 'static) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).hook = Some(Box::new(f));
    }

    /// The tails a conversation's earlier requests sent, put back on a request made for it
    /// outside a turn (the keep-warm read), so that it is the prefix the next turn
    /// sends: byte for byte what `begin` puts back, for the key the newest tail was sent to.
    /// False, and nothing put back, when the newest tail may not be sent again under `now` (its
    /// key taken as the newest's): the read has no repeat, so it sends nothing, and it never
    /// falls back to an older tail. What may not be sent again is let go from the kept tails.
    pub(crate) fn warm(&self, session: &str, now: &Under, compiled: &mut Compiled) -> bool {
        let held = {
            let mut h = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if h.stale.contains(session) {
                return false;
            }
            let Some(v) = h.by_session.get_mut(session) else {
                return true;
            };
            let newest = v.last().map(|s| Under {
                key: s.key.clone(),
                ..now.clone()
            });
            let Some(newest) = newest else {
                return true;
            };
            let ok = v.last().is_some_and(|s| admissible(s, &newest));
            let was = v.len();
            v.retain(|s| {
                admissible(
                    s,
                    &Under {
                        key: s.key.clone(),
                        ..now.clone()
                    },
                )
            });
            // A tail let go: the chain it was on needs a new one first.
            if !ok || v.len() < was {
                h.stale.insert(session.to_string());
                return false;
            }
            v.clone()
        };
        let stripped = compiled.new_compilation && compiled.compilation.manifest.strip_thinking;
        let key = held.last().map(|s| s.key.clone());
        let now = Under {
            key: key.clone().unwrap_or_default(),
            ..now.clone()
        };
        put_back(compiled, held, (key.as_deref(), &now), stripped);
        true
    }

    /// Put back the tails the session's earlier requests sent, on the
    /// messages that carried them, before this request's own is attached:
    /// those sent to `now.key` and admissible under `now`, which this
    /// request's own tail keeps.
    pub(super) fn begin(
        &self,
        session: &str,
        turn: &str,
        now: Under,
        compiled: &mut Compiled,
    ) -> Option<Begun> {
        let last = compiled.request.messages.len().checked_sub(1)?;
        let stripped = compiled.new_compilation && compiled.compilation.manifest.strip_thinking;
        let held = {
            let mut h = self.0.lock().unwrap_or_else(|e| e.into_inner());
            h.recent.retain(|s| s != session);
            h.recent.push_back(session.to_string());
            let held = h.by_session.get(session).cloned().unwrap_or_default();
            // A stripped compile starts the new chain; one that would let a
            // tail go without it leaves the session needing one.
            let lets_go = held
                .iter()
                .any(|s| s.key == now.key && !admissible(s, &now));
            match stripped {
                true => h.stale.remove(session),
                false if lets_go => h.stale.insert(session.to_string()),
                false => false,
            };
            held
        };
        let (kept, here) = put_back(compiled, held, (Some(&now.key), &now), stripped);
        let m = &mut compiled.request.messages[last];
        let anchor = anchor(m);
        let blocks = blocks_of(m).map_or(0, |b| b.len());
        Some(Begun {
            turn: turn.to_string(),
            key: now.key.clone(),
            here,
            blocks,
            anchor,
            kept,
            under: now,
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
            under,
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
                    sealed: under,
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
