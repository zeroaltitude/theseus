//! The task graph in the model's view (39a, §2.4 "What the model sees"):
//! each turn whose scope holds tasks sees one line per open task (id, title,
//! state, owner, deps, a line of acceptance, version) and one per closed
//! subtree with its count, about `MAX_TOKENS` at most; past that, open tasks
//! only, with what was left out counted.
//!
//! It goes in the request's tail, after the cached prefix: `attach` adds it
//! as the last block of the request's last message, and marks the block
//! before it with the conversation's breakpoint, so the next loop's request,
//! which renders that message without the view, still begins with bytes the
//! provider has cached. `compile()` stays pure: the turn passes the view in
//! after it, and `context.compiled` carries its digest and counts. A scope
//! with no task adds nothing, so a plain turn's request and its token count
//! are unchanged.

use serde_json::{json, Value};
use theseus_protocol::tasks::TaskViewSummary;

use super::{depth, line, scope, TaskRecord};

/// The view's bound, in tokens (estimated at `BYTES_PER_TOKEN`).
pub const MAX_TOKENS: u64 = 1500;
/// The estimate's figure: conservative for English and ids.
pub const BYTES_PER_TOKEN: u64 = 3;

/// How the view's text begins: a reader of a request (a test's stand-in
/// model) can tell its block from the conversation's.
pub const HEAD: &str = "[The task graph in this conversation's scope:";

/// Whether a request's content block is the view.
pub fn is_view(block: &Value) -> bool {
    block["text"].as_str().is_some_and(|t| t.starts_with(HEAD))
}

/// The view's text and what it showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub text: String,
    pub summary: TaskViewSummary,
}

fn tokens(s: &str) -> u64 {
    (s.len() as u64).div_ceil(BYTES_PER_TOKEN)
}

/// The view of `tasks` (each as it reads now) for `session_id`'s turns;
/// None when its scope holds no task.
pub fn render(tasks: &[TaskRecord], session_id: &str) -> Option<View> {
    let scoped = scope(tasks, session_id);
    if scoped.is_empty() {
        return None;
    }
    let open = scoped.iter().filter(|t| !t.state.is_closed()).count() as u32;
    let closed = scoped.len() as u32 - open;
    // A closed task whose subtree is all closed is one line, with its count.
    let mut full: Vec<(bool, String)> = Vec::new();
    let mut folded: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for t in &scoped {
        if t.parent.as_deref().is_some_and(|p| folded.contains(p)) {
            folded.insert(&t.id);
            continue;
        }
        let pad = "  ".repeat(depth(&scoped, t));
        if t.state.is_closed() {
            let under = super::subtree(tasks, &t.id);
            if under.iter().all(|u| u.state.is_closed()) {
                folded.insert(&t.id);
                let n = under.len().saturating_sub(1);
                let more = match n {
                    0 => String::new(),
                    n => format!(", and {n} under it, all closed"),
                };
                full.push((
                    false,
                    format!(
                        "{pad}- {} \"{}\" [{}]{more}, v{}",
                        t.id,
                        t.title,
                        t.state.as_str(),
                        t.version
                    ),
                ));
                continue;
            }
        }
        full.push((!t.state.is_closed(), format!("{pad}- {}", line(t))));
    }
    let head = format!(
        "{HEAD} {open} open, {closed} closed. Edit it with \
         task.update, task.split, and task.close, naming the version you read; an objective or \
         acceptance change, or abandoning, waits for the operator.]"
    );
    let mut lines: Vec<&str> = full.iter().map(|(_, l)| l.as_str()).collect();
    let mut left_out = 0u32;
    let fits = |lines: &[&str]| {
        tokens(&head) + lines.iter().map(|l| tokens(l) + 1).sum::<u64>() <= MAX_TOKENS
    };
    if !fits(&lines) {
        // Past the bound: open tasks only, then as many as fit.
        lines = full
            .iter()
            .filter(|(o, _)| *o)
            .map(|(_, l)| l.as_str())
            .collect();
        left_out = (full.len() - lines.len()) as u32;
        while !fits(&lines) && !lines.is_empty() {
            lines.pop();
            left_out += 1;
        }
    }
    let mut text = head;
    for l in &lines {
        text.push('\n');
        text.push_str(l);
    }
    if left_out > 0 {
        text.push_str(&format!(
            "\n({left_out} more left out past the view's bound; task.list or `theseus tasks` shows them all.)"
        ));
    }
    use sha2::{Digest, Sha256};
    let digest = hex::encode(Sha256::digest(text.as_bytes()));
    let summary = TaskViewSummary {
        digest,
        open,
        closed,
        lines: lines.len() as u32,
        left_out,
        tokens: tokens(&text),
    };
    Some(View { text, summary })
}

/// Add `view` to the request: the last block of its last message, with the
/// conversation's breakpoint moved to the block before it.
pub fn attach_to(request: &mut crate::provider::ProviderRequest, view: &View) {
    let marker = request.cache_control.clone();
    let Some(last) = request.messages.last_mut() else {
        return;
    };
    let content = &mut last["content"];
    if let Value::String(s) = content {
        *content = json!([{"type": "text", "text": s.clone()}]);
    }
    let Some(blocks) = content.as_array_mut() else {
        return;
    };
    if let (Some(m), Some(Value::Object(b))) = (marker, blocks.last_mut()) {
        b.entry("cache_control").or_insert(m);
    }
    blocks.push(json!({"type": "text", "text": view.text}));
}

/// The turn's side (`compile_step`): the session's view, attached to the
/// compiled request, its tokens counted in the estimate. Returns what it
/// showed; None, and the request untouched, when the scope holds no task.
pub fn attach(
    store: &crate::store::Store,
    kernel: &theseus_kernel::Kernel,
    session_id: &str,
    compiled: &mut crate::compiler::Compiled,
) -> Option<TaskViewSummary> {
    let tasks = match super::all_shown(store, kernel) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => return None,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "the task graph was not read for the view");
            return None;
        }
    };
    let view = render(&tasks, session_id)?;
    attach_to(&mut compiled.request, &view);
    let t = view.summary.tokens;
    compiled.est_tokens += t;
    compiled.estimate.tokens += t;
    compiled.estimate.estimated += t;
    compiled.estimate.upper += t + t * crate::compiler::MARGIN_PERCENT / 100;
    Some(view.summary)
}
