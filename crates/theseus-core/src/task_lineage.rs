//! A task's place in its tree (theseus-tecj5): how deep it sits, what the
//! whole tree is for, and the check that a child does not restate the work of
//! one of its ancestors.
//!
//! - **The bound.** `[kernel] task_depth` (10) levels of tasks may sit below a
//!   conversation. The kernel enforces it in `open_task` by walking the
//!   `parent` links (`theseus_kernel::task_depth`); the core checks the same
//!   rule first so a refusal reads in words, and its tools (`task.create`,
//!   `task.fan_out`, `task.wait`) share [`check_depth`].
//! - **The block.** A child below the first level has its ancestry in its brief:
//!   the depth, the root goal (the brief of the tree's first task, cut at
//!   [`ROOT_CHARS`]) and the chain of tasks from the root, each a short id and a
//!   title cut at [`TITLE_CHARS`]. About 1,500 characters at depth ten.
//! - **The cycle.** A proposed brief equal, after trimming, to the brief of any
//!   ancestor task is refused ([`Lineage::repeat_of`]): ten levels of different
//!   work is fine, the same work restated is a loop spending money. The
//!   comparison is exact: a cleverer one would refuse real work. A brief that
//!   really is the same short instruction at two levels ("run the tests") is
//!   the false positive accepted; the refusal says how to differ.

use theseus_kernel::{Execution, Kernel, SessionKind};

use crate::store::Store;
use crate::task::short;

/// The root goal's length in a descendant's brief, in characters.
pub const ROOT_CHARS: usize = 600;
/// A title's length in the chain, in characters.
pub const TITLE_CHARS: usize = 60;

/// What a refusal says when a session sits at the limit; `reached` is the
/// depth of the session that asked.
pub fn depth_refusal(reached: usize, limit: u32) -> String {
    format!(
        "Refused: this session sits at task depth {reached} and tasks may go {limit} deep \
         (`[kernel] task_depth`), so it cannot start another. Do the work here, or finish and \
         say in your report what should be started next; the conversation that started you \
         can start it."
    )
}

/// What the owner's `task.start` and the kernel's own error say.
pub fn depth_words(reached: usize, limit: u32) -> String {
    format!(
        "Not started: a task at depth {reached} cannot start another; tasks may go {limit} deep."
    )
}

/// What a refusal says when a brief repeats an ancestor's.
pub fn cycle_refusal(ancestor: &Ancestor, depth: usize) -> String {
    format!(
        "Refused as a cycle: this brief is the same as the one of task {} (level {depth} from the root, \"{}\"), which is above you. A task tree does not restate work it is \
         already doing. Do the work here, or write a brief that says what is different.",
        ancestor.short, ancestor.title
    )
}

/// Cut `s` to `max` characters, with an ellipsis when it was longer.
fn cut(s: &str, max: usize) -> String {
    let s = s.trim();
    match s.char_indices().nth(max) {
        None => s.to_string(),
        Some((at, _)) => format!("{}…", &s[..at]),
    }
}

/// The brief inside a task's first node: what follows the header paragraph
/// (`task::brief_text` writes one paragraph, then the brief).
pub fn brief_of(first_node_text: &str) -> &str {
    match first_node_text.split_once("\n\n") {
        Some((_, brief)) => brief,
        None => first_node_text,
    }
}

/// One task above the one being started.
#[derive(Debug, Clone)]
pub struct Ancestor {
    /// Its short id, as people name it.
    pub short: String,
    pub title: String,
    /// Its brief as it was given (trimmed).
    pub brief: String,
}

/// Where a new task would sit.
#[derive(Debug, Clone)]
pub struct Lineage {
    /// The depth the new task would have.
    pub depth: usize,
    pub limit: u32,
    /// The task ancestors, outermost first. Empty below a conversation.
    pub above: Vec<Ancestor>,
}

/// The refusal for `execution_id` starting a task, in words, or its depth.
pub fn check_depth(kernel: &Kernel, execution_id: &str) -> Result<usize, String> {
    let reached = kernel
        .task_depth(execution_id)
        .map_err(|e| format!("Refused: {e:#}"))?;
    let limit = kernel.task_depth_limit();
    if reached >= limit as usize {
        return Err(depth_refusal(reached, limit));
    }
    Ok(reached)
}

impl Lineage {
    /// The lineage a task started by `parent` would have, or the depth refusal.
    pub fn of(kernel: &Kernel, store: &Store, parent: &Execution) -> Result<Self, String> {
        let reached = check_depth(kernel, &parent.id)?;
        let ancestry = kernel
            .ancestry(&parent.id)
            .map_err(|e| format!("Not started: {e:#}"))?;
        let mut above = Vec::new();
        for e in ancestry.chain.iter().rev() {
            if e.kind != SessionKind::Task {
                continue;
            }
            above.push(ancestor(store, e)?);
        }
        Ok(Self {
            depth: reached + 1,
            limit: kernel.task_depth_limit(),
            above,
        })
    }

    /// The ancestor whose brief `brief` repeats, with its depth.
    pub fn repeat_of(&self, brief: &str) -> Option<(&Ancestor, usize)> {
        let brief = brief.trim();
        self.above
            .iter()
            .enumerate()
            .find(|(_, a)| !a.brief.is_empty() && a.brief == brief)
            .map(|(i, a)| (a, i + 1))
    }

    /// The sentences a child's header carries about its place: the depth,
    /// and below the first level the root goal and the chain.
    pub fn note(&self, own_short: &str) -> String {
        let room = if self.depth < self.limit as usize {
            format!(
                "You may start tasks of your own, down to depth {}.",
                self.limit
            )
        } else {
            "You cannot start tasks of your own: this is the deepest level.".to_string()
        };
        let Some(root) = self.above.first() else {
            return format!(
                "This task is at depth {} of {}. {room}",
                self.depth, self.limit
            );
        };
        let chain: Vec<String> = self
            .above
            .iter()
            .map(|a| format!("{} \"{}\"", a.short, cut(&a.title, TITLE_CHARS)))
            .collect();
        format!(
            "This task is at depth {} of {}. Root goal, the brief of this tree's first task: \
             \"{}\". Chain from the root: {} > {own_short} (you). {room}",
            self.depth,
            self.limit,
            cut(&root.brief, ROOT_CHARS),
            chain.join(" > "),
        )
    }
}

fn ancestor(store: &Store, e: &Execution) -> Result<Ancestor, String> {
    let title = store
        .get_session::<crate::session::SessionRecord>(&e.session_id)
        .map_err(|e| format!("Not started: {e:#}"))?
        .and_then(|r| r.title)
        .unwrap_or_default();
    let brief = match store
        .first_node(&e.session_id)
        .map_err(|e| format!("Not started: {e:#}"))?
    {
        Some((_, n)) => match n.body {
            crate::node::Body::UserMessage { text, .. } => brief_of(&text).trim().to_string(),
            _ => String::new(),
        },
        None => String::new(),
    };
    Ok(Ancestor {
        short: short(&e.session_id),
        title,
        brief,
    })
}
