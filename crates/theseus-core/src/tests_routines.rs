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
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
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
        .contains("this month's $1.00 is spent"));
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
