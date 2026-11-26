//! A self-improvement session's shell in a real L1 sandbox (issue 53), through the real `theseusd` job wrapper and
//! L1 init. Every command of a self-principal session is bound to L1 (`rsi::own_tree::l1_of`, proved in the core):
//! a reviewer's and a helper's get the view every L1 job gets (`Sandbox::job_view`), a builder's that view with its
//! one build folder (`Sandbox::builder_view`). Here those two views run real commands, which print what they did, so
//! a command that did not run fails the test:
//!
//! - `git push` to the self repository's remote sends nothing;
//! - a write to the self repository's git config, a tag in it, and git's user-level config do not persist to what
//!   the daemon's git reads;
//! - the daemon's socket is not there (no card can be answered from the job), nor the store, nor the sealed sets;
//! - git finds no credential, and HOME is empty;
//! - a builder's removal in its build folder persists, and its removal in its own tree does not (the tree is an
//!   overlay in L1; main's `self_build.rs::only_the_builders_folder_persists` says the same of a write).
//!
//! As root L1 refuses every job; run as an ordinary user.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use theseus_core::rsi::builder_dir;
use theseus_kernel::job::{self, WrapperArgs, L1};

fn root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

struct Rig {
    dir: tempfile::TempDir,
    sandbox: theseus_core::sandbox::Sandbox,
    spool: theseus_kernel::Spool,
    state: PathBuf,
    _socket: std::os::unix::net::UnixListener,
}

impl Rig {
    /// A workspace holding the self repository and its bare remote, a state dir with a store, a sealed set and the
    /// daemon's socket, a builder's tree and build folder, and the daemon's own sandbox over them.
    fn new() -> Self {
        let dir = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let p = |s: &str| dir.path().join(s);
        for d in ["work", "state/store", "work/sealed", "spool"] {
            std::fs::create_dir_all(p(d)).unwrap();
        }
        std::fs::write(p("state/store/data"), "STORE-SECRET").unwrap();
        std::fs::write(p("work/sealed/item"), "SEALED-SECRET").unwrap();
        let remote = p("work/remote.git");
        git(
            dir.path(),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ],
        );
        let repo = p("work/selfrepo");
        git(
            dir.path(),
            &["init", "-q", "-b", "main", repo.to_str().unwrap()],
        );
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "a"]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&repo, &["push", "-q", "origin", "main"]);
        let tree = p("state/self/trees/t");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join("notes.txt"), "notes\n").unwrap();
        let mut cfg = theseus_core::Config::example();
        cfg.server.state_dir = p("state").to_string_lossy().into_owned();
        cfg.tools.projects_dir = Some(p("work").to_string_lossy().into_owned());
        cfg.tools.roots = vec![];
        cfg.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
        cfg.self_improve.repo = Some(repo.to_string_lossy().into_owned());
        cfg.self_improve.sealed.dir = Some(p("work/sealed").to_string_lossy().into_owned());
        let sandbox = theseus_core::toolrun::sandbox_for(&cfg);
        // The daemon's socket, inside the workspace so only the hidden list covers it, as the daemon names it.
        let sock = p("work/theseus.sock");
        let socket = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        sandbox.hide(sock);
        Self {
            spool: theseus_kernel::Spool::open(&p("spool")).unwrap(),
            state: p("state"),
            sandbox,
            _socket: socket,
            dir,
        }
    }

    fn path(&self, s: &str) -> PathBuf {
        self.dir.path().join(s)
    }

    /// `script` run by the real wrapper in `view`, in the self repository; what it printed.
    fn run(&self, id: &str, view: L1, script: &str) -> String {
        let env: Vec<(String, String)> = [
            ("SOCK", "work/theseus.sock"),
            ("STORE", "state/store/data"),
            ("SEALED", "work/sealed/item"),
            ("TREEFILE", "state/self/trees/t/notes.txt"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), self.path(v).display().to_string()))
        .chain([
            (
                "FOLDERFILE".into(),
                builder_dir::dir_in(&self.state, "t")
                    .join("out.o")
                    .display()
                    .to_string(),
            ),
            ("GIT_TERMINAL_PROMPT".into(), "0".into()),
            ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
        ])
        .collect();
        let args = WrapperArgs {
            spool_dir: self.spool.dir().to_path_buf(),
            correlation_id: id.into(),
            deadline_ms: 120_000,
            notify_socket: None,
            argv: vec!["sh".into(), "-c".into(), script.into()],
            cwd: Some(self.path("work/selfrepo")),
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
            if let Some(c) = self.spool.read_completion(id).unwrap() {
                let out = self.spool.dir().join("results").join(format!("{id}.out"));
                let text = std::fs::read_to_string(out).unwrap_or_default();
                assert!(
                    text.contains("ran=yes"),
                    "{id}: the commands did not run: {:?}\n{text}",
                    c.detail
                );
                return text;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(120),
                "{id}: no completion"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// What a self session's shell tries, each line `key=value`.
const TRIES: &str = r#"echo ran=yes
git push -q origin HEAD:refs/heads/around-the-card >/dev/null 2>&1; echo push=$?
git config remote.origin.pushurl /tmp/elsewhere >/dev/null 2>&1; echo config=$?
git tag around-the-card >/dev/null 2>&1; echo tag=$?
git config --global url./tmp/elsewhere.pushInsteadOf origin >/dev/null 2>&1; echo global=$?
if [ -S "$SOCK" ]; then echo sock=yes; else echo sock=no; fi
if [ -e "$STORE" ]; then echo store=yes; else echo store=no; fi
if [ -e "$SEALED" ]; then echo sealed=yes; else echo sealed=no; fi
echo home=$(ls -A "$HOME" | wc -l)
printf 'protocol=https\nhost=github.com\n\n' | git credential fill >/dev/null 2>&1; echo cred=$?
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then echo gh=in; else echo gh=out; fi
rm -f "$TREEFILE"; echo rmtree=$?
rm -f "$FOLDERFILE"; echo rmfolder=$?
"#;

fn said(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("no {key}= in:\n{text}"))
        .to_string()
}

/// A reviewer's, a helper's and a builder's shell: what each tries runs, and none of it reaches the remote, the
/// daemon's git, its socket, its store, its sealed sets or a credential.
#[test]
fn a_self_sessions_shell_in_l1_reaches_nothing_of_the_daemons() {
    if root() {
        assert!(theseus_sandbox::refused_here().is_some());
        eprintln!("skipped: L1 refuses a root daemon's jobs; run as an ordinary user");
        return;
    }
    let r = Rig::new();
    let folder = builder_dir::ensure(&r.state, "t").unwrap();
    std::fs::write(folder.join("out.o"), "built\n").unwrap();
    let home_config = PathBuf::from(std::env::var("HOME").unwrap()).join(".gitconfig");
    let home_before = std::fs::read(&home_config).ok();
    let views = [
        ("reviewer", r.sandbox.job_view()),
        ("helper", r.sandbox.job_view()),
        ("builder", r.sandbox.builder_view(&folder)),
    ];
    for (who, view) in views {
        let text = r.run(who, view, TRIES);
        for (key, want) in [
            ("sock", "no"),
            ("store", "no"),
            ("sealed", "no"),
            ("home", "0"),
            ("gh", "out"),
        ] {
            assert_eq!(said(&text, key), want, "{who}: {key}\n{text}");
        }
        assert_ne!(
            said(&text, "cred"),
            "0",
            "{who}: git found no credential\n{text}"
        );
        let remote = r.path("work/remote.git");
        let refs = git(&remote, &["for-each-ref", "--format=%(refname)"]);
        assert_eq!(
            refs, "refs/heads/main",
            "{who}: the push sent nothing\n{text}"
        );
        let repo = r.path("work/selfrepo");
        let config = std::fs::read_to_string(repo.join(".git/config")).unwrap();
        assert!(
            !config.contains("elsewhere"),
            "{who}: the config write did not persist\n{text}"
        );
        assert!(
            git(&repo, &["tag", "--list"]).is_empty(),
            "{who}: no tag persisted\n{text}"
        );
        assert_eq!(
            std::fs::read(&home_config).ok(),
            home_before,
            "{who}: the user's git config is unchanged"
        );
        assert!(
            r.path("state/self/trees/t/notes.txt").exists(),
            "{who}: the tree is an overlay in L1"
        );
    }
    // The builder's own folder is its one read-write bind: its removal there persisted.
    assert!(
        !folder.join("out.o").exists(),
        "the builder's removal in its build folder persisted"
    );
}
