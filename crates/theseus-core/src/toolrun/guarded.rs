//! The paths the gate guards and no L1 view shows: the workspace's roots and
//! the floor (Theseus's own state, the vault's token, and the sealed holdout
//! sets). Moved out of `toolrun.rs` to make room for the sealed dir's line
//! (theseus-pw1q.5). While `[self]` builds (theseus-pw1q.8), the self trees
//! join the roots, so a self session's file tools edit its tree
//! (`crate::rsi::pr::trees_root`).
//!
//! **The state dir the floor is over is resolved here, once, by
//! [`state_dir_in_use`], and neither [`guarded_paths`] nor [`sandbox_for`]
//! takes one.** A caller that could pass a directory could hand the floor a
//! directory no daemon serves out of, which is the whole of theseus-0h9f3's
//! harm; with the parameter gone that is unrepresentable rather than merely
//! tested for (the owner's word, Discord DM 2026-11-11 11:34).
//!
//! The approve list was a third return here until 1.0.4 (the owner,
//! 2026-11-03 09:01). It is gone, and with it the hiding it bought an L1 view
//! beyond the floor's: `~/.aws` stays hidden, because `sandbox::CREDENTIALS`
//! names it in every view whatever any list said, but `~/.ssh` and `~/.gnupg`
//! are no longer hidden from an L1 job by this path.

use std::path::PathBuf;

use crate::sandbox::Sandbox;

/// The state dir in use, and **the one place the floor resolves it**: the
/// config's own `[server].state_dir`, expanded (`Config::state_dir`) and then
/// canonicalized.
///
/// `theseusd` folds `--state-dir`/`THESEUS_STATE_DIR` into that field before
/// it opens the store (its own `state_dir_in_use`, theseus-3i6d1, the floor
/// review's F2): that function *writes* the field, this one *reads* it, so
/// the floor is over the directory the store, the spool and the bindings file
/// are actually in.
///
/// This was a `state: &Path` parameter on [`guarded_paths`] and
/// [`sandbox_for`], and every caller resolved it for itself. Three callers
/// resolved it three ways -- the runtime canonicalized `cfg.state_dir()`,
/// `theseusd check` passed the uncanonicalized value, and two tests passed a
/// directory of their own that the config knew nothing about -- so the floor
/// could be built over a directory no daemon was serving out of. The
/// parameter is gone on the owner's word (Discord DM 2026-11-11 11:34), which
/// makes that drift unrepresentable rather than merely tested for: a caller
/// has nowhere to put a second directory.
pub(crate) fn state_dir_in_use(cfg: &crate::Config) -> PathBuf {
    theseus_tools::paths::canonical_best_effort(&cfg.state_dir())
}

/// The workspace's roots and the floor's paths from `cfg` alone, over the
/// state dir in use ([`state_dir_in_use`]): what the gate guards, and what no
/// L1 view shows (M4 17b).
pub(super) fn guarded_paths(cfg: &crate::Config) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let state = state_dir_in_use(cfg);
    let t = &cfg.tools;
    let canon = |p: &str| theseus_tools::paths::canonical_best_effort(&crate::config::expand(p));
    // The workspace is what the config names: projects_dir first, then any
    // more roots. Nothing is assumed about where an operator keeps projects.
    let roots: Vec<PathBuf> = t
        .projects_dir
        .iter()
        .chain(t.roots.iter())
        .map(|r| canon(r))
        .chain(crate::rsi::pr::trees_root(cfg, &state))
        .collect();
    // The floor: Theseus's own state (EVERY store a mode can serve and the
    // spool beside each, and the bindings file, in the state dir actually in
    // use) and the 1Password CLI's credentials. Since theseus-3i6d1 the
    // owner's own instruction file in that same state dir is there too, and so
    // is the config file the daemon serves from. This sentence said "its
    // store, spool, and bindings file" until theseus-rq78r -- a singular pair,
    // which stopped being true when the floor started naming every store
    // (theseus-0h9f3 L21, the floor review's F3); the comment below is the
    // precise one.
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
        canon_path(cfg.discord.bindings_path(&state)),
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

/// L1's state for `cfg`, over the state dir in use ([`state_dir_in_use`]), as
/// the runtime builds it: `theseusd check` runs the self-test over the view a
/// job gets (theseus-gyin). It takes no directory, so that claim is true by
/// construction and not by the caller's care.
pub fn sandbox_for(cfg: &crate::Config) -> Sandbox {
    let (roots, floor) = guarded_paths(cfg);
    Sandbox::new(&cfg.sandbox, &roots, &floor)
}

#[cfg(test)]
mod tests {
    use theseus_protocol::rsi::SelfMode;

    use super::{guarded_paths, state_dir_in_use};

    /// A config whose state dir is `state`. This is the **only** way a test
    /// can put the floor over a directory of its choosing now that neither
    /// floor function takes one: the floor reads `[server].state_dir` and
    /// nothing else (theseus-0h9f3, the owner's word of 2026-11-11 11:34).
    fn cfg_over(state: &std::path::Path) -> crate::Config {
        let mut cfg = crate::Config::example();
        cfg.server.state_dir = state.to_string_lossy().into_owned();
        cfg
    }

    /// The self trees are a root only while `[self]` builds: mode act and a
    /// repo (theseus-pw1q.8), so a self session's file tools reach its tree.
    #[test]
    fn the_self_trees_are_a_root_only_while_self_builds() {
        let state = std::path::Path::new("/nonexistent/theseus-guarded/state");
        let trees = state.join("self").join("trees");
        let mut cfg = cfg_over(state);
        assert!(!guarded_paths(&cfg).0.contains(&trees), "mode off");
        cfg.self_improve.mode = SelfMode::Act;
        assert!(!guarded_paths(&cfg).0.contains(&trees), "no repo");
        cfg.self_improve.repo = Some("/nonexistent/theseus-guarded/repo".into());
        let roots = guarded_paths(&cfg).0;
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
        let cfg = cfg_over(state);
        let floor = guarded_paths(&cfg).1;
        for stdio in [false, true] {
            let store = crate::rpc::store_dir(state, stdio);
            let spool = crate::rpc::spool_dir(&store);
            assert!(floor.contains(&store), "stdio={stdio}: {store:?} {floor:?}");
            assert!(floor.contains(&spool), "stdio={stdio}: {spool:?} {floor:?}");
        }
        // The pair a `--stdio` daemon serves, by name. The floor named only
        // the socket pair before this row, so these two are the assertion
        // that was missing.
        //
        // ⚠ WRITTEN AS WORDS ON PURPOSE, AND LOAD-BEARING: THIS IS THE ONLY
        // PLACE THAT SAYS SO (theseus-rq78r, from the floor review's F2).
        // `rpc::STORE_DIR_NAMES` -- the list the floor above reads -- lives in
        // `rpc/mod.rs`, which is NOT in `scripts/keel-guard.py`'s
        // `KEEL_FILES`. So the floor's CONTENT is set in a file the keel does
        // not watch, while the assertions over it are in this file, which the
        // keel does. Which assertion carries which hazard was measured both
        // ways on 2026-11-11 against this tree, not reasoned about:
        //
        //   * NARROW the list (`[STORE_DIR, STORE_DIR_STDIO]` ->
        //     `[STORE_DIR]`): 5 red, 2 of them outside this file. Here the
        //     loop ABOVE reds first, at its `floor.contains(&store)`, because
        //     `rpc::store_dir` builds the stdio name from `STORE_DIR_STDIO`
        //     and NEVER from the list, so a narrowing desynchronises the two
        //     readers. THESE loops do not even execute -- the test is already
        //     over. (`the_floor_is_over_the_configs_state_dir_and_nothing_else`
        //     does catch it, by its own name list.)
        //   * RENAME the stdio store consistently, so both readers move
        //     together: 6 red, and now the loop above PASSES. THIS loop is
        //     what reds, with the `assert_eq!` in
        //     `the_daemons_store_name_is_one_the_floor_knows`.
        //
        // So the literal words are not a second way of catching a narrowing;
        // they are what pins the NAME, and in this file only the assertions
        // written as words do that. Deleting them to "stop repeating the
        // constant" would leave the store's name unpinned in the keel while
        // the list that holds it sits outside the keel. That is the `--stdio`
        // hole's own shape. Keep them, and keep them written as words.
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
    ///
    /// Its `assert_eq!`s name the two store directories as words, and that is
    /// load-bearing for the same measured reason as the literal loops in the
    /// test above -- see the ⚠ note there (theseus-rq78r). The
    /// `STORE_DIR_NAMES.contains` loop below is the half that catches a
    /// narrowing of the list; the `assert_eq!`s are the half that catches a
    /// rename.
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

    /// **The row's negative.** The floor is over `[server].state_dir` and
    /// over nothing else: two configs naming two directories floor their own
    /// and neither floors the other's. Neither floor function takes a
    /// directory any more, so there is no second channel for one to arrive
    /// by -- plant the `state: &Path` parameter back and this test cannot be
    /// written at all, which is the red the owner's word bought
    /// (2026-11-11 11:34; theseus-0h9f3).
    #[test]
    fn the_floor_is_over_the_configs_state_dir_and_nothing_else() {
        let a = std::path::Path::new("/nonexistent/theseus-guarded/a");
        let b = std::path::Path::new("/nonexistent/theseus-guarded/b");
        let floor_a = guarded_paths(&cfg_over(a)).1;
        let floor_b = guarded_paths(&cfg_over(b)).1;
        for name in [
            "store",
            "spool",
            "store-stdio",
            "spool-stdio",
            "bindings.toml",
            crate::project_context::USER_FILE,
        ] {
            assert!(floor_a.contains(&a.join(name)), "a: {name}: {floor_a:?}");
            assert!(floor_b.contains(&b.join(name)), "b: {name}: {floor_b:?}");
            // And nothing of the other directory is on either floor: a floor
            // over two state dirs at once is how the drift showed itself.
            assert!(!floor_a.contains(&b.join(name)), "a holds b's {name}");
            assert!(!floor_b.contains(&a.join(name)), "b holds a's {name}");
        }
    }

    /// The state dir the floor uses is the one the runtime resolves:
    /// `Config::state_dir()` **canonicalized**. Through a symlinked state dir
    /// the floor and the self-trees root are under the link's target.
    ///
    /// This is the drift that was representable before the parameter went:
    /// `toolrun::build_runtime` canonicalized before it called, and
    /// `theseusd check` passed `state_dir_in_use`'s uncanonicalized value, so
    /// the self-test's view and a job's view disagreed about this path --
    /// where `guarded.rs`'s own doc claims they are the same view.
    #[test]
    fn the_state_dir_the_floor_uses_is_the_one_the_runtime_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canon = std::fs::canonicalize(&real).unwrap();
        let mut cfg = cfg_over(&link);
        cfg.self_improve.mode = SelfMode::Act;
        cfg.self_improve.repo = Some(dir.path().join("repo").to_string_lossy().into_owned());
        assert_eq!(state_dir_in_use(&cfg), canon, "the one resolution");
        let (roots, floor) = guarded_paths(&cfg);
        let trees = |d: &std::path::Path| d.join("self").join("trees");
        assert!(roots.contains(&trees(&canon)), "canonical: {roots:?}");
        assert!(
            !roots.contains(&trees(&link)),
            "the link's own path: {roots:?}"
        );
        assert!(floor.contains(&canon.join("store")), "floor: {floor:?}");
    }

    /// `sandbox_for` -- the public entry point `theseusd check` and the
    /// self-build tests call -- hides the floor of the config it is handed,
    /// and takes no directory of its own. Before this row those three callers
    /// each passed one, and two of them passed a directory the config knew
    /// nothing about.
    #[test]
    fn sandbox_for_hides_the_floor_of_the_config_it_is_given() {
        let state = std::path::Path::new("/nonexistent/theseus-guarded/sandbox-for");
        let hidden = super::sandbox_for(&cfg_over(state)).job_view().hidden;
        for name in crate::rpc::STORE_DIR_NAMES {
            let store = state.join(name);
            let spool = crate::rpc::spool_dir(&store);
            assert!(hidden.contains(&store), "{store:?} not hidden: {hidden:?}");
            assert!(hidden.contains(&spool), "{spool:?} not hidden: {hidden:?}");
        }
    }
}
