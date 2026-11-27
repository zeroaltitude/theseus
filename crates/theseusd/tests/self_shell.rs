//! A self-improvement session's shell in a real L1 sandbox (issue 53), through the real `theseusd` job wrapper and
//! L1 init. Every command of a self-principal session is bound to L1 (`rsi::own_tree::l1_of`, proved in the core).
//! Each view here is made by the assembly `toolrun::job` gives a real call (`Sandbox::self_view`): a helper's is the
//! view every L1 job gets, a reviewer's that view with the daemon's checkout read-only, a builder's that view with its
//! build folder and its own tree read-write and the tree's `.git` file read-only. The views run real commands, which
//! print each command's own status, and every status is asserted:
//!
//! - `git push` to the self repository's remote receives nothing on the host;
//! - a write to the self repository's git config, a tag in it, and git's user-level config do not persist to what
//!   the daemon's git reads;
//! - the daemon's socket is not there (no card can be answered from the job), nor the store, nor the sealed sets,
//!   nor the live config, nor the owner's proof;
//! - a git credential and a `gh` token planted where the daemon's user keeps them are not reachable, git finds no
//!   credential, and HOME is empty;
//! - a builder's removal in its build folder and in its own tree persists; a reviewer's and a helper's in the tree
//!   does not (an overlay);
//! - a builder cannot write, remove, rename, replace or link over its tree's `.git`, nor rename the tree.
//!
//! As root L1 refuses every job; run as an ordinary user.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use theseus_core::rsi::builder_dir;
use theseus_core::sandbox::{Sandbox, SelfPlaces};
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
        for d in [
            "work",
            "state/store",
            "work/sealed",
            "spool",
            "home/.config/gh",
        ] {
            std::fs::create_dir_all(p(d)).unwrap();
        }
        std::fs::write(p("state/store/data"), "STORE-SECRET").unwrap();
        std::fs::write(p("work/sealed/item"), "SEALED-SECRET").unwrap();
        // Where the daemon's user keeps a git credential and a `gh` token, under the HOME the job is given.
        std::fs::write(
            p("home/.git-credentials"),
            "https://fixture:FAKE-GIT-CREDENTIAL@github.com\n",
        )
        .unwrap();
        std::fs::write(p("home/.gitconfig"), "[credential]\n\thelper = store\n").unwrap();
        std::fs::write(
            p("home/.config/gh/hosts.yml"),
            "github.com:\n    oauth_token: FAKE-GH-TOKEN\n",
        )
        .unwrap();
        // The live config, inside the workspace so only the floor hides it, and the owner's proof.
        std::fs::write(p("work/theseus.toml"), "LIVE-CONFIG-SECRET\n").unwrap();
        std::fs::write(
            p("state").join(theseus_core::owner_proof::FILE),
            "OWNER-PROOF-SECRET\n",
        )
        .unwrap();
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
        // The builder's tree, a worktree of the self repository as `rsi::tree::open` makes one.
        let tree = p("state/self/trees/t");
        std::fs::create_dir_all(tree.parent().unwrap()).unwrap();
        git(
            &repo,
            &["worktree", "add", "-q", "-b", "t", tree.to_str().unwrap()],
        );
        std::fs::write(tree.join("notes.txt"), "notes\n").unwrap();
        let mut cfg = theseus_core::Config::example();
        cfg.server.state_dir = p("state").to_string_lossy().into_owned();
        cfg.tools.projects_dir = Some(p("work").to_string_lossy().into_owned());
        cfg.tools.roots = vec![];
        cfg.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
        cfg.self_improve.repo = Some(repo.to_string_lossy().into_owned());
        cfg.self_improve.sealed.dir = Some(p("work/sealed").to_string_lossy().into_owned());
        cfg.config_file = Some(p("work/theseus.toml"));
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
            ("HOME", "home"),
            ("LIVECONF", "work/theseus.toml"),
            ("PROOF", "state/owner-proof.json"),
            ("GITCRED", "home/.git-credentials"),
            ("GHTOKEN", "home/.config/gh/hosts.yml"),
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
if [ -e "$LIVECONF" ]; then echo liveconf=yes; else echo liveconf=no; fi
if [ -e "$PROOF" ]; then echo proof=yes; else echo proof=no; fi
if [ -e "$GITCRED" ]; then echo gitcred=yes; else echo gitcred=no; fi
if [ -e "$GHTOKEN" ]; then echo ghtoken=yes; else echo ghtoken=no; fi
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
    let tree = r.path("state/self/trees/t");
    let checkout = r.path("state/self/review/br-one/tree");
    std::fs::create_dir_all(&checkout).unwrap();
    let views = [
        (
            "reviewer",
            SelfPlaces {
                review: Some(checkout),
                ..SelfPlaces::default()
            },
        ),
        ("helper", SelfPlaces::default()),
        (
            "builder",
            SelfPlaces {
                folder: Some(folder.clone()),
                tree: Some(tree.clone()),
                review: None,
            },
        ),
    ];
    for (who, places) in views {
        std::fs::write(tree.join("notes.txt"), "notes\n").unwrap();
        let view = Sandbox::self_view(r.sandbox.job_view(), &places);
        let text = r.run(who, view, TRIES);
        // Each command's own status: what ran in the view succeeded there, and the host shows none of it.
        for (key, want) in [
            ("push", "0"),
            ("config", "0"),
            ("tag", "0"),
            ("global", "0"),
            ("sock", "no"),
            ("store", "no"),
            ("sealed", "no"),
            ("liveconf", "no"),
            ("proof", "no"),
            ("gitcred", "no"),
            ("ghtoken", "no"),
            ("home", "0"),
            ("gh", "out"),
            ("rmtree", "0"),
            ("rmfolder", "0"),
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
        assert_eq!(
            tree.join("notes.txt").exists(),
            who != "builder",
            "{who}: the builder's own tree is bound read-write; for the others it is an overlay"
        );
    }
    // The builder's own folder is bound read-write: its removal there persisted.
    assert!(
        !folder.join("out.o").exists(),
        "the builder's removal in its build folder persisted"
    );
}

/// STOP 1 (issue 53): a reviewer's L1 view binds the daemon's checkout of the reviewed head read-only, by the
/// assembly `toolrun::job` gives a real call (`Sandbox::self_view`): the reviewer reads it, and a write there fails and leaves nothing on the host.
#[test]
fn a_reviewers_view_reads_the_daemons_checkout_and_cannot_write_it() {
    if root() {
        assert!(theseus_sandbox::refused_here().is_some());
        return;
    }
    let r = Rig::new();
    let tree = r.path("state/self/review/br-one/tree");
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::write(tree.join("seen.txt"), "the reviewed head\n").unwrap();
    let places = SelfPlaces {
        review: Some(tree.clone()),
        ..SelfPlaces::default()
    };
    let view = Sandbox::self_view(r.sandbox.job_view(), &places);
    let script = format!(
        "echo ran=yes\necho read=$(cat '{t}/seen.txt')\nif echo x > '{t}/new' 2>/dev/null; then echo write=ok; else echo write=no; fi\n",
        t = tree.display()
    );
    let text = r.run("reviewer-checkout", view, &script);
    assert_eq!(said(&text, "read"), "the reviewed head", "{text}");
    assert_eq!(said(&text, "write"), "no", "{text}");
    assert!(!tree.join("new").exists(), "nothing written on the host");
}

/// Pin (b) (issue 53): in a builder's view its tree is read-write and its `.git` file is not: the file cannot be
/// written, removed, renamed away, replaced by a rename or turned into a link, and the tree cannot be renamed; a
/// file written beside it persists, and the `.git` file on the host is the one git made. A write to the
/// repository's git directory, where the tree's `.git` points, and a commit made in the view do not persist (an
/// overlay): the head the daemon reads, reviews and gates is its own commit's (pin (e)).
#[test]
fn a_builder_cannot_change_its_trees_dot_git_or_move_its_tree() {
    if root() {
        assert!(theseus_sandbox::refused_here().is_some());
        return;
    }
    let r = Rig::new();
    let folder = builder_dir::ensure(&r.state, "t").unwrap();
    let tree = r.path("state/self/trees/t");
    let dot = std::fs::read(tree.join(".git")).unwrap();
    let admin = String::from_utf8(dot.clone()).unwrap();
    let admin = PathBuf::from(admin.trim().strip_prefix("gitdir: ").unwrap());
    let head_before = std::fs::read(admin.join("HEAD")).unwrap();
    let branch_before = git(&tree, &["rev-parse", "t"]);
    let places = SelfPlaces {
        folder: Some(folder),
        tree: Some(tree.clone()),
        review: None,
    };
    let view = Sandbox::self_view(r.sandbox.job_view(), &places);
    let script = format!(
        r#"echo ran=yes
t='{t}'
echo x > "$t/.git" 2>/dev/null; echo write=$?
rm -f "$t/.git" 2>/dev/null; echo remove=$?
mv "$t/.git" "$t/moved" 2>/dev/null; echo rename=$?
echo 'gitdir: /tmp/mine' > "$t/new.git"; mv -f "$t/new.git" "$t/.git" 2>/dev/null; echo replace=$?
ln -sfn /tmp "$t/.git" 2>/dev/null; echo link=$?
mv "$t" "$t.moved" 2>/dev/null; echo treemove=$?
echo kept > "$t/beside"; echo beside=$?
echo 'ref: refs/heads/elsewhere' > '{a}/HEAD' 2>/dev/null; echo admin=$?
git -C "$t" -c user.name=b -c user.email=b@example.invalid commit -q --allow-empty -m moved >/dev/null 2>&1; echo commit=$?
"#,
        t = tree.display(),
        a = admin.display()
    );
    let text = r.run("builder-dot-git", view, &script);
    for key in ["write", "remove", "rename", "replace", "link", "treemove"] {
        assert_ne!(said(&text, key), "0", "{key}\n{text}");
    }
    assert_eq!(said(&text, "beside"), "0", "{text}");
    assert_eq!(said(&text, "admin"), "0", "{text}");
    assert_eq!(said(&text, "commit"), "0", "{text}");
    assert_eq!(
        std::fs::read(tree.join(".git")).unwrap(),
        dot,
        "the .git file is git's"
    );
    assert!(!tree.join("moved").exists() && !r.path("state/self/trees/t.moved").exists());
    assert_eq!(
        std::fs::read_to_string(tree.join("beside")).unwrap(),
        "kept\n"
    );
    assert_eq!(
        std::fs::read(admin.join("HEAD")).unwrap(),
        head_before,
        "the git directory is an overlay"
    );
    assert_eq!(
        git(&tree, &["rev-parse", "t"]),
        branch_before,
        "a commit in the view does not move the branch the daemon reads"
    );
}
