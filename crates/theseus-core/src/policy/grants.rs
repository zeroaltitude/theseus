//! Scoped grants (theseus-drjs): "yes, and don't ask again" from a single
//! answer, for the rest of one conversation. The wire types and their words
//! are `theseus_protocol::grants`; this is the matching, the offer and the
//! gate's step.
//!
//! A grant is an earned grant's matching with a narrower life:
//!
//! - **What it covers.** A `prefix` grant: `proc.run` calls whose argv, the
//!   program by its base name, starts with the prefix, whose program is
//!   written exactly as the offered call wrote it (`/tmp/x/python3` is not
//!   `python3`), and whose words after the prefix name no URL. An `edits`
//!   grant: `fs.edit`, `fs.write` and `fs.patch` calls whose every resource,
//!   canonical (symlinks resolved, `..` taken), is under the project's
//!   directory.
//! - **What it never covers.** The gate applies it where an earned grant
//!   applies (`earned::gate`'s conditions, asked again here): only a call
//!   the tool's posture alone makes wait (`ToolPolicy::posture_only`: never
//!   the floor, a layer-1 change, the approve lists, a path outside the
//!   roots, an AWS rule), in a private place, with no `env`, for a tool no
//!   one tightened, outside L1 (an L1 job's asks, its egress among them,
//!   are its own). Every later layer of the order (a granted secret, the
//!   place's floor, T1's hold, an MCP client's floor) still raises it, and it
//!   runs at `notify`, never `open`.
//! - **What is never offered.** A shell, or a program that runs what it is
//!   given (`env`, `xargs`, `sudo`, ...), an interpreter's inline code, a
//!   program named by a relative path, a first word that is an option (but
//!   an interpreter's `-m <module>`), and a launcher's subcommand without
//!   the program it launches (`uv run` alone): no offer at all.
//! - **Where it lives.** In memory, per session (`Grants`), one lookup per
//!   gated call; the record is the ledger (`policy.granted`,
//!   `policy.grant_ended`), read back once after serving (`Core::load_grants`).
//!   A retired session's grants end in the frame that retires it.
//!
//! `covers` is the one matching rule: modes (theseus-lyxy) calls it for a
//! session's mode, as the gate does for its grants.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use serde_json::Value;
use sha2::{Digest, Sha256};
use theseus_protocol::grants::{GrantOffer, SessionGrant, KIND_EDITS, KIND_PREFIX};
use theseus_protocol::PlaceClass;
use theseus_tools::{paths, Plan, Tool};

use super::earned::find::{self, INTERPRETERS, RUNS_WHAT_IT_IS_GIVEN};
use super::{normalized_argv, prefix_match, Decision, Notice, Posture};
use crate::toolrun::ToolRuntime;

/// The tool a prefix grant is for.
pub const RUN_TOOL: &str = "proc.run";

/// The tools an edits grant covers.
pub const EDIT_TOOLS: [&str; 3] = ["fs.edit", "fs.write", "fs.patch"];

/// A setting's prefix: a notice's `setting` of `session grant sgr_…`.
pub const SETTING: &str = "session grant ";

/// Modules an interpreter's `-m` runs that run what they are given.
const CODE_MODULES: &[&str] = &[
    "timeit",
    "code",
    "pdb",
    "cProfile",
    "profile",
    "trace",
    "runpy",
    "idlelib",
    "doctest",
    "pydoc",
    "asyncio",
    "py_compile",
    "compileall",
    "zipapp",
];

/// A program's subcommands that launch the program named next: the prefix
/// takes that program too, and it must be one a prefix may name.
const LAUNCHERS: &[(&str, &str)] = &[
    ("uv", "run"),
    ("uv", "tool"),
    ("poetry", "run"),
    ("pipenv", "run"),
    ("pdm", "run"),
    ("hatch", "run"),
    ("conda", "run"),
    ("npm", "exec"),
    ("pnpm", "exec"),
    ("pnpm", "dlx"),
    ("yarn", "exec"),
    ("yarn", "dlx"),
    ("bundle", "exec"),
    ("cargo", "exec"),
    ("rustup", "run"),
    ("nix", "run"),
    ("nix", "shell"),
    ("docker", "run"),
    ("docker", "exec"),
    ("podman", "run"),
    ("podman", "exec"),
    ("kubectl", "exec"),
    ("systemd-run", "--user"),
    ("opam", "exec"),
    ("stack", "exec"),
    ("dotnet", "exec"),
    ("go", "run"),
    ("git", "submodule"),
];

/// One call, as a grant reads it.
pub struct Call<'a> {
    pub tool: &'a str,
    /// The program exactly as the call wrote it.
    pub program: Option<&'a str>,
    /// The argv, the program by its base name.
    pub nargv: &'a [String],
    /// The resources, canonical.
    pub resources: &'a [PathBuf],
}

/// Whether `g` covers `call`: the one rule.
pub fn covers(g: &SessionGrant, call: &Call<'_>) -> bool {
    match g.kind.as_str() {
        KIND_PREFIX => {
            call.tool == RUN_TOOL
                && prefix_ok(&g.prefix).is_ok()
                && g.program
                    .as_deref()
                    .is_some_and(|p| Some(p) == call.program)
                && prefix_match(call.nargv, &g.prefix)
                && !call.nargv[g.prefix.len()..]
                    .iter()
                    .any(|w| w.contains("://"))
        }
        KIND_EDITS => {
            let Some(root) = g.root.as_deref().map(Path::new) else {
                return false;
            };
            EDIT_TOOLS.contains(&call.tool)
                && root_ok(root)
                && !call.resources.is_empty()
                && call.resources.iter().all(|r| paths::within(r, root))
        }
        _ => false,
    }
}

/// Why `prefix` is never a scoped grant's: earned autonomy's rule
/// (`find::prefix_ok`), but for an interpreter's `-m <module>` naming a
/// module that is not one that runs what it is given.
pub fn prefix_ok(prefix: &[String]) -> Result<(), String> {
    if let [program, m, module] = prefix {
        if m == "-m" && python(program) {
            let plain = !module.is_empty()
                && module
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
            return match plain && !CODE_MODULES.contains(&module.as_str()) {
                true => Ok(()),
                false => Err(format!("`{program} -m {module}` runs what it is given")),
            };
        }
    }
    find::prefix_ok(prefix)?;
    // A launcher's subcommand names the program it launches too.
    if let Some(i) = launched_at(prefix) {
        match prefix.get(i) {
            Some(w) if launchable(w) => {}
            _ => return Err(format!("`{}` launches what follows it", prefix.join(" "))),
        }
    }
    Ok(())
}

/// A python, by its base name: `python3`, `python3.12`.
fn python(program: &str) -> bool {
    let base = base(program);
    base.strip_prefix("python")
        .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

fn base(program: &str) -> String {
    Path::new(program)
        .file_name()
        .map_or(program.to_string(), |b| b.to_string_lossy().into_owned())
}

fn interpreter(w: &str) -> bool {
    let b = base(w);
    INTERPRETERS.iter().any(|i| {
        b == *i
            || b.strip_prefix(i).is_some_and(|v| {
                v.starts_with('.') || v.chars().all(|c| c.is_ascii_digit() || c == '.')
            })
    })
}

/// Where the launched program's word is, for a launcher's subcommand.
fn launched_at(prefix: &[String]) -> Option<usize> {
    let (p, s) = (prefix.first()?, prefix.get(1)?);
    let p = base(p);
    LAUNCHERS
        .iter()
        .any(|(lp, ls)| *lp == p && ls == s)
        .then_some(2)
}

/// A program a launcher may be granted with: a plain word, not a shell, a
/// wrapper or an interpreter.
fn launchable(w: &str) -> bool {
    !w.starts_with('-')
        && !w.contains('/')
        && !w.contains("://")
        && !RUNS_WHAT_IT_IS_GIVEN.contains(&base(w).as_str())
        && !interpreter(w)
}

/// The prefix an offer names for `argv`, and its program as written: the
/// program and its subcommand (`cargo test`, `git status`), an interpreter's
/// `-m` and its module (`python3 -m unittest`), a launcher's program too
/// (`uv run pytest`). None when no prefix may be offered.
pub fn prefix_of(argv: &[String]) -> Option<(Vec<String>, String)> {
    let program = argv.first()?;
    if program.contains('/') && !Path::new(program).is_absolute() {
        return None;
    }
    let nargv = normalized_argv(argv);
    // An interpreter's `-m` names its module, and a launcher its program.
    let module = nargv.get(1).is_some_and(|w| w == "-m") && python(&nargv[0]);
    let len = if module || launched_at(&nargv).is_some() {
        3
    } else {
        2
    };
    let prefix: Vec<String> = nargv.get(..len)?.to_vec();
    // The subcommand is a word, not an option, a path or an assignment.
    let words_ok = prefix[1..].iter().all(|w| {
        !w.is_empty()
            && !w.contains("://")
            && !w.contains('=')
            && (w == "-m" || !w.starts_with('-'))
    });
    (words_ok && prefix_ok(&prefix).is_ok()).then(|| (prefix, program.clone()))
}

/// A project directory an edits grant may name: not `/`, and not the home
/// directory itself.
fn root_ok(root: &Path) -> bool {
    root.is_absolute()
        && root.parent().is_some()
        && std::env::var_os("HOME")
            .is_none_or(|h| paths::canonical_best_effort(Path::new(&h)) != root)
}

/// What the question asked, as the gate decided it.
pub(crate) struct Asked<'a> {
    pub approve: bool,
    pub reason: &'a str,
    pub floor: bool,
    pub external: bool,
    pub l1: bool,
}

/// The offer a question carries: only for a call the tool's posture alone
/// made wait (no later layer of the order raised it), in a private place,
/// that a grant could cover. `dir` is the session's directory.
pub(crate) fn offer(
    rt: &ToolRuntime,
    class: PlaceClass,
    dir: Option<&str>,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
    asked: &Asked<'_>,
) -> Option<GrantOffer> {
    if !asked.approve
        || asked.floor
        || asked.external
        || asked.l1
        || class != PlaceClass::Private
        || has_env(input)
        || input.get("steps").is_some_and(|s| !s.is_null())
        || rt.tightened.get(tool.name()).is_some()
        || !rt.policy.posture_only(tool, plan)
    {
        return None;
    }
    // The posture is what asked: no later layer changed its decision.
    let by_posture = rt.policy.decide(tool, plan);
    if by_posture.posture != Posture::Approve || by_posture.reason != asked.reason {
        return None;
    }
    let (nargv, program, resources) = view(plan);
    let offered = if tool.name() == RUN_TOOL {
        if !rt.policy.args_within_roots(plan) {
            return None;
        }
        let argv = plan.argv.as_deref()?;
        let (prefix, program) = prefix_of(argv)?;
        (
            GrantOffer::prefix(&prefix),
            Some(program),
            Some(prefix),
            None,
        )
    } else if EDIT_TOOLS.contains(&tool.name()) {
        let root = rt.cwd_for(dir);
        if !root_ok(&root) || !rt.ctx.roots.iter().any(|r| paths::within(&root, r)) {
            return None;
        }
        let root = root.display().to_string();
        (GrantOffer::edits(&root), None, None, Some(root))
    } else {
        return None;
    };
    // It covers the call it is offered on, or it is no offer.
    let (o, gprogram, prefix, root) = offered;
    let g = SessionGrant {
        kind: o.kind.clone(),
        prefix: prefix.unwrap_or_default(),
        program: gprogram,
        root,
        ..Default::default()
    };
    let call = Call {
        tool: tool.name(),
        program: program.as_deref(),
        nargv: &nargv,
        resources: &resources,
    };
    covers(&g, &call).then_some(o)
}

/// The offer of a question a turn asks: `offer`, for the call as its gate
/// decided it.
pub(crate) fn offered(
    rt: &ToolRuntime,
    tc: &crate::toolrun::TurnCtx<'_>,
    tool: &str,
    input: &Value,
    g: &crate::toolrun::Gated,
) -> Option<GrantOffer> {
    let tool = rt.registry.get(tool)?;
    let asked = Asked {
        approve: g.decision.posture == Posture::Approve,
        reason: &g.decision.reason,
        floor: g.decision.floor,
        external: g.decision.external.is_some(),
        l1: g.class.l1(),
    };
    offer(rt, tc.class, tc.dir, tool.as_ref(), &g.plan, input, &asked)
}

fn has_env(input: &Value) -> bool {
    input
        .get("env")
        .is_some_and(|e| !e.is_null() && !e.as_object().is_some_and(|o| o.is_empty()))
}

/// A plan as `covers` reads it: the normalised argv, the program as
/// written, the resources canonical.
fn view(plan: &Plan) -> (Vec<String>, Option<String>, Vec<PathBuf>) {
    let argv = plan.argv.as_deref().unwrap_or(&[]);
    let resources = plan
        .resources
        .iter()
        .map(|r| paths::canonical_best_effort(&r.path))
        .collect();
    (normalized_argv(argv), argv.first().cloned(), resources)
}

/// The grant an answer makes of an offer.
pub(crate) fn grant_of(
    offer: &GrantOffer,
    session_id: &str,
    correlation_id: &str,
    argv: Option<&[String]>,
    who: (&str, String, String),
    at_ms: u64,
) -> Option<SessionGrant> {
    let (prefix, program, root) = match offer.kind.as_str() {
        KIND_PREFIX => {
            let (prefix, program) = prefix_of(argv?)?;
            (prefix.join(" ") == offer.pattern).then_some(())?;
            (prefix, Some(program), None)
        }
        KIND_EDITS => (vec![], None, Some(offer.pattern.clone())),
        _ => return None,
    };
    let h = Sha256::digest(format!(
        "{session_id}\u{1}{correlation_id}\u{1}{}",
        offer.pattern
    ));
    Some(SessionGrant {
        id: format!("sgr_{}", hex::encode(&h[..4])),
        session_id: session_id.into(),
        scope: offer.scope.clone(),
        kind: offer.kind.clone(),
        pattern: offer.pattern.clone(),
        prefix,
        program,
        root,
        words: offer.words.clone(),
        correlation_id: correlation_id.into(),
        by: who.0.into(),
        who: who.1,
        via: who.2,
        at_ms,
    })
}

/// The grants in force, per session, in memory.
#[derive(Default)]
pub struct Grants {
    inner: RwLock<Inner>,
    any: AtomicBool,
}

#[derive(Default)]
struct Inner {
    by_session: BTreeMap<String, Vec<SessionGrant>>,
    /// Every grant ended in this run, so a read of the ledger that raced the
    /// end never brings it back.
    ended: BTreeSet<String>,
}

impl Grants {
    /// Whether there is any grant: the gate's cheap early exit.
    pub fn any(&self) -> bool {
        self.any.load(Ordering::Relaxed)
    }

    /// The grant of `session` that covers `call`, if one does.
    pub fn covering(&self, session: &str, call: &Call<'_>) -> Option<SessionGrant> {
        if !self.any() {
            return None;
        }
        let inner = self.inner.read().unwrap();
        inner
            .by_session
            .get(session)?
            .iter()
            .find(|g| covers(g, call))
            .cloned()
    }

    /// Whether a grant of `session` covers `call`: modes' entry.
    pub fn covers(&self, session: &str, call: &Call<'_>) -> bool {
        self.covering(session, call).is_some()
    }

    pub fn add(&self, g: SessionGrant) {
        let mut inner = self.inner.write().unwrap();
        if inner.ended.contains(&g.id) {
            return;
        }
        let list = inner.by_session.entry(g.session_id.clone()).or_default();
        if !list.iter().any(|x| x.id == g.id) {
            list.push(g);
        }
        self.any.store(true, Ordering::Relaxed);
    }

    /// End one grant: it is gone from memory, and never comes back.
    pub fn end(&self, id: &str) -> Option<SessionGrant> {
        let mut inner = self.inner.write().unwrap();
        inner.ended.insert(id.to_string());
        let mut out = None;
        for list in inner.by_session.values_mut() {
            if let Some(i) = list.iter().position(|g| g.id == id) {
                out = Some(list.remove(i));
            }
        }
        inner.by_session.retain(|_, l| !l.is_empty());
        self.any
            .store(!inner.by_session.is_empty(), Ordering::Relaxed);
        out
    }

    pub fn get(&self, id: &str) -> Option<SessionGrant> {
        let inner = self.inner.read().unwrap();
        inner
            .by_session
            .values()
            .flatten()
            .find(|g| g.id == id)
            .cloned()
    }

    /// The live grants, a session's alone when one is named, oldest first.
    pub fn list(&self, session: Option<&str>) -> Vec<SessionGrant> {
        let inner = self.inner.read().unwrap();
        let mut out: Vec<SessionGrant> = inner
            .by_session
            .iter()
            .filter(|(s, _)| session.is_none_or(|x| x == s.as_str()))
            .flat_map(|(_, l)| l.iter().cloned())
            .collect();
        out.sort_by_key(|g| g.at_ms);
        out
    }
}

/// The gate's step for a session's grants, after an earned grant's and
/// before the secret broker's: a call its tool's posture alone makes wait,
/// in a private place, outside L1, that a grant of its own session covers,
/// runs at `notify` instead, its notice naming the grant. Any other decision
/// returns as it came, and every later layer still raises it.
pub(crate) fn gate(
    rt: &ToolRuntime,
    at: &crate::toolrun::order::At<'_>,
    tool: &dyn Tool,
    plan: &Plan,
    input: &Value,
    l1: bool,
    d: Decision,
) -> Decision {
    let grants = &rt.earned.scoped;
    let (class, Some(session)) = (at.place.class, at.session) else {
        return d;
    };
    if d.posture != Posture::Approve
        || l1
        || d.floor
        || class != PlaceClass::Private
        || !grants.any()
        || rt.tightened.get(tool.name()).is_some()
        || has_env(input)
        || !rt.policy.posture_only(tool, plan)
    {
        return d;
    }
    if tool.name() == RUN_TOOL && !rt.policy.args_within_roots(plan) {
        return d;
    }
    let (nargv, program, resources) = view(plan);
    let call = Call {
        tool: tool.name(),
        program: program.as_deref(),
        nargv: &nargv,
        resources: &resources,
    };
    let Some(g) = grants.covering(session, &call) else {
        return d;
    };
    let what = match g.kind.as_str() {
        KIND_EDITS => format!("edits under {}", g.pattern),
        _ => g.pattern.clone(),
    };
    let rule = format!(
        "{} — notify (ran under your grant: {what}, this conversation; `theseus policy revoke {}` ends it)",
        tool.name(),
        g.id
    );
    Decision {
        notify: Some(Notice {
            kind: "notify".into(),
            setting: format!("{SETTING}{}", g.id),
            rule: rule.clone(),
        }),
        ..Decision::new(Posture::Notify, rule)
    }
}

#[cfg(test)]
#[path = "grants/tests.rs"]
mod tests;
