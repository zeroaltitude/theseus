//! The work board's parts, without a core: the notice ring, the ended
//! children a root keeps, the rollup's rules, and a job's usual time.

use std::time::Instant;

use theseus_kernel::{Action, ActionState, RetryClass};
use theseus_protocol::notices::{Notice, Urgency, Why};
use theseus_protocol::work::{WorkKind, WorkRef, WorkState, WorkView};

use super::settle::bump;
use super::views::blank;
use super::*;

fn frame(position: u64) -> Frame {
    Frame {
        at_ms: 1_000 + position,
        committed: Instant::now(),
        position,
        records: Vec::new(),
        nodes: Vec::new(),
    }
}

fn notice(position: u64) -> Notice {
    let r = WorkRef {
        id: "exe_1".into(),
        kind: WorkKind::Conversation,
        session_id: None,
        title: String::new(),
    };
    Notice {
        urgency: Urgency::Inform,
        why: Why::Finished,
        work: r.clone(),
        root: r,
        line: format!("n{position}"),
        question: None,
        retracts: None,
        answers: vec![],
        position,
        at_ms: position,
    }
}

/// The ring keeps the last 4,096 notices, and says whether it still holds
/// every one after a position, so a digest knows when it has a gap.
#[test]
fn the_ring_keeps_4096_notices_and_says_when_it_forgot() {
    let mut w = WorkBoard::default();
    for p in 1..=(RING as u64 + 100) {
        w.keep(notice(p));
    }
    let (all, complete) = w.notices_since(0);
    assert_eq!(all.len(), RING);
    assert!(!complete, "the first hundred are gone");
    let (some, complete) = w.notices_since(150);
    assert_eq!(some.len(), RING + 100 - 150);
    assert_eq!(some[0].position, 151);
    assert!(complete, "nothing after 150 was let go");
    // A board seeded at a position knows nothing before it.
    let mut seeded = WorkBoard {
        seeded_at: 500,
        ..WorkBoard::default()
    };
    seeded.keep(notice(501));
    assert!(!seeded.notices_since(10).1);
    assert!(seeded.notices_since(500).1);
}

fn child(root: &str, n: u64, state: WorkState) -> WorkView {
    let mut v = blank(&format!("exe_child{n:03}"), WorkKind::Task, n);
    v.parent = Some(root.into());
    v.root = root.into();
    v.state = state;
    v.ended_at_ms = state.ended().then_some(n);
    v.at_ms = n;
    v
}

/// A root keeps its 50 newest ended children; the others leave, named in
/// `gone`, and are still counted in the root's rollup.
#[test]
fn a_root_keeps_fifty_ended_children_and_the_count_of_the_rest() {
    let mut w = WorkBoard::default();
    let mut root = blank("exe_root", WorkKind::Conversation, 1);
    root.state = WorkState::Running;
    w.install(root);
    let mut ids = Vec::new();
    for n in 1..=80 {
        // Every fifth failed: a failed task stays until someone looks.
        let state = if n % 5 == 0 {
            WorkState::Failed
        } else {
            WorkState::Done
        };
        let v = child("exe_root", n, state);
        ids.push(v.id.clone());
        w.install(v);
    }
    // And one still running, which is no ended child.
    w.install(child("exe_root", 81, WorkState::Running));
    ids.push("exe_child081".into());
    let changed: BTreeSet<String> = ids.iter().cloned().collect();
    let (mut gone, mut before) = (Vec::new(), HashMap::new());
    w.prune(&frame(100), &changed, &mut gone, &mut before);
    let count = |s: WorkState| w.views.values().filter(|v| v.state == s).count();
    assert_eq!(
        count(WorkState::Done),
        ENDED_KEPT,
        "the 50 newest ended stay"
    );
    assert_eq!(count(WorkState::Failed), 16, "failed tasks stay");
    assert!(
        w.views.contains_key("exe_child081"),
        "the running one stays"
    );
    let mut left: Vec<&str> = gone.iter().map(|(id, _)| id.as_str()).collect();
    left.sort_unstable();
    let oldest: Vec<String> = (1..=17u64)
        .filter(|n| n % 5 != 0)
        .map(|n| format!("exe_child{n:03}"))
        .collect();
    assert_eq!(
        left,
        oldest.iter().map(String::as_str).collect::<Vec<_>>(),
        "the 14 oldest left"
    );
    assert!(gone.iter().all(|(_, root)| root == "exe_root"));
    let mut b = before;
    assert!(w.roll(&frame(100), "exe_root", &mut b));
    let below = w.views["exe_root"].below.unwrap();
    // 64 done, 16 failed, 1 running, counted whether kept or let go.
    assert_eq!(
        (below.done, below.failed, below.working),
        (64, 16, 1),
        "{below:?}"
    );
    assert_eq!(below.total, 81);
}

/// The rollup's rules: a question is what needs you, a failure is its own
/// count, a planned step is not work under way, a wake is counted and no more.
#[test]
fn the_rollup_counts_what_needs_you_works_and_failed() {
    let mut r = Rollup::default();
    let mut q = blank("act_q", WorkKind::Question, 1);
    q.state = WorkState::NeedsYou;
    bump(&mut r, &q);
    let mut blocked = blank("exe_b", WorkKind::Task, 1);
    blocked.state = WorkState::NeedsYou;
    bump(&mut r, &blocked);
    let mut step = blank("tsk_s", WorkKind::Step, 1);
    step.state = WorkState::Queued;
    bump(&mut r, &step);
    let mut job = blank("act_j", WorkKind::Job, 1);
    job.state = WorkState::Running;
    bump(&mut r, &job);
    let mut wake = blank("wak_w", WorkKind::Wake, 1);
    wake.state = WorkState::Waiting;
    bump(&mut r, &wake);
    let mut bad = blank("act_f", WorkKind::Job, 1);
    bad.state = WorkState::Failed;
    bump(&mut r, &bad);
    assert_eq!(
        (r.needs_you, r.working, r.done, r.failed, r.total),
        (2, 1, 0, 1, 6),
        "{r:?}"
    );
}

fn job_action(corr: &str, state: ActionState, dispatched: u64, settled: Option<u64>) -> Action {
    Action {
        correlation_id: corr.into(),
        schema: 2,
        execution_id: "exe_a".into(),
        session_id: "ses_a".into(),
        tool: "proc.run".into(),
        args_digest: String::new(),
        proposal: None,
        resource: None,
        retry_class: RetryClass::NonRepeatable,
        state,
        deadline_at_ms: 0,
        planned_at_ms: dispatched,
        authorized_at_ms: None,
        dispatched_at_ms: Some(dispatched),
        settled_at_ms: settled,
        external_op_id: None,
        result_ref: None,
        confirm: None,
        cancel: None,
        verdict: None,
        reservation_id: None,
        reserved_micros: 0,
        resolution: None,
        completions_seen: 0,
        detail: None,
        parent: None,
    }
}

/// A job that settles gives its time to its program's: the median of the
/// last runs is the estimate, and a program with one run has none.
#[test]
fn a_settled_job_teaches_its_programs_usual_time() {
    let mut w = WorkBoard::default();
    let board = Board::default();
    let mut d = Dirty::default();
    for (i, ms) in [1_000u64, 3_000, 5_000].into_iter().enumerate() {
        let corr = format!("act_{i}");
        let open = job_action(&corr, ActionState::Dispatched, 10_000, None);
        w.take_action(&board, 10 + i as u64 * 4, &open, &mut d);
        w.acts.get_mut(&corr).unwrap().argv =
            vec!["cargo".into(), "test".into(), "-p".into(), "core".into()];
        let done = job_action(&corr, ActionState::Succeeded, 10_000, Some(10_000 + ms));
        w.take_action(&board, 12 + i as u64 * 4, &done, &mut d);
        if i == 0 {
            assert!(w.usual("cargo test").is_none(), "one run is no usual");
        }
    }
    let e = w.usual("cargo test").expect("three runs");
    assert_eq!((e.usual_ms, e.runs), (3_000, 3));
    // A failed run counts too; a cancelled one does not.
    let failed = job_action("act_f", ActionState::Dispatched, 0, None);
    w.take_action(&board, 40, &failed, &mut d);
    w.acts.get_mut("act_f").unwrap().argv = vec!["cargo".into(), "test".into()];
    let failed = job_action("act_f", ActionState::Failed, 0, Some(9_000));
    w.take_action(&board, 41, &failed, &mut d);
    assert_eq!(w.usual("cargo test").unwrap().runs, 4);
    let c = job_action("act_c", ActionState::Dispatched, 0, None);
    w.take_action(&board, 50, &c, &mut d);
    w.acts.get_mut("act_c").unwrap().argv = vec!["cargo".into(), "test".into()];
    let c = job_action("act_c", ActionState::Cancelled, 0, Some(50));
    w.take_action(&board, 51, &c, &mut d);
    assert_eq!(
        w.usual("cargo test").unwrap().runs,
        4,
        "a cancelled job says nothing"
    );
}

/// A ledger row the work board reads is found by its kind, a few bytes in.
#[test]
fn a_rows_kind_is_read_without_decoding_it() {
    let row = |kind: &str| {
        serde_json::to_vec(&LedgerRow::new(
            theseus_protocol::LedgerKind::parse(kind).unwrap(),
            Some("ses_x"),
            None,
            serde_json::json!({"correlation_id": "act_1"}),
        ))
        .unwrap()
    };
    for kind in [
        "wake.fired",
        "task.ended",
        "tool.job_started",
        "turn.started",
        "execution.queued",
    ] {
        assert!(wanted(&row(kind)), "{kind}");
    }
    for kind in [
        "provider.error",
        "context.compiled",
        "tool.notified",
        "turn.ended",
    ] {
        assert!(!wanted(&row(kind)), "{kind}");
    }
    // A payload that does not look as expected is kept, never dropped.
    assert!(wanted(b"{\"unexpected\":true}"));
}
