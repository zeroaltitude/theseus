//! A running task's and a turn's checkpoints, through route.v10 (theseus-h66v4.11). route.v8's
//! checkpoint tests reached their case through the class walk (`mode(&jev, "chat" |
//! "deep_coding")`, whose verdict words are classes); the checkpoint machinery they tested did not
//! go (`turn/checkpoint_step.rs`): a task or a routed turn whose loops keep failing asks the acting
//! route pack about its recent state, and Jev's pick of a model, and of an effort, applies at the
//! next loop boundary. Here the verdict is Jev's pick (`model`) and answer (`effort`), by the
//! trigger line the checkpoint's text carries. The rig is `tests_task_brief`'s (the driver runs
//! the tasks) and `tests_turn_checkpoint`'s, with a provider that answers a script of loops a
//! little slowly, so a verdict lands at a loop boundary.
//!
//! - a checkpoint's cost is the task's (`spent`), and the row says how much;
//! - a task that fails for twelve loops asks three times, one per four loops;
//! - the work board says a task a checkpoint moved is on its new model, only that one;
//! - a verdict that takes its time never holds a loop, and applies when it comes;
//! - a checkpoint raises a turn's effort with no move, past the sticky rule, and never lowers it.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_kernel::ExecState;

use crate::catalog::CatalogRow;
use crate::provider::{FakeProvider, ProviderRequest, RequestGate};
use crate::session::{SessionRecord, TargetRef};
use crate::test_support::until_event;
use crate::tests_judge::kinds;
use crate::tests_route::{decided, rig_parts, turn, Rig};
use crate::tests_task_brief::{cli, params, BRIEF};
use crate::tests_tasks::exec;
use crate::tests_turn_checkpoint::{checkpoints, loops as turn_loops, ASK};

/// A loop whose one tool call fails: a call with no input is invalid.
fn failing(n: usize) -> crate::provider::Scripted {
    crate::provider::Scripted::tools(
        &format!("Trying approach {n}."),
        &[(&format!("f{n}"), "text_diff", json!({}))],
    )
}

/// A script of `n` failing loops, then the report.
fn loops(n: usize) -> Vec<crate::provider::Scripted> {
    let mut s: Vec<_> = (0..n).map(failing).collect();
    s.push(crate::provider::Scripted::text("Done: the report."));
    s
}

fn pick(option: &str, confidence: f64) -> Jev {
    Jev::Choice {
        option: option.into(),
        confidence,
    }
}

/// The checkpoint's verdict when its text's last line holds `trigger`: Jev's pick of `option`.
fn on_trigger_pick(jev: &FakeJev, trigger: &str, option: &str) {
    jev.script_when(trigger, None, "model", pick(option, 0.95));
}

struct Task {
    rig: Rig,
    claude: Arc<FakeProvider>,
    jev: FakeJev,
    id: String,
}

/// A task from a Fable conversation whose brief is judged a Haiku one, running `script`, on a
/// provider that takes 60 ms a loop, with the acting route pack asking `model`.
async fn task_open(
    script: Vec<crate::provider::Scripted>,
    tweak: impl FnOnce(&mut crate::Config),
    verdicts: impl FnOnce(&FakeJev),
) -> Task {
    let jev = FakeJev::start().unwrap();
    // The baseline is the cheap model a brief starts on; a verdict is a pick.
    jev.script("model", pick("haiku", 0.95));
    verdicts(&jev);
    let claude = Arc::new(FakeProvider {
        delay_ms: 60,
        ..FakeProvider::scripted(script)
    });
    // A pick runs only where the provider's key is named (a placeholder is no key).
    let rig = task_rig(&jev, claude.clone(), |c| {
        c.name_every_provider_key();
        tweak(c);
    })
    .await;
    rig.core
        .runner
        .judge
        .job_pack_with(crate::tests_job_ask::model_pack());
    task_run(rig, jev, claude).await
}

/// The rig on `jev` and `claude`, its driver running, and the home conversation on Fable.
async fn task_rig(
    jev: &FakeJev,
    claude: Arc<FakeProvider>,
    tweak: impl FnOnce(&mut crate::Config),
) -> Rig {
    let provider = claude.clone();
    let rig = rig_parts(Some(jev), 4, tweak, move |p| {
        p.providers.insert("anthropic".into(), provider);
    });
    rig.core.push.ensure(&rig.core).await.unwrap();
    tokio::spawn(crate::harness::drive(rig.core.clone()));
    let home = rig.core.home().unwrap();
    let (live, _) = rig.core.live_profile();
    let at = rig
        .core
        .runner
        .resolve_target(&live, Some("fable"), None, None)
        .unwrap();
    let mut rec: SessionRecord = rig
        .core
        .store
        .get_session(&home.session_id)
        .unwrap()
        .unwrap();
    rec.last_target = Some(TargetRef::from(&at));
    rig.core.store.put_session(&rec.session_id, &rec).unwrap();
    rig
}

/// The task, started on `rig` and run to the end of its first turn.
async fn task_run(rig: Rig, jev: FakeJev, claude: Arc<FakeProvider>) -> Task {
    let started = rig.core.task_start(&params(BRIEF, None), cli()).unwrap();
    let id = started.session_id.replacen("ses_", "exe_", 1);
    // The task's first turn ends, however long a starved host takes.
    until_event("the task's first turn", || {
        exec(&rig.core, &id).state == ExecState::Complete
    })
    .await;
    Task {
        rig,
        claude,
        jev,
        id,
    }
}

impl Task {
    /// The model each loop's request went to, by its short name.
    fn models(&self) -> Vec<&'static str> {
        self.claude
            .requests()
            .iter()
            .map(|r| match r.model.as_str() {
                "claude-haiku-5-5" => "haiku",
                "claude-sonnet-5-5" => "sonnet",
                "claude-opus-5-5" => "opus",
                "claude-fable-5-1" => "fable",
                _ => "other",
            })
            .collect()
    }

    /// The checkpoints' `route.decided` rows, in order.
    fn checkpoints(&self) -> Vec<Value> {
        decided(&self.rig.core.store)
            .into_iter()
            .filter(|d| d["source"] == "task checkpoint")
            .collect()
    }

    fn spent(&self) -> u64 {
        exec(&self.rig.core, &self.id).budget.spent_micros
    }
}

/// The first loop on `to` and every later loop on it.
fn moved_at(models: &[&str], to: &str) -> usize {
    let at = models.iter().position(|m| *m == to).expect("it moved");
    assert!(models[at..].iter().all(|m| *m == to), "{models:?}");
    at
}

/// At most one verdict per `min_loops_between` loops: a task failing for 12 loops asks three
/// times, and a verdict that keeps its model (Haiku again) moves nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_failing_for_twelve_loops_asks_three_times() {
    let t = task_open(
        loops(12),
        |_| {},
        |j| on_trigger_pick(j, "failing: ", "haiku"),
    )
    .await;
    // Each ask is a few requests to Jev (the route pack's and its family's), so the asks are
    // counted by the line each carries: the streak it found failing, at loops 3, 7 and 11.
    let seen: Vec<String> = t.jev.seen().iter().map(|s| s.body.to_string()).collect();
    let asked: Vec<usize> = (1..=12)
        .filter(|n| seen.iter().any(|b| b.contains(&format!("failing: {n} failed"))))
        .collect();
    assert_eq!(asked, [3, 7, 11], "one per four loops of the twelve");
    assert_eq!(t.checkpoints().len(), 3);
    assert_eq!(t.models(), ["haiku"; 13], "a Haiku pick keeps the task's own");
}

/// Jev's cost is the task's: its `spent` holds what the checkpoints settled at, and the row says
/// how much.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_checkpoints_cost_lands_in_the_tasks_spent() {
    let with = task_open(
        loops(8),
        |_| {},
        |j| on_trigger_pick(j, "failing: ", "haiku"),
    )
    .await;
    let off = task_open(
        loops(8),
        |c| c.routing.task_checkpoints.enabled = false,
        |_| {},
    )
    .await;
    let rows = with.checkpoints();
    assert!(!rows.is_empty(), "the task asked");
    let cost: f64 = rows.iter().map(|d| d["cost_usd"].as_f64().unwrap()).sum();
    assert!(cost > 0.0, "the fake bills its usage");
    let extra = with.spent() as i64 - off.spent() as i64;
    let micros = (cost * 1e6).round() as i64;
    assert!(
        (extra - micros).abs() <= 2,
        "the task's spent grew by the checkpoints' cost: {extra} against {micros}"
    );
}

/// The work board and its progress line say where a moved task runs, only for a task a checkpoint
/// moved off its base; the `loop.ended` rows carry it, and a task on its base has none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_board_says_a_moved_task_is_on_its_new_model() {
    let moved = task_open(
        loops(8),
        |_| {},
        |j| {
            // Jev decides at each trigger and a cheaper verdict moves a task down at once (there
            // is no base to step back to), so a task that stays moved is told so each time.
            on_trigger_pick(j, "failing: 3 failed", "sonnet");
            on_trigger_pick(j, "failing: 7 failed", "sonnet");
        },
    )
    .await;
    let kept = task_open(loops(8), |_| {}, |_| {}).await;
    let on = |t: &Task| {
        let v = t
            .rig
            .core
            .push
            .work_all()
            .remove(&t.id)
            .expect("the task's view");
        (
            v.moved_to.clone(),
            theseus_protocol::work::progress_line::progress_line(&v, v.at_ms),
        )
    };
    let (to, line) = on(&moved);
    assert_eq!(to.as_deref(), Some("sonnet"));
    assert!(line.unwrap().ends_with(" · on sonnet"));
    let (to, line) = on(&kept);
    assert_eq!(to, None);
    assert!(
        !line.unwrap_or_default().contains(" on "),
        "a task on its base says nothing"
    );
    let ended = kinds(&moved.rig.core.store, "loop.ended");
    let seen: Vec<_> = ended.iter().map(|r| r.data["on"].as_str()).collect();
    assert_eq!(seen[..3], [None, None, None], "{seen:?}");
    assert_eq!(seen.last(), Some(&Some("sonnet")));
}

/// A verdict that takes its time never holds a loop: the task runs on at its base while the
/// checkpoint is out, and moves at the first boundary after the answer comes. Ordered by events,
/// never by a clock (theseus-1b2zm): Jev holds the checkpoint's answer, the provider holds the
/// seventh request, and the test lets the verdict go and waits until it has come to the turn
/// before it lets the loop go on. So the loops after the ask (three of them, 3 to 5, and the
/// seventh's own call) ran on the base with the verdict out, and the move is at the first boundary
/// after it came: the eighth request, never another.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes() {
    const ASKED: &str = "failing: 3 failed";
    let gate = RequestGate::at(6);
    let claude = Arc::new(FakeProvider {
        gate: Some(gate.clone()),
        ..FakeProvider::scripted(loops(20))
    });
    let jev = FakeJev::start().unwrap();
    jev.script("model", pick("haiku", 0.95));
    on_trigger_pick(&jev, ASKED, "sonnet");
    // The next ask, twelve loops on, keeps it there: a cheaper one would move it down at once.
    on_trigger_pick(&jev, "failing: 15 failed", "sonnet");
    jev.hold_when(ASKED);
    let rig = task_rig(&jev, claude.clone(), |c| {
        c.name_every_provider_key();
        c.routing.task_checkpoints.min_loops_between = 12;
        // The held verdict is the test's to release: the judge's own deadline must not end it
        // first when a starved host runs the loops slowly (theseus-iubei).
        c.judge.total_secs = 3_600;
    })
    .await;
    rig.core
        .runner
        .judge
        .job_pack_with(crate::tests_job_ask::model_pack());
    let core = rig.core.clone();
    let seen = jev.clone();
    let (t, ()) = tokio::join!(task_run(rig, jev.clone(), claude), async move {
        // A held loop would never send the seventh request: nextest's kill ends such a hang.
        gate.reached().await;
        let asked = |s: &theseus_judge::fake::Seen| s.body["state"].to_string().contains(ASKED);
        until_event("the judge's request", || seen.seen().iter().any(asked)).await;
        let answers = || core.runner.judge.route_answers.load(Ordering::SeqCst);
        let before = answers();
        seen.release();
        until_event("the route answer", || answers() > before).await;
        gate.open();
    });
    let models = t.models();
    let at = moved_at(&models, "sonnet");
    assert_eq!(at, 7, "the first boundary after it came: {models:?}");
    assert!(
        models[3..at].iter().all(|m| *m == "haiku"),
        "the loops after the ask ran on: {models:?}"
    );
}

// ------------------------------------------------------------------ a turn's checkpoint effort

/// The template with Sonnet 5.5 taking no per-message effort (so a turn's level rides the top of
/// the request), and every request long enough for the sticky rule.
fn top_level_effort(c: &mut crate::Config) {
    c.cache.keep_warm_min_tokens = 1;
    let row = CatalogRow {
        per_message_effort: Some(false),
        ..Default::default()
    };
    c.catalog.insert("claude-sonnet-5-5".to_string(), row);
}

/// The top-level effort each request carried, and that every one went to Sonnet 5.5.
fn levels(claude: &FakeProvider) -> Vec<String> {
    let reqs: Vec<ProviderRequest> = claude.requests();
    assert!(reqs.iter().all(|r| r.model == "claude-sonnet-5-5"));
    reqs.iter()
        .map(|r| {
            r.output_config
                .as_ref()
                .and_then(|o| o["effort"].as_str())
                .unwrap_or("none")
                .to_string()
        })
        .collect()
}

/// Where the requests moved from `before` to `after` (at a loop after the third), and that they
/// stayed.
fn raised_at(levels: &[String], before: &str, after: &str) -> usize {
    let at = levels
        .iter()
        .position(|l| l == after)
        .unwrap_or_else(|| panic!("never {after}: {levels:?}"));
    assert!((3..=6).contains(&at), "{levels:?}");
    assert!(levels[..at].iter().all(|l| l == before), "{levels:?}");
    assert!(levels[at..].iter().all(|l| l == after), "{levels:?}");
    at
}

/// A conversation on Sonnet whose route picks Sonnet at `high`, and whose checkpoint, on three
/// failed loops, picks Sonnet again and asks `level` at `confidence`.
fn routed(level: &str, confidence: f64) -> (Rig, Arc<FakeProvider>, FakeJev) {
    let jev = FakeJev::start().unwrap();
    crate::tests_route_open::says(&jev, "sonnet", "high", 0.95);
    let at = "failing: 3 failed";
    jev.script_when(at, None, "model", pick("sonnet", 0.95));
    jev.script_when(at, None, "effort", pick(level, confidence));
    let claude = Arc::new(FakeProvider {
        delay_ms: 60,
        ..FakeProvider::scripted(turn_loops(8))
    });
    let provider = claude.clone();
    let rig = rig_parts(Some(&jev), 4, top_level_effort, move |p| {
        p.providers.insert("anthropic".into(), provider);
    });
    crate::tests_act_open::act_open(&rig);
    (rig, claude, jev)
}

/// A turn whose loops keep failing gets a verdict that keeps its model and asks `xhigh`: its next
/// request carries it though the prompt is past the sticky floor, no move is made, and the row
/// says it applied.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_checkpoint_raises_effort_without_a_move() {
    let (rig, claude, _jev) = routed("xhigh", 0.9);
    let res = turn(&rig.core, None, ASK, None).await;
    assert_eq!(res.profile, "sonnet", "{}", res.output);
    let levels = levels(&claude);
    assert_eq!(levels.len(), 9, "{}", res.output);
    raised_at(&levels, "high", "xhigh");
    let rows = checkpoints(&rig);
    let row = &rows[0];
    assert_eq!(
        (
            &row["profile"],
            &row["switch"],
            &row["effort_applied"],
            &row["effort_ran"]
        ),
        (&json!("sonnet"), &json!(false), &json!(true), &json!("xhigh")),
        "{rows:?}"
    );
}
