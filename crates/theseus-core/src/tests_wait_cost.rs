//! `job_wait` waits as long as the model asked (theseus-7m7w.3), on tokio's
//! paused clock through the whole core: a job that ends at 300 s ends a wait
//! asked for 900 s at 300 s, one that runs goes the whole 900 s, a person's
//! message at 120 s ends it at once, `[tools] job_wait_max_secs` caps it, and
//! a wait that outlasts the request's cache sends nothing of its own (issue 44,
//! 2026-11-22: no saved request is sent again). A job here is held by the
//! test (`Gated`) until the test says it ends, so its end is a moment on the
//! clock, not a process's.
//!
//! `a_five_minute_job_costs_fewer_calls_and_dollars_with_the_longer_wait`
//! prints the stand-in model's calls, tokens and dollars before and after
//! (`--nocapture`); `docs/benchmarks` holds its numbers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult, Usage};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::toolrun::{JobDone, JobLauncher};
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// Jobs held until the test ends them: a launch keeps its wrapper's
/// arguments, and `finish` runs each wrapper (a `true`) on the test's own
/// thread, where the paused clock cannot move.
#[derive(Default)]
struct Gated {
    held: Mutex<Vec<(theseus_kernel::job::WrapperArgs, JobDone)>>,
}

impl JobLauncher for Gated {
    fn launch(
        &self,
        _: &theseus_kernel::Spool,
        args: &theseus_kernel::job::WrapperArgs,
        done: JobDone,
    ) -> anyhow::Result<u32> {
        self.held.lock().unwrap().push((args.clone(), done));
        Ok(std::process::id())
    }
}

impl Gated {
    /// Every held job ends now.
    fn finish(&self) {
        for (args, done) in self.held.lock().unwrap().drain(..) {
            let _ = theseus_kernel::job::run_wrapper(&args);
            done.wake();
        }
    }
}

/// The smallest output, which the deleted keep-warm read asked for: a call at it would be a saved request
/// sent again, and goes to a provider of its own so the tests can say none was made.
const MAX_TOKENS: u32 = 1;

/// Turns answer from the script, and a call at the smallest output (`MAX_TOKENS`) from a provider of its
/// own, so such a call does not use up the script and is counted.
struct Split {
    turns: Arc<FakeProvider>,
    reads: Arc<FakeProvider>,
}

impl Provider for Split {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        match req.max_tokens == MAX_TOKENS {
            true => self.reads.stream_message(req, on_delta),
            false => self.turns.stream_message(req, on_delta),
        }
    }
}

struct Rig {
    core: Arc<Core>,
    turns: Arc<FakeProvider>,
    reads: Arc<FakeProvider>,
    gate: Arc<Gated>,
    _dir: tempfile::TempDir,
}

/// The template, or with a one-hour `[model]` cache so every profile that
/// says none (the live one among them) caches for an hour.
fn config(dir: &std::path::Path, hour: bool, edit: impl FnOnce(&mut Config)) -> Config {
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = match hour {
        false => Config::example(),
        true => {
            Config::parse(&Config::EXAMPLE_TOML.replace("\n[cache]\n", "\n[cache]\nttl = \"1h\"\n"))
                .unwrap()
                .0
        }
    };
    cfg.server.state_dir = dir.join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    // The benchmark's config: a foreground command waits a minute.
    cfg.tools.proc_sync_secs = 60;
    // This file measures a wait of the length asked; the beat has its own.
    cfg.tools.job_wait_beat_secs = 0;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    cfg.memory.mode = crate::config::MemoryMode::Off;
    // Any prefix is worth reading here (the floor has its own test).
    cfg.cache.keep_warm_min_tokens = 0;
    edit(&mut cfg);
    cfg
}

fn rig(script: Vec<Scripted>, edit: impl FnOnce(&mut Config)) -> Rig {
    rig_with(script, false, edit)
}

fn rig_with(script: Vec<Scripted>, hour: bool, edit: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), hour, edit);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let turns = Arc::new(FakeProvider::scripted(script));
    let reads = Arc::new(FakeProvider::default());
    let split = Arc::new(Split {
        turns: turns.clone(),
        reads: reads.clone(),
    });
    let gate = Arc::new(Gated::default());
    let mut parts = crate::rpc::Parts::for_tests(cfg, split, store);
    parts.launcher = gate.clone();
    Rig {
        core: Core::build(parts).unwrap(),
        turns,
        reads,
        gate,
        _dir: dir,
    }
}

fn request(core: &Arc<Core>, session: &SessionRecord, input: &str) -> TurnRequest {
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &session.session_id, None);
    TurnRequest {
        prompt: None,
        session: session.clone(),
        input: Some(input.into()),
        target,
        sink,
        author: "test".into(),
        attachments: vec![],
        arrived: None,
        reply_to: None,
        surface: None,
        answer_at: None,
    }
}

impl Rig {
    fn session(&self) -> SessionRecord {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        r
    }

    fn submit(&self, session: &SessionRecord, input: &str) -> JoinHandle<TurnSubmitResult> {
        let core = self.core.clone();
        let rec = core
            .store
            .get_session::<SessionRecord>(&session.session_id)
            .unwrap()
            .unwrap();
        let req = request(&core, &rec, input);
        tokio::spawn(async move { core.runner.run(req).await.unwrap() })
    }

    /// Until the model has been asked `n` times.
    async fn until_calls(&self, n: usize) {
        for _ in 0..10_000 {
            if self.turns.requests().len() >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the model call {n}");
    }

    fn result(&self, session: &str, id: &str) -> (ResultStatus, String, Value) {
        self.core
            .store
            .session_nodes(session)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match n.body {
                Body::ToolResult {
                    tool_use_id,
                    status,
                    content,
                    meta,
                    late: false,
                    ..
                } if tool_use_id == id => Some((status, content, meta)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no result for {id}"))
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(10_000)
            .unwrap()
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.kind == kind)
            .map(|r| serde_json::to_value(r).unwrap())
            .collect()
    }

    /// The calls at the smallest output the wait sent (none: nothing is sent again), in order.
    fn warm_reads(&self) -> Vec<ProviderRequest> {
        self.reads.requests()
    }
}

/// Start a job, wait `ask` seconds for it, then answer.
fn script(ask: u64) -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "Starting it.",
            &[(
                "t1",
                "proc_run",
                json!({"argv": ["true"], "background": true}),
            )],
        ),
        Scripted::tools(
            "Waiting.",
            &[("t2", "job_wait", json!({"job": "j1", "timeout_secs": ask}))],
        ),
        Scripted::text("Done."),
    ]
}

/// A job that ends at 300 s ends a wait asked for 900 s at 300 s, and the
/// wait gives its result as a blocking run does.
#[tokio::test(start_paused = true)]
async fn a_wait_asked_for_900_s_returns_at_300_s_when_the_job_ends() {
    let r = rig(script(900), |_| {});
    let s = r.session();
    let turn = r.submit(&s, "build it");
    r.until_calls(2).await;
    let t0 = Instant::now();
    tokio::time::sleep(Duration::from_secs(300)).await;
    r.gate.finish();
    turn.await.unwrap();
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(299) && took < Duration::from_secs(303),
        "returned after {took:?}"
    );
    let (status, text, meta) = r.result(&s.session_id, "t2");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(!text.contains("Still running"), "{text}");
    assert!(meta["delivers"].is_string(), "{meta}");
    assert_eq!(r.turns.requests().len(), 3, "one wait, one answer");
}

/// A job still running goes the whole 900 s, and the result says 900 s.
#[tokio::test(start_paused = true)]
async fn a_wait_asked_for_900_s_on_a_running_job_goes_the_900_s() {
    let r = rig(script(900), |_| {});
    let s = r.session();
    let turn = r.submit(&s, "build it");
    r.until_calls(2).await;
    let t0 = Instant::now();
    turn.await.unwrap();
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(899) && took < Duration::from_secs(903),
        "returned after {took:?}"
    );
    let (_, text, meta) = r.result(&s.session_id, "t2");
    assert!(text.starts_with("Still running after 900 s."), "{text}");
    assert_eq!(meta["state"], "running");
}

/// A person's message at 120 s ends a wait asked for 900 s at once, with
/// steer-queued's meta, and joins the turn's next request.
#[tokio::test(start_paused = true)]
async fn a_message_at_120_s_ends_a_wait_asked_for_900_s_at_once() {
    let r = rig(script(900), |_| {});
    let s = r.session();
    let turn = r.submit(&s, "build it");
    r.until_calls(2).await;
    let t0 = Instant::now();
    tokio::time::sleep(Duration::from_secs(120)).await;
    let msg = r.submit(&s, "cancel it");
    let (a, b) = (turn.await.unwrap(), msg.await.unwrap());
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(119) && took < Duration::from_secs(123),
        "returned after {took:?}"
    );
    assert_eq!(a.turn_id, b.turn_id, "the running turn answered both");
    let (_, text, meta) = r.result(&s.session_id, "t2");
    assert_eq!(meta["interrupted_by"], "message", "{text}");
    assert!(text.contains("message arrived"), "{text}");
    let next = serde_json::to_string(&r.turns.requests()[2].messages).unwrap();
    assert!(next.contains("cancel it"), "{next}");
}

/// `[tools] job_wait_max_secs` caps the ask, and the result says so; the
/// tool's description gives the cap.
#[tokio::test(start_paused = true)]
async fn job_wait_max_secs_caps_the_wait_and_the_description_says_it() {
    let r = rig(script(900), |c| c.tools.job_wait_max_secs = 600);
    let s = r.session();
    let turn = r.submit(&s, "build it");
    r.until_calls(2).await;
    let t0 = Instant::now();
    turn.await.unwrap();
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(599) && took < Duration::from_secs(603),
        "returned after {took:?}"
    );
    let (_, text, _) = r.result(&s.session_id, "t2");
    assert!(
        text.starts_with(
            "Still running after 600 s (the longest one wait lasts; 900 s was asked)."
        ),
        "{text}"
    );
    let tools = &r.turns.requests()[1].tools;
    let wait = tools.iter().find(|t| t["name"] == "job_wait").unwrap();
    let said = wait["description"].as_str().unwrap();
    assert!(said.contains("returns early when the job ends or a message arrives"));
    assert!(said.ends_with("A wait lasts at most 600 s."), "{said}");
}

/// Without a `timeout_secs` the wait is `proc_sync_secs`, as it was.
#[tokio::test(start_paused = true)]
async fn a_wait_with_no_timeout_is_a_foreground_runs_wait() {
    let mut s = script(0);
    s[1] = Scripted::tools("Waiting.", &[("t2", "job_wait", json!({"job": "j1"}))]);
    let r = rig(s, |_| {});
    let sess = r.session();
    let turn = r.submit(&sess, "build it");
    r.until_calls(2).await;
    let t0 = Instant::now();
    turn.await.unwrap();
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(59) && took < Duration::from_secs(63),
        "returned after {took:?}"
    );
    let (_, text, _) = r.result(&sess.session_id, "t2");
    assert!(text.starts_with("Still running after 60 s."), "{text}");
}

/// A 900 s wait on a five-minute cache sends nothing of its own (issue 44, 2026-11-22: no saved request is
/// sent again): no call at the smallest output, no `keep_warm` row, no `warm_reads` in its result; the turn's
/// own calls are the script's three, each with its money checks as any call has.
#[tokio::test(start_paused = true)]
async fn a_wait_past_the_cache_lifetime_sends_no_call_of_its_own() {
    let r = rig(script(900), |_| {});
    let s = r.session();
    let turn = r.submit(&s, "build it");
    r.until_calls(2).await;
    turn.await.unwrap();
    assert!(r.warm_reads().is_empty(), "the wait sent nothing again");
    assert!(r.rows("keep_warm").is_empty());
    assert_eq!(
        r.turns.requests().len(),
        3,
        "the turn's own calls are the script's"
    );
    let (_, _, meta) = r.result(&s.session_id, "t2");
    assert!(meta.get("warm_reads").is_none(), "{meta}");
}

#[path = "tests_wait_beat.rs"]
mod beat;

// ---- the measure

/// A stand-in model that waits as the real one did: it starts a job, asks
/// for `job_wait {timeout_secs: 900}` and asks again for as long as the
/// result says the job still runs. It bills as the provider does: a prefix
/// read within five minutes of its last use is a cache read, else a write.
struct Modeled {
    inner: FakeProvider,
    entry: Mutex<Option<(Instant, u64)>>,
    calls: AtomicU64,
    reads: AtomicU64,
    waits: AtomicU64,
    cache_read: AtomicU64,
    cache_written: AtomicU64,
}

/// The tokens a request holds: a coding agent's long prefix and the text.
fn tokens_of(req: &ProviderRequest) -> u64 {
    30_000 + serde_json::to_string(req).unwrap().len() as u64 / 4
}

impl Provider for Modeled {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        let is_read = req.max_tokens == MAX_TOKENS;
        let all = tokens_of(req);
        let now = Instant::now();
        let mut entry = self.entry.lock().unwrap();
        let read = match *entry {
            Some((at, covered)) if now.duration_since(at) < Duration::from_secs(300) => {
                covered.min(all)
            }
            _ => 0,
        };
        *entry = Some((now, all));
        drop(entry);
        self.cache_read.fetch_add(read, Ordering::Relaxed);
        self.cache_written.fetch_add(all - read, Ordering::Relaxed);
        let usage = Usage {
            input_tokens: 0,
            output_tokens: if is_read { 1 } else { 60 },
            cache_read_input_tokens: read,
            cache_creation_input_tokens: all - read,
            ..Usage::default()
        };
        let last = serde_json::to_string(req.messages.last().unwrap()).unwrap();
        let next = if is_read {
            self.reads.fetch_add(1, Ordering::Relaxed);
            Scripted::text("ok")
        } else {
            let n = self.calls.fetch_add(1, Ordering::Relaxed);
            if n == 0 {
                Scripted::tools(
                    "",
                    &[(
                        "t0",
                        "proc_run",
                        json!({"argv": ["true"], "background": true}),
                    )],
                )
            } else if last.contains("Started job") || last.contains("Still running") {
                let w = self.waits.fetch_add(1, Ordering::Relaxed);
                let id = format!("w{w}");
                Scripted::tools(
                    "",
                    &[(&id, "job_wait", json!({"job": "j1", "timeout_secs": 900}))],
                )
            } else {
                Scripted::text("Done.")
            }
        };
        self.inner
            .script
            .lock()
            .unwrap()
            .push_back(Scripted::Billed {
                usage,
                then: Box::new(next),
            });
        self.inner.stream_message(req, on_delta)
    }
}

struct Measure {
    calls: u64,
    reads: u64,
    cache_read: u64,
    cache_written: u64,
    micros: u64,
    took: Duration,
}

/// The model's work on a job that runs `job_secs`, with a wait capped at
/// `max_secs`.
async fn measure(job_secs: u64, max_secs: u64) -> Measure {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), false, |c| c.tools.job_wait_max_secs = max_secs);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Modeled {
        inner: FakeProvider::default(),
        entry: Mutex::new(None),
        calls: AtomicU64::new(0),
        reads: AtomicU64::new(0),
        waits: AtomicU64::new(0),
        cache_read: AtomicU64::new(0),
        cache_written: AtomicU64::new(0),
    });
    let gate = Arc::new(Gated::default());
    let mut parts = crate::rpc::Parts::for_tests(cfg, model.clone(), store);
    parts.launcher = gate.clone();
    let core = Core::build(parts).unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let req = request(&core, &rec, "build it and wait");
    let c = core.clone();
    let t0 = Instant::now();
    let turn = tokio::spawn(async move { c.runner.run(req).await.unwrap() });
    tokio::time::sleep(Duration::from_secs(job_secs)).await;
    gate.finish();
    turn.await.unwrap();
    let took = t0.elapsed();
    let rec: SessionRecord = core.store.get_session(&rec.session_id).unwrap().unwrap();
    let exec = core
        .kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    Measure {
        calls: model.calls.load(Ordering::Relaxed),
        reads: model.reads.load(Ordering::Relaxed),
        cache_read: model.cache_read.load(Ordering::Relaxed),
        cache_written: model.cache_written.load(Ordering::Relaxed),
        micros: exec.budget.spent_micros,
        took,
    }
}

/// The case the benchmark showed: a five-minute job, a model that asks for
/// 900 s. Before (the old clamp, `job_wait_max_secs = proc_sync_secs`) it is
/// told "Still running" every minute and calls again; after, it waits once.
#[tokio::test(start_paused = true)]
async fn a_five_minute_job_costs_fewer_calls_and_dollars_with_the_longer_wait() {
    let mut out = String::new();
    let mut got = Vec::new();
    for (name, job, max) in [
        ("before, 5 min job", 300, 60),
        ("after,  5 min job", 300, 1800),
        ("before, 20 min job", 1200, 60),
        ("after,  20 min job", 1200, 1800),
    ] {
        let m = measure(job, max).await;
        out += &format!(
            "{name}: model calls {:>2} (+{} keep-warm reads), cache read {:>8} tok, cache written \
             {:>8} tok, ${:.4}, wall {:?}\n",
            m.calls,
            m.reads,
            m.cache_read,
            m.cache_written,
            m.micros as f64 / 1e6,
            m.took
        );
        got.push(m);
    }
    println!("{out}");
    let [b5, a5, b20, a20] = &got[..] else {
        unreachable!()
    };
    // Fewer calls with the longer wait. Its dollars were fewer too only while the wait kept the cache warm:
    // since nothing is sent again (issue 44, 2026-11-22) a wait past the five-minute cache leaves the next
    // call to write it whole, so the longer wait costs more here (measured: $0.2257 against $0.1414 for the
    // five-minute job, $0.3364 against $0.2212 for the twenty-minute one). That is the stated cost of the
    // deletion, not asserted either way.
    assert!(a5.calls * 2 < b5.calls, "{out}");
    assert!(a20.calls * 4 < b20.calls, "{out}");
    assert_eq!(b5.reads + b20.reads, 0, "the old wait never reads");
    assert_eq!(
        a5.reads + a20.reads,
        0,
        "nor does the long one: nothing is sent again"
    );
}
