//! A self session removes what is its own (issue 20; `rsi::own_removals`), through a whole core.
//!
//! Unless a test says otherwise the fake Jev answers `irreversible_reshapeable` at 0.96, so a hold is
//! what the gate would do with the call, and what each test reads is whether the call was held. A
//! pair makes one claim: the call of a self session runs, and the same call from an operator's
//! session is held, so neither half can pass against a tree where the rule fires for everyone or for
//! no one. The places are laid out the same for every principal:
//!
//! ```text
//! <base>/state/self/trees/sails      the self task's working copy (its session's own)
//! <base>/state/self/trees/tides      another task's working copy
//! <base>/state/self/review/br-one    a review's scratch (registered for the session that runs it)
//! <base>/state, <base>/work, <base>/repo   the state dir, the workspace, the self repo
//! ```
//!
//! `<base>` is itself under the system temporary directory, which is what makes the places the rule
//! keeps (the state dir, the repo, the workspace) a test: only they stop a removal there from being
//! "in the temporary directory".

use super::*;
use theseus_kernel::Authority;
use theseus_protocol::rsi::SelfMode;
use theseus_protocol::tasks::{TaskOrigin, TaskRecord, TaskState};

pub(super) fn base(target: &Path) -> PathBuf {
    target.parent().unwrap().parent().unwrap().to_path_buf()
}

pub(super) fn tree(target: &Path, id: &str) -> PathBuf {
    base(target).join("state/self/trees").join(id)
}

pub(super) fn builder(target: &Path) -> PathBuf {
    base(target).join("state/self/builder/sails")
}

fn scratch(target: &Path) -> PathBuf {
    base(target).join("state/self/review/br-one")
}

/// `[self] mode = "act"` over a repository: the self trees are a root of the file tools.
fn act(c: &mut Config) {
    let repo = Path::new(&c.server.state_dir)
        .parent()
        .unwrap()
        .join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    c.self_improve.mode = SelfMode::Act;
    c.self_improve.repo = Some(repo.to_string_lossy().into_owned());
}

/// What a case aims a call with, from the rig's target.
type Aim<T> = fn(&Path) -> T;

pub(super) fn nothing(_: &mut Config) {}

/// The places in the module's header, holding old bytes, made before the script is built so that a
/// call may name a link inside them.
pub(super) fn lay(b: &Path) {
    let t = b.join("work/notes.txt");
    for id in ["sails", "tides"] {
        std::fs::create_dir_all(tree(&t, id)).unwrap();
        std::fs::write(tree(&t, id).join("notes.txt"), WAS).unwrap();
    }
    std::fs::create_dir_all(scratch(&t).join("tree")).unwrap();
    std::fs::write(scratch(&t).join("tree/built.txt"), WAS).unwrap();
    std::fs::write(b.join("state/notes.txt"), WAS).unwrap();
    std::fs::write(b.join("repo/README"), WAS).unwrap();
    // The item's tree is open (a build folder is named only while it is), with its build folder.
    std::fs::create_dir_all(tree(&t, "sails").join(".git")).unwrap();
    std::fs::create_dir_all(builder(&t).join("target")).unwrap();
    std::fs::write(builder(&t).join("target/out.o"), WAS).unwrap();
    // Beside the workspace, in the temporary directory and under none of the places kept.
    std::fs::write(b.join("victim.txt"), WAS).unwrap();
}

/// A core whose one scripted call is `name` with `input`, from a session of `principal` (the task
/// `sails` when that is `self`), with every place above laid out and holding old bytes. `verdict`
/// is what Jev answers.
pub(super) fn drive_says(
    verdict: &str,
    principal: &str,
    name: &'static str,
    input: impl FnOnce(&Path) -> Value,
    tweak: impl FnOnce(&mut Config),
) -> (Rig, String) {
    let r = rig_launched(
        verdict,
        0.96,
        false,
        |c| {
            act(c);
            lay(Path::new(&c.server.state_dir).parent().unwrap());
            tweak(c);
        },
        move |t| {
            vec![
                Scripted::tools("Going.", &[("c1", name, input(t))]),
                Scripted::text("Done."),
            ]
        },
        Some(Arc::new(Recorder)),
    );
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    let authority = Authority {
        principal: principal.into(),
        ..Default::default()
    };
    let exec = r
        .core
        .kernel
        .open_execution(&rec.session_id, rec.kind, authority, None, None)
        .unwrap();
    rec.execution_id = Some(exec.id);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    if principal == "self" {
        let task = TaskRecord {
            id: "sails".into(),
            version: 1,
            title: "sails".into(),
            state: TaskState::InProgress,
            session: Some(rec.session_id.clone()),
            origin: TaskOrigin {
                session: "self:backlog".into(),
                principal: "self".into(),
                ..Default::default()
            },
            ..TaskRecord::default()
        };
        let task = crate::task_graph::record(&task).unwrap();
        r.core.store.append(&[task]).unwrap();
    }
    (r, rec.session_id)
}

/// [`drive_says`] with the answer that holds.
pub(super) fn drive(
    principal: &str,
    name: &'static str,
    input: impl FnOnce(&Path) -> Value,
    tweak: impl FnOnce(&mut Config),
) -> (Rig, String) {
    drive_says(RESHAPEABLE, principal, name, input, tweak)
}

impl Rig {
    /// Whether the one call was held, as the gate's own record says it: a `Declined` result and a
    /// `tool.held` row, which must agree. Jev was asked once, held or not.
    pub(super) fn was_held(&self, sid: &str) -> bool {
        let results = self.results(sid);
        let rows = self.rows("tool.held").len();
        let declined = results.iter().any(|(s, _, _)| s == "Declined");
        assert_eq!(declined, rows == 1, "{results:?} and {rows} tool.held rows");
        assert_eq!(self.jev.seen().len(), 1, "the layer asks as it did");
        declined
    }
}

/// Every job any rig of this process launched, as the wrapper was given it. Each test is a process
/// of its own, and a job names its session (`THESEUS_SESSION`), so a test reads its own.
static JOBS: std::sync::Mutex<Vec<theseus_kernel::job::WrapperArgs>> =
    std::sync::Mutex::new(Vec::new());

/// Launches as the inline launcher does, and keeps what each job was given.
struct Recorder;

impl crate::toolrun::JobLauncher for Recorder {
    fn launch(
        &self,
        spool: &theseus_kernel::spool::Spool,
        args: &theseus_kernel::job::WrapperArgs,
        done: crate::toolrun::JobDone,
    ) -> anyhow::Result<u32> {
        JOBS.lock().unwrap().push(args.clone());
        crate::toolrun::InlineLauncher.launch(spool, args, done)
    }
}

/// The jobs `session` started, in order.
pub(super) fn jobs_of(session: &str) -> Vec<theseus_kernel::job::WrapperArgs> {
    let mark = theseus_protocol::JOB_SESSION_ENV;
    let all = JOBS.lock().unwrap();
    all.iter()
        .filter(|a| a.env.iter().any(|(k, v)| k == mark && v == session))
        .cloned()
        .collect()
}

/// Whether the session's calls were bound to L1: a job started, each with an L1 view, none at L0. In this rig an
/// L1 job's init does not start, so what the command would do in L1 is not seen here (`theseusd/tests/self_build.rs`
/// runs the real view); what is seen is that nothing ran at L0.
pub(super) fn bound_to_l1(session: &str) -> bool {
    let jobs = jobs_of(session);
    !jobs.is_empty() && jobs.iter().all(|j| j.sandbox.is_some())
}

/// Whether any call of the session ran at L0.
pub(super) fn ran_at_l0(session: &str) -> bool {
    jobs_of(session).iter().any(|j| j.sandbox.is_none())
}

pub(super) fn read(p: PathBuf) -> String {
    std::fs::read_to_string(p).unwrap()
}

fn write_to(file: fn(&Path) -> PathBuf) -> impl FnOnce(&Path) -> Value {
    move |t| json!({"path": file(t).to_string_lossy(), "content": "new\n"})
}

fn rm_of(path: fn(&Path) -> PathBuf) -> impl FnOnce(&Path) -> Value {
    move |t| json!({"argv": ["rm", "-rf", path(t).to_string_lossy()]})
}

fn notes_of_sails(t: &Path) -> PathBuf {
    tree(t, "sails").join("notes.txt")
}

fn built_in_scratch(t: &Path) -> PathBuf {
    scratch(t).join("tree")
}

// ---- the file tools ------------------------------------------------------

/// KEY: overwriting an untracked file in its own working copy, which the floor holds for anyone,
/// runs for a self session and is held, as before, for an operator's.
#[tokio::test(flavor = "multi_thread")]
async fn a_write_over_a_file_in_its_own_tree_runs_for_a_self_session_only() {
    for (principal, runs) in [("self", true), ("operator", false)] {
        let (r, sid) = drive(principal, "fs_write", write_to(notes_of_sails), nothing);
        r.turn(&sid).await;
        assert_eq!(
            r.was_held(&sid),
            !runs,
            "{principal}: {:?}",
            r.results(&sid)
        );
        let now = read(notes_of_sails(&r.target));
        assert_eq!(now, if runs { "new\n" } else { WAS }, "{principal}");
    }
}

/// KEY: a patch that deletes an untracked file of its own working copy, which is a removal.
#[tokio::test(flavor = "multi_thread")]
async fn a_patch_that_deletes_a_file_of_its_own_tree_runs_for_a_self_session_only() {
    let delete = |t: &Path| {
        let file = notes_of_sails(t);
        let patch = format!(
            "--- {0}\n+++ /dev/null\n@@ -1 +0,0 @@\n-old bytes\n",
            file.display()
        );
        json!({"patch": patch})
    };
    for (principal, runs) in [("self", true), ("operator", false)] {
        let (r, sid) = drive(principal, "fs_patch", delete, nothing);
        r.turn(&sid).await;
        assert_eq!(
            r.was_held(&sid),
            !runs,
            "{principal}: {:?}",
            r.results(&sid)
        );
        assert_eq!(
            notes_of_sails(&r.target).exists(),
            !runs,
            "{principal}: the file"
        );
    }
}

/// What is not its own is held as before, for the file tools: the workspace, the rest of the state
/// dir, the self repo, another task's tree, and a path that gets to one of them by `..` or a link.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_sessions_write_aimed_outside_its_own_places_is_held() {
    let aims: [(&str, Aim<PathBuf>); 6] = [
        ("the workspace", |t| t.to_path_buf()),
        ("the rest of the state dir", |t| {
            base(t).join("state/notes.txt")
        }),
        ("the self repo", |t| base(t).join("repo/README")),
        ("another task's tree", |t| {
            tree(t, "tides").join("notes.txt")
        }),
        ("another tree by `..`", |t| {
            tree(t, "sails").join("../tides/notes.txt")
        }),
        ("the workspace by a link", |t| {
            let link = tree(t, "sails").join("out");
            std::os::unix::fs::symlink(t.parent().unwrap(), &link).unwrap();
            link.join("notes.txt")
        }),
    ];
    for (what, aim) in aims {
        let (r, sid) = drive("self", "fs_write", write_to(aim), nothing);
        r.turn(&sid).await;
        assert!(r.was_held(&sid), "{what}: {:?}", r.results(&sid));
        assert_eq!(read(base(&r.target).join("state/notes.txt")), WAS, "{what}");
    }
}

// ---- rm, which the code reads only as an argv ----------------------------

/// KEY (issue 53): a self session's removal of a review's scratch is bound to L1, unheld, and runs nowhere at L0
/// (the daemon removes a review's scratch); the same call from an operator's session is held, as before.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_sessions_removal_of_a_reviews_scratch_is_bound_to_l1_and_an_operators_is_held() {
    for (principal, l1) in [("self", true), ("operator", false)] {
        let (r, sid) = drive(principal, "proc_run", rm_of(built_in_scratch), nothing);
        r.core
            .tools
            .self_pr
            .review_opened(&sid, &scratch(&r.target));
        r.turn(&sid).await;
        assert_eq!(r.was_held(&sid), !l1, "{principal}: {:?}", r.results(&sid));
        assert_eq!(bound_to_l1(&sid), l1, "{principal}");
        assert_eq!(
            built_in_scratch(&r.target).exists(),
            !ran_at_l0(&sid),
            "{principal}: nothing removed it at L0"
        );
    }
}

/// KEY (issue 53): a self session's removal in its item's build folder, or of the folder, the builders' root or
/// another item's folder, is bound to L1 and unheld, and runs nowhere at L0; in L1 only its own folder is a
/// read-write bind (`theseusd/tests/self_build.rs` proves what persists). An operator's session's is held.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_sessions_removal_about_its_build_folder_is_bound_to_l1_and_an_operators_is_held() {
    for (principal, runs) in [("self", true), ("operator", false)] {
        let (r, sid) = drive(
            principal,
            "proc_run",
            rm_of(|t| builder(t).join("target")),
            nothing,
        );
        r.turn(&sid).await;
        assert_eq!(
            r.was_held(&sid),
            !runs,
            "{principal}: {:?}",
            r.results(&sid)
        );
        assert_eq!(bound_to_l1(&sid), runs, "{principal}");
    }
    let aims: [(&str, Aim<PathBuf>); 3] = [
        ("the folder itself", builder),
        ("the builders' root", |t| {
            builder(t).parent().unwrap().to_path_buf()
        }),
        ("another item's folder", |t| {
            builder(t).with_file_name("tides")
        }),
    ];
    for (what, aim) in aims {
        let (r, sid) = drive("self", "proc_run", |t| rm_argv("-rf", aim(t)), nothing);
        std::fs::create_dir_all(builder(&r.target).with_file_name("tides")).unwrap();
        r.turn(&sid).await;
        assert!(!ran_at_l0(&sid), "{what}: {:?}", r.results(&sid));
        assert!(builder(&r.target).join("target/out.o").exists(), "{what}");
    }
}

/// A file of its working copy is its own. Another lane's directory under the system temporary
/// directory is nobody's scratch of this session's: it is held, and it is still there.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_sessions_removal_in_its_tree_or_another_lanes_directory_is_bound_to_l1() {
    let lane = crate::scratch::dir("self-own-other-lane");
    std::fs::create_dir_all(lane.join("worktree")).unwrap();
    std::fs::write(lane.join("worktree/unpushed.rs"), WAS).unwrap();
    let one = |p: PathBuf| json!({"argv": ["rm", "-rf", "--", p.to_string_lossy()]});
    let (r, sid) = drive("self", "proc_run", |t| one(notes_of_sails(t)), nothing);
    r.turn(&sid).await;
    assert!(!r.was_held(&sid), "{:?}", r.results(&sid));
    assert!(
        bound_to_l1(&sid),
        "its own tree's file: in L1, where the tree is no read-write bind"
    );
    for aim in [
        lane.join("worktree"),
        lane.join("worktree/unpushed.rs"),
        lane.clone(),
    ] {
        let held = aim.clone();
        let (r, sid) = drive("self", "proc_run", move |_| one(held), nothing);
        r.turn(&sid).await;
        assert!(!ran_at_l0(&sid), "{}: {:?}", aim.display(), r.results(&sid));
        assert!(
            lane.join("worktree/unpushed.rs").exists(),
            "{}",
            aim.display()
        );
    }
    let file = |_: &Path| json!({"path": lane.join("worktree/unpushed.rs").to_string_lossy(), "content": "x\n"});
    let (r, sid) = drive("self", "fs_write", file, nothing);
    r.turn(&sid).await;
    assert!(r.was_held(&sid), "{:?}", r.results(&sid));
    assert_eq!(
        read(lane.join("worktree/unpushed.rs")),
        WAS,
        "an overwrite is held too"
    );
}

/// A self session's removal aimed anywhere else (the sweep over a parent, another task's tree, the places that are
/// the layer's own, a shell line, a batch, a program of the call's own `PATH`, a flag the code does not read) is
/// bound to L1 (issue 53), where no such place is a read-write bind, and runs nowhere at L0: nothing goes.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_aimed_outside_its_own_places_runs_nowhere_at_l0() {
    let aims: [(&str, Aim<Value>); 12] = [
        (
            "the workspace",
            |t| json!({"argv": ["rm", "-rf", t.parent().unwrap()]}),
        ),
        (
            "a file of the workspace",
            |t| json!({"argv": ["rm", "-f", t]}),
        ),
        (
            "another task's tree",
            |t| json!({"argv": ["rm", "-rf", tree(t, "tides")]}),
        ),
        (
            "the state dir",
            |t| json!({"argv": ["rm", "-rf", base(t).join("state")]}),
        ),
        (
            "the self repo",
            |t| json!({"argv": ["rm", "-rf", base(t).join("repo")]}),
        ),
        (
            "the base, which holds them all",
            |t| json!({"argv": ["rm", "-rf", base(t)]}),
        ),
        (
            "its own working copy itself",
            |t| json!({"argv": ["rm", "-rf", tree(t, "sails")]}),
        ),
        (
            "the reviews' root",
            |t| json!({"argv": ["rm", "-rf", base(t).join("state/self/review")]}),
        ),
        (
            "a shell line",
            |t| json!({"command": format!("rm -rf {}", built_in_scratch(t).display())}),
        ),
        (
            "a batch",
            |t| json!({"steps": [{"argv": ["rm", "-rf", built_in_scratch(t)]}]}),
        ),
        (
            "a PATH of its own",
            |t| json!({"argv": ["rm", "-rf", built_in_scratch(t)], "env": {"PATH": "/usr/bin"}}),
        ),
        (
            "a flag it does not read",
            |t| json!({"argv": ["rm", "-rfi", built_in_scratch(t)]}),
        ),
    ];
    for (what, input) in aims {
        let (r, sid) = drive("self", "proc_run", input, nothing);
        r.core
            .tools
            .self_pr
            .review_opened(&sid, &scratch(&r.target));
        r.turn(&sid).await;
        assert!(!ran_at_l0(&sid), "{what}: {:?}", r.results(&sid));
        assert!(
            r.target.exists()
                && built_in_scratch(&r.target).exists()
                && base(&r.target).join("sealed").exists(),
            "{what}: nothing went"
        );
    }
}

/// One case of a removal that gets out of its own places: what the call is, and what must still be
/// there when it has been held.
struct Escape {
    what: &'static str,
    input: Aim<Value>,
    survivor: Aim<PathBuf>,
}

fn link(at: PathBuf, to: &Path) -> PathBuf {
    std::os::unix::fs::symlink(to, &at).unwrap();
    at
}

fn rm_argv(flags: &str, p: PathBuf) -> Value {
    json!({"argv": ["rm", flags, p.to_string_lossy()]})
}

/// A path that starts in an own place and leaves it runs nowhere at L0 (issue 53: a self session's removal is bound
/// to L1, where no such path is a read-write bind), and what it aims at is still there. A `..` is not read.
/// A link followed by a `..` is the case a resolver that drops the `..` first gets wrong (the
/// review's finding): `out/../victim.txt` is `<base>/victim.txt` to the kernel, and the resolver's
/// `<scratch>/tree/victim.txt` is its own. And a link outside that points into its own places is
/// removed as a link, which is not its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_path_that_leaves_its_own_places_runs_nowhere_at_l0() {
    let cases = [
        Escape {
            what: "a `..`",
            input: |t| rm_argv("-rf", built_in_scratch(t).join("../../../../../work")),
            survivor: |t| t.to_path_buf(),
        },
        Escape {
            what: "a link out",
            input: |t| {
                rm_argv(
                    "-rf",
                    link(built_in_scratch(t).join("out"), t.parent().unwrap()).join("notes.txt"),
                )
            },
            survivor: |t| t.to_path_buf(),
        },
        Escape {
            what: "a link, then a `..`",
            input: |t| {
                rm_argv(
                    "-f",
                    link(built_in_scratch(t).join("out"), t.parent().unwrap())
                        .join("../victim.txt"),
                )
            },
            survivor: |t| base(t).join("victim.txt"),
        },
        Escape {
            what: "a link out of its tree, then a `..`",
            input: |t| {
                rm_argv(
                    "-f",
                    link(tree(t, "sails").join("out"), t.parent().unwrap()).join("../victim.txt"),
                )
            },
            survivor: |t| base(t).join("victim.txt"),
        },
        Escape {
            what: "a link out of its tree, written with a trailing slash",
            input: |t| {
                let out = base(t).join("outside");
                std::fs::create_dir_all(&out).unwrap();
                std::fs::write(out.join("victim.txt"), WAS).unwrap();
                let l = link(tree(t, "sails").join("out"), &out);
                rm_argv("-rf", PathBuf::from(format!("{}/", l.display())))
            },
            survivor: |t| base(t).join("outside/victim.txt"),
        },
        Escape {
            what: "a link of the temporary directory to the state dir, with two slashes",
            input: |t| {
                let l = link(base(t).join("stateful"), &base(t).join("state"));
                rm_argv("-rf", PathBuf::from(format!("{}//", l.display())))
            },
            survivor: |t| base(t).join("state/notes.txt"),
        },
        Escape {
            what: "a link of the workspace that points into its tree",
            input: |t| {
                rm_argv(
                    "-f",
                    link(t.parent().unwrap().join("pointer"), &tree(t, "sails")),
                )
            },
            survivor: |t| t.parent().unwrap().join("pointer"),
        },
    ];
    for Escape {
        what,
        input,
        survivor,
    } in cases
    {
        let (r, sid) = drive("self", "proc_run", input, nothing);
        r.core
            .tools
            .self_pr
            .review_opened(&sid, &scratch(&r.target));
        r.turn(&sid).await;
        assert!(!ran_at_l0(&sid), "{what}: {:?}", r.results(&sid));
        assert!(
            survivor(&r.target).symlink_metadata().is_ok(),
            "{what}: it is still there"
        );
    }
}

/// A self session's removal of a link inside its tree is bound to L1 and unheld (issue 53); nothing ran at L0, so in
/// this rig the link and what it points at are both still there.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_inside_its_tree_is_removed_in_l1_and_nothing_at_l0() {
    let own_link = |t: &Path| {
        rm_argv(
            "-f",
            link(tree(t, "sails").join("pointer"), t.parent().unwrap()),
        )
    };
    let (r, sid) = drive("self", "proc_run", own_link, nothing);
    r.turn(&sid).await;
    assert!(!r.was_held(&sid), "{:?}", r.results(&sid));
    assert!(
        tree(&r.target, "sails")
            .join("pointer")
            .symlink_metadata()
            .is_ok()
            && bound_to_l1(&sid),
        "bound to L1, and nothing removed the link at L0"
    );
    assert!(r.target.exists(), "what it pointed at did not");
}

/// The sealed set is out of reach of every tool and job: a refusal, before any model is asked, for a
/// self session as for anyone. It is not a hold, and the rule never reads it.
#[tokio::test(flavor = "multi_thread")]
async fn the_sealed_set_is_still_refused_before_anything_is_asked() {
    let (r, sid) = drive(
        "self",
        "proc_run",
        |t| rm_argv("-rf", base(t).join("sealed")),
        nothing,
    );
    r.turn(&sid).await;
    let results = r.results(&sid);
    assert!(
        results
            .iter()
            .any(|(s, text, _)| s == "Declined" && text.contains("sealed")),
        "{results:?}"
    );
    assert!(r.jev.seen().is_empty(), "no model was asked");
    assert!(base(&r.target).join("sealed").exists());
}

/// A scratch is the session's that registered it. A self session with none, a session that is not a
/// self session's though one is registered for it, and an entry outside the reviews' own directory,
/// all hold the same call.
#[tokio::test(flavor = "multi_thread")]
async fn a_scratch_belongs_to_the_session_that_registered_it() {
    // A self session's removal is bound to L1 whatever is registered (issue 53): it runs nowhere at L0.
    let (r, sid) = drive("self", "proc_run", rm_of(built_in_scratch), nothing);
    r.turn(&sid).await;
    assert!(
        !ran_at_l0(&sid),
        "no review registered a scratch for this session"
    );
    let (r, sid) = drive("operator", "proc_run", rm_of(built_in_scratch), nothing);
    r.core
        .tools
        .self_pr
        .review_opened(&sid, &scratch(&r.target));
    r.turn(&sid).await;
    assert!(
        r.was_held(&sid),
        "a registration does not make a self session"
    );
    let (r, sid) = drive("self", "proc_run", rm_of(built_in_scratch), nothing);
    let elsewhere = base(&r.target).join("work");
    r.core.tools.self_pr.review_opened(&sid, &elsewhere);
    r.turn(&sid).await;
    assert!(
        !ran_at_l0(&sid),
        "an entry outside the reviews' directory is no scratch"
    );
}

// ---- L1 ------------------------------------------------------------------

fn sandboxed(t: &Path) -> Value {
    json!({"argv": ["rm", "-rf", t], "sandbox": true})
}

fn operator_egress(c: &mut Config) {
    c.sandbox.egress = vec!["*.amazonaws.com:443".into()];
}

/// KEY: a sandboxed removal is not held for a self session, which writes only to what its view binds
/// read-write (its own build folder): and that holds on a daemon whose `[sandbox] egress` names a
/// host, because a self session's sandboxed command does not carry the operator's list. The same
/// call from an operator's session is held as before, with the list or without; a self call that
/// names a host of its own is held.
#[tokio::test(flavor = "multi_thread")]
async fn a_sandboxed_removal_is_not_held_for_a_self_session_even_where_the_operator_has_a_network_list(
) {
    for (what, tweak) in [
        ("no list", nothing as fn(&mut Config)),
        ("the operator's list", operator_egress),
    ] {
        let (r, sid) = drive("self", "proc_run", sandboxed, tweak);
        r.turn(&sid).await;
        assert!(!r.was_held(&sid), "{what}: {:?}", r.results(&sid));
        assert!(
            r.target.exists(),
            "{what}: an L1 call's removal does not stay"
        );
        let (r, sid) = drive("operator", "proc_run", sandboxed, tweak);
        r.turn(&sid).await;
        assert!(r.was_held(&sid), "{what}: another principal");
    }
    let hosts =
        |t: &Path| json!({"argv": ["rm", "-rf", t], "sandbox": {"egress": ["example.com:443"]}});
    let (r, sid) = drive("self", "proc_run", hosts, operator_egress);
    r.turn(&sid).await;
    assert!(r.was_held(&sid), "a self call that names a host of its own");
    assert!(r.target.exists());
}

/// The builder's ordinary way of working, a shell line in the sandbox that removes and then builds,
/// runs for a self session on a daemon that has a network list, and is held for an operator.
#[tokio::test(flavor = "multi_thread")]
async fn a_sandboxed_shell_line_that_removes_a_file_of_the_working_copy_runs_for_a_self_session() {
    let line = |t: &Path| json!({"command": format!("rm -rf {} && true", notes_of_sails(t).display()), "sandbox": true});
    let (r, sid) = drive("self", "proc_run", line, operator_egress);
    r.turn(&sid).await;
    assert!(!r.was_held(&sid), "{:?}", r.results(&sid));
    let (r, sid) = drive("operator", "proc_run", line, operator_egress);
    r.turn(&sid).await;
    assert!(r.was_held(&sid), "the same line from an operator's session");
    assert!(notes_of_sails(&r.target).exists());
}

/// A self session's sandboxed command cannot reach the operator's egress host: the list its job is
/// given is the self builder's own, and an operator's session's job is given the operator's.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_sessions_sandboxed_command_is_not_given_the_operators_network() {
    let (r, sid) = drive("self", "proc_run", sandboxed, operator_egress);
    r.turn(&sid).await;
    let jobs = jobs_of(&sid);
    assert_eq!(jobs.len(), 1, "{:?}", r.results(&sid));
    let given = &jobs[0].sandbox.as_ref().expect("an L1 job").egress;
    assert_eq!(*given, crate::rsi::deps::hosts::egress());
    assert!(!given.iter().any(|h| h.contains("amazonaws")), "{given:?}");
    // An operator's session, whose call Jev calls reversible so that it runs, is given the operator's.
    let (r, sid) = drive_says(
        REVERSIBLE,
        "operator",
        "proc_run",
        |t| json!({"argv": ["true"], "cwd": t.parent().unwrap(), "sandbox": true}),
        operator_egress,
    );
    r.turn(&sid).await;
    let jobs = jobs_of(&sid);
    assert_eq!(jobs.len(), 1, "{:?}", r.results(&sid));
    assert_eq!(
        jobs[0].sandbox.as_ref().unwrap().egress,
        ["*.amazonaws.com:443"]
    );
}

/// A self session that is no builder (its item's tree is not open, as a review's session is not) is
/// given no network at all, and its sandboxed command still runs unheld.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_session_that_is_no_builder_is_given_no_network() {
    let (r, sid) = drive("self", "proc_run", sandboxed, operator_egress);
    std::fs::remove_dir_all(tree(&r.target, "sails").join(".git")).unwrap();
    r.turn(&sid).await;
    assert!(!r.was_held(&sid), "{:?}", r.results(&sid));
    let jobs = jobs_of(&sid);
    assert_eq!(jobs.len(), 1, "{:?}", r.results(&sid));
    assert!(jobs[0].sandbox.as_ref().unwrap().egress.is_empty());
}

/// A self session's shell line that main held at L0 (and told what does run) is bound to L1 and not held (issue 53),
/// so it is told none of that. An operator's held call is told what it always was.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_sessions_shell_line_is_bound_to_l1_and_not_held() {
    let line = |t: &Path| json!({"command": format!("rm -rf {}", built_in_scratch(t).display())});
    let (r, sid) = drive("self", "proc_run", line, nothing);
    r.turn(&sid).await;
    assert!(!r.was_held(&sid) && bound_to_l1(&sid));
    let said = r.results(&sid)[0].1.clone();
    for part in [
        "these run",
        "`rm` given as an argv",
        "the file tools",
        "sandbox: true",
        "review's scratch is removed",
    ] {
        assert!(!said.contains(part), "{part}: {said}");
    }
    let (r, sid) = drive("operator", "proc_run", line, nothing);
    r.turn(&sid).await;
    assert!(r.was_held(&sid));
    assert!(
        !r.results(&sid)[0].1.contains("these run"),
        "{:?}",
        r.results(&sid)
    );
}

/// The premise of the L1 half, from the code: the view a `proc.run` is given binds nothing
/// read-write, for a self session as for any other. A self builder's command is then given one more
/// bind (`with_builder_folder`), its own item's build folder, which is the session's own: the only
/// thing that persists.
#[tokio::test(flavor = "multi_thread")]
async fn the_view_a_self_sessions_command_is_given_binds_nothing_read_write() {
    let (r, _) = drive("self", "proc_run", sandboxed, nothing);
    assert!(r.core.tools.sandbox.job_view().rw.is_empty());
    let bound = crate::sandbox::Bound {
        class: crate::sandbox::Class::L1,
        egress: vec![],
    };
    let spec = theseus_tools::JobSpec {
        argv: vec!["true".into()],
        cwd: base(&r.target),
        timeout_secs: 5,
        env: vec![],
    };
    let ran_at = Posture::Open;
    let (_, view) =
        crate::sandbox::for_job(&r.core.tools, &bound, &spec, &[], None, ran_at, "corr_own").await;
    let view = view.expect("an L1 call is given a view");
    assert!(
        view.rw.is_empty(),
        "nothing a command writes stays: {:?}",
        view.rw
    );
}

// ---- what is not lowered -------------------------------------------------

/// `irreversible_final` reaches the owner for a self session's own write as for anyone's: the rule
/// lowers a hold and nothing above it.
#[tokio::test(flavor = "multi_thread")]
async fn a_final_verdict_on_a_self_sessions_own_write_still_asks_the_owner() {
    let (r, sid) = drive_says(FINAL, "self", "fs_write", write_to(notes_of_sails), nothing);
    r.core.tools.modes.set(&sid, SessionMode::Auto);
    r.turn(&sid).await;
    assert_eq!(r.rows("tool.raised").len(), 1, "the raise stands");
    assert_eq!(
        r.rows("tool.confirm_requested").len(),
        1,
        "the owner is asked"
    );
    assert!(r.rows("tool.held").is_empty());
    assert_eq!(
        read(notes_of_sails(&r.target)),
        WAS,
        "nothing is written until they answer"
    );
}

// ---- the review that makes the scratch ------------------------------------

/// The whole path of the incident, through the review's own turn: the reviewer is a session of the self principal
/// that `CheckTurn` opens, the scratch is the one its ask names, and what it removes there is bound to L1 (issue 53),
/// unheld. When it has answered, the scratch is no longer its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_reviewers_removal_in_its_scratch_is_bound_to_l1() {
    use crate::rsi::join::review::{Ask, CheckTurn, Reviewer};
    let r = rig_launched(
        RESHAPEABLE,
        0.96,
        false,
        |c| {
            act(c);
            lay(Path::new(&c.server.state_dir).parent().unwrap());
        },
        |t| {
            vec![
                Scripted::tools(
                    "Cleaning up.",
                    &[("c1", "proc_run", rm_of(built_in_scratch)(t))],
                ),
                Scripted::text(
                    "It does what it says.\nVERDICT: accept\n- the test fails without it",
                ),
            ]
        },
        Some(Arc::new(Recorder)),
    );
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let ask = Ask {
        branch: "br-one".into(),
        head: "0123abc".into(),
        item: "sails".into(),
        diff: String::new(),
        report: "a report".into(),
        evidence: "the evidence".into(),
        proof: String::new(),
        target,
        built_ms: 0,
        scratch: scratch(&r.target).display().to_string(),
        repo: base(&r.target).join("repo").display().to_string(),
        offer_fix: None,
        fixed: None,
    };
    let review = CheckTurn.review(&r.core, &ask).await.unwrap();
    let session = review.session.expect("the review's session");
    assert!(review.accept, "{:?}", review.reasons);
    assert!(
        r.rows("tool.held").is_empty(),
        "nothing the reviewer removed was held"
    );
    assert!(
        bound_to_l1(&session),
        "its removal was bound to L1, nowhere at L0"
    );
    assert_eq!(
        r.core.tools.self_pr.review_scratch(&session),
        None,
        "and the scratch is no longer its own"
    );
    let jobs = jobs_of(&session);
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    // Its L1 view binds the daemon's checkout of the reviewed head read-only (issue 53, STOP 1).
    let view = jobs[0].sandbox.as_ref().expect("an L1 view");
    let tree = crate::rsi::join::scratch::tree_of(&scratch(&r.target));
    assert!(view.ro_paths.contains(&tree), "{:?}", view.ro_paths);
    // In L1 its temporary files are the view's own `/tmp`: no `TMPDIR` under its scratch, which main gave its
    // command at L0 (`own_removals::review_tmp`).
    let tmp = jobs[0].env.iter().find(|(k, _)| k == "TMPDIR");
    let at_l0 = scratch(&r.target).join("tmp").display().to_string();
    assert_eq!(
        tmp.map(|(_, v)| v.as_str()).filter(|v| *v == at_l0),
        None,
        "{:?}",
        jobs[0].env
    );
}

/// A command of a session that is not a review is not given a `TMPDIR` of its own: the machine's.
#[tokio::test(flavor = "multi_thread")]
async fn a_command_that_is_no_reviews_keeps_the_temporary_directory_it_had() {
    let (r, sid) = drive_says(
        REVERSIBLE,
        "self",
        "proc_run",
        |t| json!({"argv": ["true"], "cwd": tree(t, "sails")}),
        nothing,
    );
    r.turn(&sid).await;
    let jobs = jobs_of(&sid);
    assert_eq!(jobs.len(), 1, "{:?}", r.results(&sid));
    let tmp = jobs[0]
        .env
        .iter()
        .find(|(k, _)| k == "TMPDIR")
        .map(|(_, v)| v.as_str());
    assert!(tmp.is_none_or(|t| !t.contains("self/review")), "{tmp:?}");
}

/// A link where the item's build folder goes (or at the builders' root) does not make what it points
/// at the session's own: the folder is a place only while it is its own resolved path. `rm` through it runs
/// nowhere at L0 (issue 53: bound to L1), a file write through it is held, a sandboxed shell line through it is
/// given no read-write bind, and the target's file stays (the round-4 review's B3).
#[tokio::test(flavor = "multi_thread")]
async fn a_link_where_the_build_folder_goes_is_not_the_sessions_own() {
    let swap = |t: &Path, root: bool| {
        let f = builder(t);
        let at = if root {
            f.parent().unwrap().to_path_buf()
        } else {
            f
        };
        std::fs::remove_dir_all(&at).unwrap();
        std::os::unix::fs::symlink(t.parent().unwrap(), &at).unwrap();
    };
    for root in [false, true] {
        let name = if root {
            "the builders' root"
        } else {
            "the folder"
        };
        let through = |t: &Path| {
            swap(t, root);
            json!({"argv": ["rm", "-f", builder(t).join("notes.txt").to_string_lossy()]})
        };
        let (r, sid) = drive("self", "proc_run", through, nothing);
        r.turn(&sid).await;
        assert!(!ran_at_l0(&sid), "{name}, rm: {:?}", r.results(&sid));
        assert!(r.target.exists(), "{name}, rm: the workspace file is there");
        let write = |t: &Path| {
            swap(t, root);
            json!({"path": builder(t).join("notes.txt").to_string_lossy(), "content": "x\n"})
        };
        let (r, sid) = drive("self", "fs_write", write, nothing);
        r.turn(&sid).await;
        assert!(r.was_held(&sid), "{name}, write: {:?}", r.results(&sid));
        assert_eq!(read(r.target.clone()), WAS, "{name}, write");
        let line = |t: &Path| {
            swap(t, root);
            json!({"command": "rm -f notes.txt", "sandbox": true, "cwd": t.parent().unwrap()})
        };
        // The folder is no place, so the session is no builder and its sandboxed call is given NO
        // read-write bind: it may run unheld, and what it removes is its overlay.
        let (r, sid) = drive("self", "proc_run", line, nothing);
        r.turn(&sid).await;
        assert!(
            !r.was_held(&sid),
            "{name}, shell line: {:?}",
            r.results(&sid)
        );
        let jobs = jobs_of(&sid);
        assert_eq!(jobs.len(), 1, "{name}: {:?}", r.results(&sid));
        let view = jobs[0].sandbox.as_ref().expect("an L1 job");
        assert!(
            view.rw.is_empty(),
            "{name}: no bind to the link: {:?}",
            view.rw
        );
        assert!(r.target.exists(), "{name}: the workspace file is there");
    }
}
