//! The notification policy: one test per row of the owner's table, the
//! delivery table, and the burst, with explicit times (this crate reads no
//! clock).

use super::*;
use crate::utc_hm;
use crate::work::{Cost, Doing, Rollup, Woke, WorkReport};
use crate::{Attention, Level};

/// 18:26 UTC.
const AT: u64 = 1_790_015_160_000;
const MIN: u64 = 60_000;

fn level(state: WorkState) -> Level {
    match state {
        WorkState::NeedsYou | WorkState::Failed => Level::NeedsYou,
        WorkState::Ready => Level::Ready,
        WorkState::Done | WorkState::Cancelled | WorkState::Expired => Level::Idle,
        _ => Level::Working,
    }
}

/// A root conversation, `harbour survey`, in `state` at `at`, at work since
/// `since`.
fn root(state: WorkState, at: u64, since: u64) -> WorkView {
    WorkView {
        id: "exe_000harbr1".into(),
        kind: WorkKind::Conversation,
        position: at / 1000,
        at_ms: at,
        parent: None,
        root: "exe_000harbr1".into(),
        session_id: Some("ses_000harbr1".into()),
        title: "harbour survey".into(),
        place: Some("cli".into()),
        state,
        previous: None,
        attention: Attention {
            level: level(state),
            label: state.as_str().into(),
            since_ms: since,
        },
        started_at_ms: since,
        state_since_ms: since,
        ended_at_ms: None,
        doing: None,
        progress: None,
        question: None,
        cost: Some(Cost {
            spent_usd: 1.0,
            limit_usd: 100.0,
            turn_usd: None,
        }),
        eta: None,
        below: None,
        why: None,
        report: None,
        woke: None,
        flagged: false,
    }
}

/// A task under the root.
fn task(title: &str, state: WorkState, at: u64) -> WorkView {
    WorkView {
        id: format!("exe_000{}", title.len()),
        kind: WorkKind::Task,
        parent: Some("exe_000harbr1".into()),
        session_id: Some(format!("ses_task{}", title.len())),
        title: title.into(),
        ..root(state, at, at - MIN)
    }
}

fn ask(id: &str, asked: u64, expires: u64) -> Question {
    let mut q = Question {
        id: id.into(),
        kind: QuestionKind::Approval,
        asked_by: WorkRef {
            id: "exe_00012".into(),
            kind: WorkKind::Task,
            session_id: None,
            title: "Beta summary".into(),
        },
        prompt: "fs.write create projects/beta-summary.txt (40 B)".into(),
        detail: None,
        input: None,
        options: Vec::new(),
        on_expiry: Some("not run".into()),
        expires_at_ms: Some(expires),
        floor: false,
        may_answer: true,
        tool: Some("fs.write".into()),
        asked_at_ms: asked,
        external_text: false,
        choices: Vec::new(),
    };
    q.options = crate::work::answer_options(&q);
    q
}

fn asking(v: WorkView, q: Question) -> WorkView {
    WorkView {
        state: WorkState::NeedsYou,
        attention: Attention {
            level: Level::NeedsYou,
            label: "confirm fs.write".into(),
            since_ms: q.asked_at_ms,
        },
        state_since_ms: q.asked_at_ms,
        doing: Some(Doing {
            what: crate::work::DoingKind::Asking,
            label: "confirm fs.write".into(),
            since_ms: q.asked_at_ms,
            turn: 1,
            loop_index: 0,
        }),
        question: Some(q),
        ..v
    }
}

fn run(prev: Option<&WorkView>, next: &WorkView) -> Option<Notice> {
    policy(prev, next, &Rules::default(), &utc_hm)
}

fn said(n: &Option<Notice>) -> (Urgency, Why, &str) {
    let n = n.as_ref().expect("a notice");
    (n.urgency, n.why, n.line.as_str())
}

/// Row 1: a question appears, of any kind, at any depth: Interrupt.
#[test]
fn row_1_a_question_interrupts() {
    let before = task("Beta summary", WorkState::Running, AT - 1_000);
    let after = asking(
        task("Beta summary", WorkState::Running, AT),
        ask("cor_1", AT, AT + 10 * MIN),
    );
    let n = run(Some(&before), &after);
    assert_eq!(
        said(&n),
        (
            Urgency::Interrupt,
            Why::Asked,
            "● Beta summary asks: fs.write create projects/beta-summary.txt (40 B)? · expires 18:36"
        )
    );
    let n = n.unwrap();
    assert_eq!(n.question.as_deref(), Some("cor_1"));
    assert_eq!(n.root.id, "exe_000harbr1", "the root of its tree");
    // A question first seen (no view before) asks too; the same one again
    // does not.
    assert_eq!(run(None, &after).map(|n| n.why), Some(Why::Asked));
    let mut later = after.clone();
    later.at_ms += 1_000;
    assert_eq!(run(Some(&after), &later), None, "once per question");
}

/// Row 2: at half its life, once: Interrupt.
#[test]
fn row_2_a_question_is_reminded_of_once_at_half_its_life() {
    let v = asking(
        task("Beta summary", WorkState::Running, AT),
        ask("cor_1", AT, AT + 14 * MIN),
    );
    assert_eq!(next_reminder(&v, &Rules::default()), Some(AT + 7 * MIN));
    let mut early = v.clone();
    early.at_ms = AT + 6 * MIN;
    assert_eq!(run(Some(&v), &early), None, "before half its life");
    let mut due = v;
    due.at_ms = AT + 7 * MIN;
    assert_eq!(
        said(&run(Some(&early), &due)),
        (
            Urgency::Interrupt,
            Why::Reminder,
            "● still waiting, 7 min left: Beta summary's fs.write"
        )
    );
    let mut after = due.clone();
    after.at_ms = AT + 8 * MIN;
    assert_eq!(run(Some(&due), &after), None, "once");
    assert_eq!(next_reminder(&due, &Rules::default()), None);
}

/// Row 3: a failure, or a block with no question: Interrupt, once per
/// transition.
#[test]
fn row_3_a_failure_or_a_block_interrupts_once() {
    let before = task("Alpha check", WorkState::Running, AT - 1_000);
    let mut failed = task("Alpha check", WorkState::Failed, AT);
    failed.attention.label = "failed: the provider refused the key".into();
    failed.report = Some(WorkReport {
        outcome: "failed".into(),
        first_line: None,
        reason: Some("the provider refused the key".into()),
    });
    assert_eq!(
        said(&run(Some(&before), &failed)),
        (
            Urgency::Interrupt,
            Why::Failed,
            "✗ Alpha check failed: the provider refused the key"
        )
    );
    let mut again = failed.clone();
    again.position += 1;
    assert_eq!(
        run(Some(&failed), &again),
        None,
        "never again for the same state"
    );
    let mut blocked = task("Alpha check", WorkState::NeedsYou, AT);
    blocked.attention.label = "budget exhausted".into();
    assert_eq!(
        said(&run(Some(&before), &blocked)),
        (
            Urgency::Interrupt,
            Why::Blocked,
            "✗ Alpha check budget exhausted"
        )
    );
}

/// Row 4: a wake the owner set fires on a root conversation: Interrupt; a
/// task's own wake informs.
#[test]
fn row_4_a_wake_on_a_root_interrupts_and_a_tasks_informs() {
    let ready = root(WorkState::Ready, AT - MIN, AT - 2 * MIN);
    let mut woke = root(WorkState::Ready, AT, AT - 2 * MIN);
    woke.title = "ops".into();
    woke.woke = Some(Woke {
        note: "the tide turns".into(),
        at_ms: AT - 30_000,
    });
    assert_eq!(
        said(&run(Some(&ready), &woke)),
        (Urgency::Interrupt, Why::Woke, "⏰ ops · the tide turns")
    );
    let mut later = woke.clone();
    later.position += 5;
    assert_eq!(run(Some(&woke), &later), None, "once");
    // From today's push: queued for a wake.
    let mut queued = root(WorkState::Queued, AT, AT);
    queued.why = Some("wake".into());
    assert_eq!(
        said(&run(Some(&ready), &queued)),
        (
            Urgency::Interrupt,
            Why::Woke,
            "⏰ harbour survey · its wake fired"
        )
    );
    let mut running = root(WorkState::Running, AT + 1, AT);
    running.why = Some("wake".into());
    assert_eq!(run(Some(&queued), &running), None, "the same wake");
    // A task's own wake: a badge.
    let sleeping = task("Gamma poll", WorkState::Waiting, AT - MIN);
    let mut t = task("Gamma poll", WorkState::Queued, AT);
    t.why = Some("wake".into());
    assert_eq!(
        said(&run(Some(&sleeping), &t)),
        (Urgency::Inform, Why::Woke, "⏰ Gamma poll · its wake fired")
    );
}

/// Row 5: a flagged root, or one that worked 5 minutes or more, goes to
/// ready: Interrupt, no sound.
#[test]
fn row_5_a_long_or_flagged_root_finishing_interrupts_without_sound() {
    let working = root(WorkState::Running, AT - 1_000, AT - 14 * MIN);
    let mut ready = root(WorkState::Ready, AT, AT);
    ready.below = Some(Rollup {
        needs_you: 0,
        working: 0,
        done: 3,
        failed: 0,
        total: 3,
    });
    let n = run(Some(&working), &ready);
    assert_eq!(
        said(&n),
        (
            Urgency::Interrupt,
            Why::Finished,
            "✓ harbour survey is ready after 14 min · 3 tasks reported"
        )
    );
    let viewer = Viewer {
        sound: true,
        ..Viewer::default()
    };
    assert_eq!(
        deliver(&n.unwrap(), &viewer),
        Delivery::Ping { sound: false },
        "a finish pings without sound"
    );
    // A short one informs; flagged, it interrupts.
    let short = root(WorkState::Running, AT - 1_000, AT - 2 * MIN);
    assert_eq!(
        said(&run(Some(&short), &ready)).0,
        Urgency::Inform,
        "a short turn's end is row 8"
    );
    let mut flagged = ready.clone();
    flagged.flagged = true;
    assert_eq!(said(&run(Some(&short), &flagged)).0, Urgency::Interrupt);
    // The rules are data: under `everything` every root's finish interrupts.
    let every = Rules {
        finished: Finished::Everything,
        ..Rules::default()
    };
    assert_eq!(
        policy(Some(&short), &ready, &every, &utc_hm).map(|n| n.urgency),
        Some(Urgency::Interrupt)
    );
}

/// Row 6: a question answered, anywhere: Quiet, and it retracts the ping.
#[test]
fn row_6_an_answer_is_quiet_and_retracts_the_ping() {
    let q = ask("cor_1", AT, AT + 10 * MIN);
    let waiting = asking(task("Beta summary", WorkState::Running, AT), q.clone());
    let resumed = task("Beta summary", WorkState::Running, AT + MIN);
    let n = run(Some(&waiting), &resumed);
    assert_eq!(
        said(&n),
        (
            Urgency::Quiet,
            Why::Answered,
            "✓ answered: Beta summary's fs.write"
        )
    );
    assert_eq!(n.unwrap().retracts.as_deref(), Some("cor_1"));
    // A question's own view, with its answer's report.
    let open = WorkView {
        id: "act_cor1".into(),
        kind: WorkKind::Question,
        title: q.prompt,
        ..waiting
    };
    let mut done = WorkView {
        state: WorkState::Done,
        at_ms: AT + MIN,
        ..open.clone()
    };
    done.report = Some(WorkReport {
        outcome: "approved".into(),
        first_line: None,
        reason: Some("by you (CLI)".into()),
    });
    let n = run(Some(&open), &done);
    assert_eq!(
        said(&n),
        (
            Urgency::Quiet,
            Why::Answered,
            "✓ approved by you (CLI): Beta summary"
        )
    );
    assert_eq!(n.unwrap().retracts.as_deref(), Some("cor_1"));
    assert_eq!(
        deliver(&run(Some(&open), &done).unwrap(), &Viewer::default()),
        Delivery::Nothing
    );
}

/// Row 7: a question expires unanswered: Inform, and it retracts.
#[test]
fn row_7_an_expired_question_informs() {
    let waiting = asking(
        task("Beta summary", WorkState::Running, AT),
        ask("cor_1", AT, AT + 10 * MIN),
    );
    let after = task("Beta summary", WorkState::Running, AT + 10 * MIN);
    let n = run(Some(&waiting), &after);
    assert_eq!(
        said(&n),
        (
            Urgency::Inform,
            Why::Expired,
            "⌛ Beta summary's fs.write expired, not run"
        )
    );
    assert_eq!(n.unwrap().retracts.as_deref(), Some("cor_1"));
}

/// Row 8: a task reports, a job ends, a short turn ends: Inform.
#[test]
fn row_8_a_report_a_jobs_end_and_a_short_turn_inform() {
    let running = task("Alpha check", WorkState::Running, AT - 1_000);
    let mut done = task("Alpha check", WorkState::Done, AT);
    done.report = Some(WorkReport {
        outcome: "done".into(),
        first_line: Some("the soundings agree".into()),
        reason: None,
    });
    assert_eq!(
        said(&run(Some(&running), &done)),
        (
            Urgency::Inform,
            Why::Reported,
            "📋 Alpha check reported: the soundings agree"
        )
    );
    let plain = task("Alpha check", WorkState::Done, AT);
    assert_eq!(
        said(&run(Some(&running), &plain)),
        (Urgency::Inform, Why::Reported, "📋 Alpha check reported")
    );
    let job = |state| WorkView {
        id: "act_job1".into(),
        kind: WorkKind::Job,
        title: "cargo test -p core".into(),
        ..task("x", state, AT)
    };
    assert_eq!(
        said(&run(Some(&job(WorkState::Running)), &job(WorkState::Done))),
        (Urgency::Inform, Why::Finished, "✓ cargo test -p core ended")
    );
    assert_eq!(
        said(&run(
            Some(&job(WorkState::Running)),
            &job(WorkState::Failed)
        )),
        (
            Urgency::Inform,
            Why::Finished,
            "✗ cargo test -p core failed"
        ),
        "a job's failure is its caller's to handle"
    );
    let short = root(WorkState::Running, AT - 1_000, AT - 30_000);
    assert_eq!(
        said(&run(Some(&short), &root(WorkState::Ready, AT, AT))),
        (Urgency::Inform, Why::Finished, "✓ harbour survey is ready")
    );
}

/// Row 9: spend crosses its limit, or a multiple of it: Inform.
#[test]
fn row_9_spend_past_its_limit_informs() {
    let spent = |usd: f64| {
        let mut v = root(WorkState::Running, AT, AT);
        v.cost = Some(Cost {
            spent_usd: usd,
            limit_usd: 100.0,
            turn_usd: None,
        });
        v
    };
    assert_eq!(
        said(&run(Some(&spent(99.0)), &spent(100.5))),
        (
            Urgency::Inform,
            Why::SpendCrossed,
            "$ harbour survey passed its $100 limit"
        )
    );
    assert_eq!(
        said(&run(Some(&spent(199.0)), &spent(201.0))).2,
        "$ harbour survey passed $200, 2× its $100 limit"
    );
    assert_eq!(
        run(Some(&spent(100.5)), &spent(150.0)),
        None,
        "once per multiple"
    );
}

/// Row 10: working to working, queued, spend only, cancelled: nothing.
#[test]
fn row_10_what_says_nothing() {
    let running = root(WorkState::Running, AT, AT - MIN);
    let mut waiting = root(WorkState::Waiting, AT + 1, AT - MIN);
    waiting.cost = Some(Cost {
        spent_usd: 3.0,
        limit_usd: 100.0,
        turn_usd: None,
    });
    assert_eq!(
        run(Some(&running), &waiting),
        None,
        "working to working, spend only"
    );
    assert_eq!(
        run(Some(&running), &root(WorkState::Queued, AT, AT)),
        None,
        "queued"
    );
    assert_eq!(
        run(Some(&running), &root(WorkState::Cancelled, AT, AT)),
        None,
        "cancelled"
    );
    assert_eq!(run(None, &running), None, "first seen, working");
}

/// `deliver()`'s table: in focus, nothing; seen, nothing; Inform, a badge;
/// quiet, no sound; a finish, no sound.
#[test]
fn delivery_follows_its_table() {
    let mut n = Notice {
        urgency: Urgency::Interrupt,
        why: Why::Asked,
        work: WorkRef {
            id: "exe_1".into(),
            kind: WorkKind::Task,
            session_id: Some("ses_1".into()),
            title: "Beta summary".into(),
        },
        root: WorkRef {
            id: "exe_0".into(),
            kind: WorkKind::Conversation,
            session_id: None,
            title: String::new(),
        },
        line: "● Beta summary asks".into(),
        question: Some("cor_1".into()),
        retracts: None,
        position: 50,
        at_ms: AT,
    };
    let v = Viewer {
        focused: None,
        seen: 0,
        away_ms: 0,
        quiet: false,
        sound: true,
    };
    assert_eq!(deliver(&n, &v), Delivery::Ping { sound: true });
    let focused = Viewer {
        focused: Some("ses_1".into()),
        ..v
    };
    assert_eq!(deliver(&n, &focused), Delivery::Nothing, "in focus");
    let away = Viewer {
        away_ms: 30_000,
        ..focused
    };
    assert_eq!(
        deliver(&n, &away),
        Delivery::Ping { sound: true },
        "focus counts while present"
    );
    let seen = Viewer {
        seen: 50,
        ..v.clone()
    };
    assert_eq!(deliver(&n, &seen), Delivery::Nothing, "seen already");
    let quiet = Viewer {
        quiet: true,
        ..v.clone()
    };
    assert_eq!(deliver(&n, &quiet), Delivery::Ping { sound: false });
    n.why = Why::Finished;
    assert_eq!(deliver(&n, &v), Delivery::Ping { sound: false }, "silent");
    n.urgency = Urgency::Inform;
    assert_eq!(deliver(&n, &v), Delivery::Badge);
    n.urgency = Urgency::Quiet;
    assert_eq!(deliver(&n, &v), Delivery::Nothing);
}

/// Three questions in 10 s ping once; the burst reads `3 questions need
/// you`; a fourth after the window is its own.
#[test]
fn a_burst_of_questions_pings_once() {
    let rules = Rules::default();
    let asked = |i: u64, at: u64| {
        let before = task(&format!("Task {i}"), WorkState::Running, at - 1);
        let after = asking(
            task(&format!("Task {i}"), WorkState::Running, at),
            ask(&format!("cor_{i}"), at, at + 10 * MIN),
        );
        run(Some(&before), &after).unwrap()
    };
    let mut b = Burst::new(&rules);
    let out: Vec<Notice> = [asked(1, AT), asked(2, AT + 3_000), asked(3, AT + 6_000)]
        .into_iter()
        .map(|n| b.fold(n))
        .collect();
    let pings = out
        .iter()
        .filter(|n| n.urgency == Urgency::Interrupt)
        .count();
    assert_eq!(pings, 1, "{out:?}");
    assert_eq!(out[2].line, "3 questions need you");
    let fourth = b.fold(asked(4, AT + 10_000));
    assert_eq!(fourth.urgency, Urgency::Interrupt, "its own");
    assert!(fourth.line.starts_with("● Task 4 asks"), "{}", fourth.line);
    // Arriving together: one Interrupt that reads `3 questions need you`.
    let mut b = Burst::new(&rules);
    let out = b.fold_all(vec![asked(1, AT), asked(2, AT + 1), asked(3, AT + 2)]);
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(
        (out[0].urgency, out[0].line.as_str()),
        (Urgency::Interrupt, "3 questions need you")
    );
}

/// The notice's wire shape keeps its optional fields absent.
#[test]
fn a_notice_round_trips() {
    let n = run(
        Some(&root(WorkState::Running, AT - 1_000, AT - 14 * MIN)),
        &root(WorkState::Ready, AT, AT),
    )
    .unwrap();
    let wire = serde_json::to_value(&n).unwrap();
    assert!(wire.get("retracts").is_none(), "{wire}");
    assert_eq!(wire["urgency"], "interrupt");
    assert_eq!(serde_json::from_value::<Notice>(wire).unwrap(), n);
    let r: Rules = serde_json::from_str(r#"{"long_run_ms": 60000}"#).unwrap();
    assert_eq!(
        (r.long_run_ms, r.burst_ms),
        (60_000, 10_000),
        "a partial table reads"
    );
}
