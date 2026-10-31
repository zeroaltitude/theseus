//! A running task's checkpoints (theseus-ypnxt.9): a task asks route.v8 about
//! its recent state at a loop boundary, and its model moves up, or back down
//! to its base, by routing's own rules. The rig is `tests_task_brief`'s (the
//! driver runs the tasks, a fake Jev answers the brief's class and each
//! checkpoint's, by the trigger line the checkpoint's text ends with), with a
//! provider that answers a script of loops, a little slowly, so a verdict
//! lands at a loop boundary.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_kernel::ExecState;

use crate::provider::{FakeProvider, Scripted};
use crate::session::{SessionRecord, TargetRef};
use crate::test_support::until_event;
use crate::tests_route::{decided, mode, rig_parts, Rig};
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

/// The checkpoint's verdict when its text's last line holds `trigger`, in the
/// words of the class the old route pack asked: Jev's pick of the model that
/// class went to (route.v10 asks `model`, theseus-h66v4, row 6).
fn on_trigger(jev: &FakeJev, trigger: &str, class: &str) {
    let option = match class {
        "trivial" | "quick" => "haiku",
        "sophisticated" | "review" | "deep_coding" => "opus",
        _ => "sonnet",
    };
    on_trigger_pick(jev, trigger, option);
}

/// The checkpoint's verdict when its text's last line holds `trigger`: Jev's
/// pick of an option.
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
}

/// [`task_with`] with the acting route pack asking `model` (theseus-ej6bj): Jev
/// decides each checkpoint, with no cap, base or step down.
async fn task_with_open(
    script: Vec<Scripted>,
    profile: Option<&str>,
    tweak: impl FnOnce(&mut crate::Config),
    verdicts: impl FnOnce(&FakeJev),
) -> Task {
    task_gated(script, profile, tweak, verdicts).await
}

/// A task from a Fable conversation whose brief is judged routine coding (the
/// rig's Haiku), running `script`, on a provider that takes 60 ms a loop.
async fn task_with(
    script: Vec<Scripted>,
    profile: Option<&str>,
    tweak: impl FnOnce(&mut crate::Config),
    verdicts: impl FnOnce(&FakeJev),
) -> Task {
    task_gated(script, profile, tweak, verdicts).await
}

async fn task_gated(
    script: Vec<Scripted>,
    profile: Option<&str>,
    tweak: impl FnOnce(&mut crate::Config),
    verdicts: impl FnOnce(&FakeJev),
) -> Task {
    let jev = FakeJev::start().unwrap();
    // The pack asks `model` and a verdict is a pick: the baseline is the cheap
    // one a routine brief starts on, as the class's was.
    pick_baseline(&jev, "haiku");
    verdicts(&jev);
    let claude = Arc::new(FakeProvider {
        delay_ms: 60,
        ..FakeProvider::scripted(script)
    });
    // A pick runs only where the provider's key is named (a placeholder is no
    // key, `Config::provider_without_key`): the open rule's picks must be able to run.
    let rig = task_rig(&jev, claude.clone(), |c| {
        c.name_every_provider_key();
        tweak(c);
    })
    .await;
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
    Task { rig, claude, jev }
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
}

// A. What a task did before checkpoints, pinned.

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

/// There is no cap on moves: `max_switches` is a retired key the loader reads
/// and ignores. Four verdicts that each name another profile make four moves,
/// up and down.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_task_has_no_cap_on_moves() {
    let t = task_with_open(
        loops(16, 0),
        None,
        |c| c.routing.task_checkpoints.max_switches = Some(1),
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
