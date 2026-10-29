//! A self branch's code in L1 (theseus-pw1q.21.1; RSI loop 1, §4.2), through
//! the real `theseusd` job wrapper and L1 init, over the view
//! `Sandbox::self_build_view` gives a self build (`Jail::prepare` builds
//! exactly what the daemon's gate runs in):
//!
//! - an `rw` bind persists and nothing else does: a job writes into the tree,
//!   its build tree, a workspace root, HOME and `/tmp`, and only the first
//!   two are on the host afterwards;
//! - a planted `build.rs` cannot see the sealed dir or the store (placed, on
//!   purpose, inside a bound root, so only the view's hidden list hides them),
//!   while a control file beside them is readable;
//! - a tree whose `Cargo.toml` names a crate the cache lacks fails, saying
//!   it was refused and not fetched;
//! - a tiny crate's gate (`cargo test` of a two-file crate) passes.
//!
//! Every job here runs in a real L1 sandbox, unprivileged. L1 refuses a root
//! daemon's jobs (theseus-pv6i), so as root each test only checks that
//! refusal; run them as an ordinary user (this VM: `unshare -U
//! --map-user=1000 --map-group=1000 <test binary>`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_core::rsi::jail::{Jail, Job};
use theseus_core::rsi::tree::{Place, Tree};
use theseus_kernel::job::{self, WrapperArgs};

fn root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

struct Rig {
    dir: tempfile::TempDir,
    place: Place,
    tree: Tree,
    jail: Jail,
    spool: theseus_kernel::Spool,
}

impl Rig {
    /// A repo with a store and a sealed set inside it (a view that hid
    /// nothing would show both), a tree and a build tree beside it, and the
    /// daemon's own sandbox over the floor those make.
    fn new() -> Self {
        Self::on(None)
    }

    /// `new`, with the tree a checkout of the caller's (`tree/` otherwise).
    fn on(checkout: Option<&Path>) -> Self {
        Self::approving(checkout, &[])
    }

    /// `on`, with `approve` joined to the approve list the config carries, as
    /// relative paths under the rig's directory: no L1 view shows an
    /// approve-listed path, whatever binds it (theseus-cr481).
    fn approving(checkout: Option<&Path>, approve: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let p = |s: &str| dir.path().join(s);
        let repo = p("repo");
        for (path, body) in [
            ("repo/state/store/data", "STORE-SECRET"),
            ("repo/sealed/item", "SEALED-SECRET"),
            ("repo/control", "control"),
        ] {
            std::fs::create_dir_all(p(path).parent().unwrap()).unwrap();
            std::fs::write(p(path), body).unwrap();
        }
        for d in ["tree", "target", "spool"] {
            std::fs::create_dir_all(p(d)).unwrap();
        }
        let mut cfg = theseus_core::Config::example();
        cfg.self_improve.sealed.dir = Some(p("repo/sealed").to_string_lossy().into_owned());
        // Made before the sandbox, so the approve list's canonical path and
        // the bind the test passes are the same path under a temp directory.
        for a in approve {
            std::fs::create_dir_all(p(a)).unwrap();
        }
        cfg.tools
            .approve_paths
            .extend(approve.iter().map(|a| p(a).to_string_lossy().into_owned()));
        let sandbox = theseus_core::toolrun::sandbox_for(&cfg, &repo.join("state"));
        let tree = Tree {
            slug: "t".into(),
            branch: "self/t".into(),
            dir: checkout.map_or_else(|| p("tree"), Path::to_path_buf),
            target: p("target"),
        };
        let place = Place {
            repo,
            state: p("state"),
            env: Vec::new(),
            jail: None,
            notify: None,
            stop: None,
        };
        Self {
            spool: theseus_kernel::Spool::open(&p("spool")).unwrap(),
            jail: Jail::new(Arc::new(sandbox)),
            tree,
            place,
            dir,
        }
    }

    fn path(&self, s: &str) -> PathBuf {
        self.dir.path().join(s)
    }

    /// `argv` as the gate runs its commands: in the self-build view, with the
    /// self-build environment, by the real wrapper. The job's completion.
    fn run(&self, argv: &[&str]) -> Value {
        self.run_ro(argv, Vec::new())
    }

    /// `run`, with `ro` bound read-only beside it, as the door binds main's
    /// copy of the gate script for a self build's gate.
    ///
    /// One job to a rig: the correlation id is fixed, so a second call reads
    /// the first job's completion back out of the spool at once. A test that
    /// wants two commands puts both in one script.
    fn run_ro(&self, argv: &[&str], ro: Vec<PathBuf>) -> Value {
        let log = self.path("job.log");
        let job = Job {
            tree: &self.tree,
            ro,
            argv: argv.iter().map(OsString::from).collect(),
            env: Vec::new(),
            log: &log,
            limit: Duration::from_secs(600),
        };
        let (view, env) = self.jail.prepare(&self.place, &job);
        let args = WrapperArgs {
            spool_dir: self.spool.dir().to_path_buf(),
            correlation_id: "self-build".into(),
            deadline_ms: 600_000,
            notify_socket: None,
            argv: argv.iter().map(|s| (*s).to_string()).collect(),
            cwd: Some(self.tree.dir.clone()),
            env,
            umask: None,
            redact: vec![],
            output_max_bytes: job::DEFAULT_OUTPUT_MAX_BYTES,
            sandbox: Some(view),
            cgroup: None,
        };
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        job::spawn_detached(&theseusd, &[job::WRAPPER_MODE], &self.spool, &args).unwrap();
        let t0 = Instant::now();
        loop {
            if let Some(c) = self.spool.read_completion(&args.correlation_id).unwrap() {
                return c.detail.unwrap_or_default();
            }
            assert!(
                t0.elapsed() < Duration::from_secs(600),
                "no completion for {argv:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap_or_default()
    }
}

/// As root L1 refuses every job; that is all there is to check.
fn refused_as_root() -> bool {
    if !root() {
        return false;
    }
    assert!(theseus_sandbox::refused_here().is_some());
    eprintln!("skipped: L1 refuses a root daemon's jobs (theseus-pv6i); run as an ordinary user");
    true
}

/// The one place a job's writes persist is `rw`: the tree and its build tree.
/// Everything else it can write, a workspace root's overlay, HOME and `/tmp`,
/// is scratch, gone when the job ends.
#[test]
fn an_rw_bind_persists_and_nothing_else_does() {
    if refused_as_root() {
        return;
    }
    let r = Rig::new();
    let home = std::env::var("HOME").unwrap();
    let mark = ".theseus-rw-probe";
    let (repo, cargo, plain) = (
        r.path("repo").join(mark),
        Path::new(&home).join(".cargo").join(mark),
        Path::new(&home).join(mark),
    );
    let script = format!(
        "w() {{ if echo x > \"$1\" 2>/dev/null; then echo \"$1 ok\"; else echo \"$1 no\"; fi; }}\n\
         {{ w {t}/f; w {g}/f; w {repo}; w {cargo}; w {plain}; w /tmp/f; }} > {t}/results",
        t = r.tree.dir.display(),
        g = r.tree.target.display(),
        repo = repo.display(),
        cargo = cargo.display(),
        plain = plain.display(),
    );
    let d = r.run(&["sh", "-c", &script]);
    assert_eq!(d["exit_code"], 0, "{d}");
    let results = r.read("tree/results");
    // In the view every one of those writes works...
    assert_eq!(results.matches(" ok").count(), 6, "{results}");
    // ... and on the host only the tree's and the build tree's are there.
    assert_eq!(r.read("tree/f"), "x\n");
    assert_eq!(r.read("target/f"), "x\n");
    for gone in [&repo, &cargo, &plain] {
        let there = gone.exists();
        let _ = std::fs::remove_file(gone);
        assert!(!there, "{} persisted", gone.display());
    }
}

/// A `build.rs` is the branch's code, run by cargo as the gate builds. It
/// reads the sealed set's item and the store's data (both inside the repo, a
/// bound root, so only the view's hidden list covers them) and a control
/// file beside them. The control is readable; the other two are not there.
#[test]
fn a_planted_build_rs_cannot_see_the_sealed_dir() {
    if refused_as_root() {
        return;
    }
    let r = Rig::new();
    let repo = r.path("repo");
    std::fs::write(
        r.path("tree/Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(r.path("tree/src")).unwrap();
    std::fs::write(r.path("tree/src/lib.rs"), "").unwrap();
    let build_rs = r#"
fn main() {
    let tree = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut out = String::new();
    for (name, path) in [
        ("sealed", "@REPO@/sealed/item"),
        ("store", "@REPO@/state/store/data"),
        ("control", "@REPO@/control"),
    ] {
        let got = match std::fs::read_to_string(path) {
            Ok(t) => format!("read:{t}"),
            Err(e) => format!("err:{:?}", e.kind()),
        };
        out += &format!("{name}={got}\n");
    }
    for dir in ["@REPO@/sealed", "@REPO@/state/store"] {
        let n = std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0);
        out += &format!("entries {dir}={n}\n");
    }
    std::fs::write(format!("{tree}/probe.out"), out).unwrap();
}
"#
    .replace("@REPO@", &repo.to_string_lossy());
    std::fs::write(r.path("tree/build.rs"), build_rs).unwrap();
    let d = r.run(&["sh", "-c", "cargo build > build.log 2>&1"]);
    let log = r.read("tree/build.log");
    assert_eq!(d["exit_code"], 0, "{d}\n{log}");
    let seen = r.read("tree/probe.out");
    assert!(
        seen.contains("control=read:control"),
        "the view: {seen}\n{log}"
    );
    assert!(seen.contains("sealed=err:NotFound"), "{seen}");
    assert!(seen.contains("store=err:NotFound"), "{seen}");
    assert!(!seen.contains("SECRET"), "{seen}");
    assert!(
        seen.contains("/sealed=0") && seen.contains("state/store=0"),
        "{seen}"
    );
}

/// A self branch that adds a crate the local cache lacks fails its gate: no
/// network, cargo told `--offline`, and the reason says it was refused.
#[test]
fn a_new_crate_fails_offline() {
    if refused_as_root() {
        return;
    }
    let r = Rig::new();
    std::fs::write(
        r.path("tree/Cargo.toml"),
        "[package]\nname = \"wants\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [dependencies]\ntheseus-no-such-crate-zq = \"1\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(r.path("tree/src")).unwrap();
    std::fs::write(r.path("tree/src/lib.rs"), "").unwrap();
    let d = r.run(&["sh", "-c", "cargo build > build.log 2>&1"]);
    let log = r.read("tree/build.log");
    assert_ne!(d["exit_code"], 0, "{d}\n{log}");
    // Cargo's own word for it: it was told not to look, not merely unable to.
    assert!(
        log.contains("offline"),
        "cargo was not run --offline: {log}"
    );
    assert!(
        theseus_core::sandbox::supply_chain(&log).is_some(),
        "the supply-chain reason is missing from cargo's words: {log}"
    );
}

/// theseus-cr481, end to end, in a real jail: the gate script the door
/// writes out of main lives in the state dir's `self/gates`, which the
/// approve list names, so no L1 view shows it — and the self build's view
/// binds that very file read-only for the gate to run. Until the fix the
/// mask covered the bind, `bash` found no script, and the job exited 127 in
/// 0 s: that is how all 69 self branches died from 2026-10-20 05:28 on.
///
/// Here the real wrapper runs a real script in a real L1 sandbox. It is
/// written with the permissions the host has (0600 in a 0700 directory), so
/// a pass also rules out the hypothesis first recorded on the row — that the
/// read-only bind lost to the file's own mode. The script reads its own
/// bytes through `$0`, which proves the bind is readable and not merely
/// present, and writes into the tree, which proves it ran.
#[test]
fn a_jailed_gate_script_runs_though_its_directory_is_approve_listed() {
    if refused_as_root() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;

    let gates = "repo/state/self/gates";
    let r = Rig::approving(None, &[gates]);
    let gate = r.path(gates).join("gate-abc.sh");
    // One script, one job: it reads its own bytes, then asks the floor for
    // the sealed set's item and the store's data, which must still be gone.
    std::fs::write(
        &gate,
        format!(
            "set -eu\n\
             printf 'gate ran, %s bytes of me\\n' \"$(wc -c < \"$0\")\" > gate.out\n\
             for f in {repo}/sealed/item {repo}/state/store/data; do\n\
               if cat \"$f\" 2>/dev/null; then echo \"READ $f\"; else echo \"gone $f\"; fi\n\
             done > floor.out\n",
            repo = r.path("repo").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&gate, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(r.path(gates), std::fs::Permissions::from_mode(0o700)).unwrap();
    // The door's own path for it, as the view will bind it.
    let gate = gate.canonicalize().unwrap();
    let d = r.run_ro(&["bash", &gate.to_string_lossy()], vec![gate.clone()]);
    assert_ne!(
        d["exit_code"], 127,
        "the gate script was bound and still not visible, as in cr481: {d}"
    );
    assert_eq!(d["exit_code"], 0, "{d}\n{}", r.read("job.log"));
    let out = r.read("tree/gate.out");
    assert!(out.starts_with("gate ran, "), "the gate did not run: {out}");
    // It read its own text through the bind, not an empty file.
    let bytes = std::fs::metadata(&gate).unwrap().len();
    assert!(out.contains(&format!("{bytes} bytes")), "{out}");
    // And the floor is masked still, with the script's directory un-masked.
    let floor = r.read("tree/floor.out");
    assert_eq!(floor.matches("gone ").count(), 2, "{floor}");
    assert!(!floor.contains("SECRET"), "{floor}");
}

/// The gate of a tiny crate: `cargo test` of two files passes in the view,
/// with the target dir the tree's own.
#[test]
fn a_tiny_crates_gate_passes_in_the_self_build_view() {
    if refused_as_root() {
        return;
    }
    let r = Rig::new();
    std::fs::write(
        r.path("tree/Cargo.toml"),
        "[package]\nname = \"tiny\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(r.path("tree/src")).unwrap();
    std::fs::write(
        r.path("tree/src/lib.rs"),
        "pub fn add(a: u32, b: u32) -> u32 { a + b }\n\
         #[cfg(test)]\nmod tests;\n",
    )
    .unwrap();
    std::fs::write(
        r.path("tree/src/tests.rs"),
        "#[test]\nfn adds() { assert_eq!(super::add(2, 3), 5); }\n",
    )
    .unwrap();
    let d = r.run(&["sh", "-c", "nice -n 19 cargo test > test.log 2>&1"]);
    let log = r.read("tree/test.log");
    assert_eq!(d["exit_code"], 0, "{d}\n{log}");
    assert!(log.contains("test result: ok. 1 passed"), "{log}");
    assert!(
        r.path("target/debug").is_dir(),
        "the build tree is the target"
    );
}

/// The cockpit phase's tools (theseus-selom): the view carries the pinned node
/// and its npm, read-only, so the gate's `npm ci --offline` and `npm run test`
/// can run. The host's own node is not the pin's, and a host without the pinned
/// toolchain has nothing to check.
#[test]
fn the_pinned_node_and_its_npm_run_in_the_self_build_view() {
    if refused_as_root() {
        return;
    }
    let pinned = theseus_core::config::expand(&format!(
        "~/.nvm/versions/node/v{}",
        theseus_core::sandbox::NODE_PIN
    ));
    if !pinned.join("bin/node").is_file() {
        eprintln!("skipped: no pinned node at {}", pinned.display());
        return;
    }
    let r = Rig::new();
    let d = r.run(&[
        "sh",
        "-c",
        "node -v > node.out 2>&1; npm -v >> node.out 2>&1",
    ]);
    assert_eq!(d["exit_code"], 0, "{d}");
    let out = r.read("tree/node.out");
    let first = out.lines().next().unwrap_or("");
    assert_eq!(
        first,
        format!("v{}", theseus_core::sandbox::NODE_PIN),
        "{out}"
    );
    let npm = out.lines().nth(1).unwrap_or("");
    assert!(
        npm.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "{out}"
    );
}

/// The live check on a real checkout (ignored: it builds a workspace). Names a
/// git checkout in `THESEUS_SELF_BUILD_TREE` (its files are written, so a
/// clone, not a tree in use) and a command in `THESEUS_SELF_BUILD_CMD`
/// (default `cargo build -p theseus-store`; the whole gate is
/// `nice -n 19 bash scripts/gate.sh`), and runs it in the self-build view,
/// printing its seconds and the tail of its log. `cargo nextest run -p
/// theseusd --test self_build --run-ignored only --no-capture`, as an
/// ordinary user.
#[test]
#[ignore = "builds a real workspace"]
fn a_real_checkouts_command_runs_in_the_view() {
    if refused_as_root() {
        return;
    }
    let tree = std::env::var("THESEUS_SELF_BUILD_TREE").expect("THESEUS_SELF_BUILD_TREE");
    let cmd = std::env::var("THESEUS_SELF_BUILD_CMD")
        .unwrap_or_else(|_| "cargo build -p theseus-store".into());
    let r = Rig::on(Some(Path::new(&tree)));
    let t0 = Instant::now();
    let d = r.run(&[
        "sh",
        "-c",
        &format!("{cmd} > {}/self-build.log 2>&1", r.path("target").display()),
    ]);
    let log = r.read("target/self-build.log");
    let tail: Vec<&str> = log.lines().rev().take(25).collect();
    eprintln!(
        "{cmd}: exit {} in {:.0}s\n{}",
        d["exit_code"],
        t0.elapsed().as_secs_f64(),
        tail.into_iter().rev().collect::<Vec<_>>().join("\n")
    );
    assert_eq!(d["exit_code"], 0, "{d}");
}
