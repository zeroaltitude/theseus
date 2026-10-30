//! A running task's checkpoints (theseus-ypnxt.9): a task asks route.v8 about
//! its recent state at a loop boundary, and its model moves up, or back down
//! to its base, by routing's own rules. The rig is `tests_task_brief`'s (the
//! driver runs the tasks, a fake Jev answers the brief's class and each
//! checkpoint's, by the trigger line the checkpoint's text ends with), with a
//! provider that answers a script of loops, a little slowly, so a verdict
//! lands at a loop boundary.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_kernel::ExecState;

use crate::provider::{FakeProvider, RequestGate, Scripted};
use crate::session::{SessionRecord, TargetRef};
use crate::test_support::until_event;
use crate::tests_judge::kinds;
use crate::tests_route::{decided, mode, rig_parts, session, Rig};
use crate::tests_task_brief::{cli, params, BRIEF};
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

/// The same, for a pack that asks `model` (the acting pack once the unified
/// switch is open, `JudgeService::route_open`): the verdict is Jev's pick of an
/// option, not a class.
fn on_trigger_pick(jev: &FakeJev, trigger: &str, option: &str) {
    jev.script_when(
        trigger,
        None,
        "model",
        Jev::Choice {
            option: option.into(),
            confidence: 0.95,
        },
    );
}

/// Jev's pick of `option` for every ask that has no trigger of its own.
fn pick_baseline(jev: &FakeJev, option: &str) {
    jev.script(
        "model",
        Jev::Choice {
            option: option.into(),
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

/// [`task_with`] with the acting route pack asking `model` (theseus-ej6bj): Jev
/// decides each checkpoint, with no cap, base or step down.
async fn task_with_open(
    script: Vec<Scripted>,
    profile: Option<&str>,
    tweak: impl FnOnce(&mut crate::Config),
    verdicts: impl FnOnce(&FakeJev),
) -> Task {
    task_gated(true, script, profile, tweak, verdicts).await
}

/// A task from a Fable conversation whose brief is judged routine coding (the
/// rig's Haiku), running `script`, on a provider that takes 60 ms a loop.
async fn task_with(
    script: Vec<Scripted>,
    profile: Option<&str>,
    tweak: impl FnOnce(&mut crate::Config),
    verdicts: impl FnOnce(&FakeJev),
) -> Task {
    task_gated(false, script, profile, tweak, verdicts).await
}

async fn task_gated(
    open: bool,
    script: Vec<Scripted>,
    profile: Option<&str>,
    tweak: impl FnOnce(&mut crate::Config),
    verdicts: impl FnOnce(&FakeJev),
) -> Task {
    let jev = FakeJev::start().unwrap();
    // Open, the pack asks `model` and a verdict is a pick: the baseline is the
    // cheap one a routine brief starts on, as the class's was.
    match open {
        true => pick_baseline(&jev, "haiku"),
        false => mode(&jev, "routine_coding", 0.95),
    }
    verdicts(&jev);
    let claude = Arc::new(FakeProvider {
        delay_ms: 60,
        ..FakeProvider::scripted(script)
    });
    // A pick runs only where the provider's key is named (a placeholder is no
    // key, `Config::provider_without_key`): the open rule's picks must be able to run.
    let rig = task_rig(&jev, claude.clone(), |c| {
        if open {
            c.name_every_provider_key();
        }
        tweak(c);
    })
    .await;
    if open {
        rig.core
            .runner
            .judge
            .job_pack_with(crate::tests_job_ask::model_pack());
    }
    task_run(rig, jev, claude, profile).await
}

/// Everything `task_with` does before the task starts: the rig on `jev` and
/// `claude`, its driver running, and the home conversation on Fable.
async fn task_rig(
    jev: &FakeJev,
    claude: Arc<FakeProvider>,
    tweak: impl FnOnce(&mut crate::Config),
) -> Rig {
    let provider = claude.clone();
    let rig = rig_parts(
        Some(jev),
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
    rig
}

/// The task, started on `rig` and run to the end of its first turn.
async fn task_run(
    rig: Rig,
    jev: FakeJev,
    claude: Arc<FakeProvider>,
    profile: Option<&str>,
) -> Task {
    let started = rig.core.task_start(&params(BRIEF, profile), cli()).unwrap();
    let id = started.session_id.replacen("ses_", "exe_", 1);
    // The task's first turn ends: the event the checks read, however long a starved host takes
    // (`tests_task_brief::run` waits the same way).
    until_event("the task's first turn", || {
        exec(&rig.core, &id).state == ExecState::Complete
    })
    .await;
    let rec = session(&rig.core, &started.session_id);
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

/// Never below the base: a trivial verdict on a task whose base is Sonnet (a
/// deep-coding brief) leaves it on Sonnet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cheap_verdict_never_takes_a_task_below_its_base() {
    let t = task_with(
        loops(8, 0),
        None,
        |_| {},
        |j| {
            mode(j, "deep_coding", 0.95);
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

/// Jev decides at each trigger (theseus-ej6bj, the owner: "I categorically
/// disagree with caps"): a task that recovers follows a cheaper verdict down at
/// once, as a dearer one took it up. There is no base to step back to, and no
/// second agreeing verdict to wait for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_task_follows_a_cheaper_verdict_down_at_once() {
    let t = task_with_open(
        loops(12, 0),
        None,
        |_| {},
        |j| {
            on_trigger_pick(j, "failing: 3 failed", "sonnet");
            on_trigger_pick(j, "failing: 7 failed", "haiku");
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
        &seen[..2],
        [("verdict".to_string(), true), ("verdict".to_string(), true)],
        "{rows:?}"
    );
    let models = t.models();
    let up = models.iter().position(|m| *m == "sonnet").expect("up");
    let down = models.iter().rposition(|m| *m == "sonnet").expect("up") + 1;
    assert!(models[down..].iter().all(|m| *m == "haiku"), "{models:?}");
    assert!(down - up >= 3, "up for the loops between: {models:?}");
}

/// Nothing is a floor: a trivial verdict takes a task whose brief picked
/// Sonnet below it, to the profile the verdict names.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_cheap_verdict_takes_a_task_below_where_it_started() {
    let t = task_with_open(
        loops(8, 0),
        None,
        |_| {},
        |j| {
            pick_baseline(j, "sonnet");
            on_trigger_pick(j, "failing: ", "haiku");
        },
    )
    .await;
    let models = t.models();
    assert_eq!(models[0], "sonnet", "the brief's pick: {models:?}");
    assert_eq!(models.last(), Some(&"haiku"), "{models:?}");
    let rows = t.checkpoints();
    assert_eq!(
        (rows[0]["profile"].as_str(), rows[0]["switch"].as_bool()),
        (Some("haiku"), Some(true)),
        "{rows:?}"
    );
}

/// There is no cap on moves: `max_switches` is still a key the loader reads
/// (row 6 retires it), and the checkpoint no longer looks at it. Four verdicts
/// that each name another profile make four moves, up and down.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_task_has_no_cap_on_moves() {
    let t = task_with_open(
        loops(16, 0),
        None,
        |c| c.routing.task_checkpoints.max_switches = 1,
        |j| {
            on_trigger_pick(j, "failing: 3 failed", "sonnet");
            on_trigger_pick(j, "failing: 7 failed", "opus");
            on_trigger_pick(j, "failing: 11 failed", "haiku");
            on_trigger_pick(j, "failing: 15 failed", "sonnet");
        },
    )
    .await;
    let rows = t.checkpoints();
    let reasons: Vec<_> = rows.iter().map(|d| d["reason"].as_str().unwrap()).collect();
    assert_eq!(reasons, ["verdict"; 4], "{rows:?}");
    let models = t.models();
    assert!(models.contains(&"opus"), "{models:?}");
    assert!(models.contains(&"haiku"), "{models:?}");
    assert_eq!(models.last(), Some(&"sonnet"), "{models:?}");
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

/// A conversation's turn the route placed has checkpoints too (theseus-a1wvq;
/// converted: before, its requests and frames were the same with checkpoints
/// on and off). Its loops fail, so it asks one, recorded as the turn's; the
/// verdict, `chat` again, keeps it where it is, so the requests to the model
/// are byte-identical, and only the checkpoint's own rows are added.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conversation_turn_asks_a_checkpoint_and_stays_where_its_verdict_keeps_it() {
    async fn go(on: bool) -> (Vec<String>, usize) {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "chat", 0.95);
        // A little slowly, so the verdict lands at a loop boundary.
        let claude = Arc::new(FakeProvider {
            delay_ms: 60,
            ..FakeProvider::scripted(loops(5, 0))
        });
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
        let asked = decided(&rig.core.store)
            .into_iter()
            .filter(|d| d["source"] == "turn checkpoint")
            .count();
        (bodies, asked)
    }
    let (on, off) = (go(true).await, go(false).await);
    assert_eq!(on.0.len(), 6);
    assert_eq!(on.0, off.0, "byte-identical requests");
    assert_eq!((on.1, off.1), (1, 0), "one checkpoint, the turn's");
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

/// A verdict that takes its time never holds a loop: the task runs on at its base while the
/// checkpoint is out, and moves at the first boundary after the answer comes. Ordered by
/// events, never by a clock (theseus-1b2zm): Jev holds the checkpoint's answer, the
/// provider holds the seventh request, and the test lets the verdict go and waits until it
/// has come to the turn before it lets the loop go on. So the loops after the ask (three
/// of them, 3 to 5, and the seventh's own call) ran on the base with the verdict out, and
/// the move is at the first boundary after it came: the eighth request, never another.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes() {
    const ASKED: &str = "failing: 3 failed";
    let gate = RequestGate::at(6);
    let claude = Arc::new(FakeProvider {
        gate: Some(gate.clone()),
        ..FakeProvider::scripted(loops(20, 0))
    });
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    on_trigger(&jev, ASKED, "deep_coding");
    jev.hold_when(ASKED);
    let rig = task_rig(&jev, claude.clone(), |c| {
        c.routing.task_checkpoints.min_loops_between = 12;
        // The held verdict is the test's to release: the judge's own deadline must not end it
        // first when a starved host runs the loops slowly (theseus-iubei).
        c.judge.total_secs = 3_600;
    })
    .await;
    let core = rig.core.clone();
    let seen = jev.clone();
    let (t, ()) = tokio::join!(task_run(rig, jev.clone(), claude, None), async move {
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

/// Not a benchmark: three scripted tasks on a Haiku base, the model each loop ran on, the
/// checkpoints asked and what Jev cost per task. One fails three loops then passes, one
/// stalls (words nobody changes), one is clean.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cost_note_on_three_scripted_tasks() {
    let stall: Vec<Scripted> = (0..10)
        .map(|n| {
            Scripted::tools(
                "",
                &[(
                    &format!("s{n}"),
                    "text_diff",
                    json!({"a": "x\n", "b": "y\n"}),
                )],
            )
        })
        .chain([Scripted::text("Done: the report.")])
        .collect();
    let mut recover = loops(3, 6);
    recover.truncate(9);
    recover.push(Scripted::text("Done: the report."));
    let cases = [
        ("fails three loops, then passes", recover),
        ("stalls for ten loops", stall),
        ("clean", loops(0, 8)),
    ];
    let mut up = Vec::new();
    for (name, script) in cases {
        let t = task_with(
            script,
            None,
            |_| {},
            |j| {
                on_trigger(j, "failing: 3 failed", "deep_coding");
                on_trigger(j, "no progress for 8 loops", "sophisticated");
            },
        )
        .await;
        let rows = t.checkpoints();
        let jev: f64 = rows.iter().map(|d| d["cost_usd"].as_f64().unwrap()).sum();
        let kinds: Vec<_> = rows
            .iter()
            .map(|d| d["trigger"].as_str().unwrap())
            .collect();
        eprintln!(
            "COSTNOTE {name}: models {:?}; checkpoints {kinds:?}; jev ${jev:.6}; task spent ${:.6}",
            t.models(),
            t.spent() as f64 / 1e6
        );
        up.push((t.models(), rows.len()));
    }
    assert!(up[0].0.contains(&"sonnet") && up[0].1 >= 1);
    assert!(up[1].0.contains(&"opus") && up[1].1 >= 1);
    assert_eq!((up[2].1, up[2].0.iter().all(|m| *m == "haiku")), (0, true));
}
