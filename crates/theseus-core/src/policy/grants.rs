//! What an edits mode covers (theseus-lyxy): `covers` is the one matching
//! rule, which `modes` calls for a session's `edits` mode. An edits grant
//! covers `fs.edit`, `fs.write` and `fs.patch` calls whose every resource,
//! canonical (symlinks resolved, `..` taken), is under the project's
//! directory. The gate's step is `modes::gate`, which applies it only to a
//! call the tool's posture alone makes wait (`ToolPolicy::posture_only`):
//! never the floor, a layer-1 change, the approve lists, a path outside the
//! roots, an AWS rule.
//!
//! Earned autonomy and scoped grants ("yes, and don't ask again", one
//! answer's, for a command's prefix) used to share this file; neither could
//! fire under `enforcement = notify` and both are gone. A plan's question
//! still offers its other yes, edits mode (`plan_offer`).

use std::path::{Path, PathBuf};

use serde_json::Value;
use theseus_protocol::grants::{GrantOffer, SessionGrant, KIND_EDITS};
use theseus_protocol::PlaceClass;
use theseus_tools::{paths, Plan};

use super::normalized_argv;

/// The tool `modes` reads a command's `args_within_roots` for.
pub const RUN_TOOL: &str = "proc.run";

/// The tools an edits grant covers.
pub const EDIT_TOOLS: [&str; 3] = ["fs.edit", "fs.write", "fs.patch"];

/// One call, as `covers` reads it.
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

/// A project directory an edits grant may name: not `/`, and not the home
/// directory itself.
pub(crate) fn root_ok(root: &Path) -> bool {
    root.is_absolute()
        && root.parent().is_some()
        && std::env::var_os("HOME")
            .is_none_or(|h| paths::canonical_best_effort(Path::new(&h)) != root)
}

/// A plan's question offers its other yes, edits mode (theseus-lyxy): no
/// grant, so none of a grant's conditions on the call apply. `approve` is
/// whether the call waits, `l1` whether it is a layer-1 job's.
pub(crate) fn plan_offer(
    tool: &str,
    class: PlaceClass,
    approve: bool,
    l1: bool,
) -> Option<GrantOffer> {
    (tool == super::modes::PLAN_PRESENT && approve && !l1 && class == PlaceClass::Private)
        .then(GrantOffer::mode_edits)
}

pub(crate) fn has_env(input: &Value) -> bool {
    input
        .get("env")
        .is_some_and(|e| !e.is_null() && !e.as_object().is_some_and(|o| o.is_empty()))
}

/// A plan as `covers` reads it: the normalised argv, the program as
/// written, the resources canonical.
pub(crate) fn view(plan: &Plan) -> (Vec<String>, Option<String>, Vec<PathBuf>) {
    let argv = plan.argv.as_deref().unwrap_or(&[]);
    let resources = plan
        .resources
        .iter()
        .map(|r| paths::canonical_best_effort(&r.path))
        .collect();
    (normalized_argv(argv), argv.first().cloned(), resources)
}

#[cfg(test)]
#[path = "grants/tests.rs"]
mod tests;
