//! China-origin drafts reviewed by Claude (theseus-a1wvq.13). The rig is
//! `tests_route`'s with `dsflash` (GLM's model on the `zai` fake, judged
//! `cn`) as `routine_coding`'s `cn_draft` and the Claude `sonnet` as its
//! `final`, a temp dir holding a scratch git repository, the route pack acting
//! with a `routine_coding` verdict, the driver taking the tasks, and a judge
//! standing behind the runner that labels a secret or a confidential marker
//! (theseus-9ymz0: no open view decides whether `dsflash` may run; the labels
//! on the draft's own session do).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_protocol::SessionKind;

use crate::content::{Flag, ItemKind};
use crate::content_wait::judging::Judging;
use crate::draft;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::test_support::{ledger_rows_100k as rows, until_within};
use crate::tests_dm_rig::{say, OWNER};
use crate::tests_route::{decided, mode, rig_parts, Rig};
use crate::tests_tasks::{answers_a_call, first_user, posts};

/// The owner's coding ask.
pub(crate) const ASK: &str = "In notes.txt, change the word beta to gamma.";
/// An earlier message of the owner's, which no draft's brief or request holds.
const PRIVATE: &str = "My Widgetco salary review is notes-mark-5521.";
/// A line the test's judge labels `confidential`.
const CONFIDENTIAL: &str = "Examplecorp-unannounced-roadmap";
/// What the test's judge labels `secret` in a tool's result. It is no secret
/// the scrubber knows (that would be withheld before a node exists), so the
/// judge's rule stands for the model's own finding.
const KEYFILE_MARK: &str = "KEYFILE-MARK-4471";

/// A provider whose answer is a function of the request; every request kept.
pub(crate) struct By {
    script: Box<dyn Fn(&ProviderRequest) -> Scripted + Send + Sync>,
    pub(crate) requests: Mutex<Vec<ProviderRequest>>,
    /// A request whose first user message holds this never gets an answer.
    hang: Mutex<Option<String>>,
}

impl By {
    pub(crate) fn new(
        f: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            script: Box::new(f),
            requests: Mutex::default(),
            hang: Mutex::default(),
        })
    }

    pub(crate) fn sent(&self) -> Vec<ProviderRequest> {
        self.requests.lock().unwrap().clone()
    }

    pub(crate) fn hang_on(&self, words: &str) {
        *self.hang.lock().unwrap() = Some(words.into());
    }
}

impl Provider for By {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let hang = self.hang.lock().unwrap().clone();
            if hang.is_some_and(|h| first_user(req).contains(&h)) {
                std::future::pending::<()>().await;
            }
            let f = FakeProvider::scripted(vec![(self.script)(req)]);
            f.stream_message(req, on_delta).await
        })
    }
}

/// `git` in `dir`, which must succeed: its stdout.
pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A scratch repository with one commit of `notes.txt`.
pub(crate) fn repo(at: &Path) -> PathBuf {
    std::fs::create_dir_all(at).unwrap();
    git(at, &["init", "-q", "-b", "main"]);
    git(at, &["config", "user.name", "Acme Builder"]);
    git(at, &["config", "user.email", "builder@acme.test"]);
    std::fs::write(at.join("notes.txt"), "alpha\nbeta\n").unwrap();
    git(at, &["add", "-A"]);
    git(at, &["commit", "-q", "-m", "first"]);
    at.canonicalize().unwrap()
}

/// A test's tampering with the review's tree.
pub(crate) type Tamper = Box<dyn FnOnce(&Path) + Send>;

/// What the draft and the review do, and what the review saw.
#[derive(Default)]
pub(crate) struct Plan {
    /// The draft's call: none reports at once.
    pub(crate) draft_call: Option<(String, Value)>,
    /// More calls the draft makes beside it, in the same answer.
    draft_also: Vec<(String, Value)>,
    /// The review's edit of the tree (old, new), before it reports.
    review_edit: Option<(String, String)>,
    review_report: String,
    /// The owner's file as the review found it.
    pub(crate) seen_by_review: Option<String>,
    /// What the first pass reports, and whether it fails instead.
    pub(crate) first_pass_report: String,
    pub(crate) first_pass_fails: bool,
    /// Run in the review's first request with its tree: a test's tampering.
    pub(crate) tamper: Option<Tamper>,
    /// The review's first request, whole.
    pub(crate) review_saw: Option<String>,
}

pub(crate) struct Draft {
    pub(crate) r: Rig,
    pub(crate) zai: Arc<By>,
    pub(crate) claude: Arc<By>,
    pub(crate) plan: Arc<Mutex<Plan>>,
    /// The open root, and the owner's repository under it.
    pub(crate) work: PathBuf,
    pub(crate) repo: PathBuf,
    _tmp: tempfile::TempDir,
}

/// The tree a draft session works in, from the request it sent.
fn tree_in(req: &ProviderRequest) -> Option<PathBuf> {
    let body = serde_json::to_string(&json!({"s": req.system, "m": req.messages})).ok()?;
    let at = body.find("/drafts/")?;
    let start = body[..at].rfind(['"', ' ', '`', '('])? + 1;
    let end = at + body[at..].find(['"', ' ', '`', ')', '\\'])?;
    Some(PathBuf::from(&body[start..end]))
}

/// The review's answer: its edit of the tree once, then its report.
fn review_answer(plan: &Mutex<Plan>, owner_file: &Path, req: &ProviderRequest) -> Scripted {
    if !first_user(req).contains("Review the draft") {
        return Scripted::text("ok");
    }
    let mut p = plan.lock().unwrap();
    p.seen_by_review
        .get_or_insert_with(|| std::fs::read_to_string(owner_file).unwrap());
    review_peek(&mut p, req);
    if !answers_a_call(req) {
        if let Some((old, new)) = p.review_edit.take() {
            let path = tree_in(req).unwrap_or_default().join("notes.txt");
            let edit = json!({"path": path, "old_string": old, "new_string": new});
            return Scripted::tools("Fixing.", &[("r1", "fs_edit", edit)]);
        }
    }
    Scripted::text(&p.review_report)
}

/// The first pass's answer: its findings, or a refusal.
fn first_pass_answer(p: &Plan) -> Scripted {
    match p.first_pass_fails {
        true => Scripted::Fail(crate::provider::ProviderError::InvalidRequest {
            status: 400,
            message: "invalid_request_error: the first pass was refused".into(),
        }),
        false => Scripted::text(&p.first_pass_report),
    }
}

/// What the test keeps of the review's first request, and its tampering.
fn review_peek(p: &mut Plan, req: &ProviderRequest) {
    p.review_saw.get_or_insert_with(|| first_user(req));
    if let Some(tamper) = p.tamper.take() {
        tamper(&tree_in(req).unwrap_or_default());
    }
}

pub(crate) fn rig(tweak: impl FnOnce(&mut crate::Config)) -> Draft {
    let jev = jev();
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    let repo = repo(&work.join("acme"));
    let work = work.canonicalize().unwrap();
    let plan = Arc::new(Mutex::new(Plan {
        review_report: "Checked it.\nChanged: 0 lines; tests fixed: 1".into(),
        ..Plan::default()
    }));
    let owner_file = repo.join("notes.txt");
    let (p, q) = (plan.clone(), plan.clone());
    let zai = By::new(move |req| {
        let mut p = p.lock().unwrap();
        if first_user(req).contains(crate::draft::first_pass::FIRST_PASS_HEADER) {
            return first_pass_answer(&p);
        }
        if answers_a_call(req) {
            return Scripted::text("Changed beta to gamma in notes.txt.");
        }
        match p.draft_call.take() {
            Some(first) => {
                let tree = tree_in(req).unwrap_or_default();
                let calls: Vec<(String, String, Value)> = std::iter::once(first)
                    .chain(p.draft_also.drain(..))
                    .enumerate()
                    .map(|(i, (name, input))| {
                        let input = serde_json::from_str(
                            &input.to_string().replace("TREE", &tree.to_string_lossy()),
                        )
                        .unwrap();
                        (format!("d{}", i + 1), name, input)
                    })
                    .collect();
                let calls: Vec<(&str, &str, Value)> = calls
                    .iter()
                    .map(|(id, name, input)| (id.as_str(), name.as_str(), input.clone()))
                    .collect();
                Scripted::tools("Editing.", &calls)
            }
            None => Scripted::text("Changed beta to gamma in notes.txt."),
        }
    });
    let claude = By::new(move |req| review_answer(&q, &owner_file, req));
    let (z, c) = (zai.clone(), claude.clone());
    let dd = tmp.path().to_path_buf();
    let w = work.clone();
    let r = rig_parts(
        Some(&jev),
        8,
        |cfg| {
            cfg.copy_profile("dsflash", "glm");
            let t = &mut cfg.routing.classes.routine_coding;
            t.cn_draft = Some("dsflash".into());
            t.final_review = vec!["sonnet".into()];
            t.profiles = vec!["sonnet".into()];
            cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
            cfg.policy.enforcement = crate::policy::Posture::Notify;
            // The state dir is on the approve list, as `~/.theseus` is by
            // default; the draft trees live outside it (theseus-iilre), and
            // the code makes their root a root of the workspace.
            let state = cfg.state_dir().to_string_lossy().into_owned();
            cfg.tools.approve_paths.push(state);
            cfg.server.drafts_dir = dd.join("drafts").to_string_lossy().into_owned();
            cfg.tools.roots = vec![w.to_string_lossy().into_owned()];
            tweak(cfg);
        },
        move |p| {
            p.providers.insert("zai".into(), z);
            p.providers.insert("anthropic".into(), c);
        },
    );
    std::mem::forget(jev);
    let judge = Judging::with(|kind, text| {
        let mut flags = Vec::new();
        if kind == ItemKind::ToolResult && text.contains(KEYFILE_MARK) {
            flags.push(Flag::Secret);
        }
        if text.contains(CONFIDENTIAL) {
            flags.push(Flag::Confidential);
        }
        flags
    });
    judge.attach(&r.core.store);
    r.core.runner.verdicts.set(judge);
    Draft {
        r,
        zai,
        claude,
        plan,
        work,
        repo,
        _tmp: tmp,
    }
}

/// Jev: every message `routine_coding`.
fn jev() -> FakeJev {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    jev
}

impl Draft {
    /// The owner's DM, working in `dir`.
    pub(crate) fn dm(&self, dir: &Path) -> String {
        let mut r = SessionRecord::new(SessionKind::Conversation, None);
        r.dir = Some(dir.to_string_lossy().into_owned());
        self.r.core.store.put_session(&r.session_id, &r).unwrap();
        self.r
            .core
            .outbox
            .bind_place(&format!("dm:{OWNER}"), &r.session_id)
            .unwrap();
        r.session_id
    }

    pub(crate) fn drive(&self) {
        tokio::spawn(crate::harness::drive(self.r.core.clone()));
    }

    /// The tasks `parent` opened, oldest first.
    pub(crate) fn tasks(&self, parent: &str) -> Vec<SessionRecord> {
        let rec: SessionRecord = self.r.core.store.get_session(parent).unwrap().unwrap();
        let exec = rec.execution_id.unwrap();
        let mut out: Vec<SessionRecord> = self
            .r
            .core
            .kernel
            .tasks(Some(&exec))
            .unwrap()
            .into_iter()
            .map(|e| {
                self.r
                    .core
                    .store
                    .get_session(&e.session_id)
                    .unwrap()
                    .unwrap()
            })
            .collect();
        out.sort_by_key(|r| r.created_at_unix_ms);
        out
    }

    pub(crate) fn file(&self) -> String {
        std::fs::read_to_string(self.repo.join("notes.txt")).unwrap()
    }

    /// The `task.draft` rows.
    pub(crate) fn drafts(&self) -> Vec<Value> {
        rows(&self.r.core, "task.draft")
            .into_iter()
            .map(|r| r.data)
            .collect()
    }

    pub(crate) async fn settled(&self) -> Value {
        until_within("the task.draft row", 60, || !self.drafts().is_empty()).await;
        let mut d = self.drafts();
        assert_eq!(d.len(), 1, "{d:?}");
        d.remove(0)
    }

    /// Every post's text of `kind`, with its session.
    pub(crate) fn texts(&self, kind: &str) -> Vec<(String, String)> {
        posts(&self.r.core, kind)
            .into_iter()
            .map(|a| {
                let body = crate::outbox::body_of(&a).to_string();
                (a.session_id, body)
            })
            .collect()
    }
}

pub(crate) fn draft_edit() -> (String, Value) {
    (
        "fs_edit".into(),
        json!({"path": "TREE/notes.txt", "old_string": "beta", "new_string": "gamma"}),
    )
}

/// The key test: an open coding ask in the DM drafts on `dsflash` in its own
/// tree; its report is never posted; a check on `sonnet` of the draft opens,
/// the place's only report is the review's, followed by the "Drafted by"
/// line, and the owner's directory changes only after the review ends.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cn_coding_draft_is_never_reported_without_a_claude_review() {
    let d = rig(|_| {});
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert_eq!(res.loops, 0, "the drafting turn made no model call");
    assert!(
        res.output.starts_with("Drafting on dsflash (") && res.output.contains("sonnet ("),
        "{}",
        res.output
    );
    assert!(
        d.claude.sent().is_empty(),
        "no Claude call in the parent turn"
    );
    d.drive();
    let row = d.settled().await;
    let tasks = d.tasks(&sid);
    assert_eq!(tasks.len(), 2, "the draft and its review");
    let (drafted, review) = (&tasks[0], &tasks[1]);
    assert!(draft::is_draft(&d.r.core.store, drafted));
    assert_eq!(drafted.last_target.as_ref().unwrap().profile, "dsflash");
    let check = review.task.as_ref().unwrap().check.clone().unwrap();
    assert_eq!(check.checked_task, drafted.session_id);
    assert_eq!(review.last_target.as_ref().unwrap().profile, "sonnet");
    // The reports the place got: the review's alone.
    let reports = d.texts("report");
    assert!(
        reports.iter().all(|(s, _)| s != &drafted.session_id),
        "the draft's report was posted: {reports:?}"
    );
    assert_eq!(
        reports
            .iter()
            .filter(|(s, _)| s == &review.session_id)
            .count(),
        1,
        "{reports:?}"
    );
    let notices = d.texts("notice");
    assert!(
        notices.iter().any(
            |(_, t)| t.contains("Drafted by dsflash, reviewed and fixed by sonnet; applied to")
        ),
        "{notices:?}"
    );
    // The owner's file was as it was while the review ran, and is changed now.
    let seen = d.plan.lock().unwrap().seen_by_review.clone();
    assert_eq!(seen.as_deref(), Some("alpha\nbeta\n"));
    assert_eq!(d.file(), "alpha\ngamma\n");
    assert_eq!(
        git(&d.repo, &["diff", "--cached"]),
        "",
        "the index is untouched"
    );
    assert_eq!(row["applied"], true, "{row}");
    assert_eq!(row["draft_profile"], "dsflash");
    assert_eq!(row["review_profile"], "sonnet");
    // The tree is gone; its branch stays.
    assert!(!draft::tree_of(&d.r.core.cfg, &drafted.session_id).exists());
    assert!(git(&d.repo, &["branch", "--list", "draft/*"]).contains("draft/"));
    // Each end again opens nothing and writes nothing.
    for s in [&drafted.session_id, &review.session_id] {
        d.r.core.runner.draft_ended(s).await;
    }
    assert_eq!(d.tasks(&sid).len(), 2);
    assert_eq!(d.drafts().len(), 1);
}

/// With no usable reviewer once the draft has opened (Sonnet's key
/// withdrawn), nothing is applied: the owner's directory is byte for byte as
/// it was, the place is asked once, the branch and the tree stay, and the row
/// says `applied: false` with no review.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_usable_reviewer_leaves_the_draft_unapplied_and_asks() {
    let d = rig(|_| {});
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert_eq!(res.stop_reason, "drafted", "{res:?}");
    let cfg = &d.r.core.cfg;
    let key = cfg.all_providers()["anthropic"].api_key_secret.clone();
    d.r.core.runner.secrets.withdraw(&key, "a test withdrew it");
    d.drive();
    let row = d.settled().await;
    assert_eq!(row["applied"], false, "{row}");
    assert!(
        row["review_task"].is_null() && row["review_cost_usd"].is_null(),
        "{row}"
    );
    assert_eq!(d.file(), "alpha\nbeta\n", "the owner's file is untouched");
    assert_eq!(git(&d.repo, &["status", "--porcelain"]), "");
    let asked: Vec<_> = d
        .texts("notice")
        .into_iter()
        .filter(|(_, t)| t.contains("not reviewed"))
        .collect();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(asked[0].1.contains("is on branch draft/"), "{asked:?}");
    let tasks = d.tasks(&sid);
    assert_eq!(tasks.len(), 1, "no review opened");
    assert!(
        draft::tree_of(cfg, &tasks[0].session_id).exists(),
        "the tree stays"
    );
    assert!(git(&d.repo, &["branch", "--list", "draft/*"]).contains("draft/"));
}

/// A draft's request offers no program, and a `proc_run` it makes is refused
/// at the gate; an ordinary open task working under the roots is still
/// offered them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_is_never_offered_a_program() {
    let d = rig(|_| {});
    d.plan.lock().unwrap().draft_call =
        Some(("proc_run".into(), json!({"argv": ["ls"], "cwd": "TREE"})));
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    d.settled().await;
    let names = |q: &ProviderRequest| -> Vec<String> {
        q.tools
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect()
    };
    let program = |n: &String| crate::draft_view::runs_tool(&n.replace('_', "."));
    let sent = d.zai.sent();
    assert!(!sent.is_empty());
    for q in &sent {
        let offered = names(q);
        assert!(offered.iter().any(|n| n == "fs_edit"), "{offered:?}");
        assert!(
            !offered.iter().any(program),
            "a program offered: {offered:?}"
        );
    }
    let draft = &d.tasks(&sid)[0];
    let refused =
        d.r.core
            .store
            .session_nodes(&draft.session_id)
            .unwrap()
            .into_iter()
            .any(|(_, n)| format!("{:?}", n.body).contains("is not offered in an open request"));
    assert!(refused, "the gate refused the call");
    // An ordinary task in the same directory is offered them.
    let mut p =
        crate::tests_task_brief::params("List the files of the acme tree.", Some("dsflash"));
    p.home = Some(sid.clone());
    let task =
        d.r.core
            .task_start(&p, crate::tests_task_brief::cli())
            .unwrap();
    until_within("the open task's request", 30, || {
        d.zai
            .sent()
            .iter()
            .any(|q| first_user(q).contains("List the files"))
    })
    .await;
    let q = d
        .zai
        .sent()
        .into_iter()
        .find(|q| first_user(q).contains("List the files"))
        .unwrap();
    assert!(names(&q).iter().any(program), "{:?}", names(&q));
    let _ = task;
}

/// A draft's open roots are its tree alone (theseus-a1wvq.13): its
/// `fs_write` of the owner's own file, under `open_paths`, is refused at the
/// gate, and so is a write of git's own files in its tree; the owner's file is
/// as it was until the review applies.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_writes_only_its_tree() {
    let d = rig(|_| {});
    let owner_file = d.repo.join("notes.txt");
    {
        let mut p = d.plan.lock().unwrap();
        p.draft_call = Some(draft_edit());
        p.draft_also = vec![
            (
                "fs_write".into(),
                json!({"path": owner_file, "content": "alpha\nthe draft's\n"}),
            ),
            (
                "fs_write".into(),
                json!({"path": "TREE/.git/hooks/post-commit", "content": "#!/bin/sh\n"}),
            ),
        ];
    }
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    let row = d.settled().await;
    let seen = d.plan.lock().unwrap().seen_by_review.clone();
    assert_eq!(
        seen.as_deref(),
        Some("alpha\nbeta\n"),
        "the owner's file changed before the review"
    );
    let draft = &d.tasks(&sid)[0];
    let bodies: Vec<String> =
        d.r.core
            .store
            .session_nodes(&draft.session_id)
            .unwrap()
            .into_iter()
            .map(|(_, n)| format!("{:?}", n.body))
            .collect();
    assert!(
        bodies
            .iter()
            .any(|b| b.contains("is outside the open paths")),
        "the owner's file was not refused: {bodies:?}"
    );
    assert!(
        bodies.iter().any(|b| b.contains("is git's own")),
        "git's own file was not refused: {bodies:?}"
    );
    assert_eq!(row["applied"], true, "{row}");
    assert_eq!(d.file(), "alpha\ngamma\n");
}

/// A draft from a subdirectory of the repository works in a tree of the
/// whole repository, so its apply runs at the repository's top: a reviewed
/// edit outside the subdirectory lands, never dropped while the line says
/// applied (theseus-a1wvq.13).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_from_a_subdirectory_applies_at_the_top() {
    let d = rig(|_| {});
    let sub = d.repo.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("readme.txt"), "sub\n").unwrap();
    git(&d.repo, &["add", "-A"]);
    git(&d.repo, &["commit", "-q", "-m", "sub"]);
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&sub);
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert_eq!(res.stop_reason, "drafted", "{:?}", res.route);
    d.drive();
    let row = d.settled().await;
    assert_eq!(row["applied"], true, "{row}");
    assert_eq!(
        d.file(),
        "alpha\ngamma\n",
        "the edit at the top was dropped"
    );
}

/// A draft that reads a file with a secret in it ends there: the judge labels
/// the result, no later request of the draft goes to its model or to the
/// `[model]` default, and its review opens with "no usable draft".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_stopped_by_a_label_ends_and_is_reviewed() {
    let d = rig(|_| {});
    std::fs::write(d.repo.join("keys.txt"), format!("token {KEYFILE_MARK}\n")).unwrap();
    git(&d.repo, &["add", "-A"]);
    git(&d.repo, &["commit", "-q", "-m", "keys"]);
    d.plan.lock().unwrap().draft_call = Some(("fs_read".into(), json!({"path": "TREE/keys.txt"})));
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    let row = d.settled().await;
    assert_eq!(row["draft_outcome"], "stopped", "{row}");
    assert_eq!(d.zai.sent().len(), 1, "the draft's one request");
    let to_claude = d.claude.sent();
    assert!(
        to_claude
            .iter()
            .all(|q| first_user(q).contains("Review the draft")),
        "a draft's request reached Claude"
    );
    let review = &d.tasks(&sid)[1];
    let nodes = d.r.core.store.session_nodes(&review.session_id).unwrap();
    assert!(
        nodes
            .iter()
            .any(|(_, n)| format!("{:?}", n.body).contains("no usable draft: stopped")),
        "the claim says so"
    );
}

/// The row has both tasks, their profiles, models and costs, the lines the
/// review changed (`--numstat`, added plus deleted), and the reviewer's own
/// count of the tests it fixed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn draft_and_review_costs_are_recorded() {
    let d = rig(|_| {});
    {
        let mut p = d.plan.lock().unwrap();
        p.draft_call = Some(draft_edit());
        p.review_edit = Some(("gamma".into(), "delta\nepsilon".into()));
        p.review_report = "Fixed the word.\nChanged: 3 lines; tests fixed: 2".into();
    }
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    let row = d.settled().await;
    let tasks = d.tasks(&sid);
    assert_eq!(row["draft_task"], tasks[0].session_id.as_str());
    assert_eq!(row["review_task"], tasks[1].session_id.as_str());
    assert_eq!(
        row["draft_model"],
        tasks[0].last_target.as_ref().unwrap().model.as_str()
    );
    assert_eq!(
        row["review_model"],
        tasks[1].last_target.as_ref().unwrap().model.as_str()
    );
    assert!(row["draft_cost_usd"].as_f64().unwrap() > 0.0, "{row}");
    assert!(row["review_cost_usd"].as_f64().unwrap() > 0.0, "{row}");
    // gamma -> delta, epsilon: one line deleted, two added.
    assert_eq!(row["lines_changed"], 3, "{row}");
    assert_eq!(row["tests_fixed_reported"], 2, "{row}");
    assert_eq!(row["applied"], true);
    assert_eq!(d.file(), "alpha\ndelta\nepsilon\n");
}

/// The template with `routing` appended, loaded and checked as the daemon does.
pub(crate) fn load(routing: &str) -> Result<crate::Config, String> {
    // The template declares the deepseek provider and its key itself.
    let extra = "[profiles.dsflash]\nprovider = \"deepseek\"\nmodel = \"deepseek-flash\"\n";
    let text = format!("{}\n{extra}\n{routing}\n", crate::Config::EXAMPLE_TOML);
    crate::Config::parse(&text)
        .map(|(cfg, _)| cfg)
        .map_err(|e| format!("{e:#}"))
}

#[test]
fn final_must_name_claude_profiles() {
    let ok = load(
        "[routing.classes.routine_coding]\ncn_draft = \"dsflash\"\nfinal = [\"sonnet\", \"opus\"]",
    )
    .unwrap();
    let t = &ok.routing.classes.routine_coding;
    assert_eq!(t.cn_draft.as_deref(), Some("dsflash"));
    assert_eq!(t.final_review, ["sonnet", "opus"]);
    for (bad, says) in [
        (
            "cn_draft = \"dsflash\"\nfinal = [\"dsflash\"]",
            "routing.classes.routine_coding.final: dsflash is not a Claude profile: a Claude model reviews every draft",
        ),
        (
            "cn_draft = \"dsflash\"\nfinal = [\"glm\"]",
            "routing.classes.routine_coding.final: glm is not a Claude profile",
        ),
        (
            "cn_draft = \"dsflash\"\nfinal = [\"nope\"]",
            "routing.classes.routine_coding.final: nope is not a Claude profile",
        ),
        (
            "cn_draft = \"dsflash\"",
            "routing.classes.routine_coding.final: cn_draft needs it",
        ),
        (
            "final = [\"sonnet\"]",
            "routing.classes.routine_coding.final: it reviews a draft, and the class has no cn_draft",
        ),
    ] {
        let e = load(&format!("[routing.classes.routine_coding]\n{bad}")).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
    // The template alone still loads, and its sparse form names neither key.
    let plain = load("").unwrap();
    let d = toml::to_string(&plain.routing).unwrap();
    assert!(!d.contains("cn_draft") && !d.contains("final"), "{d}");
}

#[test]
fn cn_draft_only_on_coding_classes() {
    load("[routing.classes.deep_coding]\ncn_draft = \"dsflash\"\nfinal = [\"opus\"]").unwrap();
    for m in ["chat", "trivial", "quick", "sophisticated", "review"] {
        let e = load(&format!(
            "[routing.classes.{m}]\ncn_draft = \"dsflash\"\nfinal = [\"sonnet\"]"
        ))
        .unwrap_err();
        assert!(
            e.contains(&format!(
                "routing.classes.{m}.cn_draft: a China-origin model drafts only code"
            )),
            "{m}: {e}"
        );
    }
    let e = load("[routing.classes.routine_coding]\ncn_draft = \"sonnet\"\nfinal = [\"opus\"]")
        .unwrap_err();
    assert!(
        e.contains("routing.classes.routine_coding.cn_draft: \"sonnet\" is not a configured profile of restricted origin"),
        "{e}"
    );
    // A misspelt key is refused by name.
    let e = load("[routing.classes.routine_coding]\ncn_drafts = \"dsflash\"").unwrap_err();
    assert!(e.contains("cn_drafts"), "{e}");
}

/// `cn_models = "never"` keeps a coding turn off its `cn_draft` profile
/// (theseus-gyjyo.6, found by the R5a review). The draft gate used to skip
/// only for a label's word, so `never` passed it and the turn opened a draft
/// task and ended `drafted` with no model call; the draft's own first request
/// was then refused by its floor, and the owner got a "drafted" notice and a
/// failed draft in place of an answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn never_does_not_draft() {
    let d = rig(|c| {
        c.content_based_security.cn_models = crate::config::content_based_security::CnModels::Never
    });
    let sid = d.dm(&d.repo);
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert_ne!(res.stop_reason, "drafted", "{:?}", res.route);
    assert_eq!(skip_of(&d), "draft_unusable");
    assert!(d.tasks(&sid).is_empty(), "no draft under never");
}

/// The route row's `draft_skip` of `sid`'s last turn.
fn skip_of(d: &Draft) -> Value {
    decided(&d.r.core.store)
        .last()
        .map(|r| r["draft_skip"].clone())
        .unwrap_or_default()
}

/// A coding turn that may not draft runs on the class's `profiles` as
/// before, and its row names the first reason; a class with no `cn_draft`
/// writes no `draft_skip`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ineligible_coding_turn_runs_as_before() {
    let d = rig(|_| {});
    let plain = d.work.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    // A session whose labels keep a model of restricted origin off.
    let labelled = d.dm(&d.repo);
    {
        let mut rec: SessionRecord = d.r.core.store.get_session(&labelled).unwrap().unwrap();
        let mut flags = crate::content::ContentFlags::default();
        flags.raise(crate::content::Label {
            flag: Flag::Confidential,
            node: None,
            kind: ItemKind::ToolResult,
            source: "fs.read /srv/notes.txt".into(),
            confidence: 0.9,
            band: theseus_judge::band::Band::Confirm,
            at_ms: 1,
            judgment: String::new(),
        });
        rec.data = Some(Box::new(flags));
        d.r.core.store.put_session(&labelled, &rec).unwrap();
    }
    let mut sid = String::new();
    for (session, dir, word) in [
        (None, plain.as_path(), "not_git"),
        (Some(labelled), d.repo.as_path(), "labelled"),
    ] {
        sid = session.unwrap_or_else(|| d.dm(dir));
        let res = say(&d.r.core, &sid, ASK, None).await;
        assert_eq!(res.profile, "sonnet", "{word}: {:?}", res.route);
        assert!(res.loops >= 1, "{word}");
        assert_eq!(skip_of(&d), word, "{:?}", decided(&d.r.core.store).last());
        assert!(d.tasks(&sid).is_empty(), "{word}: no draft");
    }
    // A task's own turn, under the last conversation, in the repository: it
    // carries that conversation's label (theseus-gyjyo.5), which says so first.
    let before = decided(&d.r.core.store).len();
    d.drive();
    let mut p = crate::tests_task_brief::params("Rename beta in the acme tree.", None);
    p.home = Some(sid.clone());
    let task =
        d.r.core
            .task_start(&p, crate::tests_task_brief::cli())
            .unwrap();
    until_within("the task's route row", 30, || {
        decided(&d.r.core.store).len() > before
    })
    .await;
    let _ = task;
    assert_eq!(
        skip_of(&d),
        "labelled",
        "{:?}",
        decided(&d.r.core.store).last()
    );
    assert!(d.zai.sent().is_empty(), "nothing drafted");

    // A class with no cn_draft.
    let d = rig(|c| {
        let t = &mut c.routing.classes.routine_coding;
        t.cn_draft = None;
        t.final_review = vec![];
    });
    let sid = d.dm(&d.repo);
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert_eq!(res.profile, "sonnet");
    let rows = decided(&d.r.core.store);
    assert!(
        rows.iter().all(|r| r.get("draft_skip").is_none()),
        "{rows:?}"
    );
}

/// The draft's brief is this turn's ask alone: an earlier message is in
/// neither the brief nor any request to `dsflash`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_draft_brief_holds_only_this_turns_ask() {
    let d = rig(|_| {});
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    // An earlier message, already in the transcript: no draft is made of it.
    let earlier = crate::node::Node::user(&sid, None, "test", PRIVATE);
    d.r.core.store.append(&[earlier.record().unwrap()]).unwrap();
    let res = say(&d.r.core, &sid, ASK, None).await;
    assert_eq!(res.stop_reason, "drafted", "{:?}", res.route);
    d.drive();
    d.settled().await;
    let draft = &d.tasks(&sid)[0];
    let nodes = d.r.core.store.session_nodes(&draft.session_id).unwrap();
    let brief = format!("{:?}", nodes[0].1.body);
    assert!(
        brief.contains(draft::DRAFT_HEADER) && brief.contains(ASK),
        "{brief}"
    );
    assert!(!brief.contains("notes-mark-5521"), "{brief}");
    for q in d.zai.sent() {
        let body = serde_json::to_string(&json!({"m": q.messages, "s": q.system})).unwrap();
        assert!(
            !body.contains("notes-mark-5521"),
            "the earlier message reached dsflash"
        );
    }
}

/// Test 28 of theseus-9ymz0: a brief that carries a confidential line raises
/// `confidential` on the draft's own session (its brief is an item, judged like
/// any other), and the draft's model is refused: nothing goes to `dsflash`,
/// the draft ends as one that was never admitted, and its review opens with
/// "no usable draft".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_brief_with_a_confidential_line_refuses_the_drafts_model() {
    let d = rig(|_| {});
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    let ask = format!("{ASK} The context is the {CONFIDENTIAL}.");
    let res = say(&d.r.core, &sid, &ask, None).await;
    assert_eq!(res.stop_reason, "drafted", "{:?}", res.route);
    d.drive();
    let row = d.settled().await;
    assert_eq!(row["draft_outcome"], "not_admitted", "{row}");
    assert!(d.zai.sent().is_empty(), "nothing went to the draft's model");
    let drafted = &d.tasks(&sid)[0];
    let flags = crate::content_wait::flags_of(&d.r.core.store, &drafted.session_id)
        .expect("the draft's own session carries labels");
    assert!(flags.has(Flag::Confidential), "{flags:?}");
    let review = &d.tasks(&sid)[1];
    let nodes = d.r.core.store.session_nodes(&review.session_id).unwrap();
    assert!(
        nodes
            .iter()
            .any(|(_, n)| format!("{:?}", n.body).contains("no usable draft")),
        "the claim says so"
    );
}

/// The owner edits the same line while the draft runs: `git apply --check`
/// fails, nothing is applied, the owner's index is untouched, and the line
/// names the branch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_review_conflict_applies_nothing() {
    let d = rig(|_| {});
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    // The owner's own edit, after the tree was made and before it is applied.
    std::fs::write(d.repo.join("notes.txt"), "alpha\nbeta, the owner's\n").unwrap();
    git(&d.repo, &["commit", "-q", "-am", "the owner's edit"]);
    d.drive();
    let row = d.settled().await;
    assert_eq!(row["applied"], false, "{row}");
    assert_eq!(
        d.file(),
        "alpha\nbeta, the owner's\n",
        "the owner's edit stands"
    );
    assert_eq!(git(&d.repo, &["diff", "--cached"]), "");
    assert!(!d.file().contains("<<<<<<<"));
    let line: Vec<_> = d
        .texts("notice")
        .into_iter()
        .filter(|(_, t)| t.contains("not applied: it does not apply"))
        .collect();
    assert_eq!(line.len(), 1, "{:?}", d.texts("notice"));
    assert!(line[0].1.contains("(branch draft/"), "{line:?}");
}
