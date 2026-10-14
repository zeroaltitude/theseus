//! What the gate guards (M4 17b), apart from `toolrun.rs`, whose length the
//! shape budget caps: the workspace roots, the floor, and the approve list.
//! While `[self]` builds (theseus-pw1q.8), the self trees join the roots, so
//! a self session's file tools edit its tree (`crate::rsi::pr::trees_root`).

use std::path::PathBuf;

/// `cfg` with `state` the state dir in use: what the gate guards, and what
/// no L1 view shows (M4 17b).
pub(super) fn guarded_paths(
    cfg: &crate::Config,
    state: &std::path::Path,
) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
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
    let canon_path = |p: PathBuf| theseus_tools::paths::canonical_best_effort(&p);
    let mut floor_paths = vec![
        canon_path(state.join("store")),
        canon_path(state.join("spool")),
        canon_path(cfg.discord.bindings_path(state)),
        canon("~/.config/op"),
    ];
    // The token file the daemon was given, by flag or by environment, and
    // the config note's last-known-good copy (theseus-2fo).
    for f in cfg.op_token_file.iter().chain(&cfg.config_copy) {
        floor_paths.push(canon_path(f.clone()));
    }
    let approve: Vec<PathBuf> = t.approve_paths.iter().map(|p| canon(p)).collect();
    (roots, floor_paths, approve)
}
