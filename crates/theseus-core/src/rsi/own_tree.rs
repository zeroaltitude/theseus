//! A self task's own tree (theseus-pw1q.21.9). The trees live under the state
//! dir (`<state>/self/trees/<slug>`), which the approve list holds, so a
//! self session's file tools asked the owner for every read and write in the
//! tree it was building. The gate now lets a `"self"` principal's file tools
//! work in ITS OWN tree: the one directory its task's item names
//! (`tree::slug` of the root task of the session). Nothing else changes: the
//! rest of the state dir, other trees, the store, the secrets and the sealed
//! sets keep their holds, a path that resolves out of the tree by a link or
//! `..` is judged where it lands, and `proc.run` is untouched (its code runs
//! in L1 as `sandbox::self_build_view` says, never through this rule).
//!
//! The tree is named by the task's id, which the backlog gives the model as
//! `item`, and never by the model's own input to the tool.

use std::path::PathBuf;

use theseus_tools::paths;

use super::{budget, pr, tree, SELF_PRINCIPAL};
use crate::store::Store;
use crate::toolrun::ToolRuntime;

/// The tree directory the self task that owns `session` works in, or none:
/// the session's principal is not `"self"`, it has no task of the self
/// principal, or the trees are not a workspace root (`[self] mode` is not
/// `act`, or no repo is set: `pr::trees_root`).
pub(crate) fn of(rt: &ToolRuntime, session: &str) -> Option<PathBuf> {
    let core = rt.self_pr.core.get().and_then(std::sync::Weak::upgrade)?;
    let trees = paths::canonical_best_effort(&pr::trees_root(&core.cfg, &rt.self_pr.state)?);
    let rec: crate::session::SessionRecord = core.store.get_session(session).ok().flatten()?;
    let execution = rec.execution_id?;
    if crate::task_graph::principal_of(&core.kernel, &execution) != SELF_PRINCIPAL {
        return None;
    }
    let root = root_of(rt, &core.store, session)?;
    Some(trees.join(tree::slug(&root)))
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
