//! A self task's own tree (theseus-pw1q.21.9). The trees live under the state
//! dir (`<state>/self/trees/<slug>`), which the approve list holds, so a
//! self session's file tools asked the owner for every read and write in the
//! tree it was building. The gate now lets a `"self"` principal work in two
//! places the approve list's hold on the state dir does not reach:
//!
//! - **its own tree**, for the file tools that write: the one directory its
//!   task's item names (`tree::slug` of the root task of the session);
//! - **the trees root and every tree under it** (theseus-qh2tt), for the
//!   read-only file and git tools (`fs.read`, `fs.glob`, `fs.grep`,
//!   `fs.list`, `git.log`, `git.diff`): a self session looks for its tree, or
//!   reads how another item went, without asking.
//!
//! Nothing else changes: the rest of the state dir, the store, the secrets
//! and the sealed sets keep their holds, a path that resolves out of the
//! tree (or out of the trees root) by a link or `..` is judged where it
//! lands, a write in another tree asks, and `proc.run` is untouched (its code
//! runs in L1 as `sandbox::self_build_view` says, never through this rule).
//! A session of any other principal gets neither.
//!
//! The tree is named by the task's id, which the backlog gives the model as
//! `item`, and never by the model's own input to the tool.

use std::path::PathBuf;

use theseus_tools::{paths, Tool, ToolClass};

use super::{budget, pr, tree, SELF_PRINCIPAL};
use crate::store::Store;
use crate::toolrun::ToolRuntime;

/// The trees root, canonical, and the core, for a `"self"` session: none when
/// the session's principal is not `"self"`, it has no execution, or the trees
/// are not a workspace root (`[self] mode` is not `act`, or no repo is set:
/// `pr::trees_root`).
fn self_session(
    rt: &ToolRuntime,
    session: &str,
) -> Option<(std::sync::Arc<crate::rpc::Core>, PathBuf)> {
    let core = rt.self_pr.core.get().and_then(std::sync::Weak::upgrade)?;
    let trees = paths::canonical_best_effort(&pr::trees_root(&core.cfg, &rt.self_pr.state)?);
    let rec: crate::session::SessionRecord = core.store.get_session(session).ok().flatten()?;
    let execution = rec.execution_id?;
    (crate::task_graph::principal_of(&core.kernel, &execution) == SELF_PRINCIPAL)
        .then_some((core, trees))
}

/// Whether `session` belongs to a task of the self principal. No trees
/// needed: the fail-open rule (theseus-cudar) holds in any `[self] mode`.
pub(crate) fn is_self(rt: &ToolRuntime, session: &str) -> bool {
    let Some(core) = rt.self_pr.core.get().and_then(std::sync::Weak::upgrade) else {
        return false;
    };
    let rec: Option<crate::session::SessionRecord> = core.store.get_session(session).ok().flatten();
    rec.and_then(|r| r.execution_id)
        .is_some_and(|e| crate::task_graph::principal_of(&core.kernel, &e) == SELF_PRINCIPAL)
}

/// The policy layer's decision for a call of `session`, with a self
/// session's soft approval hold made a notice that runs (theseus-cudar):
/// not the floor, not an L1 call, and only what `ToolPolicy::fails_open`
/// lets through.
pub(crate) fn failing_open(
    rt: &ToolRuntime,
    session: Option<&str>,
    tool: &dyn Tool,
    plan: &theseus_tools::Plan,
    l1: bool,
    decision: crate::policy::Decision,
) -> crate::policy::Decision {
    match session {
        Some(s)
            if decision.posture == crate::policy::Posture::Approve
                && !decision.floor
                && !l1
                && is_self(rt, s)
                && rt.policy.fails_open(plan, rt.self_pr.state_dir()) =>
        {
            fail_open(decision, tool.name())
        }
        _ => decision,
    }
}

/// A self session's call that waited for approval, now a notice that runs
/// (theseus-cudar): the posture's own reason is kept, so the notice says
/// what would have asked. The floor and the owner's other holds never come
/// here (`ToolPolicy::fails_open`).
pub(crate) fn fail_open(d: crate::policy::Decision, tool: &str) -> crate::policy::Decision {
    let rule = format!("{tool} — notify (self-repair fails open: {})", d.reason);
    crate::policy::Decision {
        notify: Some(crate::policy::Notice {
            kind: "notify".into(),
            setting: "self-improvement fails open".into(),
            rule: rule.clone(),
        }),
        ..crate::policy::Decision::new(crate::policy::Posture::Notify, rule)
    }
}

/// The slug of the self item `session` works on, which names its reports and
/// its tree, or none for a session that is no self task's.
pub(crate) fn item_slug(store: &Store, session: &str) -> Option<String> {
    let rec = crate::task_graph::all(store)
        .ok()?
        .into_iter()
        .find(|t| t.session.as_deref() == Some(session) && t.origin.principal == SELF_PRINCIPAL)?;
    Some(tree::slug(&budget::root_in(store, rec)))
}

/// The tree directory the self task that owns `session` works in, or none:
/// the session is no self session, or it has no task of the self principal.
pub(crate) fn of(rt: &ToolRuntime, session: &str) -> Option<PathBuf> {
    let (core, trees) = self_session(rt, session)?;
    let root = root_of(rt, &core.store, session)?;
    Some(trees.join(tree::slug(&root)))
}

/// What the approve list's hold on the state dir leaves open to `tool` in
/// `session` (`ToolPolicy::decide_in`'s `own`): the trees root for a
/// read-only file or git tool of a self session, its own tree for a file
/// tool that writes, else nothing.
pub(crate) fn open_to(rt: &ToolRuntime, session: &str, tool: &dyn Tool) -> Option<PathBuf> {
    let name = tool.name();
    match tool.class() {
        ToolClass::Read if name.starts_with("fs.") || name.starts_with("git.") => {
            self_session(rt, session).map(|(_, trees)| trees)
        }
        _ if name.starts_with("fs.") => of(rt, session),
        _ => None,
    }
}

/// The root task of the session's self task, found once and kept.
fn root_of(rt: &ToolRuntime, store: &Store, session: &str) -> Option<String> {
    let kept = rt
        .self_pr
        .roots
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(r) = kept.get(session) {
        return Some(r.clone());
    }
    drop(kept);
    let rec = crate::task_graph::all(store)
        .ok()?
        .into_iter()
        .find(|t| t.session.as_deref() == Some(session) && t.origin.principal == SELF_PRINCIPAL)?;
    let root = budget::root_in(store, rec);
    rt.self_pr
        .roots
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(session.to_string(), root.clone());
    Some(root)
}
