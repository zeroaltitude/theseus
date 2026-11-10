use super::*;
use crate::config::{ProgramGrant, SecretPosture};

/// A directory of stand-in programs: each an executable script.
fn bin(names: &[&str]) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    for n in names {
        let p = d.path().join(n);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    d
}

fn broker(bin: &Path, board: Arc<SecretBoard>, postures: &[(&str, Posture)]) -> Broker {
    let mut cfg = BrokerConfig::default();
    cfg.programs.insert(
        "gh".into(),
        ProgramGrant {
            env: [("GH_TOKEN".to_string(), "github_token".to_string())].into(),
            aws_account: None,
        },
    );
    for (n, p) in postures {
        cfg.secrets
            .insert(n.to_string(), SecretPosture { posture: *p });
    }
    Broker::new(
        &cfg,
        board,
        Some(format!("{}:/usr/bin:/bin", bin.display())),
    )
    .with_wait(Duration::from_millis(200))
}

fn ready(pairs: &[(&str, &str)]) -> Arc<SecretBoard> {
    let board = SecretBoard::new(pairs.iter().map(|(n, _)| n.to_string()), Instant::now());
    board.publish(
        pairs
            .iter()
            .map(|(n, v)| (n.to_string(), Ok(Secret::new(v.to_string()))))
            .collect(),
        "test",
    );
    board
}

fn argv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// Direct argv only: `gh`, or its own path, gets the variable; a shell,
/// `env`, or a `gh` elsewhere gets nothing, and the note says why.
#[tokio::test]
async fn a_program_gets_its_variable_only_by_its_own_argv() {
    let b = bin(&["gh"]);
    let other = bin(&["gh"]);
    let br = broker(b.path(), ready(&[("github_token", "tok-123456")]), &[]);
    let path = format!("{}:/usr/bin:/bin", b.path().display());
    let cwd = other.path();
    let gh = b.path().join("gh").display().to_string();
    for direct in [argv(&["gh", "api", "user"]), argv(&[&gh, "api"])] {
        let j = br
            .for_job(&direct, &[], cwd, Some(&path), Posture::Notify)
            .await;
        assert_eq!(j.env.len(), 1, "{direct:?}");
        assert_eq!(
            (j.env[0].0.as_str(), j.env[0].1.expose()),
            ("GH_TOKEN", "tok-123456")
        );
        assert_eq!(got(&j.granted).as_deref(), Some("gh got GH_TOKEN"));
        assert!(j.note().is_none());
    }
    for (indirect, says) in [
        (
            argv(&["sh", "-c", "gh api user"]),
            "it is run by `sh`, not by its own argv",
        ),
        (argv(&["env", "gh", "api"]), "it is run by `env`"),
        (
            argv(&["bash", "-lc", "cd x && /usr/bin/gh pr list"]),
            "it is run by `bash`",
        ),
        (
            argv(&["./gh", "api"]),
            "`./gh` is not the gh on the daemon's PATH",
        ),
    ] {
        let j = br
            .for_job(&indirect, &[], cwd, Some(&path), Posture::Notify)
            .await;
        assert!(j.env.is_empty(), "{indirect:?}");
        let note = j.note().unwrap();
        assert!(note.starts_with("[gh got no GH_TOKEN: "), "{note}");
        assert!(note.contains(says), "{note}");
    }
    // A PATH the model set, with its own gh first, is not the daemon's.
    let evil = format!("{}:{path}", other.path().display());
    let j = br
        .for_job(
            &argv(&["gh", "api"]),
            &[],
            cwd,
            Some(&evil),
            Posture::Notify,
        )
        .await;
    assert!(j.env.is_empty());
    // A program the broker does not know, naming none, gets no note.
    let j = br
        .for_job(
            &argv(&["ls", "-la"]),
            &[],
            cwd,
            Some(&path),
            Posture::Notify,
        )
        .await;
    assert!(j.env.is_empty() && j.note().is_none());
}

/// A secret that has not settled is waited for, bounded, and then
/// withheld, never replaced; one that settles within the wait is given.
/// A failed one is withheld at once, with its reason.
#[tokio::test]
async fn an_unresolved_secret_is_waited_for_then_withheld() {
    let b = bin(&["gh"]);
    let path = format!("{}:/usr/bin:/bin", b.path().display());
    let board = SecretBoard::new(["github_token".to_string()], Instant::now());
    let br = broker(b.path(), board.clone(), &[]);
    let t0 = Instant::now();
    let j = br
        .for_job(
            &argv(&["gh", "api"]),
            &[],
            b.path(),
            Some(&path),
            Posture::Notify,
        )
        .await;
    assert!(t0.elapsed() >= Duration::from_millis(200), "it waited");
    assert!(j.env.is_empty());
    let note = j.note().unwrap();
    assert!(
        note.contains(
            "gh got no GH_TOKEN: github_token was still resolving after 200 ms; it ran \
             without it"
        ),
        "{note}"
    );
    let later = board.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        later.publish(
            [("github_token".to_string(), Ok(Secret::new("tok".into())))].into(),
            "test",
        );
    });
    let j = br
        .for_job(
            &argv(&["gh", "api"]),
            &[],
            b.path(),
            Some(&path),
            Posture::Notify,
        )
        .await;
    assert_eq!(j.env.len(), 1, "given once it resolved");
    let failed = SecretBoard::new(["github_token".to_string()], Instant::now());
    failed.publish(
        [("github_token".to_string(), Err("vault said no".to_string()))].into(),
        "test",
    );
    let br = broker(b.path(), failed, &[]);
    let j = br
        .for_job(
            &argv(&["gh", "api"]),
            &[],
            b.path(),
            Some(&path),
            Posture::Notify,
        )
        .await;
    assert!(j
        .note()
        .unwrap()
        .contains("github_token did not resolve (vault said no)"));
}

/// A secret's posture holds the call to it: the gate gets the strictest
/// posture and its setting, and a spawn at a looser posture than the
/// secret's withholds it.
#[tokio::test]
async fn a_secrets_posture_is_the_least_a_call_given_it_runs_at() {
    let b = bin(&["gh"]);
    let path = format!("{}:/usr/bin:/bin", b.path().display());
    let br = broker(
        b.path(),
        ready(&[("github_token", "tok")]),
        &[("github_token", Posture::Approve)],
    );
    let grants = br.at_gate(
        "proc.run",
        Some(&argv(&["gh", "api"])),
        &[],
        b.path(),
        Some(&path),
    );
    assert_eq!(gets(&grants).as_deref(), Some("gh gets GH_TOKEN"));
    assert_eq!(
        br.need(&grants),
        Some((
            Posture::Approve,
            "[broker.secrets.github_token] posture = approve".into()
        ))
    );
    let j = br
        .for_job(
            &argv(&["gh", "api"]),
            &[],
            b.path(),
            Some(&path),
            Posture::Notify,
        )
        .await;
    assert!(j.env.is_empty());
    assert!(j
        .note()
        .unwrap()
        .contains("github_token's posture is approve, and this call ran at notify"));
    let j = br
        .for_job(
            &argv(&["gh", "api"]),
            &[],
            b.path(),
            Some(&path),
            Posture::Approve,
        )
        .await;
    assert_eq!(j.env.len(), 1);
    let (p, setting) = broker(b.path(), ready(&[]), &[]).posture_of("github_token");
    assert_eq!(p, Posture::Notify, "{setting}");
}

/// A toollet gets only what the wiring granted it, and each use counts;
/// health lists every grant by name, never a value.
#[tokio::test]
async fn a_toollet_gets_only_its_grant_and_health_counts_uses() {
    let b = bin(&["gh"]);
    let br = broker(
        b.path(),
        ready(&[("github_token", "tok"), ("search_key", "key-9")]),
        &[],
    );
    br.grant_tool("test.keyed", "search_key");
    br.settle_for_tool("test.keyed").await;
    let v = br
        .secret_for_tool("test.keyed", "search_key", Posture::Notify)
        .unwrap();
    assert_eq!(v.expose(), "key-9");
    assert!(br
        .secret_for_tool("test.keyed", "github_token", Posture::Notify)
        .unwrap_err()
        .contains("was not granted github_token"));
    assert!(br
        .secret_for_tool("fs.read", "search_key", Posture::Open)
        .is_err());
    let s = br.status();
    let show = serde_json::to_string(&s).unwrap();
    assert!(
        !show.contains("key-9") && !show.contains("\"tok\""),
        "{show}"
    );
    let row = |to: &str| s.iter().find(|g| g.to == to).unwrap().clone();
    assert_eq!(
        (row("test.keyed").kind.as_str(), row("test.keyed").uses),
        ("tool", 1)
    );
    assert_eq!(
        (
            row("gh").variable.as_deref(),
            row("gh").secret.as_str(),
            row("gh").posture.as_str()
        ),
        (Some("GH_TOKEN"), "github_token", "notify")
    );
}

/// Review 2's H7: a launcher's grant reaches only the program the call
/// names. gh gets it for its own commands, never an alias, an extension,
/// `--web`'s browser, or a program in git's flags after `--`; git for its
/// commands that reach a remote, never an alias, `-c`, or a program that
/// an option or a URL names; and no program gets it from a call that
/// sets its own environment. The gate's view agrees with the spawn's.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn a_launchers_grant_reaches_only_the_program_the_call_names() {
    let b = bin(&["gh", "git"]);
    let mut cfg = BrokerConfig::default();
    for program in ["gh", "git"] {
        cfg.programs.insert(
            program.into(),
            ProgramGrant {
                env: [("GH_TOKEN".to_string(), "github_token".to_string())].into(),
                aws_account: None,
            },
        );
    }
    let path = format!("{}:/usr/bin:/bin", b.path().display());
    let br = Broker::new(
        &cfg,
        ready(&[("github_token", "tok-123456")]),
        Some(path.clone()),
    );
    let given: [&[&str]; 9] = [
        &["gh", "pr", "list"],
        &["gh", "api", "user", "--jq", ".login"],
        &["gh", "repo", "clone", "invented/x", "--", "--depth=1"],
        &["git", "push", "-u", "origin", "main"],
        &["git", "-C", "sub", "--no-pager", "fetch", "--prune"],
        &["git", "--git-dir=x/.git", "pull", "--rebase"],
        &["git", "clone", "--depth", "1", "ssh://git.invented/x.git"],
        &["git", "clone", "git@git.invented:x.git"],
        &["git", "ls-remote", "origin"],
    ];
    for a in given {
        let j = br
            .for_job(&argv(a), &[], b.path(), Some(&path), Posture::Notify)
            .await;
        // A git given the secret says its hooks are off (theseus-ur1t).
        let pinned = (a[0] == "git").then(|| {
            "[git ran with its hooks and core.fsmonitor off, since it got GH_TOKEN]".to_string()
        });
        assert_eq!(
            (j.env.len(), j.git_pinned, j.note()),
            (1, true, pinned),
            "{a:?}"
        );
        let at_gate = br.at_gate("proc.run", Some(&argv(a)), &[], b.path(), Some(&path));
        assert_eq!(at_gate.len(), 1, "{a:?}");
    }
    let withheld: [(&[&str], &[&str], &str); 25] = [
        (
            &["gh", "leak"],
            &[],
            "`gh leak` is not one of gh's own commands",
        ),
        (
            &["gh", "extension", "exec", "leak"],
            &[],
            "`gh extension` is not",
        ),
        (&["gh", "ext", "exec", "leak"], &[], "`gh ext` is not"),
        (&["gh", "help", "leak"], &[], "`gh help` is not"),
        (&["gh", "browse"], &[], "`gh browse` is not"),
        (&["gh", "codespace", "ssh"], &[], "`gh codespace` is not"),
        (&["gh"], &[], "the call runs none of gh's commands"),
        (
            &["gh", "pr", "view", "--web"],
            &[],
            "`--web` opens a browser",
        ),
        (
            &["gh", "issue", "list", "-w"],
            &[],
            "`--web` opens a browser",
        ),
        (&["gh", "pr", "view", "-cw"], &[], "`--web` opens a browser"),
        (
            &["gh", "repo", "clone", "invented/x", "--", "-u", "leak"],
            &[],
            "`-u` names a program for git to run",
        ),
        (
            &["gh", "api", "user"],
            &["GH_REPO"],
            "the call sets GH_REPO, and a variable",
        ),
        (
            &["git", "push"],
            &["PATH", "GIT_EXEC_PATH"],
            "the call sets PATH and GIT_EXEC_PATH",
        ),
        (
            &["git", "leak"],
            &[],
            "`git leak` is not one of git's commands that reach",
        ),
        (&["git", "status"], &[], "`git status` is not one of"),
        (
            &["git", "-c", "alias.x=!leak", "x"],
            &[],
            "`-c` before git's command",
        ),
        (
            &["git", "--exec-path=/tmp", "push"],
            &[],
            "`--exec-path=/tmp` before",
        ),
        (
            &["git", "fetch", "--upload-pack=leak", "."],
            &[],
            "names a program",
        ),
        (
            &["git", "fetch", "--upload=leak", "."],
            &[],
            "names a program",
        ),
        (
            &["git", "push", "--receive-pack", "leak", "origin"],
            &[],
            "names a program",
        ),
        (
            &["git", "clone", "-u", "leak", "src"],
            &[],
            "names a program",
        ),
        (
            &["git", "clone", "-c", "core.fsmonitor=leak", "src"],
            &[],
            "names a program",
        ),
        (
            &["git", "clone", "--template=t", "src"],
            &[],
            "names a program",
        ),
        (
            &["git", "fetch", "leak::x"],
            &[],
            "names the transport `leak`",
        ),
        (
            &["git", "push", "--repo=leak://x"],
            &[],
            "names the transport `leak`",
        ),
    ];
    for (a, env, says) in withheld {
        let j = br
            .for_job(&argv(a), env, b.path(), Some(&path), Posture::Notify)
            .await;
        assert!(j.env.is_empty(), "{a:?}");
        let note = j.note().unwrap_or_default();
        assert!(
            note.starts_with(&format!("[{} got no GH_TOKEN: ", a[0])),
            "{note}"
        );
        assert!(note.contains(says), "{a:?}: {note}");
        let at_gate = br.at_gate("proc.run", Some(&argv(a)), env, b.path(), Some(&path));
        assert!(at_gate.is_empty(), "{a:?}");
    }
}

/// A broker that grants `HARBOR_TOKEN` to each of `programs`, found in
/// `bin`.
fn granting(bin: &Path, programs: &[&str]) -> Broker {
    let mut cfg = BrokerConfig::default();
    for p in programs {
        cfg.programs.insert(
            p.to_string(),
            ProgramGrant {
                env: [("HARBOR_TOKEN".to_string(), "harbor_token".to_string())].into(),
                aws_account: None,
            },
        );
    }
    Broker::new(
        &cfg,
        ready(&[("harbor_token", "harbor-not-a-secret")]),
        Some(format!("{}:/usr/bin:/bin", bin.display())),
    )
}

/// theseus-txvt: the launchers are known by name, a version suffix
/// aside, and other programs are not.
#[test]
fn a_launcher_is_known_by_its_name() {
    for (name, what) in [
        ("bash", "a shell"),
        ("sh", "a shell"),
        ("python3.12", "an interpreter"),
        ("python3", "an interpreter"),
        ("node18", "an interpreter"),
        ("env", "a program that runs the command it is given"),
        ("sudo", "a program that runs the command it is given"),
        ("make", "a runner of a project's own scripts"),
        ("npx", "a runner of a project's own scripts"),
        ("pip3", "a runner of a project's own scripts"),
    ] {
        assert_eq!(launcher(name), Some(what), "{name}");
    }
    for name in ["gh", "git", "cargo", "npm", "harbor", "python3-config", "3"] {
        assert_eq!(launcher(name), None, "{name}");
    }
}

/// theseus-txvt: a granted name that resolves to a launcher's file (a
/// link named for a leaf program) gets nothing at the call, which the
/// config's check of the name cannot see.
#[tokio::test]
async fn a_granted_name_that_is_a_launchers_file_gets_nothing() {
    let b = bin(&["python3.12"]);
    std::os::unix::fs::symlink(b.path().join("python3.12"), b.path().join("harbor")).unwrap();
    let path = format!("{}:/usr/bin:/bin", b.path().display());
    let br = granting(b.path(), &["harbor"]);
    let j = br
        .for_job(
            &argv(&["harbor", "sync"]),
            &[],
            b.path(),
            Some(&path),
            Posture::Notify,
        )
        .await;
    assert!(j.env.is_empty());
    let note = j.note().unwrap();
    assert!(
        note.starts_with("[harbor got no HARBOR_TOKEN: it is ")
            && note.contains("(an interpreter)"),
        "{note}"
    );
    assert!(br
        .at_gate(
            "proc.run",
            Some(&argv(&["harbor", "sync"])),
            &[],
            b.path(),
            Some(&path)
        )
        .is_empty());
}

/// `program`, granted `HARBOR_TOKEN`: each call of `given` gets it with
/// no note; each of `withheld` gets nothing, and a note that says why.
/// The gate's view agrees with the spawn's.
async fn grants_only(program: &str, given: &[&[&str]], withheld: &[(&[&str], &str)]) {
    let b = bin(&[program]);
    let path = format!("{}:/usr/bin:/bin", b.path().display());
    let br = granting(b.path(), &[program]);
    for a in given {
        let j = br
            .for_job(&argv(a), &[], b.path(), Some(&path), Posture::Notify)
            .await;
        assert_eq!((j.env.len(), j.note()), (1, None), "{a:?}");
        let at_gate = br.at_gate("proc.run", Some(&argv(a)), &[], b.path(), Some(&path));
        assert_eq!(at_gate.len(), 1, "{a:?}");
    }
    for (a, says) in withheld {
        let j = br
            .for_job(&argv(a), &[], b.path(), Some(&path), Posture::Notify)
            .await;
        assert!(j.env.is_empty(), "{a:?}");
        let note = j.note().unwrap_or_default();
        assert!(
            note.starts_with(&format!("[{program} got no HARBOR_TOKEN: ")),
            "{note}"
        );
        assert!(note.contains(says), "{a:?}: {note}");
        let at_gate = br.at_gate("proc.run", Some(&argv(a)), &[], b.path(), Some(&path));
        assert!(at_gate.is_empty(), "{a:?}");
    }
}

/// theseus-txvt: cargo gets its grant only for its registry commands that
/// build nothing, `publish` only with `--no-verify`, and never with a
/// toolchain, `--config`, or `-Z`.
#[tokio::test]
async fn cargo_gets_a_grant_only_for_registry_commands_that_build_nothing() {
    grants_only(
        "cargo",
        &[
            &["cargo", "publish", "--no-verify"],
            &["cargo", "-q", "publish", "--no-verify", "--allow-dirty"],
            &["cargo", "owner", "--add", "ada", "harbor"],
            &["cargo", "yank", "--version", "1.0.0", "harbor"],
        ],
        &[
            (
                &["cargo", "publish"],
                "builds the package unless `--no-verify`",
            ),
            (
                &["cargo", "build"],
                "`cargo build` is not one of cargo's registry commands",
            ),
            (&["cargo", "leak"], "or a `cargo-leak` program"),
            (
                &["cargo"],
                "the call runs none of cargo's registry commands",
            ),
            (
                &["cargo", "+nightly", "publish", "--no-verify"],
                "`+nightly` before cargo's command",
            ),
            (
                &["cargo", "--config", "x=1", "owner"],
                "`--config` before cargo's command",
            ),
            (
                &[
                    "cargo",
                    "publish",
                    "--no-verify",
                    "--config=build.rustc-wrapper='leak'",
                ],
                "can name a setting",
            ),
            (
                &["cargo", "yank", "-Zunstable-options"],
                "`-Zunstable-options` can name",
            ),
        ],
    )
    .await;
}

/// theseus-txvt: npm gets its grant only for its registry commands that
/// run no package's scripts, `publish` only with `--ignore-scripts` and a
/// local package, and never with an option that names a program.
#[tokio::test]
async fn npm_gets_a_grant_only_for_registry_commands_that_run_no_scripts() {
    let given: [&[&str]; 5] = [
        &["npm", "publish", "--ignore-scripts"],
        &[
            "npm",
            "publish",
            "--ignore-scripts",
            "--registry",
            "https://registry.invented/",
        ],
        &[
            "npm",
            "publish",
            "./harbor-1.0.0.tgz",
            "--ignore-scripts=true",
        ],
        &["npm", "whoami"],
        &["npm", "view", "harbor", "version"],
    ];
    let withheld: [(&[&str], &str); 8] = [
        (
            &["npm", "publish"],
            "runs the package's lifecycle scripts unless",
        ),
        (
            &["npm", "publish", "--ignore-scripts", "false"],
            "unless `--ignore-scripts`",
        ),
        (
            &["npm", "publish", "--ignore-scripts", "--no-ignore-scripts"],
            "unless",
        ),
        (
            &["npm", "install"],
            "`npm install` is not one of npm's registry commands",
        ),
        (&["npm", "exec", "leak"], "`npm exec` is not one of"),
        (
            &[
                "npm",
                "publish",
                "--ignore-scripts",
                "github:invented/harbor",
            ],
            "npm fetches and prepares",
        ),
        (
            &["npm", "whoami", "--script-shell=leak"],
            "`--script-shell=leak` names a program",
        ),
        (
            &["npm", "publish", "--ignore-scripts", "--userconfig", "rc"],
            "`--userconfig` names",
        ),
    ];
    grants_only("npm", &given, &withheld).await;
}

/// theseus-ur1t: the pins follow the `GIT_CONFIG_*` pairs a job has
/// already, so both apply.
#[test]
fn the_git_pins_follow_the_pairs_a_job_has() {
    let mut env = vec![
        ("PATH".to_string(), "/usr/bin".to_string()),
        ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
        ("GIT_CONFIG_KEY_0".to_string(), "color.ui".to_string()),
        ("GIT_CONFIG_VALUE_0".to_string(), "never".to_string()),
    ];
    pin_git(&mut env);
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("GIT_CONFIG_COUNT"), Some("3"));
    assert_eq!(get("GIT_CONFIG_KEY_0"), Some("color.ui"));
    assert_eq!(get("GIT_CONFIG_KEY_1"), Some("core.hooksPath"));
    assert_eq!(get("GIT_CONFIG_VALUE_1"), Some("/dev/null"));
    assert_eq!(get("GIT_CONFIG_KEY_2"), Some("core.fsmonitor"));
    assert_eq!(get("GIT_CONFIG_VALUE_2"), Some(""));
    assert_eq!(
        env.iter().filter(|(k, _)| k == "GIT_CONFIG_COUNT").count(),
        1
    );
}

/// A real git in `dir`, hermetic (`home` its home, no system or global
/// config, an invented author), with `extra` in its environment; it must
/// succeed.
fn git_in(home: &Path, dir: &Path, args: &[&str], extra: &[(String, String)]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@invented.example")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@invented.example")
        .envs(extra.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// theseus-ur1t, against a real git: a job's git given a secret runs
/// neither the repository's hooks (`reference-transaction`, which every
/// fetch's ref update runs) nor its `core.fsmonitor` program, so neither
/// sees the value; without the pins both do. A grant withheld pins
/// nothing, so the operator's own hooks still run then.
#[tokio::test]
async fn a_granted_gits_hooks_and_fsmonitor_never_see_the_variable() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let (src, dst) = (d.path().join("src"), d.path().join("dst"));
    let git = |dir: &Path, args: &[&str], extra: &[(String, String)]| {
        git_in(d.path(), dir, args, extra);
    };
    std::fs::create_dir_all(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"], &[]);
    git(&src, &["commit", "-q", "--allow-empty", "-m", "one"], &[]);
    git(d.path(), &["clone", "-q", "src", "dst"], &[]);
    // What a cloned project's hook manager or an earlier job leaves.
    let marker = d.path().join("leaked");
    // Each writes the variable down: the hook lets the update go on,
    // and the fsmonitor fails, so git scans the tree itself.
    let script = |name: &str, exit: u8| {
        let p = d.path().join(name);
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\necho \"$HARBOR_TOKEN\" >> {}\nexit {exit}\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    let hook = script("reference-transaction", 0);
    let hooks = dst.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::copy(&hook, hooks.join("reference-transaction")).unwrap();
    let monitor = script("fsmonitor", 1);
    git(
        &dst,
        &["config", "core.fsmonitor", &monitor.to_string_lossy()],
        &[],
    );
    // The job's environment, as the broker leaves it for a granted git.
    let b = bin(&["git"]);
    let path = format!("{}:/usr/bin:/bin", b.path().display());
    let br = granting(b.path(), &["git"]);
    let j = br
        .for_job(
            &argv(&["git", "fetch"]),
            &[],
            &dst,
            Some(&path),
            Posture::Notify,
        )
        .await;
    assert!(j.git_pinned);
    let mut env: Vec<(String, String)> = j
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.expose().to_string()))
        .collect();
    let unpinned = env.clone();
    j.pin(&mut env);
    git(&src, &["commit", "-q", "--allow-empty", "-m", "two"], &[]);
    git(&dst, &["fetch", "-q"], &env);
    git(&dst, &["status", "--short"], &env);
    assert!(
        !marker.exists(),
        "a hook or the fsmonitor ran with the pins"
    );
    // The same calls without the pins: both programs see the value.
    git(&src, &["commit", "-q", "--allow-empty", "-m", "three"], &[]);
    git(&dst, &["fetch", "-q"], &unpinned);
    git(&dst, &["status", "--short"], &unpinned);
    let leaked = std::fs::read_to_string(&marker).unwrap_or_default();
    assert!(
        leaked
            .lines()
            .filter(|l| *l == "harbor-not-a-secret")
            .count()
            >= 2,
        "{leaked:?}"
    );
    // A withheld grant pins nothing.
    let w = br
        .for_job(
            &argv(&["git", "status"]),
            &[],
            &dst,
            Some(&path),
            Posture::Notify,
        )
        .await;
    assert!(w.env.is_empty() && !w.git_pinned);
}

// The config's `[broker]` checks (`Config::validate`), beside the broker
// they describe (moved from config.rs under the shape budget's file
// ceiling, theseus-goa8).

/// The broker is empty until a grant is added (theseus-dcy), so a note
/// without `[broker]` loads as it did. A grant names a program, a
/// variable, and a `[secrets]` entry; anything else fails to load, and
/// the error says which.
#[test]
fn a_broker_grant_names_a_program_a_variable_and_a_secret() {
    use crate::config::Config;
    assert!(Config::example().broker.is_empty());
    let with = |broker: &str| {
        Config::parse(&format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\ngithub_token = \"op://v/g/f\"\n\n{broker}\n"
        ))
        .map(|(c, _)| c)
    };
    let ok = with(
        "[broker.programs.gh]\nenv = { GH_TOKEN = \"github_token\" }\n\
         [broker.secrets.github_token]\nposture = \"approve\"",
    )
    .unwrap();
    assert_eq!(ok.broker.secrets["github_token"].posture, Posture::Approve);
    let default = with("[broker.secrets.github_token]").unwrap();
    assert_eq!(
        default.broker.secrets["github_token"].posture,
        Posture::Notify
    );
    for (bad, says) in [
        (
            "[broker.programs.gh]\nenv = { GH_TOKEN = \"aws_key\" }",
            "has no matching entry under [secrets]",
        ),
        (
            "[broker.programs.\"/usr/bin/gh\"]\nenv = { GH_TOKEN = \"github_token\" }",
            "with no '/'",
        ),
        (
            "[broker.programs.gh]\nenv = { \"GH-TOKEN\" = \"github_token\" }",
            "is not an environment variable's name",
        ),
        ("[broker.programs.gh]\nenv = {}", "grants nothing"),
        (
            "[broker.secrets.nope]",
            "broker.secrets.nope has no matching entry",
        ),
        ("[broker.secrets.github_token]\nposture = \"never\"", "open"),
        (
            "[broker.programs.gh]\nenv = { GH_TOKEN = \"github_token\" }\nshell = true",
            "shell",
        ),
    ] {
        let e = format!("{:#}", with(bad).unwrap_err());
        assert!(e.contains(says), "{bad}: {e}");
    }
}

/// theseus-txvt: a grant to a launcher (a shell, an interpreter, a
/// wrapper that runs the command it is given, or a runner of a project's
/// scripts) fails to load, naming the rule; a leaf program, and the four
/// whose commands the broker knows (gh, git, cargo, npm), load.
#[test]
fn a_grant_to_a_launcher_fails_to_load_naming_the_rule() {
    use crate::config::Config;
    let with = |program: &str| {
        Config::parse(&format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\nharbor_token = \"op://v/h/f\"\n\n\
             [broker.programs.\"{program}\"]\nenv = {{ HARBOR_TOKEN = \"harbor_token\" }}\n"
        ))
        .map(|(c, _)| c)
    };
    for program in [
        "bash", "python3", "node", "env", "sudo", "xargs", "make", "npx",
    ] {
        let e = format!("{:#}", with(program).unwrap_err());
        assert!(
            e.contains(&format!("broker.programs.{program}: a secret granted to a program reaches only that program, never one it can be made to run")),
            "{program}: {e}"
        );
    }
    for program in ["gh", "git", "cargo", "npm", "harbor"] {
        assert!(with(program).is_ok(), "{program}");
    }
}

/// theseus-gh7: the AWS keys and the providers' keys are harness-only,
/// and the line names them. A `[broker]` entry that names one makes it a
/// secret a job may be handed, and the line says so aloud instead.
#[test]
fn the_aws_and_provider_keys_are_harness_only_unless_the_broker_names_one() {
    use crate::config::Config;
    let with = |broker: &str| {
        let (cfg, _) = Config::parse(&format!(
            "[secrets]\nanthropic_api_key = \"op://v/a/f\"\ngithub_token = \"op://v/g/f\"\n\
             aws_access_key_id = \"op://v/k/f#AWS_ACCESS_KEY_ID\"\n\
             aws_secret_access_key = \"op://v/k/f#AWS_SECRET_ACCESS_KEY\"\n\n{broker}"
        ))
        .unwrap();
        let b = Broker::new(&cfg.broker, SecretBoard::empty(), None);
        harness_only(&cfg, &b)
    };
    let none = with("");
    assert!(none.exposed.is_empty());
    assert_eq!(
        none.line(),
        "broker: a job may be handed no secret; harness-only: the AWS keys \
         (aws_access_key_id, aws_secret_access_key) and the providers' keys (anthropic_api_key)"
    );
    let named = with(
        "[broker.secrets.github_token]\nposture = \"notify\"\n\
         [broker.secrets.anthropic_api_key]\nposture = \"approve\"\n",
    );
    assert_eq!(named.exposed, ["anthropic_api_key"]);
    assert_eq!(
        named.line(),
        "broker: a job may be handed 2 secrets (anthropic_api_key, github_token); \
         harness-only: the AWS keys (aws_access_key_id, aws_secret_access_key); not \
         harness-only, since [broker] names them: anthropic_api_key"
    );
    // The speech provider's key (rows 77 and 78) is a provider's: its
    // [secrets] line comes first, so it lands in that table.
    let voice = "deepgram_api_key = \"op://v/d/f\"\n[voice]\nenabled = true\n";
    let spoken = with(voice);
    assert_eq!(spoken.providers, ["anthropic_api_key", "deepgram_api_key"]);
    assert!(spoken.exposed.is_empty());
    let handed = with(&format!(
        "{voice}[broker.secrets.deepgram_api_key]\nposture = \"approve\"\n"
    ));
    assert_eq!(handed.exposed, ["deepgram_api_key"]);
}
