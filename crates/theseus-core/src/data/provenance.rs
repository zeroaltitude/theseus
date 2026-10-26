//! A program's provenance (theseus-ouqru, design §2-§4): whether the system
//! that ran a command recorded that its read reach held nothing private.
//!
//! A command line does not bound what a program reads: quoting, a redirect,
//! a symlink, a glob, an expansion, or a build that loads a config of its own
//! all read past any reading of the line. So nothing here reads one. A
//! program's result passes the screen only when one of two things the system
//! itself established holds:
//!
//! - **(a) an enforced L1 job.** L1 runs the command in its own mount
//!   namespace over a view built from an empty root, and nothing falls back to
//!   L0, so the host paths a job can read are exactly the ones its view binds.
//!   The job wrapper writes that view into the job's completion after the
//!   sandbox spawned (`detail.sandbox.view`; `theseus-kernel`'s `job_l1`). The
//!   screen passes it when the view is `class: l1` with no error, binds the
//!   pinned system set and otherwise only the open roots and public paths,
//!   mounts HOME as an empty tmpfs, covers the daemon's own config, and holds
//!   no network and no granted variable. A view that is absent, old, or
//!   different in any of these fails closed.
//! - **(b) a sealed benchmark container** ([`crate::seal`]): the machine is a
//!   public image and no host filesystem reaches it.
//!
//! Anything else is [`Unscreened::Program`], as every program result was
//! before. The content detectors (the scrubber, `[privacy] sensitive_names`)
//! run in front of this and are never the reason a result passes.
//!
//! **Not a reach control on a command line.** The policy floor compares a
//! plan's resources to `[tools] roots` and a command's path arguments to
//! `approve_paths` only; it does not bound what a program reads, and this
//! module does not lean on it.
//!
//! The named residual: `/etc` is in the system set and is bound whole (every
//! job needs `/etc/ssl` and `/etc/passwd`). It is distro-owned except for the
//! daemon's own config, which the view must cover (`sandbox::config_covers`).
//! An operator who keeps private files elsewhere under `/etc` is outside this
//! boundary.

use std::path::{Path, PathBuf};

use serde_json::Value;
use theseus_tools::paths;

use super::{Screening, Unscreened};
use crate::open_view::Open;
use crate::seal::Seal;

/// Read-only paths every L1 job's toolchain may bind, public by construction:
/// the system's own directories (`theseus_sandbox::SYSTEM` less `/etc`, which
/// is the view's own set and is pinned, not a bind the screen judges).
const BUILT_IN: [&str; 7] = [
    "/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32",
];

/// What the system recorded of how a program ran, as the screen reads it.
pub struct Proof<'a> {
    /// The result node's `meta.detail`: the job's completion detail.
    pub detail: Option<&'a Value>,
    /// The result node's `meta.steps`, a batch's rows, each with the
    /// `sandbox` record of its own run.
    pub steps: Option<&'a Value>,
    /// This machine's seal verdict.
    pub seal: &'a Seal,
    /// The paths certified public ([`public_reach`]).
    pub public_reach: &'a [PathBuf],
    /// The daemon's config paths a view must cover
    /// (`sandbox::config_covers`).
    pub must_cover: &'a [PathBuf],
}

/// How a program's provenance was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// An enforced L1 job with a public view.
    L1,
    /// A sealed benchmark container.
    Seal,
}

impl Via {
    pub fn as_str(self) -> &'static str {
        match self {
            Via::L1 => "l1",
            Via::Seal => "seal",
        }
    }
}

/// The paths a program's view may bind read-only: the built-in system
/// directories and `[routing.data] public_reach`, each expanded and canonical.
pub fn public_reach(configured: &[String]) -> Vec<PathBuf> {
    BUILT_IN
        .iter()
        .map(|p| (*p).to_string())
        .chain(configured.iter().cloned())
        .map(|p| paths::canonical_best_effort(&crate::config::expand(&p)))
        .collect()
}

/// How `s`'s program result got its provenance, or none.
pub fn via(s: &Screening<'_>) -> Option<Via> {
    let p = &s.proof;
    if p.seal.is_sealed() {
        return Some(Via::Seal);
    }
    let own = p.detail.is_some_and(|d| held(d.get("sandbox"), p, s.open));
    let steps = p.steps.is_none_or(|rows| steps_held(rows, p, s.open));
    (s.tool == "proc.run" && own && steps).then_some(Via::L1)
}

/// A program result's provenance: `Ok` when [`via`] finds one, else
/// [`Unscreened::Program`].
pub fn provenance(s: &Screening<'_>) -> Result<(), Unscreened> {
    via(s).map(drop).ok_or(Unscreened::Program)
}

/// Every step of a batch that ran holds its own recorded view: the rows each
/// carry their own `sandbox`, so no working directory is worked out again. A
/// step that did not run read nothing and is ignored; a row that does not say
/// it did not run is taken as one that did.
fn steps_held(rows: &Value, p: &Proof<'_>, open: &Open) -> bool {
    let Some(rows) = rows.as_array() else {
        return false;
    };
    rows.iter()
        .filter(|r| r.get("ran") != Some(&Value::Bool(false)))
        .all(|r| held(r.get("sandbox"), p, open))
}

/// A job's `sandbox` record names an L1 view that [`view_is_public`].
fn held(sandbox: Option<&Value>, p: &Proof<'_>, open: &Open) -> bool {
    let Some(sandbox) = sandbox else {
        return false;
    };
    sandbox.get("class").and_then(Value::as_str) == Some("l1")
        && sandbox.get("error").is_none()
        && sandbox
            .get("view")
            .is_some_and(|v| view_is_public(v, p, open))
}

/// The strings of `v[key]`, when it is an array of them.
fn strings<'a>(v: &'a Value, key: &str) -> Option<Vec<&'a str>> {
    v.get(key)?.as_array()?.iter().map(Value::as_str).collect()
}

/// Whether a recorded view binds nothing private. Every field must be there:
/// a view from an older wrapper, which has none of them, is not one.
///
/// 1. `system` is exactly the sandbox's pinned set, so a sandbox that binds
///    one more directory refuses every program result until this is read again;
/// 2. every path in `workspace`, `ro_paths` and `cwd` is canonically within an
///    open root or `public_reach`, and `rw` is empty (a writable bind is a
///    self build's, and what it writes persists on the host);
/// 3. `hidden` covers every path in `must_cover`;
/// 4. `home_tmpfs` is true;
/// 5. `egress` and `granted` are empty.
fn view_is_public(v: &Value, p: &Proof<'_>, open: &Open) -> bool {
    let (Some(system), Some(work), Some(ro), Some(rw), Some(hidden)) = (
        strings(v, "system"),
        strings(v, "workspace"),
        strings(v, "ro_paths"),
        strings(v, "rw"),
        strings(v, "hidden"),
    ) else {
        return false;
    };
    let (Some(egress), Some(granted), Some(cwd)) = (
        strings(v, "egress"),
        strings(v, "granted"),
        v.get("cwd").and_then(Value::as_str),
    ) else {
        return false;
    };
    let public = |path: &str| {
        let path = Path::new(path);
        let path = paths::canonical_best_effort(path);
        path.is_absolute()
            && open
                .roots
                .iter()
                .chain(p.public_reach)
                .any(|root| paths::within(&path, root))
    };
    let covered = |cover: &PathBuf| {
        hidden.iter().any(|h| {
            Path::new(h).is_absolute()
                && paths::within(cover, &paths::canonical_best_effort(Path::new(h)))
        })
    };
    system == theseus_sandbox::SYSTEM
        && v.get("home_tmpfs") == Some(&Value::Bool(true))
        && egress.is_empty()
        && granted.is_empty()
        && rw.is_empty()
        && Path::new(cwd).is_absolute()
        && work.iter().chain(&ro).copied().chain([cwd]).all(public)
        && p.must_cover.iter().all(covered)
}
