//! The core's output, as a golden (theseus-j6qn, C2's first part).
//!
//! Scripted scenarios run through the whole core: a plain turn, tool calls,
//! a background job and its late result, a confirm approved and one
//! declined, a stop, a failed turn and its retry, a task and its report, a
//! wake, and a budget question. After each step the transcript takes, in
//! order, every frame the store committed (each record's kind and key, and
//! each ledger row and outbox post whole), every notification published (its
//! method and params), and every narrative line. A turn's span tree is drawn
//! under the `turn.trace` row that carries it. The channels are each in their
//! own order; how a step's notifications and narrative lines interleave is no
//! contract, so it is not recorded.
//!
//! It is checked against `tests/golden/core_output.txt`. `THESEUS_GOLDEN=write`
//! rewrites it, for a change you mean.
//!
//! It pins shapes, not numbers (theseus-ptx1): every number in JSON is `#`,
//! and so is every other run of digits, once ids are aliased. So it holds
//! the kinds, keys, strings, and order, and which records each frame holds,
//! and a token estimate, a byte count, a cost, or a time moves none of it;
//! the money and telemetry tests, and the frame budget's, hold the numbers
//! that matter. Ids are aliased as C6's kernel golden aliases them (`_#n`, by
//! first appearance), and a short id (`…a1b2c3`) as the whole id it ends. A
//! duration in a sentence is `<n>` whole, since the machine's load picks its
//! unit, a sha256 is `<sha256>`, and the scenario's temporary directory is
//! `<dir>`. The driver's own loop is concurrent, so `drive` stands in for one
//! pass of it, in execution order.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_kernel::ExecState;
use theseus_protocol::{Message, NarrativeLine, SessionKind, TurnSubmitResult, Usage};
use theseus_store::wal::{self, FrameRead};
use theseus_store::{kinds, Record};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use crate::bus::EventSink;
use crate::policy::Posture;
use crate::provider::{FakeProvider, ProviderError, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::{TurnError, TurnRequest};
use crate::{Config, Core};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/core_output.txt");

/// One core over its own store, and what it has shown the transcript.
struct World {
    core: Arc<Core>,
    root: PathBuf,
    wal: PathBuf,
    mask: Mask,
    /// The last WAL position the transcript has shown.
    shown: u64,
    notes: UnboundedReceiver<Message>,
    lines: UnboundedReceiver<Message>,
    _dir: tempfile::TempDir,
}

fn world(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> World {
    // A path of one length wherever the test runs: the request carries the
    // working directory, so its size, its token estimate, and every price
    // after it would follow `TMPDIR`'s length.
    let dir = tempfile::Builder::new()
        .prefix("golden-")
        .rand_bytes(6)
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    // The template's deployment: what acts runs with a notice, and a read
    // stays quiet. A write waits for an answer.
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.insert("fs.write".into(), Posture::Approve);
    cfg.tools.proc_sync_secs = 1;
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    // The vault has answered, so no turn waits for it: a wait of a few
    // microseconds, or none, would come and go with the machine.
    let secrets =
        crate::secrets::SecretBoard::new(["anthropic_api_key".to_string()], Instant::now());
    secrets.publish(
        [(
            "anthropic_api_key".to_string(),
            Ok(crate::secrets::Secret::new("sk-test-not-a-key".into())),
        )]
        .into(),
        "fake",
    );
    let core = Core::build(crate::rpc::Parts {
        secrets,
        ..crate::rpc::Parts::for_tests(cfg, fake, store)
    })
    .unwrap();
    let (tx, notes) = unbounded_channel();
    *core.bus.tap.lock().unwrap() = Some(tx);
    let (tx, lines) = unbounded_channel();
    assert!(
        core.narrator.watch("golden", tx).is_some(),
        "narration is on"
    );
    let mut dirs = vec![root.to_string_lossy().into_owned()];
    dirs.push(dir.path().to_string_lossy().into_owned());
    if let Ok(c) = dir.path().canonicalize() {
        dirs.push(c.to_string_lossy().into_owned());
    }
    let shown = core.store.last_position();
    World {
        wal: dir.path().join("store").join("wal"),
        core,
        root,
        mask: Mask { dirs },
        shown,
        notes,
        lines,
        _dir: dir,
    }
}

impl World {
    /// A new conversation, posting to `place` when one is given.
    fn session(&self, place: Option<&str>) -> String {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&rec.session_id, &rec).unwrap();
        if let Some(p) = place {
            self.core.outbox.bind_place(p, &rec.session_id).unwrap();
            // A DM's person is its owner once the binding binds it (the
            // place rule, theseus-zmgb).
            self.core
                .runner
                .place_rule
                .bind_one(crate::places::BoundPlace {
                    target: format!("discord:{p}"),
                    name: "DM".into(),
                    private: false,
                });
        }
        rec.session_id
    }

    /// The step `name`: what `what` says it did, then every frame, every
    /// notification, and every narrative line since the last step.
    fn take(&mut self, out: &mut String, name: &str, what: &str) {
        out.push_str(&format!("\n-- {name}\n"));
        if what.starts_with('{') {
            out.push_str(&format!("{}\n", self.mask.json(what)));
        } else if !what.is_empty() {
            out.push_str(&format!("{}\n", self.mask.text(what)));
        }
        let mut trees = Vec::new();
        for f in frames_after(&self.wal, self.shown) {
            self.shown = f.last().map_or(self.shown, |r| r.position);
            frame(out, &f, &self.mask, &mut trees);
        }
        while let Ok(m) = self.notes.try_recv() {
            notification(out, &m, &self.mask, &trees);
        }
        while let Ok(m) = self.lines.try_recv() {
            narrative(out, &m, &self.mask);
        }
    }

    fn exec(&self, sid: &str) -> theseus_kernel::Execution {
        let rec: SessionRecord = self.core.store.get_session(sid).unwrap().unwrap();
        self.core
            .kernel
            .execution(rec.execution_id.as_deref().unwrap())
            .unwrap()
            .unwrap()
    }
}

/// An input's turn, as a client's `turn.submit` runs it.
async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
}

/// What a turn's caller is told.
fn told(r: &anyhow::Result<TurnSubmitResult>) -> String {
    match r {
        Ok(r) => format!(
            "result: {} loops, {} tool calls, stop {}, awaiting {}, output {:?}",
            r.loops,
            r.tool_calls,
            r.stop_reason,
            r.awaiting_confirm.as_deref().unwrap_or("-"),
            r.output
        ),
        Err(e) => match e.downcast_ref::<TurnError>() {
            Some(t) => format!("error: class {}, transient {}: {e:#}", t.class, t.transient),
            None => format!("error: {e:#}"),
        },
    }
}

/// One pass of the driver's loop (`harness::drive`), in execution order and
/// one at a time: a due wake fires, and an execution queued with something
/// to read takes its continuation turn.
async fn drive(core: &Arc<Core>) -> String {
    let mut said = Vec::new();
    let now = core.kernel.now_ms();
    for e in core.kernel.open_executions().unwrap() {
        let e = if theseus_kernel::wakes::due_now(&e, now) {
            match core.kernel.fire_due(&e.id).unwrap() {
                Some(queued) => queued,
                None => continue,
            }
        } else {
            e
        };
        if e.state != ExecState::Queued || !(e.resume_pending || !e.queued_results.is_empty()) {
            continue;
        }
        let r = core.continue_execution(&e.id).await;
        said.push(match r {
            Ok(Some(r)) => format!("continued {}: {}", e.id, told(&Ok(r))),
            Ok(None) => format!("continued {}: no turn", e.id),
            Err(err) => format!("continued {}: {}", e.id, told(&Err(err))),
        });
    }
    said.join("\n")
}

/// Wait for `f`, at most 30 s; on a timeout, the transcript so far says
/// where the scenario went.
async fn until(out: &str, what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "no {what} in 30 s; the transcript so far:\n{out}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn bad_request() -> Scripted {
    Scripted::Fail(ProviderError::InvalidRequest {
        status: 400,
        message: "invalid_request_error: model: the model claude-lighthouse-9 is not served".into(),
    })
}

/// A conversation that posts to a place, and one that sets a wake: every
/// scenario but the budget's.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn conversation(out: &mut String) {
    out.push_str("== a conversation\n");
    let mut w = world(
        vec![
            // a plain turn
            Scripted::text("Hello."),
            // tool calls, one loop each
            Scripted::tools(
                "Reading.",
                &[("r1", "fs_read", json!({"path": "notes.txt"}))],
            ),
            Scripted::tools(
                "Diffing.",
                &[("d1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
            ),
            Scripted::text("The notes say alpha and beta."),
            // a background job, and its late result
            Scripted::tools(
                "",
                &[(
                    "j1",
                    "proc_run",
                    json!({"argv": ["bash", "-c", "sleep 1.5; echo slow done"]}),
                )],
            ),
            Scripted::text("Started; I'll report back."),
            Scripted::text("The slow job finished."),
            // a confirm approved
            Scripted::tools(
                "",
                &[(
                    "w1",
                    "fs_write",
                    json!({"path": "out.txt", "content": "x\n"}),
                )],
            ),
            Scripted::text("Written."),
            // a confirm declined
            Scripted::tools(
                "",
                &[(
                    "w2",
                    "fs_write",
                    json!({"path": "no.txt", "content": "x\n"}),
                )],
            ),
            Scripted::text("Understood: not written."),
            // a stop
            Scripted::tools(
                "",
                &[(
                    "w3",
                    "fs_write",
                    json!({"path": "stop.txt", "content": "x\n"}),
                )],
            ),
            Scripted::text("Stopped, then."),
            // a failed turn, and its retry
            bad_request(),
            bad_request(),
            // a task and its report
            Scripted::tools(
                "Starting it.",
                &[(
                    "t1",
                    "task_create",
                    json!({"brief": "Count to three.", "budget_usd": 5.0, "arrangement":
                           {"pieces": [{"quote": "a task that counts to three", "role": "objective"}]}}),
                )],
            ),
            Scripted::text("Started."),
            Scripted::text("One, two, three."),
            Scripted::text("It counted to three."),
            // a wake
            Scripted::tools(
                "Setting it.",
                &[(
                    "k1",
                    "wake_at",
                    json!({"after": "1s", "note": "check the build"}),
                )],
            ),
            Scripted::text("Wake set."),
            Scripted::text("Checked: the build is green."),
        ],
        |_| {},
    );
    std::fs::write(w.root.join("notes.txt"), "alpha\nbeta\n").unwrap();
    let core = w.core.clone();
    let a = w.session(Some("dm:42"));
    w.take(out, "a session that posts to dm:42", "");

    let r = turn(&core, &a, "Say hello.").await;
    w.take(out, "a plain turn", &told(&r));

    let r = turn(&core, &a, "Read notes.txt, then diff two lines.").await;
    w.take(out, "a turn with two tool calls", &told(&r));

    let r = turn(&core, &a, "Run the slow job.").await;
    w.take(out, "a job that goes on in the background", &told(&r));
    let job = core
        .kernel
        .actions()
        .unwrap()
        .into_iter()
        .find(|a| a.tool == "proc.run")
        .expect("the job")
        .correlation_id;
    until(out, "job's completion", || core.spool.has_completion(&job)).await;
    core.heartbeat("test");
    w.take(out, "the heartbeat takes the job's completion", "");
    let d = drive(&core).await;
    w.take(out, "the driver: the late result's turn", &d);

    let r = turn(&core, &a, "Write out.txt.").await;
    let q = r.as_ref().unwrap().awaiting_confirm.clone().unwrap();
    w.take(out, "a write that waits for approval", &told(&r));
    let c = core.confirm_action(&q, true, None, "test").unwrap();
    w.take(out, "the operator approves it", &json!(c).to_string());
    let d = drive(&core).await;
    w.take(out, "the driver: the approved write's turn", &d);

    let r = turn(&core, &a, "Write no.txt.").await;
    let q = r.as_ref().unwrap().awaiting_confirm.clone().unwrap();
    w.take(out, "a second write that waits", &told(&r));
    let c = core
        .confirm_action(&q, false, Some("not that one"), "test")
        .unwrap();
    w.take(out, "the operator declines it", &json!(c).to_string());
    let d = drive(&core).await;
    w.take(out, "the driver: the declined write's turn", &d);

    let r = turn(&core, &a, "Write stop.txt.").await;
    w.take(out, "a third write that waits", &told(&r));
    let s = core
        .stop_execution(&w.exec(&a).id, "discord:operator")
        .await
        .unwrap();
    w.take(out, "a stop", &json!(s).to_string());
    let r = turn(&core, &a, "Never mind.").await;
    w.take(out, "the turn after the stop", &told(&r));

    let r = turn(&core, &a, "Fail, please.").await;
    w.take(out, "a turn the provider refuses", &told(&r));
    let d = drive(&core).await;
    w.take(out, "the driver: its one retry, refused too", &d);

    let r = turn(&core, &a, "Start a task that counts to three.").await;
    w.take(out, "a turn that starts a task", &told(&r));
    let d = drive(&core).await;
    w.take(out, "the driver: the task's turn and its report", &d);
    let r = turn(&core, &a, "What did the task say?").await;
    w.take(out, "the parent's turn reads the report", &told(&r));

    let b = w.session(Some("dm:43"));
    let r = turn(&core, &b, "Wake me in a second.").await;
    w.take(out, "a turn that sets a wake", &told(&r));
    until(out, "wake due", || {
        let e = w.exec(&b);
        theseus_kernel::wakes::due_now(&e, core.kernel.now_ms())
    })
    .await;
    let d = drive(&core).await;
    w.take(out, "the driver: the wake fires and its turn runs", &d);
}

/// A session at its spend limit asks, and an approved reset makes the call
/// that did not fit.
async fn budget(out: &mut String) {
    out.push_str("\n== a budget question\n");
    let first = Scripted::Billed {
        usage: Usage {
            input_tokens: 1_000,
            output_tokens: 30_000,
            ..Default::default()
        },
        then: Box::new(Scripted::tools(
            "Diffing.",
            &[("d1", "text_diff", json!({"a": "x\n", "b": "y\n"}))],
        )),
    };
    let mut w = world(vec![first, Scripted::text("The diff is one line.")], |c| {
        c.kernel.spend_limit_usd = 1.40
    });
    let core = w.core.clone();
    let s = w.session(Some("dm:44"));
    w.take(out, "a session that posts to dm:44", "");
    let r = turn(&core, &s, "Diff x and y.").await;
    let q = r.as_ref().unwrap().awaiting_confirm.clone().unwrap();
    w.take(
        out,
        "the second call does not fit, and the turn asks",
        &told(&r),
    );
    let c = core.confirm_action(&q, true, None, "test").unwrap();
    w.take(out, "the operator approves a reset", &json!(c).to_string());
    let d = drive(&core).await;
    w.take(out, "the driver: the call that did not fit", &d);
}

#[tokio::test]
async fn the_cores_output_matches_its_golden() {
    let mut out = String::new();
    conversation(&mut out).await;
    budget(&mut out).await;
    let got = shapes(&alias(&out));
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        let to = std::env::var("THESEUS_GOLDEN_TO").unwrap_or_else(|_| GOLDEN.to_string());
        std::fs::create_dir_all(Path::new(&to).parent().unwrap()).unwrap();
        std::fs::write(&to, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(GOLDEN).unwrap_or_default();
    if got != want {
        let first = got
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(got.lines().count().min(want.lines().count()));
        panic!(
            "the core's output differs from {GOLDEN} at line {}:\n  got:  {}\n  want: {}\n({} lines \
             got, {} want; THESEUS_GOLDEN=write rewrites it, for a change you mean)",
            first + 1,
            got.lines().nth(first).unwrap_or("<end>"),
            want.lines().nth(first).unwrap_or("<end>"),
            got.lines().count(),
            want.lines().count()
        );
    }
}

// ---------------------------------------------------------------- reading the WAL

/// The whole frames after position `after`, each as its records.
fn frames_after(wal: &Path, after: u64) -> Vec<Vec<Record>> {
    let mut out = Vec::new();
    let mut next = 1;
    for seg in wal::list_segments(wal).unwrap() {
        let bytes = std::fs::read(wal::segment_path(wal, seg)).unwrap();
        let mut i = 0;
        while i < bytes.len() {
            match wal::read_frame(&bytes, i, seg, 0, next) {
                FrameRead::Whole { end, records, .. } => {
                    let recs: Vec<Record> = records.into_iter().map(|(r, _)| r).collect();
                    if let Some(r) = recs.last() {
                        next = r.position + 1;
                    }
                    if recs.first().is_some_and(|r| r.position > after) {
                        out.push(recs);
                    }
                    i = end;
                }
                FrameRead::Partial { .. } => break,
                FrameRead::Wrong(e) => panic!("segment {seg}, offset {i}: {e}"),
            }
        }
    }
    out
}

// ---------------------------------------------------------------- the transcript's lines

fn frame(out: &mut String, recs: &[Record], m: &Mask, trees: &mut Vec<String>) {
    let head: Vec<String> = recs
        .iter()
        .map(|r| {
            let k = kinds::name(r.kind);
            match &r.key {
                Some(key) => format!("{k} {key}"),
                None => k.to_string(),
            }
        })
        .collect();
    out.push_str(&format!("frame: {}\n", head.join(" · ")));
    for r in recs {
        let scope = r
            .scope
            .as_deref()
            .map(|s| format!(" scope={s}"))
            .unwrap_or_default();
        let raw = String::from_utf8_lossy(&r.payload);
        match r.kind {
            kinds::LEDGER => {
                let row: Value = serde_json::from_slice(&r.payload).unwrap();
                if row["kind"] == "turn.trace" {
                    out.push_str(&format!(
                        "  ledger turn.trace{scope} session={} turn={}, the span tree:\n",
                        row["session_id"].as_str().unwrap_or("-"),
                        row["turn_id"].as_str().unwrap_or("-")
                    ));
                    let mut t = String::new();
                    tree(&mut t, &row["data"], 2, m);
                    out.push_str(&t);
                    trees.push(t);
                } else {
                    out.push_str(&format!("  ledger{scope} {}\n", m.json(&raw)));
                }
            }
            kinds::OUTBOX => out.push_str(&format!("  outbox{scope} {}\n", m.json(&raw))),
            _ => {}
        }
    }
}

/// A span and its children, one per line, indented by depth: its name, its
/// kind, and its attributes. Its times are the clock's.
fn tree(out: &mut String, span: &Value, depth: usize, m: &Mask) {
    let open = if span["end_us"].is_null() {
        " (open)"
    } else {
        ""
    };
    out.push_str(&format!(
        "{}{} [{}]{open} {}\n",
        "  ".repeat(depth),
        span["name"].as_str().unwrap_or("?"),
        span["kind"].as_str().unwrap_or("?"),
        m.json(&span["attrs"].to_string())
    ));
    for c in span["children"].as_array().into_iter().flatten() {
        tree(out, c, depth + 1, m);
    }
}

fn notification(out: &mut String, msg: &Message, m: &Mask, trees: &[String]) {
    let Message::Notification(n) = msg else {
        out.push_str(&format!("message {}\n", m.json(&json!(msg).to_string())));
        return;
    };
    let mut params = n.params.clone();
    let trace = params
        .as_object_mut()
        .and_then(|o| o.remove("trace"))
        .filter(|t| !t.is_null());
    out.push_str(&format!(
        "notify {} {}\n",
        n.method,
        m.json(&params.to_string())
    ));
    if let Some(t) = trace {
        let mut drawn = String::new();
        tree(&mut drawn, &t, 2, m);
        if trees.contains(&drawn) {
            out.push_str("  its trace: the span tree drawn above\n");
        } else {
            out.push_str("  its trace:\n");
            out.push_str(&drawn);
        }
    }
}

/// The one line the machine's load decides: a turn says how long it waited
/// for admission only when that took 50 ms or more. Its presence is the
/// load's, which no mask of numbers takes out.
fn by_the_load(text: &str) -> bool {
    text.starts_with("It waited ") && text.ends_with(" for admission and the turn lock.")
}

fn narrative(out: &mut String, msg: &Message, m: &Mask) {
    let Message::Notification(n) = msg else {
        return;
    };
    let l: NarrativeLine = serde_json::from_value(n.params.clone()).unwrap();
    if by_the_load(&l.text) {
        return;
    }
    out.push_str(&format!(
        "narrate {} session={} turn={} {}\n",
        l.part.as_str(),
        l.session_id.as_deref().unwrap_or("-"),
        l.turn_id.as_deref().unwrap_or("-"),
        m.text(&l.text)
    ));
}

// ---------------------------------------------------------------- the masks

/// What the clock and the machine decide, masked.
struct Mask {
    /// The scenario's temporary directories, as `<dir>`.
    dirs: Vec<String>,
}

impl Mask {
    /// JSON text, as written: each string masked as text, and each number as
    /// `#`.
    fn json(&self, raw: &str) -> String {
        let b = raw.as_bytes();
        let mut out = String::with_capacity(raw.len());
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'"' => {
                    let start = i;
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        if b[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    i = (i + 1).min(b.len());
                    out.push_str(&self.text(&raw[start..i]));
                }
                b'-' | b'0'..=b'9' => {
                    while i < b.len()
                        && matches!(b[i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                    {
                        i += 1;
                    }
                    out.push('#');
                }
                c => {
                    out.push(c as char);
                    i += 1;
                }
            }
        }
        out
    }

    /// Text: the directories, the digests, and the durations. Its other
    /// digits are `shapes`'.
    fn text(&self, s: &str) -> String {
        let mut s = s.to_string();
        for d in &self.dirs {
            s = s.replace(d.as_str(), "<dir>");
        }
        durations(&digests(&s))
    }
}

/// Every run of digits left, as `#`, once ids are aliased: a number in a
/// sentence, a key, a date, a time of day, a pid. An alias's own number
/// (`_#3`, `#3`, `…#3`) names the id, and stays.
fn shapes(text: &str) -> String {
    let cs: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < cs.len() {
        if !cs[i].is_ascii_digit() {
            out.push(cs[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < cs.len() && cs[i].is_ascii_digit() {
            i += 1;
        }
        if start > 0 && cs[start - 1] == '#' {
            out.extend(&cs[start..i]);
        } else {
            out.push('#');
        }
    }
    out
}

/// A sha256 in hex: what it digests holds ids and times.
fn digests(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| c.is_ascii_digit() || (b'a'..=b'f').contains(&c);
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        let run = b[i..].iter().take_while(|c| hex(**c)).count();
        if run == 64
            && (i == 0 || !b[i - 1].is_ascii_alphanumeric())
            && b.get(i + 64).is_none_or(|c| !c.is_ascii_alphanumeric())
        {
            out.push_str("<sha256>");
            i += 64;
            continue;
        }
        let n = run.max(1);
        let end = (i + n..=b.len())
            .find(|&e| s.is_char_boundary(e))
            .unwrap_or(b.len());
        out.push_str(&s[i..end]);
        i = end;
    }
    out
}

/// `3 ms`, `2.3 s`, `4 min 12 s`: a sentence's durations are the clock's, and
/// so are their units (a turn that takes a second under load says `s`, not
/// `ms`), so each is `<n>`, unit and all (theseus-6a7o).
fn durations(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    // Where the number at `i` ends, and where its unit ends, if one follows.
    let number = |i: usize| -> (usize, Option<usize>) {
        let mut j = i;
        while j < cs.len() && (cs[j].is_ascii_digit() || cs[j] == '.') {
            j += 1;
        }
        let unit = [" ms", " s", " min"].into_iter().find_map(|u| {
            let n = u.chars().count();
            (cs.get(j..j + n)
                .is_some_and(|w| w.iter().copied().eq(u.chars()))
                && cs.get(j + n).is_none_or(|c| !c.is_alphanumeric()))
            .then_some(j + n)
        });
        (j, unit)
    };
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < cs.len() {
        let starts = cs[i].is_ascii_digit()
            && (i == 0 || !(cs[i - 1].is_alphanumeric() || matches!(cs[i - 1], '.' | '_' | ',')));
        if !starts {
            out.push(cs[i]);
            i += 1;
            continue;
        }
        match number(i) {
            (_, Some(mut end)) => {
                // `4 min 12 s` is one duration.
                while cs.get(end) == Some(&' ') && cs.get(end + 1).is_some_and(char::is_ascii_digit)
                {
                    match number(end + 1) {
                        (_, Some(next)) => end = next,
                        (_, None) => break,
                    }
                }
                out.push_str("<n>");
                i = end;
            }
            (j, None) => {
                out.extend(&cs[i..j]);
                i = j;
            }
        }
    }
    out
}

/// A duration's unit is the clock's too: `950 ms` and `1.2 s` read alike, and
/// so do `59 s` and `1 min 2 s`, so a turn slowed past a second by the load
/// moves no line (theseus-6a7o).
#[test]
fn a_durations_unit_is_masked_with_it() {
    for (a, b) in [
        (
            "Turn ended after 1 loop in 950 ms.",
            "Turn ended after 1 loop in 1.2 s.",
        ),
        (
            "It waited 59 s (2 tries).",
            "It waited 1 min 2 s (2 tries).",
        ),
    ] {
        assert_eq!(durations(a), durations(b), "{a} / {b}");
    }
    assert_eq!(durations("in 950 ms, 12 rows"), "in <n>, 12 rows");
}

/// Every id's uuid tail (32 hex digits after `_`) becomes `#n`, numbered by
/// first appearance. A short id becomes the whole id's number: `…a1b2c3`
/// (`narrative::short`) as `…#n`, and a task's or a wake's `a1b2c3`
/// (`task::short`) as `#n`, when it stands alone and ends an id seen. A
/// number in JSON or after a decimal point is never a short id.
fn alias(text: &str) -> String {
    let b = text.as_bytes();
    let tail = |i: usize| -> Option<&str> {
        let t = b.get(i..i + 33)?;
        (t[0] == b'_'
            && t[1..]
                .iter()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
            && b.get(i + 33).is_none_or(|c| !c.is_ascii_hexdigit()))
        .then(|| &text[i + 1..i + 33])
    };
    let mut seen: Vec<String> = Vec::new();
    for i in 0..b.len() {
        if let Some(id) = tail(i) {
            if !seen.iter().any(|s| s == id) {
                seen.push(id.to_string());
            }
        }
    }
    let number = |id: &str| seen.iter().position(|s| s == id).map(|n| n + 1);
    let hex = |c: &u8| c.is_ascii_digit() || (b'a'..=b'f').contains(c);
    // A task's or a wake's short id: six hex digits standing alone, after
    // a space, a quote, or a parenthesis, that end an id seen.
    let short = |i: usize| -> Option<usize> {
        let s = b.get(i..i + 6)?;
        let alone = i > 0
            && matches!(b[i - 1], b' ' | b'"' | b'(')
            && s.iter().all(hex)
            && b.get(i + 6).is_none_or(|c| !c.is_ascii_alphanumeric());
        let s = std::str::from_utf8(s).ok()?;
        alone
            .then(|| seen.iter().position(|id| id.ends_with(s)))
            .flatten()
            .map(|n| n + 1)
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        if let Some(id) = tail(i) {
            out.push_str(&format!("_#{}", number(id).unwrap()));
            i += 33;
            continue;
        }
        if let Some(n) = short(i) {
            out.push_str(&format!("#{n}"));
            i += 6;
            continue;
        }
        let c = text[i..].chars().next().unwrap();
        if c == '…' {
            let j = i + c.len_utf8();
            let short = b.get(j..j + 6).filter(|s| {
                s.iter()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
                    && b.get(j + 6).is_none_or(|c| !c.is_ascii_hexdigit())
            });
            if let Some(s) = short {
                let s = std::str::from_utf8(s).unwrap();
                match seen.iter().position(|id| id.ends_with(s)) {
                    Some(n) => out.push_str(&format!("…#{}", n + 1)),
                    None => out.push_str("…#?"),
                }
                i = j + 6;
                continue;
            }
        }
        out.push(c);
        i += c.len_utf8();
    }
    out
}
