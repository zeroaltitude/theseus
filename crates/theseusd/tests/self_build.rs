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
//! - a tiny crate's gate (`cargo test` of a two-file crate) passes;
//! - a self builder's own command (`Sandbox::self_view`, as `toolrun::job`
//!   assembles it) keeps two directories, its item's build folder and its
//!   own tree, and a second `cargo test` in the folder reuses the first's
//!   build; nothing else it writes persists, and the gate's view neither sees
//!   that folder nor builds in it;
//! - a limit above what the process that starts the job may give (its own hard
//!   limit, as when the whole suite runs inside a self build's view) is set as
//!   high as it may be, the job starts, and its record says what it got;
//! - the guard: this file's other tests, run again from inside a self build's
//!   view as the join runs the suite, must pass there too (`the_tests_of_this_file_pass_inside_a_self_builds_view`).
//!
//! Every job here runs in a real L1 sandbox, unprivileged. L1 refuses a root
//! daemon's jobs (theseus-pv6i), so as root each test only checks that
//! refusal; run them as an ordinary user (this VM: `unshare -U
//! --map-user=1000 --map-group=1000 <test binary>`).

use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_core::rsi::builder_dir;
use theseus_core::rsi::jail::{Jail, Job};
use theseus_core::rsi::tree::{Place, Tree};
use theseus_kernel::job::{self, WrapperArgs};

/// A temporary directory that is on disk. `/tmp` is memory (a tmpfs) inside a self build's view,
/// where the join runs this suite, and a rig whose builder folder is under it could not tell that
/// the folder is not on the memory-backed mount, which is what `a_builders_temp_files_land_on_disk_in_its_folder`
/// asks. The build tree (`CARGO_TARGET_DIR`) is the place the view binds read-write on disk; cargo's
/// own scratch place for these tests (`CARGO_TARGET_TMPDIR`) is where a run outside the view has one.
fn on_disk() -> tempfile::TempDir {
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from);
    std::fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}

/// How long a job of `run` may take: 600 s, and `THESEUS_SELF_BUILD_JOB_SECS` when it is set, for the
/// flake hunt (`scripts/flake-hunt.sh --view`), whose one job is a cold build and then every suite run.
fn job_secs() -> u64 {
    std::env::var("THESEUS_SELF_BUILD_JOB_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600)
}

fn root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

struct Rig {
    sandbox: Arc<theseus_core::sandbox::Sandbox>,
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
        Self::hiding(checkout, &[])
    }

    /// `on`, with `hidden` put on the floor, as relative paths under the
    /// rig's directory: no L1 view shows a floor path, whatever binds it
    /// (theseus-cr481). They went on the approve list until the owner retired
    /// it (2026-11-03, theseus-c4i8m); the floor is the only config-driven
    /// hiding left, and the token file and the config copy are its two slots
    /// a test can point anywhere.
    fn hiding(checkout: Option<&Path>, hidden: &[&str]) -> Self {
        Self::made(checkout, hidden, false)
    }

    /// A rig as the daemon has it while `[self]` builds: the state dir is
    /// outside the repo, the self trees under it are a workspace root, and the
    /// tree and its build tree are where `tree::open` puts them, with the
    /// builder's folder beside them.
    fn builder() -> Self {
        Self::made(None, &[], true)
    }

    fn made(checkout: Option<&Path>, hidden: &[&str], building: bool) -> Self {
        let dir = on_disk();
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
        let (tree_dir, target) = if building {
            let (t, g) = (p("state/self/trees/t"), p("state/self/target/t"));
            std::fs::create_dir_all(&t).unwrap();
            std::fs::create_dir_all(&g).unwrap();
            (t.canonicalize().unwrap(), g.canonicalize().unwrap())
        } else {
            (p("tree"), p("target"))
        };
        let mut cfg = theseus_core::Config::example();
        // The rig's state dir, on the config, which is now the only way to
        // say it: `sandbox_for` takes no directory (theseus-0h9f3, the
        // owner's word of 2026-11-11 11:34). This was a second argument to
        // that call, and `[server].state_dir` stayed at its default, so the
        // floor this rig built was over the rig's `repo/state` and the
        // config's state dir at once -- exactly the drift the parameter made
        // possible.
        cfg.server.state_dir = if building {
            p("state")
        } else {
            repo.join("state")
        }
        .to_string_lossy()
        .into_owned();
        cfg.self_improve.sealed.dir = Some(p("repo/sealed").to_string_lossy().into_owned());
        if building {
            cfg.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
            cfg.self_improve.repo = Some(repo.to_string_lossy().into_owned());
        }
        // Made before the sandbox, so the floor's canonical path and the bind
        // the test passes are the same path under a temp directory.
        assert!(hidden.len() <= 2, "the floor has two slots a test can set");
        for a in hidden {
            std::fs::create_dir_all(p(a)).unwrap();
        }
        cfg.op_token_file = hidden.first().map(|a| p(a));
        cfg.config_copy = hidden.get(1).map(|a| p(a));
        let sandbox = theseus_core::toolrun::sandbox_for(&cfg);
        let tree = Tree {
            slug: "t".into(),
            branch: "self/t".into(),
            dir: checkout.map_or(tree_dir, Path::to_path_buf),
            target,
        };
        let place = Place {
            repo,
            state: p("state"),
            env: Vec::new(),
            jail: None,
            notify: None,
            stop: None,
            owner: None,
        };
        let sandbox = Arc::new(sandbox);
        Self {
            spool: theseus_kernel::Spool::open(&p("spool")).unwrap(),
            jail: Jail::new(sandbox.clone()),
            sandbox,
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
            limit: Duration::from_secs(job_secs()),
        };
        let (view, env) = self.jail.prepare(&self.place, &job);
        let args = WrapperArgs {
            spool_dir: self.spool.dir().to_path_buf(),
            correlation_id: "self-build".into(),
            deadline_ms: job_secs() * 1000,
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
                t0.elapsed() < Duration::from_secs(job_secs()),
                "no completion for {argv:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// `argv` as a self builder's own `proc.run` runs it: the builder's view
    /// over this item's build folder and its own tree (`Sandbox::self_view`),
    /// cargo pointed at the folder, by the real wrapper.
    /// `id` names the job (one completion each). The job's completion.
    fn run_as_builder(&self, id: &str, argv: &[&str]) -> Value {
        let state = self.place.state.clone();
        let folder = builder_dir::ensure(&state, &self.tree.slug).unwrap();
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        let mut env = theseus_core::sandbox::self_build_env(&home, &folder);
        for (k, v) in theseus_core::sandbox::builder_env(&folder) {
            env.retain(|(ek, _)| *ek != k);
            env.push((k, v));
        }
        let args = WrapperArgs {
            spool_dir: self.spool.dir().to_path_buf(),
            correlation_id: id.into(),
            deadline_ms: 600_000,
            notify_socket: None,
            argv: argv.iter().map(|s| (*s).to_string()).collect(),
            cwd: Some(self.tree.dir.clone()),
            env,
            umask: None,
            redact: vec![],
            output_max_bytes: job::DEFAULT_OUTPUT_MAX_BYTES,
            sandbox: Some(theseus_core::sandbox::Sandbox::self_view(
                self.sandbox.job_view(),
                &theseus_core::sandbox::SelfPlaces {
                    folder: Some(folder.clone()),
                    tree: Some(self.tree.dir.clone()),
                    review: None,
                },
            )),
            cgroup: None,
        };
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        job::spawn_detached(&theseusd, &[job::WRAPPER_MODE], &self.spool, &args).unwrap();
        let t0 = Instant::now();
        loop {
            if let Some(c) = self.spool.read_completion(id).unwrap() {
                return c.detail.unwrap_or_default();
            }
            assert!(
                t0.elapsed() < Duration::from_secs(600),
                "no completion for {argv:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The folder `run_as_builder` builds in.
    fn folder(&self) -> PathBuf {
        builder_dir::dir_in(&self.place.state, &self.tree.slug)
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
/// writes out of main lives in the state dir's `self/gates`, a directory no
/// L1 view shows — and the self build's view binds that very file read-only
/// for the gate to run. The approve list named that directory until the
/// owner retired it (2026-11-03, theseus-c4i8m); the rig puts it on the
/// floor, which hides it the same way, and the filter under test is the
/// view's and is unchanged. Until the fix the
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
fn a_jailed_gate_script_runs_though_its_directory_is_hidden() {
    if refused_as_root() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;

    let gates = "repo/state/self/gates";
    let r = Rig::hiding(None, &[gates]);
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

/// The live check of issue 20 on a real checkout (ignored: it builds the core's tests). Clones the
/// git checkout named by `THESEUS_SELF_BUILD_TREE` into the builder rig's tree, runs
/// `THESEUS_SELF_BUILD_CMD` (default `cargo test -p theseus-core --no-run`) twice as a builder
/// does, and prints each run's seconds, what it compiled, and what the item's build folder holds
/// after. `cargo nextest run -p theseusd --test self_build --run-ignored only --no-capture`, as
/// an ordinary user.
#[test]
#[ignore = "builds a real workspace"]
fn a_builders_core_test_build_completes_in_its_folder_and_is_reused() {
    if refused_as_root() {
        return;
    }
    let src = std::env::var("THESEUS_SELF_BUILD_TREE").expect("THESEUS_SELF_BUILD_TREE");
    let cmd = std::env::var("THESEUS_SELF_BUILD_CMD")
        .unwrap_or_else(|_| "cargo test -p theseus-core --no-run".into());
    let r = Rig::builder();
    let cloned = std::process::Command::new("git")
        .args(["clone", "-q", &src])
        .arg(&r.tree.dir)
        .status()
        .unwrap();
    assert!(cloned.success(), "cloning {src}");
    let f = r.folder();
    // The build twice, then a run of tests from it (no network, no store, no sealed set).
    let run = std::env::var("THESEUS_SELF_BUILD_RUN")
        .unwrap_or_else(|_| "cargo test -p theseus-core --lib rsi::capacity".into());
    let mut compiled = Vec::new();
    for (id, log, cmd) in [
        ("one", "one.log", cmd.clone()),
        ("two", "two.log", cmd),
        ("three", "three.log", run),
    ] {
        let t0 = Instant::now();
        let d = r.run_as_builder(
            id,
            &["sh", "-c", &format!("{cmd} > {}/{log} 2>&1", f.display())],
        );
        let said = std::fs::read_to_string(f.join(log)).unwrap_or_default();
        let tail: Vec<&str> = said.lines().rev().take(12).collect();
        let n = said.lines().filter(|l| l.contains("Compiling")).count();
        eprintln!(
            "{id}: {cmd}: exit {} in {:.0}s, {n} crates compiled, the folder holds {:.2} GB\n{}",
            d["exit_code"],
            t0.elapsed().as_secs_f64(),
            builder_dir::size(&f) as f64 / f64::from(1u32 << 30),
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        );
        if d["exit_code"] != 0 {
            let why: Vec<&str> = said
                .lines()
                .filter(|l| {
                    ["error", "signal", "No space", "limit"]
                        .iter()
                        .any(|w| l.contains(w))
                })
                .take(12)
                .collect();
            // A long line is read from its end: that is where rustc says how it died.
            let ends: Vec<String> = why
                .iter()
                .map(|l| {
                    l.chars()
                        .rev()
                        .take(160)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect()
                })
                .collect();
            eprintln!("{d}\nwhat it said:\n{}", ends.join("\n"));
        }
        assert_eq!(d["exit_code"], 0, "{id} failed");
        compiled.push(n);
    }
    assert!(
        compiled[0] > 0 && compiled[1] == 0,
        "the second run reuses the first: {compiled:?}"
    );
    let ran = std::fs::read_to_string(f.join("three.log")).unwrap_or_default();
    assert!(ran.contains("test result: ok"), "the tests ran: {ran}");
}

/// The tiny crate of `a_tiny_crates_gate_passes_in_the_self_build_view`, in the rig's tree.
fn tiny_crate(r: &Rig) {
    let t = &r.tree.dir;
    std::fs::create_dir_all(t.join("src")).unwrap();
    std::fs::write(
        t.join("Cargo.toml"),
        "[package]\nname = \"tiny\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        t.join("src/lib.rs"),
        "pub fn add(a: u32, b: u32) -> u32 { a + b }\n#[cfg(test)]\nmod tests;\n",
    )
    .unwrap();
    std::fs::write(
        t.join("src/tests.rs"),
        "#[test]\nfn adds() { assert_eq!(super::add(2, 3), 5); }\n",
    )
    .unwrap();
}

/// KEY (issue 20): a builder's `cargo test` builds inside its sandbox, into the item's build
/// folder, and a second run there reuses what the first built: the first compiles the crate, the
/// second does not. Its commands cannot write the tree, so the logs go in the folder as well.
#[test]
fn a_builders_tests_build_in_its_folder_and_a_second_run_reuses_the_build() {
    if refused_as_root() {
        return;
    }
    let r = Rig::builder();
    tiny_crate(&r);
    let f = r.folder();
    for (id, log) in [("first", "first.log"), ("second", "second.log")] {
        let script = format!("cargo test > {}/{log} 2>&1", f.display());
        let d = r.run_as_builder(id, &["sh", "-c", &script]);
        let said = r.read(&format!("state/self/builder/t/{log}"));
        assert_eq!(d["exit_code"], 0, "{d}\n{said}");
        assert!(said.contains("test result: ok. 1 passed"), "{said}");
    }
    let (first, second) = (
        r.read("state/self/builder/t/first.log"),
        r.read("state/self/builder/t/second.log"),
    );
    assert!(first.contains("Compiling tiny"), "{first}");
    assert!(!second.contains("Compiling tiny"), "reused: {second}");
    assert!(
        f.join("target/debug").is_dir(),
        "the build is on the host, in the folder"
    );
    assert!(builder_dir::size(&f) > 0);
    assert!(!r.tree.dir.join("target").exists(), "none in the tree");
}

/// The lint a self builder is told to run before it proposes (`rsi::backlog::brief`, rule 4) works
/// where it builds: clippy is in the pinned toolchain, runs offline into the item's folder, and a
/// warning fails it, as in the gate.
#[test]
fn a_builders_lint_runs_in_its_folder_and_a_warning_fails_it() {
    if refused_as_root() {
        return;
    }
    let r = Rig::builder();
    tiny_crate(&r);
    let f = r.folder();
    let lint = |id: &str| {
        let script = format!(
            "cargo clippy -p tiny --all-targets -- -D warnings > {}/{id}.log 2>&1",
            f.display()
        );
        let d = r.run_as_builder(id, &["sh", "-c", &script]);
        (
            d["exit_code"].clone(),
            r.read(&format!("state/self/builder/t/{id}.log")),
        )
    };
    let (code, said) = lint("clean");
    assert_eq!(code, 0, "{said}");
    std::fs::write(
        r.tree.dir.join("src/lib.rs"),
        "pub fn add(a: u32, b: u32) -> u32 { a + b }\n\
         pub fn flag(a: bool) -> bool { if a == true { true } else { false } }\n\
         #[cfg(test)]\nmod tests;\n",
    )
    .unwrap();
    let (code, said) = lint("warned");
    assert_ne!(code, 0, "{said}");
    assert!(said.contains("bool_comparison"), "{said}");
}

/// What persists is the folder and the builder's own tree, and nothing else: its writes to HOME and
/// `/tmp` are gone when the command ends, its build tree for the gate is not writable from its view
/// at all, and its tree's `.git` file is not writable either.
#[test]
fn only_the_builders_folder_and_its_own_tree_persist() {
    if refused_as_root() {
        return;
    }
    let r = Rig::builder();
    let (t, g, f) = (r.tree.dir.clone(), r.tree.target.clone(), r.folder());
    let script = format!(
        "w() {{ if echo x > \"$1\" 2>/dev/null; then echo \"$1 ok\"; else echo \"$1 no\"; fi; }}\n\
         {{ w {f}/kept; w {t}/kept-in-tree; w {t}/.git; w {g}/gates; w $HOME/lost; w /tmp/lost; }} > {f}/results",
        f = f.display(),
        t = t.display(),
        g = g.display(),
    );
    let d = r.run_as_builder("writes", &["sh", "-c", &script]);
    assert_eq!(d["exit_code"], 0, "{d}");
    let results = r.read("state/self/builder/t/results");
    assert!(
        results.contains(&format!("{}/kept ok", f.display())),
        "{results}"
    );
    assert!(
        results.contains(&format!("{}/gates no", g.display())),
        "the gate's build tree: {results}"
    );
    assert_eq!(r.read("state/self/builder/t/kept"), "x\n");
    assert!(
        results.contains(&format!("{}/.git no", t.display())),
        "the tree's .git file: {results}"
    );
    assert_eq!(
        std::fs::read_to_string(t.join("kept-in-tree")).unwrap(),
        "x\n",
        "the builder's own tree is bound read-write"
    );
    assert!(
        std::fs::read_to_string(t.join(".git"))
            .unwrap()
            .starts_with("gitdir: "),
        "the .git file is git's"
    );
    assert!(
        !g.join("gates").exists(),
        "the gate's build tree is not written"
    );
}

/// The gate never reads what the builder compiled: a marker the builder leaves in its folder is
/// not there in the view the gate runs in (it binds the tree and its own build tree and neither
/// holds the folder), and the gate's build tree is untouched.
#[test]
fn the_gates_view_cannot_see_what_the_builder_left() {
    if refused_as_root() {
        return;
    }
    let r = Rig::builder();
    let f = r.folder();
    let script = format!("echo built-by-a-model > {}/artifact.rlib", f.display());
    let d = r.run_as_builder("compiles", &["sh", "-c", &script]);
    assert_eq!(d["exit_code"], 0, "{d}");
    assert!(f.join("artifact.rlib").is_file());
    let probe = format!(
        "if [ -e {a} ] || [ -e {f} ]; then echo seen > seen; else echo absent > seen; fi",
        a = f.join("artifact.rlib").display(),
        f = f.display()
    );
    let d = r.run(&["sh", "-c", &probe]);
    assert_eq!(d["exit_code"], 0, "{d}");
    assert_eq!(r.read("state/self/trees/t/seen"), "absent\n");
    assert!(
        std::fs::read_dir(&r.tree.target).unwrap().next().is_none(),
        "the gate's build tree holds nothing the builder made"
    );
}

/// The builder's temporary files land on disk, in its folder (`TMPDIR`), and not in the memory-backed
/// `/tmp` (tmpfs, 4 GiB): `mktemp` writes under the folder's `tmp`, where the file is kept, and the
/// view's `/tmp` is a tmpfs of the size the constant says while `tmp` is not on it.
#[test]
fn a_builders_temp_files_land_on_disk_in_its_folder() {
    if refused_as_root() {
        return;
    }
    let r = Rig::builder();
    let f = r.folder();
    let script = format!(
        "t=$(mktemp); echo x > \"$t\"; echo \"$t\" > {f}/name; \
         stat -f -c '%T %b %S' /tmp > {f}/tmpfs; stat -f -c %T {f}/tmp > {f}/tmpdir",
        f = f.display()
    );
    let d = r.run_as_builder("temp", &["sh", "-c", &script]);
    assert_eq!(d["exit_code"], 0, "{d}");
    let name = r.read("state/self/builder/t/name");
    let tmp = f.join("tmp");
    assert!(
        Path::new(name.trim()).starts_with(&tmp),
        "{name} is under {}",
        tmp.display()
    );
    assert!(Path::new(name.trim()).is_file(), "it is on the host: disk");
    let tmpfs = r.read("state/self/builder/t/tmpfs");
    let mut it = tmpfs.split_whitespace();
    let (kind, blocks, size) = (it.next(), it.next(), it.next());
    assert_eq!(kind, Some("tmpfs"), "/tmp is the memory-backed mount");
    let bytes = blocks.unwrap().parse::<u64>().unwrap() * size.unwrap().parse::<u64>().unwrap();
    assert_eq!(bytes, 4 * 1024 * 1024 * 1024, "4 GiB, the constant");
    assert_ne!(
        r.read("state/self/builder/t/tmpdir").trim(),
        "tmpfs",
        "the folder's tmp is disk"
    );
}

/// Set on the re-run of the test below, whose own hard limits are lowered.
const LOWERED_RUN: &str = "THESEUS_TEST_LIMITS_LOWERED";
const LOWERED_TEST: &str = "a_limit_above_the_processs_own_is_set_as_high_as_it_may_be_and_said";

/// Lowers this process's hard limit on `resource` to at most `to`, its soft limit with it. Only
/// system calls, as a child between its fork and its exec may make.
fn lower_hard(resource: i32, to: u64) -> std::io::Result<()> {
    let mut r: libc::rlimit = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrlimit(resource as _, &mut r) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    r.rlim_max = r.rlim_max.min(to as libc::rlim_t);
    r.rlim_cur = r.rlim_cur.min(r.rlim_max);
    match unsafe { libc::setrlimit(resource as _, &r) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// This process's hard limit on `resource`.
fn hard(resource: i32) -> u64 {
    let mut r: libc::rlimit = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::getrlimit(resource as _, &mut r) }, 0);
    r.rlim_max as u64
}

/// KEY (issue 20): the builder's view asks for 32,768 processes and a 16 GiB file, and a process
/// may not raise a hard limit above its own, so a job started from a process with less (the whole
/// suite, run inside a self build's view, has 8,192 processes) could not start at all. It starts,
/// with each limit set as high as the process may set it, and its record says what it was asked
/// and what it got. Needs no special view: the test runs itself again as a child whose own hard
/// limits it has lowered first, so nothing leaks into any other test.
#[test]
fn a_limit_above_the_processs_own_is_set_as_high_as_it_may_be_and_said() {
    if refused_as_root() {
        return;
    }
    if std::env::var_os(LOWERED_RUN).is_none() {
        let mut inner = std::process::Command::new(std::env::current_exe().unwrap());
        inner
            .args(["--exact", LOWERED_TEST, "--nocapture"])
            .env(LOWERED_RUN, "1");
        // SAFETY: the closure makes system calls only (`lower_hard`).
        unsafe {
            inner.pre_exec(|| {
                lower_hard(libc::RLIMIT_NPROC as i32, 16 * 1024)?;
                lower_hard(libc::RLIMIT_FSIZE as i32, 4 << 30)
            })
        };
        let out = inner.output().unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && said.contains("1 passed"),
            "{said}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return;
    }
    let r = Rig::builder();
    let f = r.folder();
    let wanted = r.sandbox.builder_view(&f).limits;
    let (pids, fsize) = (
        hard(libc::RLIMIT_NPROC as i32),
        hard(libc::RLIMIT_FSIZE as i32),
    );
    assert!(
        pids < wanted.pids,
        "the view asks for more processes than this run has"
    );
    assert!(fsize < wanted.output_mb << 20, "and for a larger file");
    let script = format!("cat /proc/self/limits > {}/limits", f.display());
    let d = r.run_as_builder("lowered", &["sh", "-c", &script]);
    assert_eq!(d["exit_code"], 0, "{d}");
    // The record: each limit lowered, with what was asked and what it got.
    assert_eq!(
        d["sandbox"]["lowered"],
        serde_json::json!([
            {"limit": "output_mb", "asked": wanted.output_mb, "got": fsize >> 20},
            {"limit": "pids", "asked": wanted.pids, "got": pids},
        ]),
        "{d}"
    );
    // And what the job had: both the soft and the hard limit are what the record says.
    let limits = r.read("state/self/builder/t/limits");
    let row = |name: &str| -> Vec<u64> {
        let line = limits
            .lines()
            .find(|l| l.starts_with(name))
            .unwrap_or_default();
        line[name.len()..]
            .split_whitespace()
            .take(2)
            .map(|v| v.parse().unwrap())
            .collect()
    };
    assert_eq!(row("Max processes"), [pids, pids], "{limits}");
    assert_eq!(row("Max file size"), [fsize, fsize], "{limits}");
}

/// GUARD (issue 20): the join runs main's whole suite inside a self build's view, and a test that
/// passes unjailed can fail there (four of this file's did, on a limit the view's own process could
/// not raise, and no change could join). So this file's other tests are run once more from inside
/// that view, the one `Jail::prepare` gives the gate, by this very test binary (bound read-only,
/// with the daemon's binary the tests start their jobs with), and every one must pass there. It
/// covers THIS file's tests alone: the other test binaries that start jobs are not run in the
/// view by it, and the whole of the join's run is still the join's own. It skips itself inside a
/// view (the suite runs these tests there already) and as root (L1 refuses root).
#[test]
fn the_tests_of_this_file_pass_inside_a_self_builds_view() {
    if refused_as_root() {
        return;
    }
    if std::env::var_os("THESEUS_VIEW_GRANTS").is_some() {
        eprintln!("skipped: already inside a self build's view, where the suite runs these tests");
        return;
    }
    let me = "the_tests_of_this_file_pass_inside_a_self_builds_view";
    let exe = std::env::current_exe().unwrap();
    // The tests this binary has, less the ones `#[ignore]`d and this one: what must pass.
    let count = |flags: &[&str]| {
        let listed = std::process::Command::new(&exe)
            .args(["--list", "--format", "terse"])
            .args(flags)
            .output()
            .unwrap();
        String::from_utf8_lossy(&listed.stdout)
            .lines()
            .filter(|l| l.ends_with(": test"))
            .count()
    };
    let must_pass = (count(&[]) - count(&["--ignored"])).saturating_sub(1);
    assert!(must_pass > 0, "this file's tests were not listed");
    let r = Rig::new();
    let script = format!(
        "{} --skip {me} --test-threads 4 > {}/guard.out 2>&1",
        exe.display(),
        r.tree.dir.display()
    );
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let d = r.run_ro(&["sh", "-c", &script], vec![exe, theseusd]);
    let said = r.read("tree/guard.out");
    let tail = &said[said.len().saturating_sub(6000)..];
    assert_eq!(d["exit_code"], 0, "{d}\n{tail}");
    assert!(
        said.contains(&format!("{must_pass} passed; 0 failed")),
        "every other test passed in the view: {tail}"
    );
}
