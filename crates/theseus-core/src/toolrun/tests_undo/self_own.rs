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

fn base(target: &Path) -> PathBuf {
    target.parent().unwrap().parent().unwrap().to_path_buf()
}

fn tree(target: &Path, id: &str) -> PathBuf {
    base(target).join("state/self/trees").join(id)
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

fn nothing(_: &mut Config) {}

/// The places in the module's header, holding old bytes, made before the script is built so that a
/// call may name a link inside them.
fn lay(b: &Path) {
    let t = b.join("work/notes.txt");
    for id in ["sails", "tides"] {
        std::fs::create_dir_all(tree(&t, id)).unwrap();
        std::fs::write(tree(&t, id).join("notes.txt"), WAS).unwrap();
    }
    std::fs::create_dir_all(scratch(&t).join("tree")).unwrap();
    std::fs::write(scratch(&t).join("tree/built.txt"), WAS).unwrap();
    std::fs::write(b.join("state/notes.txt"), WAS).unwrap();
    std::fs::write(b.join("repo/README"), WAS).unwrap();
}

/// A core whose one scripted call is `name` with `input`, from a session of `principal` (the task
/// `sails` when that is `self`), with every place above laid out and holding old bytes. `verdict`
/// is what Jev answers.
fn drive_says(
    verdict: &str,
    principal: &str,
    name: &'static str,
    input: impl FnOnce(&Path) -> Value,
    tweak: impl FnOnce(&mut Config),
) -> (Rig, String) {
    let r = rig_scripted(
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
fn drive(
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
    fn was_held(&self, sid: &str) -> bool {
        let results = self.results(sid);
        let rows = self.rows("tool.held").len();
        let declined = results.iter().any(|(s, _, _)| s == "Declined");
        assert_eq!(declined, rows == 1, "{results:?} and {rows} tool.held rows");
        assert_eq!(self.jev.seen().len(), 1, "the layer asks as it did");
        declined
    }
}

fn read(p: PathBuf) -> String {
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

/// KEY: a review's scratch, removed by the session that runs the review (its registry entry is the
/// scratch's only door); the same call from an operator's session is held.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_session_removes_its_reviews_scratch_and_an_operator_cannot() {
    for (principal, runs) in [("self", true), ("operator", false)] {
        let (r, sid) = drive(principal, "proc_run", rm_of(built_in_scratch), nothing);
        r.core
            .tools
            .self_pr
            .review_opened(&sid, &scratch(&r.target));
        r.turn(&sid).await;
        assert_eq!(
            r.was_held(&sid),
            !runs,
            "{principal}: {:?}",
            r.results(&sid)
        );
        assert_eq!(built_in_scratch(&r.target).exists(), !runs, "{principal}");
    }
}

/// A file of its working copy, and one in the temporary directory, are its own too.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_session_removes_a_file_of_its_tree_and_one_in_the_temporary_directory() {
    let outside = crate::scratch::dir("self-own-tmp");
    std::fs::create_dir_all(&outside).unwrap();
    let theirs = outside.join("build.o");
    std::fs::write(&theirs, WAS).unwrap();
    let one = |p: PathBuf| json!({"argv": ["rm", "-f", "--", p.to_string_lossy()]});
    let (r, sid) = drive("self", "proc_run", |t| one(notes_of_sails(t)), nothing);
    r.turn(&sid).await;
    assert!(!r.was_held(&sid), "{:?}", r.results(&sid));
    assert!(!notes_of_sails(&r.target).exists(), "its own tree's file");
    let (r, sid) = drive("self", "proc_run", |_| one(theirs.clone()), nothing);
    r.turn(&sid).await;
    assert!(!r.was_held(&sid), "{:?}", r.results(&sid));
    assert!(!theirs.exists(), "removed from the temporary directory");
}

/// A removal aimed anywhere else is held: the sweep over a parent, another task's tree, the places
/// that are the layer's own, and the shapes the code does not read (a shell line, a batch, a program
/// of the call's own `PATH`, a flag it does not know). The session has a scratch registered, so each
/// of the last four would run but for its shape.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_aimed_outside_its_own_places_is_held() {
    let aims: [(&str, Aim<Value>); 13] = [
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
            "the sealed set",
            |t| json!({"argv": ["rm", "-rf", base(t).join("sealed")]}),
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
        assert!(r.was_held(&sid), "{what}: {:?}", r.results(&sid));
        assert!(
            r.target.exists()
                && built_in_scratch(&r.target).exists()
                && base(&r.target).join("sealed").exists(),
            "{what}: nothing went"
        );
    }
}

/// A path that starts in the scratch and leaves it, by `..` or by a link, is judged where it lands.
#[tokio::test(flavor = "multi_thread")]
async fn a_path_that_leaves_the_scratch_is_judged_where_it_lands() {
    let by_dots = |t: &Path| {
        let up = built_in_scratch(t).join("../../../../../work");
        json!({"argv": ["rm", "-rf", up.to_string_lossy()]})
    };
    let by_link = |t: &Path| {
        let link = built_in_scratch(t).join("out");
        std::os::unix::fs::symlink(t.parent().unwrap(), &link).unwrap();
        json!({"argv": ["rm", "-rf", link.join("notes.txt").to_string_lossy()]})
    };
    let aims: [(&str, Aim<Value>); 2] = [("`..`", by_dots), ("a link", by_link)];
    for (what, input) in aims {
        let (r, sid) = drive("self", "proc_run", input, nothing);
        r.core
            .tools
            .self_pr
            .review_opened(&sid, &scratch(&r.target));
        r.turn(&sid).await;
        assert!(r.was_held(&sid), "{what}: {:?}", r.results(&sid));
        assert!(r.target.exists(), "{what}: the workspace file is there");
    }
}

/// A scratch is the session's that registered it. A self session with none, a session that is not a
/// self session's though one is registered for it, and an entry outside the reviews' own directory,
/// all hold the same call.
#[tokio::test(flavor = "multi_thread")]
async fn a_scratch_belongs_to_the_session_that_registered_it() {
    let (r, sid) = drive("self", "proc_run", rm_of(built_in_scratch), nothing);
    r.turn(&sid).await;
    assert!(
        r.was_held(&sid),
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
        r.was_held(&sid),
        "an entry outside the reviews' directory is no scratch"
    );
}

// ---- L1 ------------------------------------------------------------------

fn sandboxed(t: &Path) -> Value {
    json!({"argv": ["rm", "-rf", t], "sandbox": true})
}

/// KEY: a sandboxed removal is not held for a self session, which writes only to what its view binds
/// read-write, and that is nothing: the file is still there. The operator's is held as before, and
/// so is a call that carries a network list (its own, or `[sandbox] egress`'s): such a job could
/// reach what is not on this disk.
#[tokio::test(flavor = "multi_thread")]
async fn a_sandboxed_removal_is_not_held_for_a_self_session_with_no_network() {
    let (r, sid) = drive("self", "proc_run", sandboxed, nothing);
    r.turn(&sid).await;
    assert!(!r.was_held(&sid), "{:?}", r.results(&sid));
    assert!(r.target.exists(), "an L1 call's removal does not stay");
    let (r, sid) = drive("operator", "proc_run", sandboxed, nothing);
    r.turn(&sid).await;
    assert!(r.was_held(&sid), "another principal");
    let hosts =
        |t: &Path| json!({"argv": ["rm", "-rf", t], "sandbox": {"egress": ["example.com:443"]}});
    let (r, sid) = drive("self", "proc_run", hosts, nothing);
    r.turn(&sid).await;
    assert!(r.was_held(&sid), "a call that names hosts");
    let listed = |c: &mut Config| c.sandbox.egress = vec!["index.crates.io:443".into()];
    let (r, sid) = drive("self", "proc_run", sandboxed, listed);
    r.turn(&sid).await;
    assert!(r.was_held(&sid), "[sandbox] egress names a host");
    assert!(r.target.exists());
}

/// The premise of the L1 half, from the code: the view a `proc.run` is given binds nothing
/// read-write, for a self session as for any other, so nothing it writes persists.
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

/// The whole path of the incident, through the review's own turn: the reviewer is a session of the
/// self principal that `CheckTurn` opens, the scratch is the one its ask names, and what it removes
/// there runs. When it has answered, the scratch is no longer its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_reviewer_removes_the_checkout_it_made_in_its_scratch() {
    use crate::rsi::join::review::{Ask, CheckTurn, Reviewer};
    let r = rig_scripted(
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
        target,
        built_ms: 0,
        scratch: scratch(&r.target).display().to_string(),
        repo: base(&r.target).join("repo").display().to_string(),
    };
    let review = CheckTurn.review(&r.core, &ask).await.unwrap();
    let session = review.session.expect("the review's session");
    assert!(review.accept, "{:?}", review.reasons);
    assert!(
        r.rows("tool.held").is_empty(),
        "nothing the reviewer removed was held"
    );
    assert!(
        !built_in_scratch(&r.target).exists(),
        "the checkout it made is gone"
    );
    assert_eq!(
        r.core.tools.self_pr.review_scratch(&session),
        None,
        "and the scratch is no longer its own"
    );
}
