//! A running task's checkpoints (theseus-ypnxt.9): a task asks route.v6 about
//! its recent state at a loop boundary, and its model moves up, or back down
//! to its base, by routing's own rules. The rig is `tests_task_brief`'s (the
//! driver runs the tasks, a fake Jev answers the brief's class and each
//! checkpoint's, by the trigger line the checkpoint's text ends with), with a
//! provider that answers a script of loops, a little slowly, so a verdict
//! lands at a loop boundary.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::provider::{FakeProvider, Scripted};
use crate::session::{SessionRecord, TargetRef};
use crate::tests_judge::kinds;
use crate::tests_route::{decided, mode, rig_parts, session, Rig};
use crate::tests_task_brief::{params, run, BRIEF};
use crate::tests_tasks::exec;

/// A loop whose one tool call fails: a call with no input is invalid.
fn failing(n: usize) -> Scripted {
    Scripted::tools(
        &format!("Trying approach {n}."),
        &[(&format!("f{n}"), "text_diff", json!({}))],
    )
}

/// A loop whose one tool call works, and whose words are its own.
fn passing(n: usize) -> Scripted {
    Scripted::tools(
        &format!("Checking part {n}."),
        &[(
            &format!("p{n}"),
            "text_diff",
            json!({"a": "x\n", "b": "y\n"}),
        )],
    )
}

/// A script: `failing` loops, `passing` loops, then the report.
fn loops(failing_n: usize, passing_n: usize) -> Vec<Scripted> {
    let mut s: Vec<Scripted> = (0..failing_n).map(failing).collect();
    s.extend((0..passing_n).map(passing));
    s.push(Scripted::text("Done: the report."));
    s
}

/// The checkpoint's verdict when its text's last line holds `trigger`.
fn on_trigger(jev: &FakeJev, trigger: &str, class: &str) {
    jev.script_when(
        trigger,
        None,
        "mode",
        Jev::Choice {
            option: class.into(),
            confidence: 0.95,
        },
    );
}

struct Task {
    rig: Rig,
    claude: Arc<FakeProvider>,
    jev: FakeJev,
    id: String,
    rec: SessionRecord,
}

/// A task from a Fable conversation whose brief is judged routine coding (the
/// rig's Haiku), running `script`, on a provider that takes 60 ms a loop.
async fn task_with(
    script: Vec<Scripted>,
    profile: Option<&str>,
    tweak: impl FnOnce(&mut crate::Config),
    verdicts: impl FnOnce(&FakeJev),
) -> Task {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    verdicts(&jev);
    let claude = Arc::new(FakeProvider {
        delay_ms: 60,
        ..FakeProvider::scripted(script)
    });
    let provider = claude.clone();
    let rig = rig_parts(
        Some(&jev),
        4,
        |c| {
            c.routing.classes.deep_coding.profiles = vec!["sonnet".into()];
            c.routing.classes.sophisticated.profiles = vec!["opus".into()];
            tweak(c);
        },
        move |p| {
            p.providers.insert("anthropic".into(), provider);
        },
    );
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
    let (started, rec) = run(&rig, &params(BRIEF, profile)).await;
    let id = started.session_id.replacen("ses_", "exe_", 1);
    Task {
        rig,
        claude,
        jev,
        id,
        rec,
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

// A. What a task did before checkpoints, pinned.

/// A task on a cheap base whose loops fail many times in a row ran every loop
/// on its base, and the route pack was asked once, for the brief. (Converted
/// below: with checkpoints it moves up.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_task_with_checkpoints_off_stays_on_its_base() {
    let t = task_with(
        loops(8, 0),
        None,
        |c| c.routing.task_checkpoints.enabled = false,
        |_| {},
    )
    .await;
    assert_eq!(t.models(), ["haiku"; 9], "every loop on the brief's base");
    assert_eq!(t.jev.connections(), 1, "the brief is the one question");
    assert_eq!(t.rec.last_target.unwrap().profile, "haikuhi");
}

/// A task started with a profile runs every loop on it, and the route pack is
/// asked nothing, whatever its loops do.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_task_runs_every_loop_on_its_profile_and_asks_nothing() {
    let t = task_with(
        loops(8, 0),
        Some("opus"),
        |_| {},
        |j| {
            on_trigger(j, "failing: ", "sophisticated");
        },
    )
    .await;
    assert_eq!(t.models(), ["opus"; 9]);
    assert_eq!(t.jev.connections(), 0, "a pinned task asks nothing");
    assert!(t.checkpoints().is_empty());
}

// B to E. The checkpoints and the rule.

/// A task on Haiku whose loops fail three times in a row moves to Sonnet at a
/// later loop boundary, recorded with what the cold switch costs. (The old
/// assertion, `["haiku"; 9]` and one question, is the test above's.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_whose_loops_fail_moves_to_sonnet_at_a_later_loop() {
    let t = task_with(
        loops(8, 0),
        None,
        |_| {},
        |j| {
            on_trigger(j, "failing: 3 failed", "deep_coding");
        },
    )
    .await;
    let models = t.models();
    assert_eq!(models.len(), 9);
    let at = moved_at(&models, "sonnet");
    assert!(
        (3..=6).contains(&at),
        "asked at the third loop's end, applied at a later boundary: {models:?}"
    );
    assert!(models[..3].iter().all(|m| *m == "haiku"));
    let rows = t.checkpoints();
    let first = &rows[0];
    assert_eq!(
        (
            first["trigger"].as_str(),
            first["profile"].as_str(),
            first["from"].as_str(),
            first["switch"].as_bool(),
            first["mode"].as_str(),
        ),
        (
            Some("failing"),
            Some("sonnet"),
            Some("haikuhi"),
            Some(true),
            Some("deep_coding")
        )
    );
    assert!(first["cold_tokens"].as_u64().is_some(), "{first}");
    assert!(first["cache_usd"].as_f64().is_some(), "{first}");
    let after = session(&t.rig.core, &t.rec.session_id);
    assert_eq!(after.last_target.unwrap().profile, "sonnet");
    let routed = after.routed.unwrap();
    assert_eq!(
        (routed.profile.as_deref(), routed.from.as_deref()),
        (Some("sonnet"), Some("haikuhi")),
        "the base is kept for the way back down"
    );
}

/// A task that recovers steps back to its base only after two agreeing
/// verdicts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_steps_back_to_its_base_after_two_agreeing_verdicts() {
    let t = task_with(
        loops(16, 0),
        None,
        |_| {},
        |j| {
            on_trigger(j, "failing: 3 failed", "deep_coding");
            on_trigger(j, "failing: 7 failed", "routine_coding");
            on_trigger(j, "failing: 11 failed", "routine_coding");
        },
    )
    .await;
    let rows = t.checkpoints();
    let seen: Vec<_> = rows
        .iter()
        .map(|d| {
            (
                d["reason"].as_str().unwrap().to_string(),
                d["switch"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        &seen[..3],
        [
            ("verdict".to_string(), true),
            ("cache_hold".to_string(), false),
            ("step_down".to_string(), true)
        ],
        "{rows:?}"
    );
    let models = t.models();
    let up = models.iter().position(|m| *m == "sonnet").expect("up");
    let down = models.iter().rposition(|m| *m == "sonnet").expect("up") + 1;
    assert!(models[down..].iter().all(|m| *m == "haiku"), "{models:?}");
    assert!(
        down - up >= 4,
        "up for the loops between the verdicts: {models:?}"
    );
}

/// One agreeing verdict, then one that disagrees, leaves the task up.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_agreeing_verdict_then_a_disagreeing_one_leaves_it_up() {
    let t = task_with(
        loops(12, 0),
        None,
        |_| {},
        |j| {
            on_trigger(j, "failing: 3 failed", "deep_coding");
            on_trigger(j, "failing: 7 failed", "routine_coding");
            on_trigger(j, "failing: 11 failed", "deep_coding");
        },
    )
    .await;
    let reasons: Vec<_> = t
        .checkpoints()
        .iter()
        .map(|d| d["reason"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(reasons[..3], ["verdict", "cache_hold", "verdict"]);
    let models = t.models();
    assert_eq!(models.last(), Some(&"sonnet"), "{models:?}");
    assert!(!models[models.iter().position(|m| *m == "sonnet").unwrap()..].contains(&"haiku"));
}

/// Never below the base: a trivial verdict on a task whose base is Sonnet
/// leaves it on Sonnet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cheap_verdict_never_takes_a_task_below_its_base() {
    let t = task_with(
        loops(8, 0),
        None,
        |c| {
            c.routing.classes.routine_coding.profiles = vec!["sonnet".into()];
        },
        |j| {
            on_trigger(j, "failing: ", "trivial");
        },
    )
    .await;
    assert_eq!(t.models(), ["sonnet"; 9]);
    let rows = t.checkpoints();
    assert!(!rows.is_empty());
    assert!(
        rows.iter()
            .all(|d| d["profile"] == "sonnet" && d["switch"] == false),
        "{rows:?}"
    );
}

/// At an estimate over `cold_switch_tokens` the first agreeing verdict holds
/// and the second switches.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cold_switch_waits_for_a_second_agreeing_verdict() {
    let t = task_with(
        loops(12, 0),
        None,
        |c| c.routing.cold_switch_tokens = 1,
        |j| {
            on_trigger(j, "failing: ", "deep_coding");
        },
    )
    .await;
    let reasons: Vec<_> = t
        .checkpoints()
        .iter()
        .map(|d| d["reason"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(reasons[..2], ["cache_hold", "verdict"], "{reasons:?}");
    let models = t.models();
    let at = moved_at(&models, "sonnet");
    assert!(
        at >= 7,
        "the second verdict (asked at the seventh loop's end) moves it: {models:?}"
    );
}

/// After `max_switches` moves up a further elevating verdict moves nothing,
/// and a step back down still works.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cap_stops_moves_up_and_not_the_way_down() {
    let t = task_with(
        loops(16, 0),
        None,
        |c| c.routing.task_checkpoints.max_switches = 1,
        |j| {
            on_trigger(j, "failing: 3 failed", "deep_coding");
            on_trigger(j, "failing: 7 failed", "sophisticated");
            on_trigger(j, "failing: 11 failed", "routine_coding");
            on_trigger(j, "failing: 15 failed", "routine_coding");
        },
    )
    .await;
    let rows = t.checkpoints();
    let reasons: Vec<_> = rows.iter().map(|d| d["reason"].as_str().unwrap()).collect();
    assert_eq!(
        reasons,
        ["verdict", "switch_cap", "cache_hold", "step_down"],
        "{rows:?}"
    );
    assert!(!t.models().contains(&"opus"), "{:?}", t.models());
    let models = t.models();
    assert_eq!(
        models.last(),
        Some(&"haiku"),
        "back at its base: {models:?}"
    );
}

/// At most one verdict per `min_loops_between` loops: a task failing for 12
/// loops asks at most three times.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_failing_for_twelve_loops_asks_three_times() {
    let t = task_with(
        loops(12, 0),
        None,
        |_| {},
        |j| {
            on_trigger(j, "failing: ", "chat");
        },
    )
    .await;
    let asked = t.jev.connections() - 1;
    assert_eq!(asked, 3, "the brief, then one per four loops");
    assert_eq!(t.checkpoints().len(), 3);
    assert_eq!(t.models(), ["haiku"; 13], "chat keeps the task's own");
}

/// Jev's cost is the task's: its `spent` holds what the checkpoints settled
/// at, and the row says how much.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_checkpoints_cost_lands_in_the_tasks_spent() {
    let with = task_with(
        loops(8, 0),
        None,
        |_| {},
        |j| on_trigger(j, "failing: ", "chat"),
    )
    .await;
    let off = task_with(
        loops(8, 0),
        None,
        |c| c.routing.task_checkpoints.enabled = false,
        |_| {},
    )
    .await;
    let cost: f64 = with
        .checkpoints()
        .iter()
        .map(|d| d["cost_usd"].as_f64().unwrap())
        .sum();
    assert!(cost > 0.0, "the fake bills its usage");
    let extra = with.spent() as i64 - off.spent() as i64;
    let micros = (cost * 1e6).round() as i64;
    assert!(
        (extra - micros).abs() <= 2,
        "the task's spent grew by the checkpoints' cost: {extra} against {micros}"
    );
}

/// A conversation's turn is untouched: the requests and the frames are the
/// same with checkpoints on and off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conversation_turn_is_the_same_with_checkpoints_on() {
    async fn go(on: bool) -> (Vec<String>, u64) {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "chat", 0.95);
        let claude = Arc::new(FakeProvider::scripted(loops(5, 0)));
        let provider = claude.clone();
        let rig = rig_parts(
            Some(&jev),
            1,
            move |c| c.routing.task_checkpoints.enabled = on,
            move |p| {
                p.providers.insert("anthropic".into(), provider);
            },
        );
        crate::tests_route::turn(&rig.core, None, "Please look into it.", None).await;
        let bodies = claude
            .requests()
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect();
        (bodies, rig.core.store.last_position())
    }
    let (on, off) = (go(true).await, go(false).await);
    assert_eq!(on.0.len(), 6);
    assert_eq!(on.0, off.0, "byte-identical requests");
    assert_eq!(on.1, off.1, "the same frames");
}

/// The text a checkpoint sends is bounded whatever the task wrote.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_checkpoints_text_is_bounded_for_a_task_of_long_loops() {
    let long = "word ".repeat(300);
    let script: Vec<Scripted> = (0..12)
        .map(|n| {
            Scripted::tools(
                &format!("{long} {n}"),
                &[(&format!("f{n}"), "text_diff", json!({}))],
            )
        })
        .chain([Scripted::text("Done.")])
        .collect();
    let t = task_with(script, None, |_| {}, |j| on_trigger(j, "failing: ", "chat")).await;
    let body = t.jev.seen().last().unwrap().body.to_string();
    let at = body.find("Task: ").expect("the checkpoint's text");
    let end = body[at..]
        .find("in a row")
        .expect("the trigger line is kept")
        + at;
    let text = &body[at..end + "in a row".len()];
    let escapes = text.matches("\\n").count();
    assert!(
        (text.chars().count() - escapes)
            <= crate::routing::checkpoint::MAX_TOKENS * crate::routing::checkpoint::CHARS_PER_TOKEN,
        "{} characters",
        text.chars().count()
    );
}

/// The work board and its progress line say where a moved task runs, only for a task a
/// checkpoint moved off its base; the `loop.ended` rows carry it, and a task on its base
/// has none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_board_says_a_moved_task_is_on_its_new_model() {
    let moved = task_with(
        loops(8, 0),
        None,
        |_| {},
        |j| {
            on_trigger(j, "failing: 3 failed", "deep_coding");
        },
    )
    .await;
    let kept = task_with(loops(8, 0), None, |_| {}, |_| {}).await;
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
