//! The builder's build folder (`rsi::builder_dir`) through the core: a self builder's sandboxed
//! command is given one read-write directory, named by its own task, and cargo is pointed at it;
//! no other call is. The launcher here keeps each job's `WrapperArgs`, as a real wrapper would be
//! handed them; the jobs themselves, in real L1, are theseusd's `tests/self_build.rs`.

use std::os::unix::fs::MetadataExt;
use std::sync::Mutex;

use theseus_kernel::job::WrapperArgs;
use theseus_kernel::{Completion, Outcome, Spool};

use super::*;
use crate::rsi::builder_dir;
use crate::rsi::tree;

/// Keeps every job's arguments; settles each job at once unless told to hold it running.
#[derive(Default)]
struct Kept {
    args: Mutex<Vec<WrapperArgs>>,
    hold: bool,
}

impl crate::toolrun::JobLauncher for Kept {
    fn launch(
        &self,
        spool: &Spool,
        args: &WrapperArgs,
        _: crate::toolrun::JobDone,
    ) -> anyhow::Result<u32> {
        self.args.lock().unwrap().push(args.clone());
        if !self.hold {
            let out = spool.result_path(&args.correlation_id);
            std::fs::write(&out, "built\n")?;
            let now = theseus_protocol::now_unix_ms();
            spool.write(&Completion {
                correlation_id: args.correlation_id.clone(),
                outcome: Outcome::Succeeded,
                result_ref: Some(out.to_string_lossy().into_owned()),
                external_op_id: None,
                started_at_ms: now,
                finished_at_ms: now,
                producer: "test".into(),
                signature: None,
                cost_micros: None,
                detail: Some(json!({"exit_code": 0, "sandbox": {"class": "l1"}})),
            })?;
        }
        Ok(std::process::id())
    }
}

fn rig(kept: &Arc<Kept>) -> Rig {
    rig_launching(
        fixture(),
        SelfMode::Act,
        None,
        |cfg| {
            cfg.policy.tools.remove("proc.run");
            cfg.tools.proc_sync_secs = 1;
        },
        Some(kept.clone()),
    )
}

/// A self session, its task, and the tree its item opened (a real worktree: a closed item has
/// none, and the folder is only for an open one).
fn item(r: &Rig, id: &str) -> (String, tree::Tree) {
    let sid = task_record(r, id);
    let t = tree::open(&r.f.place, id, theseus_protocol::now_unix_ms()).unwrap();
    (sid, t)
}

fn built(cwd: &Path, sandbox: bool) -> Value {
    json!({"argv": ["cargo", "test"], "cwd": cwd, "sandbox": sandbox})
}

fn only(kept: &Kept) -> WrapperArgs {
    let all = kept.args.lock().unwrap();
    assert_eq!(all.len(), 1, "one job was started");
    all[0].clone()
}

fn state(r: &Rig) -> PathBuf {
    r.f.place.state.canonicalize().unwrap()
}

/// KEY: the command is given exactly two directories it can write and keep, its item's build folder
/// and its item's own tree, both named by its task and not by the call; the tree's `.git` file is
/// bound read-only over the tree; cargo is told to build in the folder; the rest of its view is an
/// ordinary job's.
#[tokio::test]
async fn a_self_builders_sandboxed_command_keeps_its_folder_and_its_own_tree() {
    let kept = Arc::new(Kept::default());
    let r = rig(&kept);
    let (sid, t) = item(&r, "sails");
    // Another item's tree, open: the call runs in it and names it, and it is not given it.
    let (_, other) = item(&r, "tides");
    // The call tries to point cargo elsewhere: its own word does not stand.
    let mut call = built(&other.dir, true);
    call["env"] = json!({"CARGO_TARGET_DIR": "/tmp/mine", "TMPDIR": "/tmp/mine"});
    r.turn(&sid, "proc_run", call).await;
    let args = only(&kept);
    let folder = builder_dir::dir_in(&state(&r), "sails");
    let tree = t.dir.canonicalize().unwrap();
    let view = args.sandbox.clone().expect("it ran in L1");
    assert_eq!(
        view.rw,
        [folder.clone(), tree.clone()],
        "the two writable, kept places, its own"
    );
    let trees = r.f.place.trees().canonicalize().unwrap();
    assert!(
        view.rw.iter().all(|w| *w != trees && !trees.starts_with(w)),
        "no parent of the trees"
    );
    assert!(
        !view.rw.contains(&other.dir.canonicalize().unwrap()),
        "no other item's tree"
    );
    assert_eq!(
        view.ro_paths
            .iter()
            .filter(|p| p.starts_with(&tree))
            .collect::<Vec<_>>(),
        [&tree.join(".git")],
        "the tree's .git file, read-only over the tree"
    );
    assert!(folder.is_dir(), "made for the command");
    // Cargo builds in the folder's `target`, and temporary files go to its `tmp`: both on disk, in
    // the folder, so large temporaries are not in the view's memory-backed `/tmp`.
    let var = |name: &str| -> Vec<&str> {
        args.env
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect()
    };
    assert_eq!(
        var("CARGO_TARGET_DIR"),
        [folder.join("target").to_str().unwrap()],
        "cargo builds in it"
    );
    assert_eq!(
        var("TMPDIR"),
        [folder.join("tmp").to_str().unwrap()],
        "temporary files go to disk in it"
    );
    assert!(folder.join("target").is_dir() && folder.join("tmp").is_dir());
    // Nothing else of the view moved: an ordinary job's, less the call's own egress list.
    let mut rest = view;
    rest.rw.clear();
    rest.ro_paths.retain(|p| *p != tree.join(".git"));
    let mut ordinary = r.core.tools.sandbox.job_view();
    ordinary.egress.clone_from(&rest.egress);
    // The view's own allocations (the owner, 2026-11-15 17:39:14); every other limit is the job's.
    assert_eq!(
        (
            rest.limits.scratch_mb,
            rest.limits.tmp_mb,
            rest.limits.output_mb,
            rest.limits.pids
        ),
        (4 * 1024, 4 * 1024, 16 * 1024, 32 * 1024)
    );
    rest.limits = ordinary.limits;
    assert_eq!(rest, ordinary);
    assert!(ordinary.rw.is_empty());
    // And it is not the gate's build tree, nor in the tree, nor a slot.
    assert_ne!(folder, t.target);
    assert!(!folder.starts_with(&t.dir) && !folder.starts_with(&t.target));
    assert!(!folder.starts_with(r.f.place.trees().canonicalize().unwrap()));
    // Nothing the daemon keeps of its own lies under either bind: its state, store, git directory,
    // reports and gate's build tree are each outside both.
    let repo_git = r.f.place.repo.join(".git").canonicalize().unwrap();
    for own in [state(&r), repo_git, t.target.clone(), r.f.place.root()] {
        for w in &args.sandbox.as_ref().unwrap().rw {
            assert!(
                !own.starts_with(w),
                "{} is under {}",
                own.display(),
                w.display()
            );
        }
    }
}

/// A tree that is not its own resolved path, or whose `.git` is not a plain file, is not bound: the
/// bind would land wherever the link points.
#[tokio::test]
async fn a_linked_tree_or_a_linked_dot_git_is_not_bound() {
    for linked in ["tree", "dot-git"] {
        let kept = Arc::new(Kept::default());
        let r = rig(&kept);
        let (sid, t) = item(&r, "sails");
        if linked == "tree" {
            let away = r.f.place.root().join("away");
            std::fs::rename(&t.dir, &away).unwrap();
            std::os::unix::fs::symlink(&away, &t.dir).unwrap();
        } else {
            let real = t.dir.join("dot-git-file");
            std::fs::rename(t.dir.join(".git"), &real).unwrap();
            std::os::unix::fs::symlink(&real, t.dir.join(".git")).unwrap();
        }
        r.turn(&sid, "proc_run", built(&t.dir, true)).await;
        let view = only(&kept).sandbox.expect("it ran in L1");
        assert_eq!(
            view.rw,
            [builder_dir::dir_in(&state(&r), "sails")],
            "{linked}: the folder alone"
        );
        assert!(
            view.ro_paths.iter().all(|p| !p.ends_with(".git")),
            "{linked}"
        );
    }
}

/// No other call gets it: not another principal's, not a self session whose tree is not open. The
/// folder is not made for them. A self session's call that asks for L0 runs in L1 all the same
/// (issue 53), with its folder.
#[tokio::test]
async fn only_a_self_builders_sandboxed_command_with_an_open_tree_gets_the_folder() {
    let kept = Arc::new(Kept::default());
    let r = rig(&kept);
    let (sid, t) = item(&r, "sails");
    let operator = r.session("operator");
    let work = r._work.path().canonicalize().unwrap();
    r.turn(&operator, "proc_run", built(&work, true)).await;
    let (first, kept_args) = (only(&kept), kept.args.lock().unwrap().len());
    assert_eq!(kept_args, 1);
    assert!(first.sandbox.unwrap().rw.is_empty(), "another principal");
    assert!(first.env.iter().all(|(k, _)| k != "CARGO_TARGET_DIR"));
    assert!(!builder_dir::root_in(&state(&r)).exists(), "none made");

    // A self session's call that asks for L0 runs in L1 all the same.
    r.turn(&sid, "proc_run", built(&t.dir, false)).await;
    let asked_l0 = kept.args.lock().unwrap()[1].clone();
    assert!(
        asked_l0.sandbox.is_some(),
        "a self session's command runs in L1"
    );
    assert!(
        asked_l0.env.iter().any(|(k, _)| k == "CARGO_TARGET_DIR"),
        "with its builder's folder"
    );
    assert!(
        builder_dir::root_in(&state(&r)).exists(),
        "the folder made for it"
    );

    // A self session whose item has no open tree.
    let closed = task_record(&r, "tides");
    r.turn(&closed, "proc_run", built(&work, true)).await;
    let none = kept.args.lock().unwrap()[2].clone();
    assert!(none.sandbox.unwrap().rw.is_empty(), "no open tree");
    assert!(!builder_dir::dir_in(&state(&r), "tides").exists());
}

/// A folder at its bound refuses the next command, and the result tells the builder what to do;
/// emptied, the next command starts.
#[tokio::test]
async fn a_full_folder_refuses_the_next_command_and_says_how_to_empty_it() {
    let kept = Arc::new(Kept::default());
    let r = rig(&kept);
    let (sid, t) = item(&r, "sails");
    builder_dir::set_max_for_test(1 << 20);
    let folder = builder_dir::ensure(&state(&r), "sails").unwrap();
    std::fs::write(folder.join("debug.bin"), vec![7u8; 2 << 20]).unwrap();
    let said = r.turn(&sid, "proc_run", built(&t.dir, true)).await;
    assert!(kept.args.lock().unwrap().is_empty(), "no job started");
    for want in [
        "reached its limit",
        "cargo clean",
        "CARGO_TARGET_DIR",
        "TMPDIR",
    ] {
        assert!(said.contains(want), "{want}: {said}");
    }
    std::fs::remove_file(folder.join("debug.bin")).unwrap();
    r.turn(&sid, "proc_run", built(&t.dir, true)).await;
    assert_eq!(kept.args.lock().unwrap().len(), 1, "emptied, it starts");
}

/// A command already running when its folder reaches the bound is stopped by the daemon, with the
/// reason as its result's; a folder under the bound, and a job that is no builder's, are left.
#[tokio::test]
async fn a_running_command_is_stopped_when_its_folder_reaches_the_bound() {
    let kept = Arc::new(Kept {
        hold: true,
        ..Kept::default()
    });
    let r = rig(&kept);
    let (sid, t) = item(&r, "sails");
    builder_dir::set_max_for_test(1 << 20);
    r.turn(&sid, "proc_run", built(&t.dir, true)).await;
    let job = only(&kept).correlation_id;
    let wrapper = crate::peer::Standin::start(&job);
    r.core.spool.write_pid(&job, wrapper.wrapper).unwrap();
    assert_eq!(r.core.stop_jobs_over_builder_bound().await, 0, "under it");

    let folder = builder_dir::dir_in(&state(&r), "sails");
    std::fs::write(folder.join("debug.bin"), vec![7u8; 2 << 20]).unwrap();
    assert_eq!(r.core.stop_jobs_over_builder_bound().await, 1);
    let a = r.core.kernel.action(&job).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Cancelled);
    let why = a.resolution.unwrap();
    assert!(
        why.starts_with("stopped by the build folder's limit") && why.contains("cargo clean"),
        "{why}"
    );
    assert_eq!(
        r.core.stop_jobs_over_builder_bound().await,
        0,
        "not stopped twice"
    );
}

/// The folder lives and dies with the item's tree: `close` removes it (either way a tree is
/// closed), and an open starts with none, whatever an earlier item of that name left.
#[tokio::test]
async fn closing_a_tree_removes_its_folder_and_opening_one_starts_with_none() {
    let kept = Arc::new(Kept::default());
    let r = rig(&kept);
    let state = state(&r);
    for keeping in [false, true] {
        let id = if keeping { "tides" } else { "sails" };
        let t = tree::open(&r.f.place, id, theseus_protocol::now_unix_ms()).unwrap();
        let folder = builder_dir::ensure(&state, id).unwrap();
        std::fs::write(folder.join("lib.rlib"), "what a builder compiled").unwrap();
        // A build left part of it read-only; and the gate's build tree too: both still go.
        let target_too = if keeping {
            vec![]
        } else {
            vec![t.target.join("ro/x")]
        };
        for d in std::iter::once(folder.join("ro/x")).chain(target_too) {
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("f"), "x").unwrap();
            std::fs::set_permissions(&d, std::os::unix::fs::PermissionsExt::from_mode(0o500))
                .unwrap();
            std::fs::set_permissions(
                d.parent().unwrap(),
                std::os::unix::fs::PermissionsExt::from_mode(0o500),
            )
            .unwrap();
        }
        if keeping {
            tree::close_keeping_target(&r.f.place, &t).unwrap();
        } else {
            tree::close(&r.f.place, &t).unwrap();
        }
        assert!(!folder.exists(), "closed (keeping the target: {keeping})");
        assert_eq!(t.target.exists(), keeping, "the gate's build tree");
    }
    // Left behind by an earlier item of the name (a close that could not finish): not inherited.
    let stale = builder_dir::ensure(&state, "gale").unwrap();
    std::fs::write(stale.join("old.rlib"), "an earlier item's").unwrap();
    tree::open(&r.f.place, "gale", theseus_protocol::now_unix_ms()).unwrap();
    assert!(
        !stale.join("old.rlib").exists(),
        "a new item's folder is its own"
    );
}

/// The folder is no slot, and it is on the filesystem the capacity check reads: the check is
/// given a path under `<state>/self`, and the trees it counts are `<state>/self/trees`.
#[tokio::test]
async fn the_folder_is_beside_the_trees_and_on_the_disk_the_capacity_check_reads() {
    let kept = Arc::new(Kept::default());
    let r = rig(&kept);
    let p = &r.f.place;
    let (_sid, _t) = item(&r, "sails");
    let before = crate::rsi::capacity::trees_open(&p.trees());
    let folder = builder_dir::ensure(&state(&r), "sails").unwrap();
    std::fs::write(folder.join("a.rlib"), vec![1u8; 4096]).unwrap();
    assert_eq!(crate::rsi::capacity::trees_open(&p.trees()), before);
    assert!(!folder.starts_with(p.trees().canonicalize().unwrap()));
    assert_eq!(p.builders().join("sails"), folder);
    // `Core::pipeline_room` reads `place.root()`, or the state dir before it exists.
    let read = std::fs::metadata(p.root()).unwrap().dev();
    assert_eq!(std::fs::metadata(&folder).unwrap().dev(), read);
    assert!(builder_dir::size(&folder) >= 4096, "its bytes are counted");
}

/// Pin (d): what a builder's command leaves in its tree, read-write in its view, is not followed when the
/// daemon closes the tree: a `target` link (the name the gate's link takes) and a link to a directory with a
/// file outside the tree are removed as links, and what they point at is untouched.
#[tokio::test]
async fn closing_a_tree_does_not_follow_the_links_its_builder_left() {
    let kept = Arc::new(Kept::default());
    let r = rig(&kept);
    let (_, t) = item(&r, "sails");
    let outside = r.f.place.root().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("daemons"), "kept\n").unwrap();
    std::os::unix::fs::symlink(&outside, t.dir.join("target")).unwrap();
    std::fs::create_dir_all(t.dir.join("deep")).unwrap();
    std::os::unix::fs::symlink(&outside, t.dir.join("deep/away")).unwrap();
    tree::close(&r.f.place, &t).unwrap();
    assert!(!t.dir.exists(), "the tree is closed");
    assert_eq!(
        std::fs::read_to_string(outside.join("daemons")).unwrap(),
        "kept\n"
    );
}
