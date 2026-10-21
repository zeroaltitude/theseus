//! The work board's seed against its replay (theseus-hkx30): the board a
//! daemon seeds from the store when someone first watches shows the open
//! work the board built frame by frame would show.

use std::collections::BTreeMap;

use serde_json::json;
use theseus_protocol::work::{WorkKind, WorkState, WorkView};

use super::super::board::seed_inputs;
use super::super::work as board_work;
use crate::provider::Scripted;
use crate::test_support::board_caught_up;
use crate::tests_tasks::{
    answers_a_call, arranged, first_user, last_user, parent_session, rig, turn, until, ASK,
};

/// What a surface shows of a view that is not over, without the times and
/// positions only a frame knows.
type Shape = (
    WorkKind,
    Option<String>,
    String,
    String,
    WorkState,
    Option<(String, String)>,
    Option<(u32, u32, Option<String>)>,
    Option<(u32, u32)>,
);

fn shapes(all: BTreeMap<String, WorkView>) -> BTreeMap<String, Shape> {
    all.into_iter()
        .filter(|(_, v)| !v.state.ended())
        .map(|(id, v)| {
            (
                id,
                (
                    v.kind,
                    v.parent,
                    v.root,
                    v.title,
                    v.state,
                    v.question.map(|q| (q.id, q.prompt)),
                    v.progress.map(|p| (p.steps_done, p.steps_total, p.current)),
                    v.below.map(|b| (b.needs_you, b.working)),
                ),
            )
        })
        .collect()
}

/// The conversation: it starts two tasks and plans three steps, runs a job,
/// and sets a wake; Q asks a question, and R waits at the gate.
fn script(req: &crate::provider::ProviderRequest) -> Scripted {
    let (first, last) = (first_user(req), last_user(req));
    if first.contains("CHILD-Q") {
        return if answers_a_call(req) {
            Scripted::text("wrote it")
        } else {
            Scripted::tools(
                "",
                &[("q1", "fs_write", json!({"path": "q.txt", "content": "x"}))],
            )
        };
    }
    if first.contains("CHILD-R") {
        return Scripted::text("never reached");
    }
    if answers_a_call(req) {
        return Scripted::text("ok");
    }
    if last.contains("START") {
        let mut calls: Vec<(String, &str, serde_json::Value)> = ["Q", "R"]
                    .iter()
                    .map(|n| {
                        (
                            format!("t_{n}"),
                            "task_create",
                            json!({"brief": format!("CHILD-{n}: part"), "budget_usd": 1.0, "arrangement": arranged()}),
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
        let calls: Vec<(&str, &str, serde_json::Value)> = calls
            .iter()
            .map(|(a, b, c)| (a.as_str(), *b, c.clone()))
            .collect();
        return Scripted::tools("Starting.", &calls);
    }
    if last.contains("JOB") {
        return Scripted::tools(
            "Running it.",
            &[(
                "j1",
                "proc_run",
                json!({"argv": ["sleep", "20"], "background": true}),
            )],
        );
    }
    if last.contains("WAKE") {
        return Scripted::tools(
            "Setting it.",
            &[(
                "w1",
                "wake_at",
                json!({"after": "2h", "note": "check the build"}),
            )],
        );
    }
    Scripted::text("Noted.")
}

/// Open work, built by frames and seeded from the store, is the same tree:
/// a conversation with two tasks (one asking a question), three plan steps,
/// a job that runs, and a wake that waits.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fresh_seed_shows_the_open_work_the_frames_built() {
    let mut r = rig(script, |c| {
        c.policy
            .tools
            .insert("fs.write".into(), crate::policy::Posture::Approve);
    });
    *r.model.hold.lock().unwrap() = Some("CHILD-R".into());
    let parent = parent_session(&r.core);
    r.core.push.ensure(&r.core).await.unwrap();
    turn(&r.core, &parent, &format!("{ASK}; and plan it")).await;
    turn(&r.core, &parent, "JOB sleep a while").await;
    turn(&r.core, &parent, "WAKE in two hours").await;
    r.entered.recv().await.unwrap();
    let live = |r: &crate::tests_tasks::Rig| shapes(r.core.push.work.lock().unwrap().all());
    until("the tree is built", || {
        let s = live(&r);
        let kinds = |k: WorkKind| s.values().filter(|v| v.0 == k).count();
        kinds(WorkKind::Task) == 2
            && kinds(WorkKind::Step) == 3
            && kinds(WorkKind::Job) == 1
            && kinds(WorkKind::Wake) == 1
            && kinds(WorkKind::Question) == 1
    })
    .await;
    // The tree holds its kinds, but the board applies frames on a thread of
    // its own: a conversation's last frame can still be queued, and the seed
    // reads it from the store (theseus-fpvn8).
    board_caught_up(&r.core, &parent).await;
    // Let the warm-up read the job's command, as it would after the first
    // watch, then seed another board from the store alone.
    let core = r.core.clone();
    let seeded = tokio::task::spawn_blocking(move || {
        let (board, execs, actions) = seed_inputs(&core, super::super::hot::RECENT).unwrap();
        let mut w = board_work::seed(&core, &board, &execs, &actions);
        let open = w.open_jobs();
        let (runs, argvs) = board_work::read_warm(&core, &open);
        w.warm(runs, argvs);
        shapes(w.all())
    })
    .await
    .unwrap();
    assert_eq!(seeded, live(&r), "the seed and the replay show one tree");
    let titles: Vec<&str> = seeded.values().map(|s| s.3.as_str()).collect();
    assert!(
        titles.contains(&"sleep 20"),
        "the job's command came back from the ledger: {titles:?}"
    );
}
