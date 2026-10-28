//! The matching rule, pure: what an edits grant covers.

use std::path::PathBuf;

use theseus_protocol::grants::{SessionGrant, KIND_EDITS};

use super::{covers, Call};

#[test]
fn an_edits_grant_covers_the_three_edit_tools_under_its_root() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap().join("app");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let g = SessionGrant {
        kind: KIND_EDITS.into(),
        root: Some(root.display().to_string()),
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
