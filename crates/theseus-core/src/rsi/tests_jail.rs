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

/// **The invariant the whole grant rests on: only `self_build_view` grants**
/// (theseus-1eemp). `Sandbox::job_view` is the single source for both job
/// families that are not self builds — `proc.run`'s L1 jobs, which clone it
/// in `for_job`, and every L1 MCP server — so one literal there is the
/// boundary for both, and nothing but that literal held it before this test.
/// A merge resolution that carried `self_build::FACILITIES`
/// onto `job_view` would hand every model-proposed job a pty, the extra
/// device nodes, the mount calls and an unmasked `/proc`, and the rest of
/// the suite would stay green. The exam runner is asserted for the same
/// reason and is the sharpest case of it: its subject is the sealed set the
/// floor hides.
///
/// The grants are spelled out rather than compared against `FACILITIES` on
/// purpose — the point is to pin the decided value, so that flipping the
/// constant reds here and must be done deliberately. `visible_proc` is on by
/// the owner's decision of 2026-11-11 16:32; if the 1.0.6 line finds
/// `nesting` suffices and takes it back off, this test is the edit that
/// records it.
#[test]
fn only_the_self_build_view_carries_facility_grants() {
    let sb = sandbox();
    assert_eq!(
        sb.job_view().facilities,
        theseus_sandbox::Facilities::NONE,
        "an ordinary L1 job (and every L1 MCP server) must be granted nothing"
    );
    let view = sb.self_build_view(&build(&[], Path::new("/home/op")));
    assert_eq!(
        view.facilities,
        theseus_sandbox::Facilities {
            pty: true,
            devices: true,
            nesting: true,
            visible_proc: true,
        },
        "the self-build view carries exactly the four decided grants"
    );
    let exam = super::sealed::runner::runner_view(&view, Path::new("/exam"), "relay:80");
    assert_eq!(
        exam.facilities,
        theseus_sandbox::Facilities::NONE,
        "an exam runner inherits no grant from the self-build view it is cut from"
    );
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

/// The cockpit phase of the gate needs node and npm (`cockpit_modules`, then
/// `npm run test`), and the host's own are a node 12 and no npm: the pinned
/// toolchain is bound read-only at its own path, and only that one version's
/// directory, not the operator's whole `~/.nvm` (theseus-selom). It adds no
/// writable bind and no network.
#[test]
fn the_view_binds_the_pinned_node_toolchain_read_only_and_nothing_more_of_nvm() {
    let sb = sandbox();
    let gate = PathBuf::from("/state/self/gates/gate-abc.sh");
    let ro = [gate];
    let view = sb.self_build_view(&build(&ro, Path::new("/home/op")));
    let node = PathBuf::from(format!(
        "/home/op/.nvm/versions/node/v{}",
        crate::sandbox::NODE_PIN
    ));
    assert!(view.ro_paths.contains(&node), "{:?}", view.ro_paths);
    let nvm: Vec<&PathBuf> = view
        .ro_paths
        .iter()
        .chain(&view.workspace)
        .chain(&view.rw)
        .filter(|p| p.starts_with("/home/op/.nvm"))
        .collect();
    assert_eq!(nvm, [&node], "one version of nvm's, and read-only");
    assert!(view.egress.is_empty(), "still no network");
    assert_eq!(
        view.rw,
        [
            PathBuf::from("/state/self/trees/t"),
            PathBuf::from("/state/self/target/t")
        ],
        "nothing writable was gained"
    );
}

/// The pinned toolchain's `bin` comes before the system's: the host's own
/// `/usr/bin/node` is old enough that the cockpit's tests do not run on it.
#[test]
fn the_pinned_node_comes_first_on_a_self_builds_path() {
    let env = crate::sandbox::self_build_env(Path::new("/home/op"), Path::new("/t/target"));
    let path = env
        .iter()
        .find(|(n, _)| n == "PATH")
        .map(|(_, v)| v.as_str());
    let want = format!(
        "/home/op/.nvm/versions/node/v{}/bin:",
        crate::sandbox::NODE_PIN
    );
    assert!(path.unwrap().starts_with(&want), "{path:?}");
}

/// A hidden entry can name the whole state directory (`~/.theseus`), which
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
        &[
            PathBuf::from("/state/store"),
            PathBuf::from("/state/spool"),
            PathBuf::from("/state"),
            PathBuf::from("/home/op/.ssh"),
        ],
    );
    let gate = PathBuf::from("/state/self/gates/gate-abc.sh");
    let ro = [gate];
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

/// The security review of the fix above (theseus-cr481), against the approve
/// list the operator's config actually carries: not the whole state directory
/// but its subdirectories one by one, `self/gates` among them — and
/// `self/trees` and `self/target` deliberately off it. So exactly one mask
/// covers a bind, and the set the self build loses must be exactly that one.
/// Every other floor and approve path is named here, the sealed sets with
/// them, so a later widening of the filter fails this test by name.
#[test]
fn a_self_builds_view_drops_the_one_mask_over_a_bind_and_no_other() {
    let p = PathBuf::from;
    // The floor: the store, the spool, the bindings, the vault's token, and
    // the sealed holdout sets (`rsi::sealed::Guard::paths`).
    let floor = [
        p("/state/store"),
        p("/state/spool"),
        p("/state/bindings.toml"),
        p("/home/op/.config/op"),
        p("/srv/theseus-sealed"),
        p("/srv/theseus-sealed/behaviour"),
    ];
    // More of the hidden set, in the shape the operator's state directory has
    // it. These were the approve list's until 1.0.4 retired it; the masking
    // this test pins is the view's, whatever puts a path in the hidden set.
    let hidden_too = [
        p("/state/self/gates"),
        p("/state/1password-service-token"),
        p("/state/config.last-good.toml"),
        p("/state/durability"),
        p("/state/index"),
        p("/state/learning"),
        p("/state/mcp-server-key"),
        p("/state/outputs"),
        p("/state/persona"),
        p("/state/personas"),
        p("/state/projects"),
        p("/state/theseus.sock"),
        p("/home/op/.ssh"),
        p("/home/op/.gnupg"),
        p("/home/op/.aws"),
    ];
    // The tree and the build tree are out of the hidden set on purpose, so no
    // mask ever covered them: the gate script's directory was the only one.
    for off in ["/state/self/trees", "/state/self/target"] {
        assert!(!hidden_too.contains(&p(off)), "{off} is hidden");
    }
    let all: Vec<PathBuf> = floor.iter().chain(&hidden_too).cloned().collect();
    let sb = Sandbox::new(
        &crate::sandbox::SandboxConfig::default(),
        &[p("/work")],
        &all,
    );
    let gate = p("/state/self/gates/gate-abc.sh");
    let ro = [gate.clone()];
    let view = sb.self_build_view(&build(&ro, Path::new("/home/op")));
    // The whole of it: the self build's view loses the gate's directory and
    // nothing else at all.
    let base = sb.job_view().hidden;
    let dropped: Vec<&PathBuf> = base.iter().filter(|h| !view.hidden.contains(h)).collect();
    assert_eq!(dropped, [&p("/state/self/gates")], "{base:?}");
    // And each of them said by name, the sealed sets among them.
    for kept in floor.iter().chain(&hidden_too[1..]) {
        assert!(view.hidden.contains(kept), "{}", kept.display());
    }
    // The bind the mask was covering is there, and reachable.
    assert!(view.ro_paths.contains(&gate));
    // Nothing writable was gained: the tree and its build tree, as before.
    assert_eq!(
        view.rw,
        [p("/state/self/trees/t"), p("/state/self/target/t")]
    );
}

/// The filter is narrow in the two ways a mask could be lost by accident: a
/// mask equal to a bind is kept (only a bind *below* a mask drops it), and
/// the prefix is matched by whole path components, so a directory whose name
/// merely starts with a masked one never unmasks it.
#[test]
fn only_a_bind_below_a_mask_drops_it_and_the_prefix_is_component_wise() {
    let p = PathBuf::from;
    let sb = Sandbox::new(
        &crate::sandbox::SandboxConfig::default(),
        &[p("/work")],
        &[
            // Equal to a bind: the repo is a workspace overlay in the view.
            p("/repo"),
            // One component short of the gate's directory.
            p("/state/self/gate"),
            // One component short of the tree.
            p("/state/self/tree"),
        ],
    );
    let ro = [p("/state/self/gates/gate-abc.sh")];
    let view = sb.self_build_view(&build(&ro, Path::new("/home/op")));
    for kept in ["/repo", "/state/self/gate", "/state/self/tree"] {
        assert!(view.hidden.contains(&p(kept)), "{kept}: {:?}", view.hidden);
    }
    // An ordinary job's view is untouched by any of this.
    assert_eq!(sb.job_view().hidden, view.hidden);
}

/// An operator's `[sandbox] ro_paths` entry is config the filter has not
/// seen: one that points inside a masked directory must not un-mask the
/// directory for a self build (theseus-i3h47). Only the view's own binds,
/// the gate among them, drop a mask.
#[test]
fn a_configured_ro_path_inside_a_mask_does_not_unmask_it_in_a_self_build() {
    let p = PathBuf::from;
    let cfg = crate::sandbox::SandboxConfig {
        ro_paths: vec!["/state/store/shared".into()],
        ..Default::default()
    };
    let sb = Sandbox::new(&cfg, &[p("/work")], &[p("/state/store")]);
    let ro = [p("/state/self/gates/gate-abc.sh")];
    let view = sb.self_build_view(&build(&ro, Path::new("/home/op")));
    // The entry is still bound, and the directory above it still hidden.
    assert!(view.ro_paths.contains(&p("/state/store/shared")));
    let state: Vec<&PathBuf> = view
        .hidden
        .iter()
        .filter(|h| h.starts_with("/state"))
        .collect();
    assert_eq!(state, [&p("/state/store")]);
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
    assert!(get("PATH").unwrap().contains("/home/op/.cargo/bin:"));
    // What this bar was built for, and still is: nothing of the daemon's own
    // state and no secret. Two `THESEUS_` names are allowed BY NAME, and only
    // these two — they carry the gate's honesty line about the view the run
    // was judged in (theseus-1eemp), hold no path of the daemon's and no
    // credential, and a reader of a self run's tail is owed both. An
    // allowlist, so a third appearing fails here rather than riding in behind
    // them.
    const TOLD: [&str; 2] = ["THESEUS_VIEW_GRANTS", "THESEUS_VIEW_WITHHELD"];
    let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    assert!(
        names
            .iter()
            .all(|n| (!n.starts_with("THESEUS_") || TOLD.contains(n)) && !n.contains("TOKEN")),
        "{names:?}"
    );
    // And what the two say, so they are a contract rather than two strings.
    assert_eq!(
        get("THESEUS_VIEW_GRANTS"),
        Some("a self build's, granting pty, devices, nesting, visible_proc")
    );
    // AND THE SECOND IS A SET, NOT A SENTENCE -- which is this assertion's
    // whole subject, and the reason it was rewritten on 2026-11-12.
    //
    // It used to read `withheld.contains("4 tests no grant reaches")`: it
    // asserted that the gate SAID a sentence, so the sentence satisfied it
    // whatever the run did. Nothing filtered on that variable -- `gate.sh`
    // appended it to the verdict's skipped list inside `summary()`, after
    // every test had already run -- and on three self builds (2026-11-11
    // 23:24, 2026-11-11 23:59, 2026-11-12 05:31) the sentence was false of
    // ALL FOUR tests it named: the L1-vs-L0 probe and the self-mint fixture
    // tier ran and FAILED, and were two of each run's own survivors, while
    // the two `term::tests_keep` tests ran and PASSED in about 24 s and 35 s.
    // A test that a string can satisfy is what let that live for three runs.
    //
    // So this asserts the MECHANISM instead: that the value is a set in the
    // form the gate turns into a real nextest exclusion, and that the gate
    // still does turn it into one.
    let withheld = get("THESEUS_VIEW_WITHHELD").unwrap();
    let members: Vec<&str> = withheld.lines().collect();
    assert!(!members.is_empty(), "{withheld:?}");
    for m in &members {
        // `<binary id><SP><test name>`, exactly what nextest prints, so one
        // string drives the filter, names the test in the verdict, and is
        // what a reader greps the run's log for.
        let (bid, test) = m
            .split_once(' ')
            .unwrap_or_else(|| panic!("not `<binary id> <test name>`: {m:?}"));
        assert!(!bid.is_empty() && !test.is_empty(), "{m:?}");
        assert!(!test.contains(' '), "a test name, never prose: {m:?}");
    }
    // The three the owner's answer of 2026-11-12 10:41:48 MST covers, and the
    // two the old sentence already CLAIMED were skipped and were not.
    for named in [
        "theseus-sandbox::contract clause_04_no_localhost_services",
        "theseus-sandbox::contract clause_11_not_granted_is_denied",
        "theseus-sandbox::facilities a_visible_proc_grant_takes_the_masks_off_and_nothing_else_does",
        "theseusd::sandbox a_probe_script_in_l1_shows_the_contract_and_l0_the_contrast",
        "theseus-core rpc::tests_self_mint::a_validation_runs_its_tiers_writes_one_row_and_loads_nothing",
    ] {
        assert!(members.contains(&named), "{named} is not in {members:?}");
    }
    // AND THE TWO TERM TESTS ARE NOT IN IT, measured rather than assumed:
    // they ran and PASSED in all three self builds. A passing test in a skip
    // set is cover given up for nothing, and the owner's answer says in terms
    // that no fourth test is covered by it.
    assert!(
        !withheld.contains("tests_keep"),
        "the term close-and-fork tests run and pass inside the view: {withheld:?}"
    );
    // And the gate really does exclude them. `scripts/gate.sh` is the other
    // half of this mechanism and lives outside this crate, so read its source
    // -- the same way `every_file_the_keel_guard_holds_is_in_a_class` reads
    // the keel guard's. A `gate.sh` that stopped filtering would leave this
    // value a claim again, which is precisely the state being fixed.
    let gate = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/gate.sh"),
    )
    .unwrap();
    assert!(
        gate.contains("view_withheld_filter") && gate.contains("binary_id(=$bid) & test(=$tname)"),
        "gate.sh no longer builds a nextest exclusion from THESEUS_VIEW_WITHHELD"
    );
    assert!(
        gate.contains("view_withheld_check"),
        "gate.sh no longer checks that the tests it calls withheld were absent from the run"
    );
    assert!(
        !gate.contains("4 tests no grant reaches"),
        "the hand-written sentence is back in gate.sh"
    );
    // Neither may ever carry a path OF THE OPERATOR'S: the gate echoes both
    // into a log a self run pushes. `/proc` is a kernel name and is fine; a
    // home, a state dir or a tree is not.
    for n in TOLD {
        let v = get(n).unwrap();
        for bad in ["/home/", "/root", ".theseus", "/tmp/", "$"] {
            assert!(
                !v.contains(bad),
                "{n} must name facilities, never one of the operator's paths: {v:?}"
            );
        }
    }
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
        notify: None,
        stop: None,
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
