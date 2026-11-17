//! A self session removes what is its own (issue 20; the owner, 2026-11-15 17:39:14, chat message
//! 1571686049867112519: "Removals should be possible ... I will tend to favor smart relaxing of the
//! standards here to allow for maximum self improvement power").
//!
//! The reversibility layer (`toolrun/reversible.rs`, `undo.v1`) holds a call that would lose
//! something nothing holds a copy of, and its one-way floor (`judge/undo/floor.rs`) forces that hold
//! on a plain file write. Both were built for a person's files. For the loop's own scratch they
//! protect nothing: on 2026-11-14 a reviewer was not allowed to remove the checkout it had made, and
//! that checkout held the pipeline's only slot for thirteen hours.
//!
//! So for a session of the self principal this module LOWERS A HOLD, and only a hold, when every
//! place the call can remove from is the session's own. It never lowers `irreversible_final`, which
//! reaches the owner, and it runs after `refusals` (the sealed set, the place rule, plan mode, the
//! injection freeze), so none of those moved. What it covers, and why each is certain:
//!
//! - **A program call in L1 with no host beyond the self builder's network** (`rsi::deps::hosts`:
//!   the package registries; a self session's sandboxed command does NOT carry the operator's
//!   `[sandbox] egress`, the gate gives it that list instead, so on any daemon its call reaches the
//!   registries and nothing of the operator's).
//!   Writes persist only to what the view binds read-write. The view a `proc.run` is given binds
//!   none ([`sandbox_keeps_nothing`] reads the base view, so a bind added to it turns this off), and
//!   a self builder's command is then given one more, its own item's build folder and no one else's
//!   (`sandbox::Sandbox::with_builder_folder`, `own_tree::builder_folder`, which answers only for a
//!   self session, and only for a folder that is its own resolved path): the only thing that
//!   persists from such a call, and its own by construction.
//!   This covers a shell line, which the rest of the rule cannot read, because nothing it does
//!   outside that folder stays. Whatever else the call removes is the overlay it was given.
//! - **A plain file tool (`fs.write`, `fs.edit`, `fs.patch`) whose every write target is its own**,
//!   which the tool's plan names exactly.
//! - **The system's `rm` with an `argv` and no `env`, whose flags are from a closed set and whose
//!   operands are all its own.** An `argv` has no shell, so an operand is the literal path. A shell
//!   line, a batch of steps and every other program (`git worktree remove` among them) are held as
//!   today: the code cannot say what they remove.
//!
//! "Its own" is [`Own`]: strictly inside its working copy or its item's build folder, or its review
//! scratch (registered by the review that made it, never named by a call). Nothing under the
//! system temporary directory is its own: that directory is shared with everything on the machine.
//! The temporary directory of a review's commands is `<its scratch>/tmp` ([`review_tmp`]), and a
//! builder's is in its build folder. A path is resolved before it is judged, and the file
//! tools act on the path as resolved, so for them a link or a `..` that leaves is judged where it
//! lands. **`rm` is judged as the kernel acts**, which is not the same: the kernel follows a link
//! and then applies a `..` after it, where [`paths::canonical_best_effort`] drops the `..` first. So
//! an operand with a `..` in it, or ending in a `/` (which `rm -r` follows through a link), is not
//! read at all ([`Own::named`]), and an operand's last part is
//! not followed, because `rm` removes a link and not what it points at.
//! `docs/design/self-own-removals.md` has the rest.

use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::sync::PoisonError;

use serde_json::Value;
use theseus_tools::{paths, Access, Backend, Plan, Tool};

use super::own_tree;
use super::pr::Board;
use crate::judge::undo::{floor, Says, UndoVerdict};
use crate::toolrun::ToolRuntime;

/// The session's review scratch, resolved, or none. It is registered by the code that made it
/// (`CheckTurn`), and is checked to be a directory strictly under the reviews' own, so a registry
/// entry can never name anything else.
fn review_scratch(rt: &ToolRuntime, session: &str) -> Option<PathBuf> {
    // The registry first: almost every session has none, and the state dir is not resolved for it.
    let registered = rt.self_pr.review_scratch(session)?;
    let state = paths::canonical_best_effort(&rt.self_pr.state);
    let review = state.join("self").join("review");
    let s = paths::canonical_best_effort(&registered);
    (s != review && s.starts_with(&review)).then_some(s)
}

/// The temporary directory of a review's own commands: `<its scratch>/tmp`, made if it was not
/// there, or none when the session has no review scratch. A review's commands run outside the
/// sandbox, where the system's temporary directory is shared with everything else on the machine
/// and is nobody's own; this is the one place under the scratch they are pointed at (`TMPDIR`), so
/// what they leave there goes with the scratch when the review has answered.
pub(crate) fn review_tmp(rt: &ToolRuntime, session: &str) -> Option<PathBuf> {
    let tmp = review_scratch(rt, session)?.join("tmp");
    std::fs::create_dir_all(&tmp).ok()?;
    Some(tmp)
}

/// What a self session is told about a call of its own that was held, so it does not stop at "move
/// it aside" and leave what it made behind: what does run for it. None for any other session.
pub(crate) fn what_runs(rt: &ToolRuntime, session: &str) -> Option<&'static str> {
    own_tree::is_self(rt, session).then_some(
        "For this session these run: `rm` given as an argv (`{\"argv\": [\"rm\", \"-rf\", \"<path>\"]}`, \
         not a shell line) on paths inside its own working copy, build folder or review scratch; the \
         file tools on those paths; and any command run in the sandbox (`sandbox: true`), which reaches \
         at most the package registries and keeps nothing outside its own build folder. A review's \
         scratch is removed for it when it has answered, so it needs no clean-up.",
    )
}

/// The flags `rm` may carry for [`rm_operands`] to read its operands: recurse, force, verbose, and
/// empty directories, each in one letter or its long form. Anything else (`-i`, `--one-file-system`,
/// `--no-preserve-root`, an unknown flag) is a call this module does not read.
const RM_LONG: [&str; 4] = ["--recursive", "--force", "--verbose", "--dir"];
const RM_SHORT: &str = "rRfvd";
/// Where the system's `rm` is: the two names a program may be given by, and what a bare `rm` must
/// resolve to.
const SYSTEM_RM: [&str; 2] = ["/bin/rm", "/usr/bin/rm"];

/// A self session's own places, each resolved.
struct Own {
    /// Its working copy: what is strictly inside it. Not the copy itself: the pipeline holds it.
    tree: Option<PathBuf>,
    /// Its review scratch, the directory and all inside it.
    scratch: Option<PathBuf>,
    /// Its item's build folder (`own_tree::builder_path`): what is strictly inside it. Not the
    /// folder itself, which the pipeline removes when the item ends.
    builder: Option<PathBuf>,
}

impl Own {
    /// The places of `session`, or none when it is not a session of the self principal.
    fn of(rt: &ToolRuntime, session: &str) -> Option<Self> {
        if !own_tree::is_self(rt, session) {
            return None;
        }
        Some(Self {
            tree: own_tree::of(rt, session),
            scratch: review_scratch(rt, session),
            builder: own_tree::builder_path(rt, session).map(|b| paths::canonical_best_effort(&b)),
        })
    }

    /// Whether everything the path names, once resolved, is the session's own: for the file tools,
    /// which act on the path as [`paths::canonical_best_effort`] resolves it.
    fn holds(&self, p: &Path) -> bool {
        self.holds_resolved(paths::canonical_best_effort(p))
    }

    /// Whether what `rm` removes for this operand is the session's own: the operand with every part
    /// but the last resolved, and the last kept as it is, since `rm` removes a link and not its
    /// target. An operand with a `..` is not read: a link before it would be followed first. Nor is
    /// one that ends in a `/`: `rm -r` follows a link written that way and empties what it points
    /// at, where `Path` drops the slash and would judge the link itself.
    fn named(&self, operand: &Path) -> bool {
        if operand.as_os_str().as_bytes().ends_with(b"/")
            || operand
                .components()
                .any(|c| matches!(c, Component::ParentDir))
        {
            return false;
        }
        match (operand.parent(), operand.file_name()) {
            (Some(parent), Some(name)) => {
                self.holds_resolved(paths::canonical_best_effort(parent).join(name))
            }
            _ => false,
        }
    }

    fn holds_resolved(&self, p: PathBuf) -> bool {
        let strictly = |root: &Path| p != root && p.starts_with(root);
        if [&self.tree, &self.builder]
            .into_iter()
            .any(|own| own.as_deref().is_some_and(strictly))
        {
            return true;
        }
        self.scratch
            .as_deref()
            .is_some_and(|s| paths::within(&p, s))
    }
}

impl Board {
    /// A review's scratch, for the session that runs it: what that session may remove
    /// (`rsi::join::review::CheckTurn`, which got it from `scratch::open`).
    pub(crate) fn review_opened(&self, session: &str, scratch: &Path) {
        if scratch.as_os_str().is_empty() {
            return;
        }
        self.reviews
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session.to_string(), scratch.to_path_buf());
    }

    /// The review has answered: its session has no scratch of its own any more.
    pub(crate) fn review_closed(&self, session: &str) {
        self.reviews
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(session);
    }

    pub(crate) fn review_scratch(&self, session: &str) -> Option<PathBuf> {
        let reviews = self.reviews.lock().unwrap_or_else(PoisonError::into_inner);
        reviews.get(session).cloned()
    }
}

/// The verdict the gate acts on: a hold on a call of a self session whose every place is its own
/// becomes `reversible`; every other verdict, and every other session's, is returned as it was.
pub(crate) fn lowered(
    rt: &ToolRuntime,
    session: &str,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
    v: UndoVerdict,
) -> UndoVerdict {
    if !v.holds() {
        return v;
    }
    let Some(own) = Own::of(rt, session) else {
        return v;
    };
    match why(rt, session, tool, plan, input, &own) {
        Some(why) => {
            tracing::info!(
                session,
                tool = tool.name(),
                why,
                "a self session's own removal is not held"
            );
            UndoVerdict {
                says: Says::Reversible,
                ..v
            }
        }
        None => v,
    }
}

/// Why the call touches only what is the session's own, or none: it is held as before.
fn why(
    rt: &ToolRuntime,
    session: &str,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
    own: &Own,
) -> Option<&'static str> {
    if sandbox_keeps_nothing(rt, session, tool, plan, input) {
        return Some("a sandboxed call keeps nothing outside its own view");
    }
    if plain_file_call(tool, plan) {
        let mut writes = plan.resources.iter().filter(|r| r.access == Access::Write);
        let first = writes.next()?;
        let own_all = own.holds(&first.path) && writes.all(|r| own.holds(&r.path));
        return own_all.then_some("every path the file tool writes is its own");
    }
    let cwd = plan
        .resources
        .iter()
        .find(|r| r.access == Access::Exec)
        .map(|r| r.path.as_path())?;
    let operands = rm_operands(|| plain_rm_is_the_systems(rt), tool, plan, input)?;
    let own_all = operands.iter().all(|o| own.named(&cwd.join(o)));
    own_all.then_some("every path rm names is its own")
}

/// Whether a program call runs in L1 with no host beyond the self builder's network
/// (`rsi::deps::hosts`: the package registries, or none), over a view whose one persisting place is the
/// session's own: what such a call removes is its overlay, or its own build folder. The L1 test is
/// `undo_check`'s (`Sandbox::l1_for`), and the list is the one the gate gave the call
/// (`Sandbox::egress_for`), so a call that names a host beyond it is not read.
fn sandbox_keeps_nothing(
    rt: &ToolRuntime,
    session: &str,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
) -> bool {
    // The list the gate gives this call, which must be the self builder's own and nothing wider: the
    // rule does not trust that the gate stripped the operator's list, it reads the result.
    // Both written as the proxy shows an entry, so the comparison is of hosts and not of spelling.
    let (theirs, _) = crate::egress::list(&crate::rsi::deps::hosts::egress(), &[]);
    let (given, beyond) = crate::egress::list(
        &rt.sandbox.egress_for(rt, Some(session)),
        &crate::egress::asked(input),
    );
    tool.backend() == Backend::Job
        && own_tree::l1_of(rt, Some(session), tool, plan, input).is_some()
        && beyond.is_empty()
        && given.iter().all(|h| theirs.contains(h))
        && rt.sandbox.job_view().rw.is_empty()
}

/// A call of one of the floor's table tools that carries nothing but its file targets: the same
/// two tests `judge::undo::floor` makes before it will say a call's whole effect is its paths.
fn plain_file_call(tool: &dyn Tool, plan: &Plan) -> bool {
    floor::whole_effect_of(tool.name()).is_some()
        && tool.backend() != Backend::Job
        && plan.argv.is_none()
        && plan.steps.is_none()
        && plan.url.is_none()
}

/// The operands of a `proc.run` of `rm` that this module reads, or none: the program is `rm` by its
/// plain or its usual absolute name, the call sets no `env` (a `PATH` of its own would make `rm`
/// something else), the flags are all in [`RM_SHORT`] and [`RM_LONG`], and there is an operand. A
/// shell line is `bash -c`, so it is never read here.
fn rm_operands<'a>(
    bare_is_the_systems: impl FnOnce() -> bool,
    tool: &dyn Tool,
    plan: &'a Plan,
    input: &Value,
) -> Option<Vec<&'a str>> {
    if tool.name() != "proc.run"
        || input
            .get("env")
            .is_some_and(|e| e.as_object().is_none_or(|m| !m.is_empty()))
    {
        return None;
    }
    let (program, args) = plan.argv.as_deref()?.split_first()?;
    let system = SYSTEM_RM.iter().any(|p| Path::new(p) == Path::new(program));
    if !system && !(program == "rm" && bare_is_the_systems()) {
        return None;
    }
    let (mut flags, mut operands) = (true, Vec::new());
    for a in args {
        if flags && a == "--" {
            flags = false;
        } else if flags && a.starts_with("--") {
            RM_LONG.contains(&a.as_str()).then_some(())?;
        } else if flags && a.starts_with('-') && a.len() > 1 {
            a[1..].chars().all(|c| RM_SHORT.contains(c)).then_some(())?;
        } else {
            operands.push(a.as_str());
        }
    }
    (!operands.is_empty() && operands.iter().all(|o| !o.is_empty())).then_some(operands)
}

/// Whether a bare `rm` is the system's: the first `rm` on the `PATH` a job is given
/// (`ToolRuntime::job_base_env`) resolves to one of [`SYSTEM_RM`]. The `PATH` starts in the owner's
/// own directories on some hosts, so a program put there would otherwise be run unheld by every
/// later call. An empty or relative entry is read against the job's directory and not this one, so
/// it fails closed.
fn plain_rm_is_the_systems(rt: &ToolRuntime) -> bool {
    let env = rt.job_base_env();
    let Some((_, path)) = env.iter().find(|(k, _)| k == "PATH") else {
        return false;
    };
    let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
    if dirs.iter().any(|d| !d.is_absolute()) {
        return false;
    }
    dirs.iter()
        .map(|d| d.join("rm"))
        .find(|c| c.is_file())
        .and_then(|c| c.canonicalize().ok())
        .is_some_and(|c| SYSTEM_RM.iter().any(|p| c == Path::new(p)))
}

#[cfg(test)]
mod tests;
