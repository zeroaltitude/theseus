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
//! lands, a write in another tree asks, and `proc.run` is untouched by this rule (its code
//! runs in L1 as `sandbox::self_build_view` says, and its one writable place is the build
//! folder below).
//! A session of any other principal gets neither.
//!
//! The tree is named by the task's id, which the backlog gives the model as
//! `item`, and never by the model's own input to the tool.
//!
//! **One more place, for `proc.run`'s build output alone** ([`builder_folder`]):
//! the item's build folder (`super::builder_dir`), which a self session's
//! commands may write and keep while its tree is open.

use std::path::PathBuf;

use theseus_tools::{paths, Tool};

use super::{budget, builder_dir, pr, tree, SELF_PRINCIPAL};
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

/// The self loop's own directory (`<state>/self`), canonical: its trees, build folders and review
/// scratch, where a self branch's code writes in L1.
pub(crate) fn self_dir(rt: &ToolRuntime) -> PathBuf {
    paths::canonical_best_effort(&rt.self_pr.state.join("self"))
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
                && rt.policy.fails_open(plan) =>
        {
            fail_open(decision, tool.name())
        }
        _ => decision,
    }
}

/// A self session's L1 call that runs in its own tree, with the owner's hold on the tool
/// made a notice (issue 6, condition 1). The owner's `[policy.tools]` line for `proc.run`
/// (and a tightening) holds an L1 call at approve, and `failing_open` leaves every L1 call
/// out, so a build that ran the tests its change touches, in its own sandbox, waited for an
/// answer, and in one night its only test loop was the half-hour gate. L1 is the boundary
/// for such a call (it reaches nothing of the floor), so the hold is lifted for exactly
/// this: a self session, an L1 call, and every place it reaches inside its own tree. The
/// floor, an AWS call, a private address and a call that reaches anywhere else keep the
/// hold, as in `failing_open`.
pub(crate) fn l1_in_own_tree(
    rt: &ToolRuntime,
    session: Option<&str>,
    tool: &dyn Tool,
    plan: &theseus_tools::Plan,
    l1: bool,
    decision: crate::policy::Decision,
) -> crate::policy::Decision {
    let Some(s) = session else {
        return decision;
    };
    if !(l1 && decision.posture == crate::policy::Posture::Approve && !decision.floor) {
        return decision;
    }
    let inside = of(rt, s).is_some_and(|tree| {
        !plan.resources.is_empty()
            && plan
                .resources
                .iter()
                .all(|r| paths::within(&paths::canonical_best_effort(&r.path), &tree))
    });
    if inside && rt.policy.fails_open(plan) {
        fail_open(decision, tool.name())
    } else {
        decision
    }
}

/// Why `session`'s `proc.run` runs in L1, or none: what `Sandbox::l1_for` says, and beyond it one
/// more case, [`l1_instead_of_floor`]. Every reader of a call's class that knows the session asks
/// this and not `l1_for` alone, so the gate, the reversibility layer, a batch and the removal rule
/// agree on which class the call is bound for.
pub(crate) fn l1_of(
    rt: &ToolRuntime,
    session: Option<&str>,
    tool: &dyn Tool,
    plan: &theseus_tools::Plan,
    input: &serde_json::Value,
) -> Option<String> {
    // Every command of a self-principal session (a builder, a fixer, a picker, a reviewer, an approver, and any
    // task or helper they start: the principal, not the role) runs in L1, whatever `[sandbox] default`,
    // `l1_argv` or the call's own `sandbox: false` say (issue 53). Where L1 cannot run, the launcher refuses the
    // job; nothing falls back to L0.
    if session.is_some_and(|s| is_self(rt, s)) {
        return Some(SELF_ALWAYS_L1.to_string());
    }
    rt.sandbox
        .l1_for(plan.argv.as_deref().unwrap_or_default(), input)
        .or_else(|| l1_instead_of_floor(rt, session, tool, plan, input))
}

/// Why a self-principal session's command runs in L1.
pub(crate) const SELF_ALWAYS_L1: &str =
    "a self-improvement session's commands always run in the sandbox, never on the host";

/// A self builder's command that the floor would hold at L0 only because it runs `theseusd`, bound
/// for L1 instead (issue 20). The floor reads every word that could be a program, so
/// `cargo test -p theseusd`, the builder's own test run of the daemon crate, asked the owner for an
/// answer on 2026-11-17 and parked an unattended build. The builder's brief tells it to use
/// `sandbox: true`, and this one did not.
///
/// In L1 the floor is not in the view (the store, the spool, the bindings, the credentials, the
/// socket, the config, the owner's proof and the sealed sets are hidden), and only the build folder
/// persists, so a program of that name reaches none of it: the hold protects nothing there, and the
/// call is asked about no more than any sandboxed command. It is NOT lowered at L0: the call is
/// moved, and a call that cannot be (a root daemon L1 refuses, an explicit `sandbox: false`, any
/// other session) is held as before. Only the one program name counts: a call that also runs `op`
/// or names any path on the floor, or any line the floor did not finish reading, is held.
/// A self session that is no builder (its item's tree is not open) is not moved either.
/// `docs/design/self-own-tree-asks.md` says why the floor held it and what is and is not moved.
fn l1_instead_of_floor(
    rt: &ToolRuntime,
    session: Option<&str>,
    tool: &dyn Tool,
    plan: &theseus_tools::Plan,
    input: &serde_json::Value,
) -> Option<String> {
    // The cheap tests first: most calls are neither a program's nor a self session's.
    if tool.name() != "proc.run" || input.get("sandbox").is_some() || rt.sandbox.refusal().is_some()
    {
        return None;
    }
    let session = session?;
    (rt.policy.floor_is_only_the_daemons_binary(plan)
        && is_self(rt, session)
        && builder_path(rt, session).is_some())
    .then(|| {
        "a self builder's command that runs Theseus's own binary runs in the sandbox, where it \
         reaches none of Theseus's own state"
            .to_string()
    })
}

/// Why a self-principal session's command may not run, or none: it runs `git` or `gh` by its own argv, a program
/// the broker hands a credential to (`[broker.programs]`). With that credential such a command could publish to the
/// self repository's remote around the owner's card; a self session's git needs none. A refusal in fixed words,
/// never a card.
pub(crate) fn credential_refusal(
    rt: &ToolRuntime,
    session: Option<&str>,
    plan: &theseus_tools::Plan,
) -> Option<String> {
    let first = plan.argv.as_deref()?.first()?;
    let name = std::path::Path::new(first).file_name()?.to_str()?;
    let credentialed = ["git", "gh"].contains(&name) && rt.broker.grants_program(name);
    (credentialed && session.is_some_and(|s| is_self(rt, s)))
        .then(|| CREDENTIAL_REFUSAL.to_string())
}

/// What a self-principal session is told when it runs `git` or `gh` with a brokered credential.
pub(crate) const CREDENTIAL_REFUSAL: &str = "a self-improvement session may not run git or gh with the \
     credential the broker grants them: the self loop publishes only through the join, on the owner's yes";

/// Why a self session's call may not write where it would, or none: a path with a `.git`
/// component (a tree's `.git` file, a repository's git directory, anywhere), the self repo's
/// git directory, or git's user-level config. These are what the daemon's own git reads when it
/// runs in a tree, outside L1 (`tree_git`), so a write there would choose what that git runs.
/// Each path is judged as named, as the gate resolves it, and with every link along it followed
/// (`followed`), so a link, a dangling one included, or a `..` reaches none of them. A refusal,
/// never a card: no answer of the owner's makes such a write part of a self build. Another
/// session's writes are not judged here.
pub(crate) fn git_refusal(
    rt: &ToolRuntime,
    session: Option<&str>,
    plan: &theseus_tools::Plan,
) -> Option<String> {
    let writes: Vec<&std::path::Path> = plan
        .resources
        .iter()
        .filter(|r| r.access == theseus_tools::Access::Write)
        .map(|r| r.path.as_path())
        .collect();
    if writes.is_empty() || !is_self(rt, session?) {
        return None;
    }
    let core = rt.self_pr.core.get().and_then(std::sync::Weak::upgrade)?;
    let repo = core
        .cfg
        .self_improve
        .repo
        .as_deref()
        .map(crate::config::expand);
    let mut kept: Vec<PathBuf> = repo
        .as_deref()
        .and_then(super::tree_git::common_dir)
        .into_iter()
        .collect();
    kept.extend(git_user_config());
    writes.into_iter().find(|p| reaches_git(p, &kept)).map(|p| {
        format!(
            "Not run: {} is git's own (a .git path, the repository's git directory, or git's \
                 configuration), which a self session does not write. Theseus's own git reads \
                 it outside the sandbox. Edit the tree's files; the commit is Theseus's.",
            p.display()
        )
    })
}

/// Whether a write to `p` reaches a `.git` path or anything under `kept`, by any of the three
/// readings [`git_refusal`] names.
pub(crate) fn reaches_git(p: &std::path::Path, kept: &[PathBuf]) -> bool {
    [
        p.to_path_buf(),
        paths::canonical_best_effort(p),
        followed(p),
    ]
    .into_iter()
    .any(|at| super::tree_git::names_dot_git(&at) || kept.iter().any(|k| paths::within(&at, k)))
}

/// `p` as a write through it lands: each link along it followed, a dangling one too (which
/// `canonical_best_effort` cannot resolve, and a write creates the file it names), and each
/// `..` taken after the links before it, as the system takes it. At most 40 links.
fn followed(p: &std::path::Path) -> PathBuf {
    use std::ffi::OsString;
    let parts = |p: &std::path::Path| -> Vec<OsString> {
        p.components()
            .rev()
            .map(|c| c.as_os_str().to_os_string())
            .collect()
    };
    let (mut todo, mut out, mut hops) = (parts(p), PathBuf::new(), 0);
    while let Some(c) = todo.pop() {
        match c.to_str() {
            Some(".") => continue,
            Some("..") => {
                out.pop();
                continue;
            }
            _ => out.push(&c),
        }
        if hops < 40 {
            if let Ok(to) = std::fs::read_link(&out) {
                hops += 1;
                out.pop();
                todo.extend(parts(&to));
            }
        }
    }
    out
}

/// Git's user-level config, where it reads it: `~/.gitconfig`, the XDG `git` directory, and
/// `GIT_CONFIG_GLOBAL` when it is set; each as the gate resolves it.
fn git_user_config() -> Vec<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let home = var("HOME");
    let xdg = var("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|h| h.join(".config")));
    [
        home.map(|h| h.join(".gitconfig")),
        xdg.map(|x| x.join("git")),
        var("GIT_CONFIG_GLOBAL"),
    ]
    .into_iter()
    .flatten()
    .map(|p| paths::canonical_best_effort(&p))
    .collect()
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

/// Whether a call of a self session failed to start because its own working copy is gone (issue
/// 20): the session is a self session, the call's working directory (its `cwd`, or the session's
/// own directory when it names none) is inside the tree its task works in, that directory does
/// not exist, and neither does the tree. A tool that is not broken is not blamed for a tree that was closed.
pub(crate) fn tree_gone(
    rt: &ToolRuntime,
    tc: &crate::toolrun::TurnCtx<'_>,
    input: &serde_json::Value,
) -> bool {
    let Some(tree) = of(rt, tc.session_id) else {
        return false;
    };
    let ctx = rt.ctx_in(tc);
    let cwd = match input.get("cwd").and_then(serde_json::Value::as_str) {
        Some(c) => ctx.resolve(c),
        None => ctx.cwd.clone(),
    };
    // The tree itself must be gone: a missing folder inside a tree that is still open is the
    // model's own mistake, and is told so in the plain words it can correct.
    !tree.is_dir() && !cwd.is_dir() && paths::within(&paths::canonical_best_effort(&cwd), &tree)
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

/// Where the build folder is of the self task that owns `session`, made or not, or none: the
/// session is no self session, or its item's tree is not open (a closed item has no folder, and
/// none is made for it). The folder is named by the session's task and never by a call.
pub(crate) fn builder_path(rt: &ToolRuntime, session: &str) -> Option<PathBuf> {
    let (core, trees) = self_session(rt, session)?;
    let slug = tree::slug(&root_of(rt, &core.store, session)?);
    trees.join(&slug).join(".git").exists().then_some(())?;
    let state = paths::canonical_best_effort(&rt.self_pr.state);
    let dir = builder_dir::dir_in(&state, &slug);
    // Only a folder that is its own resolved path: a link at the folder or at the builders' root
    // would send the read-write bind, and every removal judged "its own", to the link's target.
    (paths::canonical_best_effort(&dir) == dir).then_some(dir)
}

/// The tree `session`'s self task works in, to bind read-write in its builder's view beside the
/// build folder, or none: the session is no self session, its tree is not open, the tree is not its
/// own resolved path (a link at it or above it would send the bind elsewhere), or its `.git` is not
/// the plain file `git worktree add` wrote. Named by the session's task, never by a call.
pub(crate) fn own_tree_bind(rt: &ToolRuntime, session: &str) -> Option<PathBuf> {
    let tree = of(rt, session)?;
    let dot = std::fs::symlink_metadata(tree.join(".git")).ok()?;
    (dot.file_type().is_file() && paths::canonical_best_effort(&tree) == tree).then_some(tree)
}

/// The build folder for `session`'s self task, made if it was not there, or none (no self
/// session, or its tree is closed): what a self session's `proc.run` in L1 may write and keep,
/// beside its own tree ([`own_tree_bind`]; `Sandbox::self_view` binds both read-write for this session alone). Err
/// is the words a command is refused with when the folder has reached its bound. The disk is
/// walked, so call it off the runtime's workers.
pub(crate) fn builder_folder(rt: &ToolRuntime, session: &str) -> Result<Option<PathBuf>, String> {
    let Some(dir) = builder_path(rt, session) else {
        return Ok(None);
    };
    std::fs::create_dir_all(builder_dir::target_in(&dir))
        .and_then(|()| std::fs::create_dir_all(builder_dir::tmp_in(&dir)))
        .map_err(|e| format!("the build folder for this item could not be made: {e}"))?;
    match builder_dir::Full::of(&dir) {
        Some(full) => Err(full.refusal()),
        None => Ok(Some(dir)),
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
