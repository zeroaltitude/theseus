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
    // Every store a daemon can serve and the spool beside each, out of the
    // one list the daemon picks its own name from (`rpc::STORE_DIR_NAMES`)
    // and through the same `rpc::spool_dir` it resolves its own spool with.
    // These two lines were the bare literals `state.join("store")` and
    // `state.join("spool")`, which are the SOCKET daemon's pair: a `--stdio`
    // daemon serves `store-stdio` and `spool-stdio`, so in that mode the
    // floor named a pair that was not in use and missed the pair that was --
    // a tool call naming the owner's own `--stdio` session history was never
    // held for his approval, and an L1 job's view showed it (theseus-0h9f3
    // L21; the owner's word of 2026-11-11 08:26 is on the commit that
    // changed this file). Both pairs are floored in both modes: whichever is
    // in use is held, and holding the other costs nothing, since it is
    // Theseus's own state whether a daemon is serving out of it or not. One
    // list with two readers, or the next mode's store name diverges them
    // again -- which is the ruling this project already made one level up,
    // for the state dir itself (theseus-3i6d1, the floor review's F2).
    let mut floor_paths: Vec<PathBuf> = Vec::new();
    for name in crate::rpc::STORE_DIR_NAMES {
        let store = state.join(name);
        let spool = crate::rpc::spool_dir(&store);
        floor_paths.push(canon_path(store));
        floor_paths.push(canon_path(spool));
    }
    floor_paths.extend([
        canon_path(cfg.discord.bindings_path(state)),
        // The user-level instruction file (theseus-wadu §1, theseus-3i6d1):
        // read live into the system block on every request, with the standing
        // of `[context] files` and no grant between it and the prompt. A
        // tool's `fs.write` to this one fixed, auto-discovered path would put
        // chosen text into every prompt, permanently, and block 3 is not a
        // `ContentItem`, so no injection label can ever be raised on it.
        canon_path(state.join(crate::project_context::USER_FILE)),
        canon("~/.config/op"),
    ]);
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

    /// Every store a daemon can serve is on the floor with the spool beside
    /// it, resolved the way the daemon resolves its own (theseus-0h9f3 L21).
    /// The literal names are asserted as words on purpose: this is the row's
    /// negative, so re-introducing the bare `state.join("store")` /
    /// `state.join("spool")` pair reds here as well as at the Core level
    /// (`tests_floor_stdio`).
    #[test]
    fn every_store_a_mode_serves_is_floored_with_its_spool() {
        let state = std::path::Path::new("/nonexistent/theseus-guarded/state");
        let cfg = crate::Config::example();
        let floor = guarded_paths(&cfg, state).1;
        for stdio in [false, true] {
            let store = crate::rpc::store_dir(state, stdio);
            let spool = crate::rpc::spool_dir(&store);
            assert!(floor.contains(&store), "stdio={stdio}: {store:?} {floor:?}");
            assert!(floor.contains(&spool), "stdio={stdio}: {spool:?} {floor:?}");
        }
        // The pair a `--stdio` daemon serves, by name. The floor named only
        // the socket pair before this row, so these two are the assertion
        // that was missing.
        for name in ["store-stdio", "spool-stdio"] {
            assert!(
                floor.contains(&state.join(name)),
                "{name} is not floored: {floor:?}"
            );
        }
        // And the socket pair is still there: half a fix swaps one for the
        // other.
        for name in ["store", "spool"] {
            assert!(
                floor.contains(&state.join(name)),
                "{name} is not floored: {floor:?}"
            );
        }
    }

    /// The daemon cannot pick a store name the floor does not know: it
    /// resolves through `rpc::store_dir`, which joins a name out of
    /// `STORE_DIR_NAMES`, and the floor floors every name in that list. One
    /// list with two readers (theseus-0h9f3 L21).
    #[test]
    fn the_daemons_store_name_is_one_the_floor_knows() {
        let state = std::path::Path::new("/nonexistent/theseus-guarded/state");
        assert_eq!(crate::rpc::store_dir(state, false), state.join("store"));
        assert_eq!(
            crate::rpc::store_dir(state, true),
            state.join("store-stdio")
        );
        for stdio in [false, true] {
            let name = crate::rpc::store_dir(state, stdio);
            let name = name.file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                crate::rpc::STORE_DIR_NAMES.contains(&name.as_str()),
                "stdio={stdio}: {name} is outside STORE_DIR_NAMES"
            );
        }
    }
}
