//! The work board through whole cores (theseus-hkx30): the tree a
//! conversation, its tasks, their jobs, questions and wakes make, as
//! `work.watch` and its notifications say it.

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::notices::Notice;
use theseus_protocol::work::{WorkKind, WorkState, WorkView};
use theseus_protocol::{notify, Message, WorkChanged};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use crate::provider::Scripted;
use crate::push::WorkScope;
use crate::tests_tasks::{
    answers_a_call, arranged, first_user, parent_session, rig, turn, until, ASK,
};
use crate::Core;

/// What a `work.watch` connection holds and has heard.
pub(crate) struct Watcher {
    rx: UnboundedReceiver<Message>,
    pub(crate) changed: Vec<WorkChanged>,
    notices: Vec<Notice>,
}

impl Watcher {
    /// Watch the whole board, the way `work.watch` does: subscribe, then read.
    pub(crate) async fn start(core: &Arc<Core>, scope: WorkScope) -> (Self, Vec<WorkView>) {
        core.push.ensure(core).await.unwrap();
        let (tx, rx) = unbounded_channel();
        core.bus.watch_work("w", tx, scope.clone(), false);
        let (_, views, _) = core.push.work_snapshot(200, &scope);
        (
            Self {
                rx,
                changed: Vec::new(),
                notices: Vec::new(),
            },
            views,
        )
    }

    /// Take what has arrived.
    pub(crate) fn drain(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            let Message::Notification(n) = m else {
                continue;
            };
            match n.method.as_str() {
                notify::WORK_CHANGED => {
                    self.changed.push(serde_json::from_value(n.params).unwrap())
                }
                notify::WORK_NOTICE => self.notices.push(serde_json::from_value(n.params).unwrap()),
                _ => {}
            }
        }
    }

    /// The newest version of each view heard, by the position rule.
    pub(crate) fn held(&mut self) -> std::collections::BTreeMap<String, WorkView> {
        self.drain();
        let mut out: std::collections::BTreeMap<String, WorkView> = Default::default();
        for c in &self.changed {
            for v in &c.views {
                if out.get(&v.id).is_none_or(|o| v.position > o.position) {
                    out.insert(v.id.clone(), v.clone());
                }
            }
            for g in &c.gone {
                out.remove(g);
            }
        }
        out
    }
}

fn start_one(brief: &str) -> Scripted {
    Scripted::tools(
        "Starting it.",
        &[(
            "t_task",
            "task_create",
            serde_json::json!({"brief": brief, "budget_usd": 1.5, "arrangement": arranged()}),
        )],
    )
}

/// A conversation starts a task: the board has the conversation as a root
/// and the task under it, with a title, and the root counts it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_is_a_child_of_its_conversation_and_the_root_counts_it() {
    let mut r = rig(
        |req| {
            if first_user(req).contains("CHILD:") {
                return Scripted::text("Counted.");
            }
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            start_one("CHILD: count to three")
        },
        |_| {},
    );
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let parent = parent_session(&r.core);
    let (mut w, _) = Watcher::start(&r.core, WorkScope::default()).await;
    turn(&r.core, &parent, ASK).await;
    r.entered.recv().await.unwrap();
    until("the task is on the board", || {
        w.held().values().any(|v| v.kind == WorkKind::Task)
    })
    .await;
    let held = w.held();
    let task = held.values().find(|v| v.kind == WorkKind::Task).unwrap();
    let root = held
        .get(&task.root)
        .expect("the conversation is on the board");
    assert_eq!(root.kind, WorkKind::Conversation);
    let rec: crate::session::SessionRecord = r.core.store.get_session(&parent).unwrap().unwrap();
    assert_eq!(rec.title.as_deref(), Some(ASK), "the store has it");
    assert_eq!(
        root.title, ASK,
        "the conversation's title is its first words"
    );
    assert_eq!(task.parent.as_deref(), Some(root.id.as_str()));
    assert!(task.title.contains("count to three"), "{}", task.title);
    assert_eq!(task.state, WorkState::Running);
    let below = root.below.expect("a rollup");
    assert_eq!((below.total, below.working), (1, 1), "{below:?}");
    r.model.gate.add_permits(1);
    until("the task is done", || {
        w.held()
            .values()
            .any(|v| v.kind == WorkKind::Task && v.state == WorkState::Done)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let held = w.held();
    let root = held
        .values()
        .find(|v| v.kind == WorkKind::Conversation)
        .unwrap();
    assert_eq!(root.below.unwrap().done, 1);
}

// ---------------------------------------------------------------- the scenario

use std::sync::Mutex;

use serde_json::{json, Value};
use theseus_protocol::notices::Why;

use crate::provider::ProviderRequest;
use crate::tests_tasks::last_user;

/// Wait for `f` over what the watcher holds, and say what it holds if it
/// never comes.
async fn until_held(
    what: &str,
    w: &mut Watcher,
    f: impl Fn(&std::collections::BTreeMap<String, WorkView>) -> bool,
) {
    let t0 = std::time::Instant::now();
    loop {
        let h = w.held();
        if f(&h) {
            return;
        }
        if t0.elapsed() > Duration::from_secs(20) {
            let tree: Vec<String> = h
                .values()
                .map(|v| {
                    format!(
                        "{:?} {:?} {:?} (parent {:?}) {}",
                        v.kind, v.state, v.title, v.parent, v.attention.label
                    )
                })
                .collect();
            panic!("no {what} in 20 s; the board holds:\n{}", tree.join("\n"));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The plan items the scripted parent closes: id and version, as the test
/// read them from the store.
type Steps = Arc<Mutex<Vec<(String, u64)>>>;

fn scenario_script(steps: Steps) -> impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static {
    move |req| {
        let (first, last) = (first_user(req), last_user(req));
        if first.contains("CHILD-A") {
            return Scripted::text("A is done.");
        }
        if first.contains("CHILD-B") {
            return Scripted::text("B is done.");
        }
        if first.contains("CHILD-C") {
            return if answers_a_call(req) {
                Scripted::text("C wrote it.")
            } else {
                Scripted::tools(
                    "",
                    &[("c1", "fs_write", json!({"path": "c.txt", "content": "hi"}))],
                )
            };
        }
        if answers_a_call(req) {
            return Scripted::text("ok");
        }
        if last.contains("START") {
            let mut calls: Vec<(String, &str, Value)> = ["A", "B", "C"]
                .iter()
                .map(|n| {
                    (
                        format!("t_{n}"),
                        "task_create",
                        json!({"brief": format!("CHILD-{n}: do the {n} part"),
                               "budget_usd": 1.0, "arrangement": arranged()}),
                    )
                })
                .collect();
            for i in 1..=3 {
                calls.push((
                    format!("s_{i}"),
                    "task_create",
                    json!({"title": format!("step {i}")}),
                ));
            }
            let calls: Vec<(&str, &str, Value)> = calls
                .iter()
                .map(|(id, n, v)| (id.as_str(), *n, v.clone()))
                .collect();
            return Scripted::tools("Starting.", &calls);
        }
        if last.contains("JOB sleep") {
            return Scripted::tools(
                "Running it.",
                &[(
                    "j1",
                    "proc_run",
                    json!({"argv": ["sleep", "2"], "background": true}),
                )],
            );
        }
        if last.contains("WAKE in") {
            return Scripted::tools(
                "Setting it.",
                &[(
                    "w1",
                    "wake_at",
                    json!({"after": "2s", "note": "the tide turns"}),
                )],
            );
        }
        if last.contains("CLOSE the") {
            let calls: Vec<(String, Value)> = steps
                .lock()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, (id, v))| {
                    (
                        format!("x_{i}"),
                        json!({"id": id, "version": v, "outcome": "done",
                               "evidence": [{"identity": "note", "note": "done"}]}),
                    )
                })
                .collect();
            let calls: Vec<(&str, &str, Value)> = calls
                .iter()
                .map(|(id, v)| (id.as_str(), "task_close", v.clone()))
                .collect();
            return Scripted::tools("Closing.", &calls);
        }
        if last.contains("⏰") {
            return Scripted::text("The tide turned.");
        }
        Scripted::text("Noted.")
    }
}

/// What the stages of the scenario share.
struct Scene {
    r: crate::tests_tasks::Rig,
    w: Watcher,
    parent: String,
    steps: Steps,
    drain: tokio::task::JoinHandle<()>,
}

impl Scene {
    async fn new() -> Self {
        let steps: Steps = Arc::default();
        let r = rig(scenario_script(steps.clone()), |c| {
            c.policy
                .tools
                .insert("fs.write".into(), crate::policy::Posture::Approve);
            c.policy.tools.remove("proc.run");
            // The driver's tick posts no digest of its own here.
            c.discord.digest_at = vec![];
        });
        *r.model.hold.lock().unwrap() = Some("CHILD".into());
        // A stand-in for the daemon's drain, which takes a background job's end.
        let c = r.core.clone();
        let drain = tokio::spawn(async move {
            loop {
                c.drain_spool();
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        let parent = parent_session(&r.core);
        let (w, _) = Watcher::start(&r.core, WorkScope::default()).await;
        Self {
            r,
            w,
            parent,
            steps,
            drain,
        }
    }

    async fn say(&self, text: &str) {
        turn(&self.r.core, &self.parent, text).await;
    }

    fn find(&mut self, f: impl Fn(&WorkView) -> bool) -> WorkView {
        self.w.held().values().find(|v| f(v)).unwrap().clone()
    }
}

/// Stage 1: the conversation starts three tasks and plans three steps. All
/// are in one tree under the conversation, and three steps read 0/3.
async fn the_tree_is_built(s: &mut Scene) {
    s.say(&format!("{ASK}; and plan it")).await;
    for i in 0..3 {
        tokio::time::timeout(Duration::from_secs(8), s.r.entered.recv())
            .await
            .unwrap_or_else(|_| panic!("child {i} never began"));
    }
    until_held("three tasks and three steps", &mut s.w, |h| {
        h.values().filter(|v| v.kind == WorkKind::Task).count() == 3
            && h.values().filter(|v| v.kind == WorkKind::Step).count() == 3
    })
    .await;
    let held = s.w.held();
    let root = s.find(|v| v.kind == WorkKind::Conversation);
    assert!(root.parent.is_none());
    for v in held.values().filter(|v| v.kind != WorkKind::Conversation) {
        assert_eq!(v.root, root.id, "{} is in the one tree", v.title);
        assert_eq!(v.parent.as_deref(), Some(root.id.as_str()));
    }
    let p = root.progress.clone().expect("progress");
    assert_eq!((p.steps_done, p.steps_total), (0, 3), "{p:?}");
    assert_eq!(p.current.as_deref(), Some("step 1"));
    assert_eq!(root.below.unwrap().total, 6);
}

/// Stage 2: B is cancelled at its gate; A and C go on; the conversation runs a
/// job; C asks. The question has its own view, and the root counts it.
async fn a_cancel_a_job_and_a_question(s: &mut Scene) -> WorkView {
    let b = s.find(|v| v.title.contains("CHILD-B"));
    s.r.core
        .task_cancel_by(&b.id[b.id.len() - 6..], "test")
        .await
        .unwrap();
    until_held("B cancelled", &mut s.w, |h| {
        h.get(&b.id)
            .is_some_and(|v| v.state == WorkState::Cancelled)
    })
    .await;
    // B's own wait at the gate takes a permit too.
    s.r.model.gate.add_permits(3);
    s.say("JOB sleep two seconds").await;
    until_held("a job and a question", &mut s.w, |h| {
        h.values().any(|v| v.kind == WorkKind::Job)
            && h.values().any(|v| v.kind == WorkKind::Question)
    })
    .await;
    let root = s.find(|v| v.kind == WorkKind::Conversation);
    let job = s.find(|v| v.kind == WorkKind::Job);
    assert_eq!(job.title, "sleep 2");
    assert_eq!(
        (job.parent.as_deref(), job.root.as_str()),
        (Some(root.id.as_str()), root.id.as_str())
    );
    let q = s.find(|v| v.kind == WorkKind::Question);
    let question = q.question.clone().unwrap();
    assert_eq!(question.tool.as_deref(), Some("fs.write"));
    assert!(question.may_answer);
    assert!(s
        .r
        .core
        .push
        .open_questions()
        .iter()
        .any(|o| o.id == question.id));
    until_held("the root needs you", &mut s.w, |h| {
        h.values()
            .find(|v| v.kind == WorkKind::Conversation)
            .unwrap()
            .below
            .is_some_and(|b| b.needs_you >= 1)
    })
    .await;
    q
}

/// Stage 3: the answer ends the question, and the tasks that can finish do.
async fn the_answer(s: &mut Scene, q: &WorkView) {
    let id = q.question.as_ref().unwrap().id.clone();
    s.r.core.confirm_action(&id, true, None, "test").unwrap();
    s.r.model.gate.add_permits(4);
    until_held("the question ends", &mut s.w, |h| {
        h.get(&q.id).is_some_and(|v| v.state.ended())
    })
    .await;
    until_held("every task done but the cancelled one", &mut s.w, |h| {
        h.values()
            .filter(|v| v.kind == WorkKind::Task && v.state == WorkState::Done)
            .count()
            == 2
    })
    .await;
}

/// Stage 4: a wake is set and fires, and the steps close: 3/3.
async fn a_wake_and_the_steps_close(s: &mut Scene) {
    s.say("WAKE in two seconds").await;
    until_held("a pending wake", &mut s.w, |h| {
        h.values()
            .any(|v| v.kind == WorkKind::Wake && v.state == WorkState::Waiting)
    })
    .await;
    until_held("the wake fired", &mut s.w, |h| {
        h.values()
            .any(|v| v.kind == WorkKind::Wake && v.state == WorkState::Done)
    })
    .await;
    let all = crate::task_graph::all(&s.r.core.store).unwrap();
    *s.steps.lock().unwrap() = all
        .iter()
        .filter(|t| t.session.is_none())
        .map(|t| (t.id.clone(), t.version))
        .collect();
    assert_eq!(s.steps.lock().unwrap().len(), 3);
    s.say("CLOSE the steps").await;
    until_held("3/3", &mut s.w, |h| {
        h.values()
            .find(|v| v.kind == WorkKind::Conversation)
            .and_then(|v| v.progress.clone())
            .is_some_and(|p| (p.steps_done, p.steps_total) == (3, 3))
    })
    .await;
    until_held("the job ended", &mut s.w, |h| {
        h.values()
            .any(|v| v.kind == WorkKind::Job && v.state.ended())
    })
    .await;
    // The steps can close before the CLOSE turn ends: the tree is read once
    // the conversation is between exchanges again.
    until_held("the conversation ready", &mut s.w, |h| {
        h.values()
            .any(|v| v.kind == WorkKind::Conversation && v.state == WorkState::Ready)
    })
    .await;
}

/// The whole substrate scenario: a conversation starts three tasks and three
/// plan steps; one is cancelled, one asks a question and is answered, the
/// conversation runs a job and sets a wake that fires; and the steps close.
/// Each stage of it is what a surface would hold by the position rule, and
/// the notices the policy made are the ones the owner would have heard.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_substrate_scenario_reads_as_one_tree_and_pings_for_the_right_reasons() {
    let mut s = Scene::new().await;
    the_tree_is_built(&mut s).await;
    let q = a_cancel_a_job_and_a_question(&mut s).await;
    the_answer(&mut s, &q).await;
    a_wake_and_the_steps_close(&mut s).await;
    s.w.drain();
    // One notice at most per view per frame.
    for pair in s.w.notices.windows(2) {
        assert!(
            pair[0].work.id != pair[1].work.id || pair[0].position != pair[1].position,
            "two notices for {} at {}",
            pair[0].work.id,
            pair[0].position
        );
    }
    // And every one of them was told once: a question has its own view and
    // the task that asks is quiet about it.
    let count = |why: Why| s.w.notices.iter().filter(|n| n.why == why).count();
    assert_eq!((count(Why::Asked), count(Why::Answered)), (1, 1));
    check_golden(&shape_of(&mut s.w));
    s.drain.abort();
}

/// The scenario as the owner would have had it: every notice the policy made
/// (once each) and the tree at the end, with ids as they first appear and every
/// run of digits a `#`.
fn shape_of(w: &mut Watcher) -> String {
    let held = w.held();
    let mut tidy = Tidy::default();
    let mut lines: Vec<String> = w
        .notices
        .iter()
        .map(|n| format!("{:?} {:?} {}", n.urgency, n.why, tidy.line(&n.line)))
        .collect();
    lines.sort();
    lines.dedup();
    let mut out = String::from("NOTICES\n");
    for l in lines {
        out.push_str(&format!("  {l}\n"));
    }
    out.push_str("TREE\n");
    let depth = |v: &WorkView| {
        let mut d = 0;
        let mut at = v.parent.clone();
        while let Some(p) = at {
            d += 1;
            at = held.get(&p).and_then(|x| x.parent.clone());
        }
        d
    };
    let mut rows: Vec<(usize, String)> = held
        .values()
        .map(|v| {
            let progress = v
                .progress
                .as_ref()
                // The model's own line moves with timing; the golden holds the plan's count.
                .filter(|p| p.steps_total > 0)
                .map(|p| format!(" steps {}/{}", p.steps_done, p.steps_total))
                .unwrap_or_default();
            let below = v
                .below
                .map(|b| {
                    format!(
                        " below needs_you {} working {} done {} failed {}",
                        b.needs_you, b.working, b.done, b.failed
                    )
                })
                .unwrap_or_default();
            let text = format!("{:?} {:?} {}{progress}{below}", v.kind, v.state, v.title);
            (depth(v), tidy.line(&text))
        })
        .collect();
    rows.sort();
    for (d, r) in rows {
        out.push_str(&format!("{}{r}\n", "  ".repeat(d + 1)));
    }
    out
}

/// Ids by their first appearance, the scenario's temporary directory as
/// `<work>`, and every run of digits `#`: a golden of shapes, not numbers.
#[derive(Default)]
struct Tidy {
    ids: Vec<String>,
}

impl Tidy {
    fn line(&mut self, text: &str) -> String {
        let s = self.ids_aliased(text);
        let s = match s.find("/tmp/.tmp") {
            Some(at) => {
                let end = s[at..]
                    .find("/work")
                    .map_or(s.len(), |e| at + e + "/work".len());
                format!("{}<work>{}", &s[..at], &s[end..])
            }
            None => s,
        };
        let mut out = String::new();
        let mut in_run = false;
        for c in s.chars() {
            if c.is_ascii_digit() {
                if !in_run {
                    out.push('#');
                }
                in_run = true;
            } else {
                in_run = false;
                out.push(c);
            }
        }
        out
    }

    fn ids_aliased(&mut self, text: &str) -> String {
        let mut out = String::new();
        for word in text.split_inclusive(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            let end = word
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(word.len());
            let (id, rest) = word.split_at(end);
            let prefix = ["exe_", "act_", "wak_", "tsk_", "ses_"]
                .into_iter()
                .find(|p| id.starts_with(p) && id.len() > 16);
            match prefix {
                Some(p) => {
                    let n = self.ids.iter().position(|a| a == id).unwrap_or_else(|| {
                        self.ids.push(id.to_string());
                        self.ids.len() - 1
                    });
                    out.push_str(&format!("{p}#{}{rest}", n + 1));
                }
                None => out.push_str(word),
            }
        }
        out
    }
}

/// The scenario's golden, `tests/golden/work_board.txt`: `THESEUS_GOLDEN=write`
/// rewrites it, for a change you mean.
fn check_golden(got: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/work_board.txt");
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        got, want,
        "the work board's scenario differs from {} (THESEUS_GOLDEN=write rewrites it, for a change you mean)",
        path.display()
    );
}

#[path = "tests_work_digest.rs"]
mod digest;
