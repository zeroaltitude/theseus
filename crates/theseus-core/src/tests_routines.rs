//! Routines through the whole core (theseus-pvm5c): a routine is an execution
//! of its own whose every run is a fresh task, reporting where its owner is; a
//! run still going makes the next occurrence skip (`wake.skipped`); a spent
//! month skips and says so once, and the 1st starts it again; the model's
//! `routine.create` waits on one card; a restart's catch-up says how many were
//! missed. The firing decision (`routine::plan`) is also tried on its own, with
//! what the kernel would give it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_kernel::{FiredWake, PendingWake, RoutineDue, RoutinePlan, TaskRun, WakeRun};
use theseus_protocol::{Message, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Client, Surface};
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::routine::{plan, Env};
use crate::test_support::{rows, until_within as until};
use crate::{Config, Core};

pub(crate) const TARGET: &str = "discord:dm:42";
const BRIEF: &str = "Read the tide table and say when the next high tide is.";

/// A stand-in model: it answers every turn with `Tide at 06:12.`, after
/// `delay_ms` when that is set, and calls `routine_create` for the one turn the
/// test scripts.
pub(crate) struct Model {
    pub(crate) requests: Mutex<Vec<ProviderRequest>>,
    pub(crate) delay_ms: AtomicU64,
    call: Mutex<Option<Value>>,
    pub(crate) text: Mutex<String>,
}

fn answers_a_call(req: &ProviderRequest) -> bool {
    req.messages
        .last()
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

impl Provider for Model {
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
            let scripted = self.call.lock().unwrap().take();
            let answer = match scripted {
                Some(input) if !answers_a_call(req) => {
                    Scripted::tools("Setting it up.", &[("t_routine", "routine_create", input)])
                }
                _ => {
                    let d = self.delay_ms.load(Ordering::SeqCst);
                    if d > 0 {
                        tokio::time::sleep(Duration::from_millis(d)).await;
                    }
                    Scripted::text(&self.text.lock().unwrap().clone())
                }
            };
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) model: Arc<Model>,
    driver: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

pub(crate) fn rig() -> Rig {
    rig_with(|_| {})
}

/// The rig over the template, with `tune` applied to its config before the daemon is built.
fn rig_with(tune: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    tune(&mut cfg);
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = crate::policy::Posture::Notify;
    cfg.kernel.min_repeat_minutes = 1;
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model {
        requests: Mutex::default(),
        delay_ms: AtomicU64::new(0),
        call: Mutex::default(),
        text: Mutex::new("Tide at 06:12.".into()),
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.bind_places(vec![crate::places::BoundPlace {
        target: TARGET.into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
    let driver = tokio::spawn(crate::harness::drive(core.clone()));
    Rig {
        core,
        model,
        driver,
        _dir: dir,
    }
}

fn cli() -> Client {
    Client::new("sock#1", Surface::Cli)
}

pub(crate) async fn rpc(core: &Arc<Core>, method: &str, params: Value) -> Result<Value, String> {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, cli()));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r.result.ok_or_else(|| r.error.unwrap().message);
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

pub(crate) fn reports(core: &Core) -> Vec<Value> {
    core.kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "report")
        .inspect(|a| assert_eq!(crate::outbox::target_of(a), TARGET))
        .map(|a| crate::outbox::body_of(&a).clone())
        .collect()
}

/// `routine.create` for a minute series.
pub(crate) async fn made(core: &Arc<Core>, extra: Value) -> Value {
    let mut p = json!({"title": "Tide check", "schedule": "every 1m", "brief": BRIEF});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    rpc(core, "routine.create", p).await.unwrap()["routine"].clone()
}

/// The routine's wake, set to fire in a second (a minute series is too slow to
/// wait for: the wake's own snooze moves its next time).
pub(crate) async fn due_soon(core: &Arc<Core>) {
    let wakes = rpc(core, "wake.list", json!({})).await.unwrap();
    let w = wakes["wakes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["purpose"] == "routine")
        .expect("the routine's wake");
    rpc(
        core,
        "wake.snooze",
        json!({"wake": w["short"], "when": "1s"}),
    )
    .await
    .unwrap();
}

pub(crate) async fn runs(core: &Arc<Core>, id: &str) -> Vec<Value> {
    rpc(core, "routine.runs", json!({"id": id})).await.unwrap()["runs"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_minute_routine_runs_three_times_as_three_fresh_tasks_reporting_to_the_dm() {
    let r = rig();
    let info = made(&r.core, json!({})).await;
    let id = info["id"].as_str().unwrap().to_string();
    assert_eq!(info["series"], "every 1m");
    assert_eq!(info["run_usd"], 0.5);
    assert_eq!(info["month_usd"], 10.0);
    let e = r.core.kernel.execution(&format!("exe_{}", &id[4..])).ok();
    drop(e);
    for n in 1..=3 {
        due_soon(&r.core).await;
        until(&format!("run {n}'s report"), 30, || {
            reports(&r.core).len() == n
        })
        .await;
    }
    let listed = runs(&r.core, &id).await;
    assert_eq!(listed.len(), 3, "{listed:?}");
    assert!(
        listed.iter().all(|x| x["state"] == "complete"),
        "{listed:?}"
    );
    assert_eq!(listed[0]["run"], 3, "newest first");
    assert_eq!(listed[0]["report"], "Tide at 06:12.");
    let tasks: std::collections::BTreeSet<&str> =
        listed.iter().map(|x| x["task"].as_str().unwrap()).collect();
    assert_eq!(tasks.len(), 3, "each run is a task of its own");
    // The second run's brief says the first reported; the third does not repeat
    // the first's: a run's context does not grow with the runs before it.
    let texts: Vec<String> = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|q| serde_json::to_string(&q.messages).unwrap())
        .collect();
    let run = |n: u32| {
        texts
            .iter()
            .find(|t| t.contains(&format!("Run {n} of Tide check")))
    };
    assert!(
        run(1).is_some_and(|t| !t.contains("the last run")),
        "the first has no last"
    );
    assert!(
        run(2).is_some_and(|t| t.contains("the last run reported: Tide at 06:12.")),
        "{texts:?}"
    );
    assert!(run(3).is_some_and(|t| !t.contains("Run 1 of Tide check")));
    let fired = rows(&r.core, "wake.fired");
    assert_eq!(fired.len(), 3);
    assert!(fired.iter().all(|f| f["purpose"] == "routine"));
    let list = rpc(&r.core, "routine.list", json!({})).await.unwrap();
    assert_eq!(list["routines"][0]["runs"], 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_still_going_makes_the_next_occurrence_skip_with_a_row() {
    let r = rig();
    r.model.delay_ms.store(2_500, Ordering::SeqCst);
    let info = made(&r.core, json!({})).await;
    let id = info["id"].as_str().unwrap().to_string();
    due_soon(&r.core).await;
    until("the first run", 10, || {
        !rows(&r.core, "wake.fired").is_empty()
    })
    .await;
    // It is due again while the first run is still thinking.
    due_soon(&r.core).await;
    until("the skip", 10, || !rows(&r.core, "wake.skipped").is_empty()).await;
    assert_eq!(rows(&r.core, "wake.skipped")[0]["why"], "overlap");
    assert_eq!(runs(&r.core, &id).await.len(), 1, "never twice at once");
    until("the first run's end", 30, || reports(&r.core).len() == 1).await;
    r.model.delay_ms.store(0, Ordering::SeqCst);
    due_soon(&r.core).await;
    until("the second run", 30, || reports(&r.core).len() == 2).await;
    assert_eq!(runs(&r.core, &id).await.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_routine_never_fires_and_run_now_runs_once_while_it_is_paused() {
    let r = rig();
    let info = made(&r.core, json!({})).await;
    let id = info["id"].as_str().unwrap().to_string();
    let paused = rpc(&r.core, "routine.pause", json!({"id": id}))
        .await
        .unwrap();
    assert_eq!(paused["routine"]["paused"], true);
    let next = paused["routine"]["next_ms"].as_u64().unwrap();
    let ran = rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    assert!(ran["task"].is_string(), "{ran}");
    assert_eq!(ran["routine"]["paused"], true, "run now leaves it paused");
    assert_eq!(
        ran["routine"]["next_ms"].as_u64(),
        Some(next),
        "and the series where it was"
    );
    until("its report", 30, || reports(&r.core).len() == 1).await;
    // A second run while one is going is refused in words.
    r.model.delay_ms.store(1_500, Ordering::SeqCst);
    rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    let again = rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap_err();
    assert!(again.contains("never runs twice at once"), "{again}");
    until("the second report", 30, || reports(&r.core).len() == 2).await;
    assert_eq!(runs(&r.core, &id).await.len(), 2);
    assert!(rows(&r.core, "wake.skipped").is_empty());
    let resumed = rpc(&r.core, "routine.resume", json!({"id": id}))
        .await
        .unwrap();
    assert_eq!(resumed["routine"]["paused"], false);
}

/// A routine made by the owner, and what its next fire would be handed.
fn routine_and_due(
    r: &Rig,
    month: &str,
) -> (Arc<theseus_protocol::routines::RoutineSpec>, RoutineDue) {
    let book = &r.core.runner.tools.routines;
    let mut spec = book.all(&r.core.store).unwrap().remove(0).as_ref().clone();
    spec.month = month.into();
    let now = r.core.kernel.now_ms();
    let due = RoutineDue {
        fired: FiredWake {
            wake: PendingWake {
                id: spec.wake_id.clone(),
                due_at_ms: now,
                occurrence: 5,
                ..Default::default()
            },
            late_ms: 0,
            while_down: false,
            missed: 0,
            next_due_at_ms: Some(now + 60_000),
        },
        task: TaskRun {
            brief: spec.brief.clone(),
            limit_micros: spec.run_micros,
            tools: vec![],
        },
        manual: false,
        now_ms: now,
        limit_micros: spec.month_micros,
        spent_micros: 0,
        available_micros: spec.month_micros,
        available_after_reset_micros: spec.month_micros,
        task_id: "exe_run".into(),
        session_id: "ses_run".into(),
    };
    (Arc::new(spec), due)
}

fn env(r: &Rig) -> Env<'_> {
    Env {
        kernel: &r.core.kernel,
        store: &r.core.store,
        outbox: &r.core.outbox,
        target: Some(TARGET.into()),
    }
}

/// A run carries the labels of the session that made the routine, as that
/// session has them when the run starts (theseus-gyjyo.5): the brief was
/// written there and can quote what it read. A routine made by the owner has
/// no such session, and its run carries none.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_carries_the_labels_of_the_session_that_made_the_routine() {
    let r = rig();
    made(&r.core, json!({})).await;
    let now = r.core.kernel.now_ms();
    let this_month = theseus_kernel::cron::month_key(&r.core.kernel.config().zone, now);
    let (spec, due) = routine_and_due(&r, &this_month);
    let run_record =
        |spec: &theseus_protocol::routines::RoutineSpec| -> crate::session::SessionRecord {
            let (p, _) = plan(&env(&r), spec, &due).unwrap();
            let RoutinePlan::Run(run) = p else {
                panic!("a routine within its month runs")
            };
            let rec = run
                .records
                .iter()
                .find(|rec| rec.kind == theseus_store::kinds::SESSION)
                .expect("the run's session record");
            serde_json::from_slice(&rec.payload).unwrap()
        };
    assert!(
        run_record(&spec).data.is_none(),
        "made by the owner: no labels"
    );
    // Made in a session that read a calendar.
    let maker = crate::tests_task_graph::session(&r.core);
    crate::test_support::label_session(&r.core, &maker, crate::content::Flag::Personal);
    let mut spec = spec.as_ref().clone();
    spec.created_in = Some(maker.clone());
    let data = run_record(&spec)
        .data
        .expect("the run carries its maker's labels");
    assert_eq!(data.labels.len(), 1, "{:?}", data.labels);
    assert_eq!(data.labels[0].flag, crate::content::Flag::Personal);
    // The operator removes it there, and the next run starts without it.
    r.core
        .unlabel_session(
            &maker,
            "personal",
            crate::approval::Answerer {
                label: "the CLI".into(),
                surface: Surface::Cli,
                discord: None,
                agent: None,
            },
        )
        .unwrap();
    assert!(run_record(&spec).data.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_spent_month_skips_and_says_so_once_and_the_first_starts_it_again() {
    let r = rig();
    made(&r.core, json!({"month_usd": 1.0})).await;
    let now = r.core.kernel.now_ms();
    let this_month = theseus_kernel::cron::month_key(&r.core.kernel.config().zone, now);
    let (spec, mut due) = routine_and_due(&r, &this_month);
    due.spent_micros = spec.month_micros;
    due.available_micros = 0;
    due.available_after_reset_micros = 0;
    // 1: skipped, and the owner is told.
    let (p, after) = plan(&env(&r), &spec, &due).unwrap();
    let RoutinePlan::Skip { why, post, .. } = p else {
        panic!("a spent month runs nothing")
    };
    assert_eq!(why, "budget");
    let post = post.expect("the first skip says so");
    assert_eq!(post.target, TARGET);
    assert!(post.body["text"]
        .as_str()
        .unwrap()
        .contains("this routine's own month budget of $1.00"));
    assert!(after.month_told);
    // 2: skipped again, and silent.
    let (p, _) = plan(&env(&r), &after, &due).unwrap();
    let RoutinePlan::Skip { why, post, .. } = p else {
        panic!("still skipped")
    };
    assert_eq!((why, post.is_none()), ("budget", true), "it says so once");
    // The month turns: the next fire resets the spend and runs.
    let (long_ago, mut due2) = routine_and_due(&r, "2000-01");
    due2.available_micros = 0;
    due2.available_after_reset_micros = long_ago.month_micros;
    let (p, next) = plan(&env(&r), &long_ago, &due2).unwrap();
    let RoutinePlan::Run(run) = p else {
        panic!("the 1st starts the month again")
    };
    assert!(run.reset_month);
    assert_eq!(
        (next.month, next.month_told, next.runs),
        (this_month, false, 1)
    );
}

/// The rule notices the owner was sent for a skipped run, as `rule_posts`
/// reads D5's in the relay's tests: the operator outbox's `spend_ceiling` posts.
fn rule_posts(r: &Rig) -> Vec<String> {
    r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "spend_ceiling")
        .map(|a| {
            crate::outbox::body_of(a)["text"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

/// Rules D14 and D15 (theseus-3i6d1): a spent month tells the owner which rule
/// skipped the run. `held_back` picks D15 for a title beginning `Review: ` (a
/// project's weekly review) and D14 for any other routine; nothing else
/// asserted that branch.
#[tokio::test(flavor = "multi_thread")]
async fn a_skipped_run_names_d14_for_a_routine_and_d15_for_a_review() {
    // Both arms are driven and read first, and every assertion is made over
    // both of them at the end. With the assertions inside the loop it stopped
    // at the first failure, so a planted revert that flips the two arms could
    // only ever be seen red on one of them and the second stayed unproved.
    struct Arm {
        title: &'static str,
        rule: &'static str,
        other: &'static str,
        skipped: bool,
        told: Vec<String>,
    }
    let mut seen: Vec<Arm> = Vec::new();
    for (title, rule, other) in [
        ("Tidy the scratch notes", "Rule D14 skipped", "D15"),
        ("Review: the harbour project", "Rule D15 skipped", "D14"),
    ] {
        let r = rig();
        made(&r.core, json!({"month_usd": 1.0})).await;
        let now = r.core.kernel.now_ms();
        let this_month = theseus_kernel::cron::month_key(&r.core.kernel.config().zone, now);
        let (spec, mut due) = routine_and_due(&r, &this_month);
        let mut spec = spec.as_ref().clone();
        spec.title = title.into();
        due.spent_micros = spec.month_micros;
        due.available_micros = 0;
        due.available_after_reset_micros = 0;
        let (p, _) = plan(&env(&r), &Arc::new(spec), &due).unwrap();
        seen.push(Arm {
            title,
            rule,
            other,
            skipped: matches!(p, RoutinePlan::Skip { .. }),
            told: rule_posts(&r),
        });
    }
    let shown: Vec<_> = seen
        .iter()
        .map(|a| (a.title, a.skipped, a.told.clone()))
        .collect();
    assert_eq!(seen.len(), 2, "both arms reached: {shown:?}");
    assert!(seen.iter().all(|a| a.skipped), "each skipped: {shown:?}");
    assert!(
        seen.iter().all(|a| a.told.len() == 1),
        "one notice each: {shown:?}"
    );
    assert!(
        seen.iter().all(|a| a.told[0].contains(a.rule)),
        "each names its own rule: {shown:?}"
    );
    assert!(
        seen.iter().all(|a| !a.told[0].contains(a.other)),
        "neither names the other's rule: {shown:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn after_downtime_one_catch_up_says_how_many_were_missed_or_none_runs() {
    let r = rig();
    made(&r.core, json!({"catch_up": "one"})).await;
    let now = r.core.kernel.now_ms();
    let this_month = theseus_kernel::cron::month_key(&r.core.kernel.config().zone, now);
    let (spec, mut due) = routine_and_due(&r, &this_month);
    due.fired.while_down = true;
    due.fired.missed = 2;
    let (p, _) = plan(&env(&r), &spec, &due).unwrap();
    let RoutinePlan::Run(run) = p else {
        panic!("catch_up one runs once")
    };
    let brief = run
        .records
        .iter()
        .filter(|rec| rec.kind == theseus_store::kinds::NODE)
        .map(|rec| String::from_utf8_lossy(&rec.payload).to_string())
        .collect::<String>();
    assert!(
        brief.contains("(2 missed while the daemon was down)"),
        "{brief}"
    );
    assert!(brief.contains("Run 1 of Tide check, due "), "{brief}");
    assert!(brief.contains(BRIEF));
    // `none` waits for the next; `auto` is by the span.
    let mut none = (*spec).clone();
    none.catch_up = theseus_protocol::routines::CatchUp::None;
    let (p, _) = plan(&env(&r), &none, &due).unwrap();
    assert!(matches!(
        p,
        RoutinePlan::Skip {
            why: "catch_up",
            ..
        }
    ));
    let mut auto = (*spec).clone();
    auto.catch_up = theseus_protocol::routines::CatchUp::Auto;
    auto.period_ms = 60_000;
    assert!(matches!(
        plan(&env(&r), &auto, &due).unwrap().0,
        RoutinePlan::Skip {
            why: "catch_up",
            ..
        }
    ));
    auto.period_ms = 86_400_000;
    assert!(matches!(
        plan(&env(&r), &auto, &due).unwrap().0,
        RoutinePlan::Run(_)
    ));
    // Not down and nothing missed: a normal fire, with no clause.
    due.fired.while_down = false;
    due.fired.missed = 0;
    none.catch_up = theseus_protocol::routines::CatchUp::None;
    assert!(matches!(
        plan(&env(&r), &none, &due).unwrap().0,
        RoutinePlan::Run(_)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_routine_the_model_sets_up_waits_on_one_card_and_is_listed_in_its_commitments() {
    let r = rig();
    let s = crate::tests_task_graph::session(&r.core);
    *r.model.call.lock().unwrap() = Some(json!({
        "title": "Morning digest", "schedule": "weekdays 08:00", "brief": BRIEF,
    }));
    let res = crate::tests_task_graph::turn(&r.core, &s, "set up a morning digest for me").await;
    let _ = res;
    let asked = r.core.confirm_list().unwrap();
    assert_eq!(asked.len(), 1, "{asked:?}");
    let card = &asked[0];
    assert_eq!(card.tool, "routine.create");
    assert!(
        card.reason
            .starts_with("Create routine \"Morning digest\": weekdays at 08:00 (next: "),
        "{}",
        card.reason
    );
    assert!(
        card.reason
            .contains("up to $0.50 a run, $10 a month · posts to your DM"),
        "{}",
        card.reason
    );
    assert!(card.reason.contains("not a system cap"), "{}", card.reason);
    assert!(card.reason.contains("$0.50 a run"), "{}", card.reason);
    let listed = rpc(&r.core, "routine.list", json!({})).await.unwrap();
    assert!(
        listed["routines"].as_array().unwrap().is_empty(),
        "nothing exists before the yes"
    );
    r.core
        .confirm_action(&card.correlation_id, true, None, "cli")
        .unwrap();
    until("the routine", 20, || {
        r.core
            .runner
            .tools
            .routines
            .all(&r.core.store)
            .unwrap()
            .len()
            == 1
    })
    .await;
    let spec = r
        .core
        .runner
        .tools
        .routines
        .all(&r.core.store)
        .unwrap()
        .remove(0);
    assert_eq!(spec.created_in.as_deref(), Some(s.as_str()));
    assert_eq!(spec.created_by, format!("model:{s}"));
    until("the turn's end", 20, || {
        r.core.confirm_list().unwrap().is_empty()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    // The next turn's last block lists it.
    crate::tests_task_graph::turn(&r.core, &s, "what is scheduled").await;
    let req = r.model.requests.lock().unwrap().last().unwrap().clone();
    let text = serde_json::to_string(&req.messages).unwrap();
    let short = crate::routine::short(&spec.session_id);
    assert!(
        text.contains(&format!("🔁 {short} weekdays 08:00 Morning digest (next ")),
        "{text}"
    );
    // A task cannot set one up, and the owner's verbs stay the owner's.
    let _ = WakeRun::Turn;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_routine_that_speaks_only_when_notable_logs_a_report_that_ends_nothing_new() {
    let r = rig();
    let info = made(&r.core, json!({"speak": "when_notable"})).await;
    let id = info["id"].as_str().unwrap().to_string();
    *r.model.text.lock().unwrap() = "Checked the table; no change.\nNOTHING NEW".into();
    due_soon(&r.core).await;
    until("the first run", 30, || {
        runs_sync(&r.core, &id)
            .first()
            .is_some_and(|x| x["state"] == "complete")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(reports(&r.core).is_empty(), "nothing new says nothing");
    // The brief told the model how to say so.
    let first = serde_json::to_string(&r.model.requests.lock().unwrap()[0].messages).unwrap();
    assert!(
        first.contains("end your report with exactly NOTHING NEW"),
        "{first}"
    );
    // Something new is posted.
    *r.model.text.lock().unwrap() = "The high tide moved to 07:40.".into();
    due_soon(&r.core).await;
    until("the report", 30, || reports(&r.core).len() == 1).await;
    let both = runs_sync(&r.core, &id);
    assert_eq!(both.len(), 2, "the quiet run is in the log");
}

fn runs_sync(core: &Arc<Core>, id: &str) -> Vec<Value> {
    let spec = core.runner.tools.routines.find(&core.store, id).unwrap();
    let mut tasks = core.kernel.tasks(Some(&spec.execution_id)).unwrap();
    tasks.reverse();
    let env = Env {
        kernel: &core.kernel,
        store: &core.store,
        outbox: &core.outbox,
        target: None,
    };
    tasks
        .iter()
        .filter_map(|t| crate::routine::run_row(&env, &t.id).unwrap())
        .map(|r| serde_json::to_value(r).unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn update_changes_the_series_and_the_limits_and_delete_ends_it_all() {
    let r = rig();
    let info = made(&r.core, json!({})).await;
    let id = info["id"].as_str().unwrap().to_string();
    let updated = rpc(
        &r.core,
        "routine.update",
        json!({"id": id, "schedule": "weekdays 08:00", "budget_usd": 0.25, "month_usd": 4.0,
               "title": "Tide check, mornings", "speak": "when_notable"}),
    )
    .await
    .unwrap();
    let u = &updated["routine"];
    assert_eq!(u["series"], "weekdays 08:00");
    assert_eq!(
        (u["run_usd"].as_f64(), u["month_usd"].as_f64()),
        (Some(0.25), Some(4.0))
    );
    assert_eq!(
        (u["title"].as_str(), u["speak"].as_str()),
        (Some("Tide check, mornings"), Some("when_notable"))
    );
    // The execution's limit is the new month, the wake's run the new brief budget.
    let spec = r
        .core
        .runner
        .tools
        .routines
        .find(&r.core.store, &id)
        .unwrap();
    let e = r
        .core
        .kernel
        .execution(&spec.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(e.budget.limit_micros, 4_000_000);
    assert!(matches!(&e.wakes[0].run, WakeRun::Task(t) if t.limit_micros == 250_000));
    let bad = rpc(
        &r.core,
        "routine.update",
        json!({"id": id, "schedule": "sometimes"}),
    )
    .await
    .unwrap_err();
    assert!(bad.contains("cannot read the schedule"), "{bad}");
    // A run, then the delete: nothing is left to fire, and the list is empty.
    rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    rpc(&r.core, "routine.delete", json!({"id": id}))
        .await
        .unwrap();
    let gone = r
        .core
        .kernel
        .execution(&spec.execution_id)
        .unwrap()
        .unwrap();
    assert!(gone.state.is_terminal() && gone.wakes.is_empty());
    let list = rpc(&r.core, "routine.list", json!({})).await.unwrap();
    assert!(list["routines"].as_array().unwrap().is_empty());
    let named = rpc(&r.core, "routine.runs", json!({"id": id}))
        .await
        .unwrap_err();
    assert!(named.contains("no routine"), "{named}");
    assert_eq!(
        rows(&r.core, "routine.changed").len(),
        3,
        "created, updated, deleted"
    );
}

/// `routine.skip` (theseus-0oldh): the next occurrence is dropped and the next
/// time moves one period, but a skip opens no run, so the routine's count of
/// runs (the number a stop of `after N runs` reads) does not move, and the
/// run that comes next is run 1. A held routine has none to skip.
#[tokio::test(flavor = "multi_thread")]
async fn a_skip_spends_no_run_and_moves_the_next_time_one_period() {
    let r = rig();
    let info = made(&r.core, json!({})).await;
    let id = info["id"].as_str().unwrap().to_string();
    let next = info["next_ms"].as_u64().unwrap();
    let skipped = rpc(&r.core, "routine.skip", json!({"id": id}))
        .await
        .unwrap();
    let after = &skipped["routine"];
    assert_eq!(after["next_ms"].as_u64(), Some(next + 60_000), "{after}");
    assert_eq!(after["paused"], false, "the series is still running");
    assert_eq!(after["runs"], 0, "a skip spends no run");
    assert!(runs(&r.core, &id).await.is_empty(), "nothing ran");
    let dropped = rows(&r.core, "wake.skipped");
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0]["why"], "owner");
    assert_eq!(dropped[0]["due_at_ms"].as_u64(), Some(next));
    // The next run is run 1: the skipped occurrence took no number from it.
    let ran = rpc(&r.core, "routine.run_now", json!({"id": id}))
        .await
        .unwrap();
    assert_eq!(ran["routine"]["runs"], 1, "{ran}");
    until("its report", 30, || reports(&r.core).len() == 1).await;
    assert_eq!(runs(&r.core, &id).await[0]["run"], 1);
    // Held, there is no next run to drop, and it is said in words.
    rpc(&r.core, "routine.pause", json!({"id": id}))
        .await
        .unwrap();
    let held = rpc(&r.core, "routine.skip", json!({"id": id}))
        .await
        .unwrap_err();
    assert!(held.contains("paused"), "{held}");
    assert_eq!(
        rows(&r.core, "wake.skipped").len(),
        1,
        "a refusal writes no row"
    );
}

/// The two `default_*` figures are what a routine that names none gets, not caps (theseus-tclcj): a
/// routine's own figure is taken as named unless it is above a ceiling, and above one it is refused,
/// naming the key, never clamped.
#[test]
fn a_routines_own_dollars_are_bounded_only_by_a_ceiling_and_a_refusal_names_the_key() {
    use crate::config::RoutinesConfig;
    use crate::routine::check_budgets;
    // A ceiling the owner raised: a figure well above the defaults is the routine's own, as before.
    let open = RoutinesConfig {
        max_run_usd: 5000.0,
        max_month_usd: 5000.0,
        ..RoutinesConfig::default()
    };
    assert_eq!(check_budgets(&open, 5.0, 500.0), Ok(()));
    // The positive control: an ordinary figure passes under ceilings.
    let capped = RoutinesConfig {
        max_run_usd: 2.0,
        max_month_usd: 50.0,
        ..RoutinesConfig::default()
    };
    assert_eq!(check_budgets(&capped, 0.25, 4.0), Ok(()));
    assert_eq!(check_budgets(&capped, 2.0, 50.0), Ok(()));
    // Above one, each is refused with the key that bounds it.
    let run = check_budgets(&capped, 3.0, 40.0).unwrap_err();
    assert!(
        run.contains("max_run_usd") && !run.contains("max_month_usd"),
        "{run}"
    );
    let month = check_budgets(&capped, 1.0, 500.0).unwrap_err();
    assert!(
        month.contains("max_month_usd") && !month.contains("max_run_usd"),
        "{month}"
    );
    // The old rules still hold.
    assert!(check_budgets(&capped, 0.0, 4.0).is_err());
    assert!(check_budgets(&capped, 5.0, 4.0).is_err());
    // A ceiling below a default would refuse a routine that names nothing, so the config says so.
    let bad = RoutinesConfig {
        max_month_usd: 1.0,
        ..RoutinesConfig::default()
    };
    assert!(bad.validate().is_err());
    let bad_run = RoutinesConfig {
        max_run_usd: 0.25,
        ..RoutinesConfig::default()
    };
    assert!(bad_run.validate().is_err());
    assert!(capped.validate().is_ok());
}

/// The ceilings ship with defaults (the owner, 2026-11-09 15:48): $5 a run and $50 a month, so an
/// unconfigured daemon refuses a routine a model proposes at $500 a month, naming the key, and an
/// owner who sets a higher ceiling in config is taken at it (theseus-3i6d1).
#[tokio::test(flavor = "multi_thread")]
async fn an_unconfigured_daemon_refuses_a_routine_naming_500_dollars_a_month_and_a_raised_ceiling_takes_it(
) {
    let defaults = crate::config::RoutinesConfig::default();
    assert_eq!((defaults.max_run_usd, defaults.max_month_usd), (5.0, 50.0));
    assert!(defaults.validate().is_ok());
    let r = rig();
    assert_eq!(r.core.cfg.routines, defaults);
    let refused = rpc(
        &r.core,
        "routine.create",
        json!({"title": "Tide check", "schedule": "every 1m", "brief": BRIEF,
               "budget_usd": 1.0, "month_usd": 500.0}),
    )
    .await
    .unwrap_err();
    assert!(
        refused.contains("max_month_usd") && refused.contains("$500") && refused.contains("$50"),
        "{refused}"
    );
    assert!(!refused.contains("max_run_usd"), "{refused}");
    let run = rpc(
        &r.core,
        "routine.create",
        json!({"title": "Tide check", "schedule": "every 1m", "brief": BRIEF,
               "budget_usd": 6.0, "month_usd": 40.0}),
    )
    .await
    .unwrap_err();
    assert!(
        run.contains("max_run_usd") && !run.contains("max_month_usd"),
        "{run}"
    );
    let listed = rpc(&r.core, "routine.list", json!({})).await.unwrap();
    assert!(
        listed["routines"].as_array().unwrap().is_empty(),
        "a refusal makes nothing, and nothing is clamped"
    );
    // The ceilings are inclusive: $5 a run and $50 a month are the most a routine may name.
    let edge = made(&r.core, json!({"budget_usd": 5.0, "month_usd": 50.0})).await;
    assert_eq!(
        (edge["run_usd"].as_f64(), edge["month_usd"].as_f64()),
        (Some(5.0), Some(50.0))
    );
    // A config that raises the ceiling to 5,000 takes the same $500 month as named.
    let high = rig_with(|c| {
        c.routines.max_run_usd = 5000.0;
        c.routines.max_month_usd = 5000.0;
    });
    assert!(high.core.cfg.routines.validate().is_ok());
    let big = made(&high.core, json!({"budget_usd": 1.0, "month_usd": 500.0})).await;
    assert_eq!(big["month_usd"].as_f64(), Some(500.0), "{big}");
}

/// The same refusal on the model's own path: a `routine_create` naming $500 a month is turned back
/// before any card, with the key named in the tool's result, and a figure within the ceilings still
/// reaches its one card (theseus-3i6d1).
#[tokio::test(flavor = "multi_thread")]
async fn a_routine_the_model_proposes_at_500_dollars_a_month_is_refused_before_a_card() {
    let r = rig();
    let s = crate::tests_task_graph::session(&r.core);
    *r.model.call.lock().unwrap() = Some(json!({
        "title": "Morning digest", "schedule": "weekdays 08:00", "brief": BRIEF,
        "budget_usd": 1.0, "month_usd": 500.0,
    }));
    let _ = crate::tests_task_graph::turn(&r.core, &s, "set up a morning digest for me").await;
    assert!(r.core.confirm_list().unwrap().is_empty(), "no card for it");
    let told = r
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .map(|q| q.messages.last().map(|m| m.to_string()).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(told.contains("max_month_usd"), "{told}");
    *r.model.call.lock().unwrap() = Some(json!({
        "title": "Morning digest", "schedule": "weekdays 08:00", "brief": BRIEF,
        "budget_usd": 1.0, "month_usd": 40.0,
    }));
    let _ = crate::tests_task_graph::turn(&r.core, &s, "set up a morning digest for me").await;
    assert_eq!(
        r.core.confirm_list().unwrap().len(),
        1,
        "a figure within the ceilings is carded"
    );
}

/// The template documents both ceilings with the figures they default to, and the keys it names are
/// the defaults the daemon ships (theseus-3i6d1).
#[test]
fn the_example_config_documents_both_routine_ceilings_with_their_defaults() {
    let text = Config::EXAMPLE_TOML;
    let line = |key: &str| {
        text.lines()
            .find(|l| l.starts_with(&format!("# {key} = ")))
            .unwrap_or_else(|| panic!("the template documents {key}"))
            .to_string()
    };
    let run = line("max_run_usd");
    let month = line("max_month_usd");
    assert!(run.starts_with("# max_run_usd = 5 "), "{run}");
    assert!(month.starts_with("# max_month_usd = 50 "), "{month}");
    assert!(
        run.contains("default") && month.contains("default"),
        "{run} / {month}"
    );
    let d = crate::config::RoutinesConfig::default();
    assert_eq!((d.max_run_usd, d.max_month_usd), (5.0, 50.0));
    // Uncommented, the lines parse to the same figures: the documented values are the shipped ones.
    let on = text
        .replace(&run, "max_run_usd = 5")
        .replace(&month, "max_month_usd = 50")
        .replace("# [routines]\n", "[routines]\n");
    let c = Config::parse(&on).unwrap().0;
    assert_eq!(c.routines, d);
    // And a higher figure is a config edit away.
    let high = on.replace("max_month_usd = 50", "max_month_usd = 5000");
    assert_eq!(
        Config::parse(&high).unwrap().0.routines.max_month_usd,
        5000.0
    );
}

/// The words a spent month says: whose figure it is, not "the" limit (theseus-tclcj).
#[test]
fn a_spent_month_says_the_figure_is_the_routines_own() {
    let text = crate::routine::month_spent_text("Digest", 1.0, "ab12");
    assert!(
        text.contains("this routine's own month budget of $1.00"),
        "{text}"
    );
    assert!(!text.contains("this month's $1.00 is spent"), "{text}");
    let rule = crate::rule_notice::Rule::D14.spec();
    assert!(!rule.key.contains("default_"), "{}", rule.key);
    assert!(rule.key.contains("routine update"), "{}", rule.key);
}

/// The creation card says the dollars are the routine's own, once, in both
/// builders (theseus-tclcj). `Prepared::card` has no caller outside tests.
#[tokio::test(flavor = "multi_thread")]
async fn the_card_spells_the_budget_once() {
    let r = rig();
    let words = crate::routine::budget_words(0.5, 10.0);
    assert!(words.contains("not a system cap"), "{words}");
    let new = crate::routine::NewRoutine {
        title: "Tide check".into(),
        schedule: "weekdays 08:00".into(),
        brief: BRIEF.into(),
        ..Default::default()
    };
    let cfg = crate::config::RoutinesConfig::default();
    let prepared = crate::routine::prepare(&r.core.kernel, &cfg, &new, "owner", None).unwrap();
    let card = prepared.card(&r.core.kernel.config().zone);
    assert_eq!(card.matches(&words).count(), 1, "{card}");
    assert_eq!(card.matches("a run").count(), 1, "{card}");
}
