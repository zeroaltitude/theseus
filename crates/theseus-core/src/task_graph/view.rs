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
//!
//! A check's view (theseus-w8ys) shows the checked task, every task under
//! it, and any record of a session its basis excludes by id, title, and
//! state alone (`restricted`): no owner, deps, acceptance, or version, so
//! the check reads what the task set out to do only through its basis's
//! objective and claim. Its other lines, and every other session's view,
//! are as they were.

use serde_json::{json, Value};
use theseus_protocol::tasks::TaskViewSummary;

use super::{depth, line, scope, TaskRecord, OWNERS_MARK};

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
    render_for(tasks, session_id, None)
}

/// The tasks the check `session_id` sees by id, title, and state alone: the
/// checked task and every task under it, and each record whose session its
/// basis excludes; never the check's own task or a task under it.
pub fn restricted_ids(
    tasks: &[TaskRecord],
    check: &theseus_protocol::TaskCheck,
    session_id: &str,
) -> std::collections::HashSet<String> {
    let checked = super::of_session(&check.checked_task);
    let mut ids: std::collections::HashSet<String> = super::subtree(tasks, &checked)
        .into_iter()
        .map(|t| t.id.clone())
        .collect();
    ids.extend(
        tasks
            .iter()
            .filter(|t| {
                t.session
                    .as_ref()
                    .is_some_and(|s| check.excluded_sessions.contains(s))
            })
            .map(|t| t.id.clone()),
    );
    for own in super::subtree(tasks, &super::of_session(session_id)) {
        ids.remove(&own.id);
    }
    ids
}

/// A restricted line: id, title, and state.
fn bare(t: &TaskRecord) -> String {
    format!("{} \"{}\" [{}]", t.id, t.title, t.state.as_str())
}

/// `render`, for a check's session when `check` is its basis (theseus-w8ys).
pub fn render_for(
    tasks: &[TaskRecord],
    session_id: &str,
    check: Option<&theseus_protocol::TaskCheck>,
) -> Option<View> {
    let hidden = check
        .map(|c| restricted_ids(tasks, c, session_id))
        .unwrap_or_default();
    let scoped = scope(tasks, session_id);
    if scoped.is_empty() {
        return None;
    }
    let open = scoped.iter().filter(|t| !t.state.is_closed()).count() as u32;
    let closed = scoped.len() as u32 - open;
    // A closed task whose subtree is all closed is one line, with its count.
    // Each line: whether it is open, whether it is restricted, its text.
    let mut full: Vec<(bool, bool, String)> = Vec::new();
    let mut folded: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for t in &scoped {
        if t.parent.as_deref().is_some_and(|p| folded.contains(p)) {
            folded.insert(&t.id);
            continue;
        }
        let pad = "  ".repeat(depth(&scoped, t));
        if hidden.contains(&t.id) {
            if t.state.is_closed()
                && super::subtree(tasks, &t.id)
                    .iter()
                    .all(|u| u.state.is_closed())
            {
                folded.insert(&t.id);
            }
            full.push((!t.state.is_closed(), true, format!("{pad}- {}", bare(t))));
            continue;
        }
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
        full.push((!t.state.is_closed(), false, format!("{pad}- {}", line(t))));
    }
    let head = format!(
        "{HEAD} {open} open, {closed} closed. Edit it with \
         task.update, task.split, and task.close, naming the version you read. On a task marked \
         \"{OWNERS_MARK}\", an objective or acceptance change, or abandoning, waits for the \
         operator; on any other, it applies at once.]"
    );
    let mut lines: Vec<&(bool, bool, String)> = full.iter().collect();
    let mut left_out = 0u32;
    let fits = |lines: &[&(bool, bool, String)]| {
        tokens(&head) + lines.iter().map(|l| tokens(&l.2) + 1).sum::<u64>() <= MAX_TOKENS
    };
    if !fits(&lines) {
        // Past the bound: open tasks only, then as many as fit.
        lines = full.iter().filter(|(o, _, _)| *o).collect();
        left_out = (full.len() - lines.len()) as u32;
        while !fits(&lines) && !lines.is_empty() {
            lines.pop();
            left_out += 1;
        }
    }
    let mut text = head;
    for l in &lines {
        text.push('\n');
        text.push_str(&l.2);
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
        restricted: lines.iter().filter(|l| l.1).count() as u32,
    };
    Some(View { text, summary })
}

/// Add `view` to the request: the last block of its last message, with the
/// conversation's breakpoint moved to the block before it: moved, not
/// copied, since the top-level one takes a slot of the provider's four too,
/// and the session's block took the last free one (theseus-aab7).
pub fn attach_to(request: &mut crate::provider::ProviderRequest, view: &View) {
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
    if let Some(Value::Object(b)) = blocks.last_mut() {
        if let Some(m) = request.cache_control.take() {
            b.entry("cache_control").or_insert(m);
        }
    }
    blocks.push(json!({"type": "text", "text": view.text}));
}

/// The turn's side (`compile_step`): the session's view, attached to the
/// compiled request, its tokens counted in the estimate; a check's is
/// restricted by its basis (theseus-w8ys). Returns what it
/// showed; None, and the request untouched, when the scope holds no task.
pub fn attach(
    store: &crate::store::Store,
    kernel: &theseus_kernel::Kernel,
    session: &crate::session::SessionRecord,
    compiled: &mut crate::compiler::Compiled,
) -> Option<TaskViewSummary> {
    let session_id = session.session_id.as_str();
    let check = session.task.as_ref().and_then(|t| t.check.as_ref());
    let tasks = match super::all_shown(store, kernel) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => return None,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "the task graph was not read for the view");
            return None;
        }
    };
    let view = render_for(&tasks, session_id, check)?;
    attach_to(&mut compiled.request, &view);
    let t = view.summary.tokens;
    compiled.est_tokens += t;
    compiled.estimate.tokens += t;
    compiled.estimate.estimated += t;
    compiled.estimate.upper += t + t * crate::compiler::MARGIN_PERCENT / 100;
    Some(view.summary)
}
