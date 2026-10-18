//! A running task's model across its loops (theseus-ypnxt.9). Pinned first, as
//! main had it: a task's model never moved after its brief. The rig is `tests_task_brief`'s (the
//! driver runs the tasks, a fake Jev answers the brief's class and each
//! checkpoint's, by the trigger line the checkpoint's text ends with), with a
//! provider that answers a script of loops, a little slowly, so a verdict
//! lands at a loop boundary.

use std::sync::Arc;

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::provider::{FakeProvider, Scripted};
use crate::session::{SessionRecord, TargetRef};
use crate::tests_route::{mode, rig_parts, Rig};
use crate::tests_task_brief::{params, run, BRIEF};

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

#[allow(dead_code)]
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
}

#[allow(dead_code)]
/// The first loop on `to` and every later loop on it.
fn moved_at(models: &[&str], to: &str) -> usize {
    let at = models.iter().position(|m| *m == to).expect("it moved");
    assert!(models[at..].iter().all(|m| *m == to), "{models:?}");
    at
}

// A. What a task did before checkpoints, pinned.

/// A task on a cheap base whose loops fail many times in a row runs every loop
/// on its base, and the route pack is asked once, for the brief.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_task_stays_on_its_base_through_every_loop() {
    let t = task_with(loops(8, 0), None, |_| {}, |_| {}).await;
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
}
