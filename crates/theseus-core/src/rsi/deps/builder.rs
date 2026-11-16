//! The builder's half: a self builder's `proc.run` may reach the registries
//! ([`super::hosts::HOSTS`]) and keeps what it downloads in a package cache of
//! its own, inside the item's build folder (`rsi::builder_dir`), so a second
//! command does not download again.
//!
//! The cache is the builder's: the gate's view never binds the build folder, and
//! the pipeline's fetch ([`super::provide`]) writes a cache of its own beside the
//! tree, so a package the builder downloaded never reaches a verdict. A package
//! the builder tried is judged again, from the registry, when its branch is
//! built.
//!
//! Only a self builder's job gets any of this. Its egress list gains the
//! registries' hosts and no others, and its environment two variables.

use std::path::{Path, PathBuf};

use theseus_kernel::job::L1;

use super::hosts;
use crate::toolrun::ToolRuntime;

/// Where a builder's package caches are, inside its build folder `folder`.
pub fn cache_in(folder: &Path) -> PathBuf {
    folder.join(".packages")
}

/// The variables a self builder's command is given for its caches: cargo's
/// home (its registry, index and downloads) and npm's cache. They replace any
/// a call set.
pub fn env(folder: &Path) -> Vec<(String, String)> {
    let cache = cache_in(folder);
    vec![
        (
            "CARGO_HOME".to_string(),
            cache.join("cargo").display().to_string(),
        ),
        (
            "npm_config_cache".to_string(),
            cache.join("npm").display().to_string(),
        ),
    ]
}

/// The build folder of the self task that owns `session` (`<state>/self/builder/<slug>`, the
/// folder `rsi::builder_dir` names), or none: the session is no self builder's, or its item's
/// tree is not open, and then nothing here applies to it. Named by the session's own task, never
/// by a call.
pub(crate) fn folder_of(rt: &ToolRuntime, session: &str) -> Option<PathBuf> {
    let tree = crate::rsi::own_tree::of(rt, session)?;
    tree.join(".git").exists().then_some(())?;
    let slug = tree.file_name()?.to_str()?.to_string();
    let state = theseus_tools::paths::canonical_best_effort(&rt.self_pr.state);
    Some(state.join("self").join("builder").join(slug))
}

/// What a self builder's command is given for `folder`: the registries on its egress list,
/// and its cache directory writable (a mask over a directory that holds it is lifted, as for
/// any bind the view makes). Makes the directory, so call it off the runtime's workers.
pub(crate) fn apply(view: &mut L1, folder: &Path) -> std::io::Result<()> {
    extend(view);
    let cache = cache_in(folder);
    std::fs::create_dir_all(&cache)?;
    view.hidden
        .retain(|h| !(cache != *h && cache.starts_with(h)));
    if !view.rw.iter().any(|r| cache.starts_with(r)) {
        view.rw.push(cache);
    }
    Ok(())
}

/// `view`'s egress list with the registries' hosts added, each once.
pub fn extend(view: &mut L1) {
    for h in hosts::egress() {
        if !view.egress.contains(&h) {
            view.egress.push(h);
        }
    }
}

/// [`operator_list`] of the operator's `[sandbox] egress` for `session`'s job: the registries too
/// when it is a self builder's, and as configured for any other.
pub(crate) fn operator_list_of(rt: &ToolRuntime, session: &str) -> Vec<String> {
    match folder_of(rt, session) {
        Some(_) => operator_list(&rt.sandbox.cfg.egress),
        None => rt.sandbox.cfg.egress.clone(),
    }
}

/// The operator's `[sandbox] egress` list as a self builder's job reads it:
/// the registries are the owner's own decision (2026-11-15 17:46:56), so a
/// job that reached only them has reached no host beyond the list, and its
/// result is not marked as outside text.
pub fn operator_list(operator: &[String]) -> Vec<String> {
    let mut list = operator.to_vec();
    for h in hosts::egress() {
        if !list.contains(&h) {
            list.push(h);
        }
    }
    list
}
