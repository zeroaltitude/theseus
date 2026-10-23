//! Where a draft's tree lives (theseus-iilre, option a): outside the state dir,
//! which is on the approve list, so the review's runs there ask nothing, while
//! the rest of the state dir is guarded as it was.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use theseus_tools::{Access, Plan, Resource, Retry, Tool, ToolClass, ToolCtx};

use crate::draft::{self, Draft};
use crate::policy::{floor_argv, Posture, ToolPolicy};
use crate::tests_draft::{git, repo};

const DRAFTING: &str =
    "[routing.classes.routine_coding]\ncn_draft = \"dsflash\"\nfinal = [\"sonnet\"]";

/// A tool by name only: the gate looks at nothing else about it.
struct T(&'static str);
impl Tool for T {
    fn name(&self) -> &'static str {
        self.0
    }
    fn description(&self) -> &'static str {
        ""
    }
    fn input_schema(&self) -> Value {
        json!({})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, _: &Value, _: &ToolCtx) -> Result<Plan, String> {
        Ok(Plan::default())
    }
}

fn plan(path: PathBuf, access: Access, argv: Option<Vec<&str>>) -> Plan {
    Plan {
        resources: vec![Resource { path, access }],
        argv: argv.map(|v| v.into_iter().map(String::from).collect()),
        summary: "the call".into(),
        ..Default::default()
    }
}

/// The gate as the runtime builds it from `cfg`, at the notify posture, with
/// the state dir on the approve list as the defaults have it.
fn policy(cfg: &crate::Config, state: &Path) -> ToolPolicy {
    let (roots, floor_paths, approve_paths) =
        crate::toolrun::draft_roots::guarded_with_drafts(cfg, state);
    ToolPolicy {
        roots,
        approve_paths,
        floor_paths,
        allow_argv: vec![],
        approve_argv: vec![],
        enforcement: Posture::Notify,
        tools: Default::default(),
        mcp: Default::default(),
        aws: Default::default(),
        confirmer: "operator".into(),
        floor_argv: floor_argv(),
        private_addresses: Default::default(),
    }
}

/// A drafting config whose state dir and drafts dir are under `base`.
fn config(base: &Path) -> crate::Config {
    let mut cfg = crate::tests_draft::load(DRAFTING).unwrap();
    cfg.server.state_dir = base.join("state").to_string_lossy().into_owned();
    cfg.server.drafts_dir = base.join("cache/drafts").to_string_lossy().into_owned();
    cfg.tools.approve_paths = vec![cfg.server.state_dir.clone()];
    cfg
}

/// The key test: a review's `cargo test` in the draft's tree takes the tool's
/// posture. With the tree under the state dir, the approve list held it.
#[test]
fn a_drafts_cargo_test_runs_without_asking() {
    let base = tempfile::tempdir().unwrap();
    let base = base.path().canonicalize().unwrap();
    let cfg = config(&base);
    let state = cfg.state_dir();
    let tree = draft::tree_of(&cfg, "ses_reviewed0001");
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let p = policy(&cfg, &state);
    let tree = tree.canonicalize().unwrap();
    let run = plan(tree.clone(), Access::Exec, Some(vec!["cargo", "test"]));
    let out = p.decide(&T("proc.run"), &run);
    assert_eq!(out.posture, Posture::Notify, "{}", out.reason);
    for access in [Access::Read, Access::Write] {
        let out = p.decide(&T("fs.x"), &plan(tree.join("src/lib.rs"), access, None));
        assert_eq!(out.posture, Posture::Notify, "{}", out.reason);
    }
}

#[test]
fn the_state_dir_is_still_protected_beside_the_drafts_root() {
    let base = tempfile::tempdir().unwrap();
    let base = base.path().canonicalize().unwrap();
    let cfg = config(&base);
    let state = cfg.state_dir();
    std::fs::create_dir_all(state.join("store")).unwrap();
    let p = policy(&cfg, &state);
    for (what, path, floor) in [
        ("a file of the state dir", state.join("notes.txt"), false),
        (
            "the state dir's own drafts",
            state.join("drafts/x/a.rs"),
            false,
        ),
        ("the store", state.join("store/x"), true),
    ] {
        for access in [Access::Read, Access::Write] {
            let out = p.decide(&T("fs.x"), &plan(path.clone(), access, None));
            assert_eq!(out.posture, Posture::Approve, "{what}: {}", out.reason);
            assert_eq!(out.floor, floor, "{what}: {}", out.reason);
        }
    }
    // The drafts root is a root only while a class drafts.
    let mut none = cfg;
    none.routing.classes.routine_coding.cn_draft = None;
    none.routing.classes.routine_coding.final_review = vec![];
    let roots = crate::toolrun::draft_roots::guarded_with_drafts(&none, &state).0;
    assert!(!roots.contains(&none.drafts_dir()), "{roots:?}");
}

#[test]
fn a_draft_tree_is_outside_the_state_dir() {
    let cfg = crate::tests_draft::load("").unwrap();
    let state = cfg.state_dir();
    assert_eq!(cfg.server.drafts_dir, "~/.cache/theseus/drafts");
    // A draft's tree and its first pass's, from their session ids.
    for sid in ["ses_draft0000001", "ses_firstpass0001"] {
        let tree = draft::tree_of(&cfg, sid);
        assert!(tree.starts_with(cfg.drafts_dir()), "{tree:?}");
        assert!(!theseus_tools::paths::within(&tree, &state), "{tree:?}");
        for held in &cfg.tools.approve_paths {
            let held = crate::config::expand(held);
            assert!(!theseus_tools::paths::within(&tree, &held), "{tree:?}");
        }
    }
}

#[test]
fn a_drafts_dir_inside_the_state_dir_is_refused() {
    let with = |dir: &str| {
        let text = crate::Config::EXAMPLE_TOML.replace(
            "drafts_dir = \"~/.cache/theseus/drafts\"",
            &format!("drafts_dir = \"{dir}\""),
        );
        crate::Config::parse(&text)
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
    };
    with("~/.cache/theseus/drafts").unwrap();
    for (dir, key) in [
        ("~/.theseus/drafts", "server.state_dir"),
        ("~/.ssh/drafts", "tools.approve_paths"),
    ] {
        let e = with(dir).unwrap_err();
        assert!(
            e.contains("server.drafts_dir") && e.contains(key),
            "{dir}: {e}"
        );
        assert!(e.contains("would ask every time: keep"), "{dir}: {e}");
    }
}

/// A draft made before the move has its tree under the state dir: it settles
/// there, its change is applied, and its tree is removed.
#[test]
fn an_old_draft_under_the_state_dir_still_settles() {
    let base = tempfile::tempdir().unwrap();
    let base = base.path().canonicalize().unwrap();
    let cfg = config(&base);
    let owner = repo(&base.join("acme"));
    let sid = "ses_olddraft0001";
    let old = cfg.state_dir().join("drafts").join(crate::task::short(sid));
    let (head, git_dir) = draft::open_tree(&owner, &old, &draft::branch_of(sid)).unwrap();
    assert_eq!(draft::tree_of(&cfg, sid), old, "an old tree keeps its path");
    assert_eq!(
        draft::tree_of(&cfg, "ses_newdraft0002").parent().unwrap(),
        cfg.drafts_dir()
    );
    std::fs::write(old.join("notes.txt"), "alpha\ngamma\n").unwrap();
    let d = Draft {
        dir: owner.to_string_lossy().into_owned(),
        tree: old.to_string_lossy().into_owned(),
        branch: draft::branch_of(sid),
        base: head,
        git_dir,
        ..Draft::default()
    };
    let settled = draft::settle_tree(&d, "sonnet", true).unwrap();
    assert!(matches!(settled.applied, Ok(draft::Apply::Done)));
    assert!(!old.exists(), "the old tree was not removed");
    assert_eq!(
        std::fs::read_to_string(owner.join("notes.txt")).unwrap(),
        "alpha\ngamma\n"
    );
    git(&owner, &["status", "--porcelain"]);
}
