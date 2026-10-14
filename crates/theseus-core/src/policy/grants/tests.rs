//! The matching rules, pure: what a prefix may be, what an offer names,
//! and what a grant covers.

use std::path::PathBuf;

use theseus_protocol::grants::{GrantOffer, SessionGrant, KIND_EDITS, KIND_PREFIX};

use super::{covers, prefix_of, prefix_ok, Call};

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn prefix_grant(prefix: &str, program: &str) -> SessionGrant {
    SessionGrant {
        id: "sgr_00000001".into(),
        session_id: "ses_a".into(),
        kind: KIND_PREFIX.into(),
        pattern: prefix.into(),
        prefix: words(prefix),
        program: Some(program.into()),
        ..Default::default()
    }
}

fn run(g: &SessionGrant, argv: &str) -> bool {
    let argv = words(argv);
    let nargv = super::normalized_argv(&argv);
    covers(
        g,
        &Call {
            tool: "proc.run",
            program: argv.first().map(String::as_str),
            nargv: &nargv,
            resources: &[],
        },
    )
}

#[test]
fn an_offer_names_the_program_and_its_subcommand() {
    let of = |s: &str| prefix_of(&words(s)).map(|(p, prog)| (p.join(" "), prog));
    assert_eq!(
        of("python3 -m unittest -v tests.test_app"),
        Some(("python3 -m unittest".into(), "python3".into()))
    );
    assert_eq!(
        of("cargo test -p app"),
        Some(("cargo test".into(), "cargo".into()))
    );
    assert_eq!(of("git status"), Some(("git status".into(), "git".into())));
    assert_eq!(
        of("/usr/bin/git status --short"),
        Some(("git status".into(), "/usr/bin/git".into()))
    );
    assert_eq!(
        of("uv run pytest -q"),
        Some(("uv run pytest".into(), "uv".into()))
    );
}

#[test]
fn no_offer_for_a_shell_a_wrapper_or_inline_code() {
    for argv in [
        "sh -c python3",
        "bash -c python3 -m unittest",
        "bash script.sh",
        "env python3 -m unittest",
        "xargs python3",
        "sudo cargo test",
        "python3 -c print(1)",
        "python3 -m timeit 1",
        "python3 -m pdb x.py",
        "node -e 1",
        "uv run sh -c x",
        "uv run python3 -c 1",
        "uv run",
        "git -C /tmp status",
        "ls",
        "ls -la",
        "./run.sh test",
        "bin/tool build",
        "curl https://example.invalid/x",
        "make FOO=1",
    ] {
        assert_eq!(prefix_of(&words(argv)), None, "{argv} must offer nothing");
    }
}

/// A launcher whose next word is an image, a container, a pod or a package's
/// shell runs whatever follows it: no prefix of it is offered, and a grant
/// stored on one covers nothing (the join's security review). A submodule's
/// own subcommands still offer theirs.
#[test]
fn no_offer_for_a_launcher_that_runs_any_command() {
    for argv in [
        "git submodule foreach git pull",
        "docker run ubuntu ls",
        "docker exec web ls",
        "podman run alpine ls",
        "podman exec web ls",
        "kubectl exec pod-1 ls",
        "nix shell nixpkgs#hello",
        "uv tool run ruff",
    ] {
        assert_eq!(prefix_of(&words(argv)), None, "{argv} must offer nothing");
    }
    assert_eq!(
        prefix_of(&words("git submodule update --init")).map(|(p, _)| p),
        Some(words("git submodule update"))
    );
    let g = prefix_grant("git submodule foreach", "git");
    assert!(!run(&g, "git submodule foreach rm -rf x"));
}

#[test]
fn prefix_ok_is_asked_again_of_every_grant() {
    assert!(prefix_ok(&words("python3 -m unittest")).is_ok());
    assert!(prefix_ok(&words("python3 -m code")).is_err());
    assert!(prefix_ok(&words("sh -c")).is_err());
    assert!(prefix_ok(&words("env python3")).is_err());
    assert!(prefix_ok(&words("cargo")).is_err());
    assert!(prefix_ok(&words("uv run bash")).is_err());
    // A grant stored by hand on a shell covers nothing.
    let g = prefix_grant("bash -c", "bash");
    assert!(!run(&g, "bash -c ls"));
}

#[test]
fn a_prefix_grant_covers_its_prefix_and_nothing_wider() {
    let g = prefix_grant("python3 -m unittest", "python3");
    assert!(run(&g, "python3 -m unittest"));
    assert!(run(&g, "python3 -m unittest -v tests.test_app"));
    // Another command, a wrapper around it, a near name, another path.
    assert!(!run(&g, "python3 -m pip install x"));
    assert!(!run(&g, "sh -c python3 -m unittest"));
    assert!(!run(&g, "bash -c python3 -m unittest"));
    assert!(!run(&g, "env python3 -m unittest"));
    assert!(!run(&g, "python3x -m unittest"));
    assert!(!run(&g, "/tmp/evil/python3 -m unittest"));
    assert!(!run(&g, "./python3 -m unittest"));
    assert!(!run(&g, "python3 -m"));
    // A URL after the prefix is an exit, never covered.
    let git = prefix_grant("git fetch", "git");
    assert!(run(&git, "git fetch origin"));
    assert!(!run(&git, "git fetch https://example.invalid/x"));
}

#[test]
fn an_edits_grant_covers_the_three_edit_tools_under_its_root() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap().join("app");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let g = SessionGrant {
        kind: KIND_EDITS.into(),
        pattern: root.display().to_string(),
        root: Some(root.display().to_string()),
        ..Default::default()
    };
    let at = |tool: &str, p: PathBuf| {
        let r = vec![theseus_tools::paths::canonical_best_effort(&p)];
        covers(
            &g,
            &Call {
                tool,
                program: None,
                nargv: &[],
                resources: &r,
            },
        )
    };
    for tool in super::EDIT_TOOLS {
        assert!(at(tool, root.join("src/main.rs")), "{tool}");
    }
    assert!(at("fs.write", root.join("new/dir/file.txt")));
    assert!(!at("fs.read", root.join("src/main.rs")));
    assert!(!at("proc.run", root.join("src/main.rs")));
    // `..` out of the project, and a link out of it.
    assert!(!at("fs.write", root.join("src/../../outside.txt")));
    assert!(!at("fs.write", root.join("new/../../../outside.txt")));
    std::os::unix::fs::symlink(d.path(), root.join("out")).unwrap();
    assert!(!at("fs.write", root.join("out/escaped.txt")));
    // A sibling whose name starts with the root's is not under it.
    std::fs::create_dir_all(d.path().join("app2")).unwrap();
    assert!(!at(
        "fs.write",
        d.path().canonicalize().unwrap().join("app2/x")
    ));
    // No resource is no edit under the root.
    assert!(!covers(
        &g,
        &Call {
            tool: "fs.write",
            program: None,
            nargv: &[],
            resources: &[],
        }
    ));
}

#[test]
fn an_edits_grant_never_names_the_root_or_the_home() {
    let g = |root: &str| SessionGrant {
        kind: KIND_EDITS.into(),
        root: Some(root.into()),
        ..Default::default()
    };
    let r = vec![PathBuf::from("/etc/passwd")];
    let call = Call {
        tool: "fs.write",
        program: None,
        nargv: &[],
        resources: &r,
    };
    assert!(!covers(&g("/"), &call));
    if let Some(home) = std::env::var_os("HOME") {
        let home = theseus_tools::paths::canonical_best_effort(std::path::Path::new(&home));
        let r = vec![home.join("x.txt")];
        let call = Call {
            tool: "fs.write",
            program: None,
            nargv: &[],
            resources: &r,
        };
        assert!(!covers(&g(&home.display().to_string()), &call));
    }
}

#[test]
fn a_grant_is_made_only_of_the_offer_it_answers() {
    let offer = GrantOffer::prefix(&words("cargo test"));
    let g = super::grant_of(
        &offer,
        "ses_a",
        "cor_1",
        Some(&words("cargo test -p app")),
        ("cli", "cli".into(), "cli".into()),
        7,
    )
    .unwrap();
    assert_eq!(
        (g.prefix.join(" "), g.program.as_deref()),
        ("cargo test".into(), Some("cargo"))
    );
    assert!(g.id.starts_with("sgr_"));
    // An offer whose pattern is not the call's own prefix makes nothing.
    let wider = GrantOffer::prefix(&words("cargo"));
    assert!(super::grant_of(
        &wider,
        "ses_a",
        "cor_1",
        Some(&words("cargo test")),
        ("cli", "cli".into(), "cli".into()),
        7
    )
    .is_none());
}

#[test]
fn the_memory_holds_each_sessions_own() {
    let grants = super::Grants::default();
    assert!(!grants.any());
    let mut a = prefix_grant("cargo test", "cargo");
    a.session_id = "ses_a".into();
    grants.add(a.clone());
    let argv = words("cargo test");
    let call = Call {
        tool: "proc.run",
        program: Some("cargo"),
        nargv: &argv,
        resources: &[],
    };
    assert!(grants.covers("ses_a", &call));
    assert!(!grants.covers("ses_b", &call));
    assert_eq!(grants.end(&a.id).map(|g| g.id), Some(a.id.clone()));
    assert!(!grants.any());
    // An ended grant is never added again (a ledger read that raced the end).
    grants.add(a);
    assert!(!grants.covers("ses_a", &call));
}

/// What a grant reads of a plan is canonical, whatever the plan says: a
/// raw `..` or a link out of the project in a plan's resource is out of it.
#[test]
fn the_view_canonicalises_a_plans_paths() {
    use theseus_tools::{Access, Plan, Resource};
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap().join("app");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::os::unix::fs::symlink(d.path(), root.join("out")).unwrap();
    let g = SessionGrant {
        kind: KIND_EDITS.into(),
        root: Some(root.display().to_string()),
        ..Default::default()
    };
    let covered = |raw: PathBuf| {
        let plan = Plan {
            resources: vec![Resource {
                path: raw,
                access: Access::Write,
            }],
            ..Default::default()
        };
        let (nargv, program, resources) = super::view(&plan);
        covers(
            &g,
            &Call {
                tool: "fs.write",
                program: program.as_deref(),
                nargv: &nargv,
                resources: &resources,
            },
        )
    };
    assert!(covered(root.join("src/a.txt")));
    assert!(!covered(root.join("src/../../outside.txt")), "`..` out");
    assert!(!covered(root.join("out/escaped.txt")), "a link out");
    assert!(
        !covered(root.join("out/app/../escaped.txt")),
        "a link, then `..`"
    );
}
