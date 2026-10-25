//! Where a self branch's code runs (theseus-pw1q.21.1): the view it gets and
//! the environment it has, as values. The jobs themselves, in real L1, are
//! theseusd's `tests/self_build.rs`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::jail::{Jail, Job};
use super::tree::{Place, Tree};
use crate::sandbox::{Sandbox, SelfBuild};

fn sandbox() -> Sandbox {
    let cfg = crate::sandbox::SandboxConfig::default();
    Sandbox::new(
        &cfg,
        &[PathBuf::from("/work")],
        &[PathBuf::from("/state/store"), PathBuf::from("/sealed")],
        &[],
    )
}

fn build<'a>(ro: &'a [PathBuf], home: &'a Path) -> SelfBuild<'a> {
    SelfBuild {
        tree: Path::new("/state/self/trees/t"),
        target: Path::new("/state/self/target/t"),
        repo: Path::new("/repo"),
        ro,
        home,
    }
}

/// The only writable binds are the tree and its build tree; the repo, cargo's
/// and npm's homes and the bench history are overlays whose writes vanish;
/// the toolchain and the gate are read-only. No network, the model's
/// workspace roots are not in it, and the floor is hidden, the sealed set
/// with it.
#[test]
fn the_self_build_view_binds_exactly_the_tree_and_its_target_read_write() {
    let sb = sandbox();
    let gate = PathBuf::from("/state/self/gates/gate-abc.sh");
    let ro = [gate.clone()];
    let view = sb.self_build_view(&build(&ro, Path::new("/home/op")));
    assert_eq!(
        view.rw,
        [
            PathBuf::from("/state/self/trees/t"),
            PathBuf::from("/state/self/target/t")
        ]
    );
    for want in [
        "/repo",
        "/home/op/.cargo",
        "/home/op/.npm",
        "/home/op/.cache/theseus",
    ] {
        assert!(view.workspace.contains(&PathBuf::from(want)), "{want}");
    }
    assert!(!view.workspace.contains(&PathBuf::from("/work")));
    assert!(view.ro_paths.contains(&PathBuf::from("/home/op/.rustup")));
    assert!(view.ro_paths.contains(&gate));
    assert!(view.egress.is_empty(), "no network");
    for floor in ["/state/store", "/sealed"] {
        assert!(
            view.hidden.contains(&PathBuf::from(floor)),
            "{floor} hidden"
        );
    }
    assert_eq!(view.limits.pids, 8192);
    // The model's view is as it was: no rw, the job default of 512.
    let jobs = sb.job_view();
    assert!(jobs.rw.is_empty());
    assert_eq!(jobs.limits.pids, 512);
}

/// The approve list names the whole state directory (`~/.theseus`), which
/// also holds the gate's copy, the tree and the build tree. A mask over it
/// would cover those binds and the gate would find no script (exit 127, every
/// self branch red); the view hides what the directory holds that is secret
/// by its own name instead.
#[test]
fn a_hidden_directory_that_holds_the_binds_is_not_masked_over_them() {
    let cfg = crate::sandbox::SandboxConfig::default();
    let sb = Sandbox::new(
        &cfg,
        &[PathBuf::from("/work")],
        &[PathBuf::from("/state/store"), PathBuf::from("/state/spool")],
        &[PathBuf::from("/state"), PathBuf::from("/home/op/.ssh")],
    );
    let gate = PathBuf::from("/state/self/gates/gate-abc.sh");
    let ro = [gate.clone()];
    let view = sb.self_build_view(&build(&ro, Path::new("/home/op")));
    assert!(
        !view.hidden.contains(&PathBuf::from("/state")),
        "{:?}",
        view.hidden
    );
    for kept in ["/state/store", "/state/spool", "/home/op/.ssh"] {
        assert!(view.hidden.contains(&PathBuf::from(kept)), "{kept}");
    }
    // The model's own view keeps the whole directory hidden.
    assert!(sb.job_view().hidden.contains(&PathBuf::from("/state")));
}

/// The command's environment is the one list: cargo told it is offline, no
/// compiler wrapper, the build tree, and nothing of the daemon's own.
#[test]
fn a_self_builds_environment_is_its_own_and_offline() {
    let env = crate::sandbox::self_build_env(Path::new("/home/op"), Path::new("/t/target"));
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("CARGO_NET_OFFLINE"), Some("true"));
    assert_eq!(get("CARGO_TARGET_DIR"), Some("/t/target"));
    assert_eq!(get("RUSTC_WRAPPER"), Some(""));
    assert_eq!(get("HOME"), Some("/home/op"));
    assert!(get("PATH").unwrap().starts_with("/home/op/.cargo/bin:"));
    let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        names
            .iter()
            .all(|n| !n.starts_with("THESEUS_") && !n.contains("TOKEN")),
        "{names:?}"
    );
}

/// `Jail::prepare` is what the gate runs in: the place's own variables (a
/// test's hermetic git) ride along, the job's beside them.
#[test]
fn a_jails_job_gets_its_view_and_its_variables() {
    let jail = Jail::new(Arc::new(sandbox()));
    let place = Place {
        repo: PathBuf::from("/repo"),
        state: PathBuf::from("/state"),
        env: vec![(
            OsString::from("GIT_CONFIG_GLOBAL"),
            OsString::from("/dev/null"),
        )],
        jail: None,
    };
    let tree = Tree {
        slug: "t".into(),
        branch: "self/t".into(),
        dir: PathBuf::from("/state/self/trees/t"),
        target: PathBuf::from("/state/self/target/t"),
    };
    let job = Job {
        tree: &tree,
        ro: Vec::new(),
        argv: vec!["true".into()],
        env: vec![("THESEUS_GATE_ROOT".into(), "/state/self/trees/t".into())],
        log: Path::new("/dev/null"),
        limit: Duration::from_secs(1),
    };
    let (view, env) = jail.prepare(&place, &job);
    assert_eq!(view.rw, [tree.dir.clone(), tree.target.clone()]);
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("GIT_CONFIG_GLOBAL"), Some("/dev/null"));
    assert_eq!(get("THESEUS_GATE_ROOT"), Some("/state/self/trees/t"));
    assert_eq!(get("CARGO_NET_OFFLINE"), Some("true"));
}

/// Cargo's and npm's words when a branch needs what the cache lacks become
/// the supply-chain reason; an ordinary compile error does not.
#[test]
fn a_missing_crate_is_a_supply_chain_failure_and_a_type_error_is_not() {
    let missing =
        "error: no matching package named `zq` found\nlocation searched: crates.io index\n\
                   As a reminder, you're using offline mode (--offline) which can sometimes cause \
                   surprising resolution failures";
    let http = "error: failed to get `zq` as a dependency\nCaused by: attempting to make an HTTP \
                request, but --offline was specified";
    let newer = "error: no matching package named `zq` found\nnote: offline mode (via `--offline`) can sometimes cause surprising resolution failures";
    for log in [missing, http, newer] {
        let why = crate::sandbox::supply_chain(log).expect(log);
        assert!(why.contains("fetches nothing"), "{why}");
    }
    assert!(crate::sandbox::supply_chain("error[E0308]: mismatched types").is_none());
}
