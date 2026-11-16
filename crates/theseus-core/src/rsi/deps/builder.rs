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

/// What a self builder's command is given beside its build folder `folder` (`rsi::builder_dir`,
/// which the caller took from the session's own task, never from a call): the registries on its
/// egress list, and its package cache made inside the folder, which is already the one directory
/// the view writes. Makes the directory, so call it off the runtime's workers.
pub(crate) fn apply(view: &mut L1, folder: &Path) -> std::io::Result<()> {
    extend(view);
    std::fs::create_dir_all(cache_in(folder))
}

/// `view`'s egress list with the registries' hosts added, each once. A self session's list reaches
/// here already holding the registries and any host its call named itself, and never the operator's
/// (`Sandbox::egress_for`, at the gate); this makes sure of the registries.
pub fn extend(view: &mut L1) {
    for h in hosts::egress() {
        if !view.egress.contains(&h) {
            view.egress.push(h);
        }
    }
}

/// The list `session`'s job counts as "the list" when it marks a result as outside text: for a self
/// session, [`operator_list`], and for any other the operator's `[sandbox] egress` as configured.
pub(crate) fn operator_list_of(rt: &ToolRuntime, session: &str) -> Vec<String> {
    rt.sandbox.egress_for(rt, Some(session))
}

/// What a self builder's job counts as the list: the registries and nothing else (the owner's own
/// decision, 2026-11-15 17:46:56). A self session's sandboxed command is not given the operator's
/// `[sandbox] egress` (`Sandbox::egress_for`, which reads [`hosts::egress`] too), so a job that
/// reached only the registries has reached no host beyond its list, and its result is not marked as
/// outside text. The removal rule (`rsi::own_removals`) reads this same list.
pub fn operator_list() -> Vec<String> {
    hosts::egress()
}
