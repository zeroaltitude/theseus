//! The paths the gate guards and no L1 view shows: the workspace's roots and
//! the floor (Theseus's own state, the vault's token, and the sealed holdout
//! sets). Moved out of `toolrun.rs` to make room for the sealed dir's line
//! (theseus-pw1q.5). While `[self]` builds (theseus-pw1q.8), the self trees
//! join the roots, so a self session's file tools edit its tree
//! (`crate::rsi::pr::trees_root`).
//!
//! The approve list was a third return here until 1.0.4 (the owner,
//! 2026-11-03 09:01). It is gone, and with it the hiding it bought an L1 view
//! beyond the floor's: `~/.aws` stays hidden, because `sandbox::CREDENTIALS`
//! names it in every view whatever any list said, but `~/.ssh` and `~/.gnupg`
//! are no longer hidden from an L1 job by this path.

use std::path::PathBuf;

use crate::sandbox::Sandbox;

/// The workspace's roots and the floor's paths, from `cfg` with `state` the
/// state dir in use: what the gate guards, and what no L1 view shows (M4 17b).
pub(super) fn guarded_paths(
    cfg: &crate::Config,
    state: &std::path::Path,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let t = &cfg.tools;
    let canon = |p: &str| theseus_tools::paths::canonical_best_effort(&crate::config::expand(p));
    // The workspace is what the config names: projects_dir first, then any
    // more roots. Nothing is assumed about where an operator keeps projects.
    let roots: Vec<PathBuf> = t
        .projects_dir
        .iter()
        .chain(t.roots.iter())
        .map(|r| canon(r))
        .chain(crate::rsi::pr::trees_root(cfg, state))
        .collect();
    // The floor: Theseus's own state (its store, spool, and bindings file, in
    // the state dir actually in use) and the 1Password CLI's credentials.
    // Since theseus-3i6d1 the owner's own instruction file in that same state
    // dir is there too, and so is the config file the daemon serves from.
    let canon_path = |p: PathBuf| theseus_tools::paths::canonical_best_effort(&p);
    let mut floor_paths = vec![
        canon_path(state.join("store")),
        canon_path(state.join("spool")),
        canon_path(cfg.discord.bindings_path(state)),
        // The user-level instruction file (theseus-wadu §1, theseus-3i6d1):
        // read live into the system block on every request, with the standing
        // of `[context] files` and no grant between it and the prompt. A
        // tool's `fs.write` to this one fixed, auto-discovered path would put
        // chosen text into every prompt, permanently, and block 3 is not a
        // `ContentItem`, so no injection label can ever be raised on it.
        canon_path(state.join(crate::project_context::USER_FILE)),
        canon("~/.config/op"),
    ];
    // The token file the daemon was given, by flag or by environment, the
    // config note's last-known-good copy (theseus-2fo), and the config file
    // itself when the config is a file (theseus-3i6d1). The copy was floored
    // and its original was not, so `fs.write` reached `theseus.toml` at the
    // tool's own posture: everything the file governs -- every posture in
    // `[policy]`, the roots, `[sandbox] egress`, where each secret is read
    // from -- was defended only by the file's ownership, and a turn that can
    // be induced to write a value into it is governed by the value it wrote
    // at the next start. All three are named by the daemon, never by the
    // TOML, so a config cannot move itself off the floor.
    for f in cfg
        .op_token_file
        .iter()
        .chain(&cfg.config_copy)
        .chain(&cfg.config_file)
    {
        floor_paths.push(canon_path(f.clone()));
    }
    // The sealed holdout sets (theseus-pw1q.5): no L1 view shows them
    // either, whatever binds them.
    if let Some(s) = crate::rsi::sealed::guard_of(cfg) {
        floor_paths.extend(s.paths().iter().cloned());
    }
    (roots, floor_paths)
}

/// L1's state for `cfg`, with `state` the state dir in use, as the runtime
/// builds it: `theseusd check` runs the self-test over the view a job gets
/// (theseus-gyin).
pub fn sandbox_for(cfg: &crate::Config, state: &std::path::Path) -> Sandbox {
    let (roots, floor) = guarded_paths(cfg, state);
    Sandbox::new(&cfg.sandbox, &roots, &floor)
}

#[cfg(test)]
mod tests {
    use theseus_protocol::rsi::SelfMode;

    use super::guarded_paths;

    /// The self trees are a root only while `[self]` builds: mode act and a
    /// repo (theseus-pw1q.8), so a self session's file tools reach its tree.
    #[test]
    fn the_self_trees_are_a_root_only_while_self_builds() {
        let state = std::path::Path::new("/nonexistent/theseus-guarded/state");
        let trees = state.join("self").join("trees");
        let mut cfg = crate::Config::example();
        assert!(!guarded_paths(&cfg, state).0.contains(&trees), "mode off");
        cfg.self_improve.mode = SelfMode::Act;
        assert!(!guarded_paths(&cfg, state).0.contains(&trees), "no repo");
        cfg.self_improve.repo = Some("/nonexistent/theseus-guarded/repo".into());
        let roots = guarded_paths(&cfg, state).0;
        assert!(roots.contains(&trees), "act and a repo: {roots:?}");
    }
}
