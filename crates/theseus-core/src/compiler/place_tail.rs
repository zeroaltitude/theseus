//! What was just said in the place (issue 43): a turn that runs in another
//! session than the one that held the place's last exchange is given that
//! exchange, and a short tail before it, as one marked block.
//!
//! - **From** the session that held the last exchange (`rpc/thread_flow.rs`
//!   chooses it): the place's own conversation, or an unsealed thread's; a
//!   sealed or seal-pending thread gives nothing, and a window holding a
//!   split's record gives nothing (its ranges are another thread's now).
//!   Threads live in private places alone, so a shared place has none.
//! - **What**: the person's messages and the replies' text, no tool calls,
//!   results or notices, from one `[threads] sticky_half_life_secs` before
//!   that exchange began; the newest first within [`TOKENS`], a message cut
//!   to keep its end; shown oldest first.
//! - **Per turn, in the tail**, as the place digest is (`turn/digest_step.rs`):
//!   after the request's last cache mark, every loop of the turn and no
//!   other. Nothing of it is written to the session that runs the turn.

use anyhow::{bail, Result};
use theseus_store::pages::ledger_session;
use theseus_store::{kinds, Page, Store as _};

use crate::node::{Body, Node, Origin};
use crate::task_graph::view::BYTES_PER_TOKEN;

/// The block's budget, in tokens: a recall pack's default (`[memory]
/// recall_budget_tokens`), as a budget of its own.
pub const TOKENS: u64 = 3_000;
/// Nodes read a page at a time, back from the newest, and at most.
const PAGE: usize = 64;
const MOST: usize = 256;

/// A block, rendered, and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tail {
    pub text: String,
    pub messages: usize,
    pub tokens: u64,
}

/// The messages of `sid` since `since_ms`, newest first: (the person's, text).
/// None when a split's record lies in the window.
fn read(
    store: &crate::store::Store,
    sid: &str,
    since_ms: u64,
) -> Result<Option<Vec<(bool, String)>>> {
    let (mut out, mut before, mut read) = (Vec::new(), None, 0);
    loop {
        let page = Page {
            kind: kinds::NODE,
            tags: vec![ledger_session(sid)],
            since_ms: Some(since_ms),
            before,
            limit: PAGE,
            ..Page::default()
        };
        let Some(got) = store.inner().page(&page)? else {
            bail!("the index is still being built");
        };
        for r in got.records.iter().rev() {
            let n: Node = r.decode()?;
            match (&n.body, n.origin) {
                (Body::Moved { .. }, _) => return Ok(None),
                (Body::UserMessage { text, .. }, Origin::Operator) => {
                    out.push((true, text.clone()))
                }
                (Body::AssistantMessage { blocks, .. }, _) => {
                    out.push((false, crate::provider::text_of(blocks)));
                }
                _ => {}
            }
        }
        read += got.records.len();
        match (got.more, got.first) {
            (true, Some(first)) if read < MOST => before = Some(first),
            _ => return Ok(Some(out)),
        }
    }
}

/// The block of `sid` (named `label`: the place's own conversation, or a
/// thread) since `since_ms`; none when it said nothing since.
pub fn block(
    store: &crate::store::Store,
    sid: &str,
    label: &str,
    since_ms: u64,
) -> Result<Option<Tail>> {
    let Some(said) = read(store, sid, since_ms)? else {
        return Ok(None);
    };
    let room = (TOKENS * BYTES_PER_TOKEN) as usize;
    let (mut lines, mut used) = (Vec::new(), 0);
    for (person, text) in said.iter().filter(|(_, t)| !t.trim().is_empty()) {
        let who = if *person { "the person" } else { "you" };
        let text = text.trim();
        if used + text.len() > room {
            if lines.is_empty() {
                let mut at = text.len() - room;
                while !text.is_char_boundary(at) {
                    at += 1;
                }
                lines.push(format!("{who}: \u{2026}{}", &text[at..]));
            }
            break;
        }
        used += text.len();
        lines.push(format!("{who}: {text}"));
    }
    if lines.is_empty() {
        return Ok(None);
    }
    lines.reverse();
    let text = format!(
        "[What was just said in this place, in {label} (session {sid}), oldest first: what the \
         person's message above may follow. The harness supplies it; do not restate it. If it is not \
         enough, memory_lookup reads that session further back.\n{}]",
        lines.join("\n")
    );
    let tokens = (text.len() as u64).div_ceil(BYTES_PER_TOKEN);
    Ok(Some(Tail {
        messages: lines.len(),
        tokens,
        text,
    }))
}
