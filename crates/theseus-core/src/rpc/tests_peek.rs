//! A running job's output on demand and live (theseus-e1hu4), through the
//! whole core: `work.peek` reads the output file's tail scrubbed; a follow
//! sends the output in order from where it was asked, then its end; a client
//! that falls behind hears `events.lost` and recovers by peeking; a job's
//! `last_line` goes out at most once a second however much it prints; and a
//! connection that reads no private text may read no private job. Jobs run in
//! process (`InlineLauncher`), as `tests_job_handles` runs them, and a task
//! stands in for the daemon's drain.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_protocol::job_follow::{FOLLOW_BEHIND, PEEK_LINES};
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{
    error_code, method, notify, ActionOutputChunk, Id, Message, Notification, Request, Response,
    SessionKind, WorkChanged, WorkPeekResult,
};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};
use tokio::sync::mpsc::unbounded_channel;

use crate::approval::{Client, Surface};
use crate::bus::EventSink;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::push::WorkScope;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// A value only the scrubber knows.
const PLANTED: &str = "kelp-and-brine-7731";

struct Rig {
    core: Arc<Core>,
    drain: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.drain.abort();
    }
}

/// A deployment whose jobs run in process and never block the turn, with a
/// drain every 20 ms and `PLANTED` known to the scrubber.
fn rig(script: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    cfg.places.owner = Some(vec![format!("discord:{}", crate::tests_places::OWNER)]);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let mut p = crate::rpc::Parts::for_tests(cfg, fake, store);
    p.scrubber = Arc::new(crate::scrub::Scrubber::with_values(vec![(
        PLANTED.into(),
        "tide_key".into(),
    )]));
    let core = Core::build(p).unwrap();
    let c = core.clone();
    let drain = tokio::spawn(async move {
        loop {
            c.drain_spool();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    Rig {
        core,
        drain,
        _dir: dir,
    }
}

/// A turn that starts the rig's background job: the job's id and its
/// session.
async fn start_job(r: &Rig) -> (String, String) {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id.clone();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = EventSink::new(r.core.bus.clone(), &sid, None);
    r.core
        .runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some("start it".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            surface: None,
            answer_at: None,
        })
        .await
        .unwrap();
    let a = r
        .core
        .kernel
        .actions_at()
        .unwrap()
        .into_iter()
        .map(|(_, a)| a)
        .find(|a| a.tool == "proc.run" && a.session_id == sid)
        .expect("the job's action");
    (a.correlation_id, sid)
}

/// A rig that starts one job: the model's two answers.
fn rig_with(job: &str) -> Rig {
    let args = json!({"argv": ["sh", "-c", job], "background": true});
    rig(vec![
        Scripted::tools("Starting it.", &[("t1", "proc_run", args)]),
        Scripted::text("Started."),
    ])
}

/// A connection that reads only when told to.
struct Wire {
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    w: WriteHalf<DuplexStream>,
    notes: Vec<Notification>,
    next: u64,
}

impl Wire {
    fn connect(core: &Arc<Core>, surface: Surface, buffer: usize) -> Self {
        let (client, server) = tokio::io::duplex(buffer);
        let (sr, sw) = tokio::io::split(server);
        tokio::spawn(
            core.clone()
                .serve_connection(sr, sw, Client::new("test", surface)),
        );
        let (cr, w) = tokio::io::split(client);
        Self {
            lines: BufReader::new(cr).lines(),
            w,
            notes: Vec::new(),
            next: 1,
        }
    }

    async fn send(&mut self, m: &str, params: Value) -> u64 {
        let id = self.next;
        self.next += 1;
        let mut line = serde_json::to_string(&Request::new(Id::Num(id), m, params)).unwrap();
        line.push('\n');
        self.w.write_all(line.as_bytes()).await.unwrap();
        id
    }

    /// Read until the answer to `id`, keeping the notifications on the way.
    async fn answer(&mut self, id: u64) -> Response {
        let read = async {
            loop {
                let line = self.lines.next_line().await.unwrap().expect("open");
                match serde_json::from_str::<Message>(&line).unwrap() {
                    Message::Response(r) if r.id == Id::Num(id) => return r,
                    Message::Notification(n) => self.notes.push(n),
                    _ => {}
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(30), read)
            .await
            .unwrap_or_else(|_| panic!("no answer to {id} in 30 s"))
    }

    async fn call(&mut self, m: &str, params: Value) -> Response {
        let id = self.send(m, params).await;
        self.answer(id).await
    }

    /// Read notifications until one satisfies `f` (kept with the others).
    async fn until(&mut self, what: &str, f: impl Fn(&Notification) -> bool) -> Notification {
        if let Some(n) = self.notes.iter().find(|n| f(n)) {
            return n.clone();
        }
        let read = async {
            loop {
                let line = self.lines.next_line().await.unwrap().expect("open");
                if let Message::Notification(n) = serde_json::from_str::<Message>(&line).unwrap() {
                    self.notes.push(n.clone());
                    if f(&n) {
                        return n;
                    }
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(30), read)
            .await
            .unwrap_or_else(|_| panic!("no {what} in 30 s"))
    }
}

fn peek_of(r: Response) -> WorkPeekResult {
    serde_json::from_value(r.result.expect("a result")).unwrap()
}

/// Peek until `f` is satisfied, or fail in 20 s.
async fn peek_until(w: &mut Wire, id: &str, f: impl Fn(&WorkPeekResult) -> bool) -> WorkPeekResult {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let p = peek_of(w.call(method::WORK_PEEK, json!({"id": id})).await);
        if f(&p) {
            return p;
        }
        assert!(Instant::now() < until, "no peek satisfied it: {p:?}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

/// A peek of a running job gives its size and its last lines, no more than
/// 20 of them, with what the scrubber knows withheld.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn a_peek_of_a_running_job_returns_its_bytes_and_last_lines_scrubbed() {
    let r = rig_with(&format!(
        "i=0; while [ $i -lt 10 ]; do echo \"row $i\"; i=$((i+1)); done; \
         echo 'leak {PLANTED} here'; while [ $i -lt 24 ]; do echo \"row $i\"; i=$((i+1)); done; \
         sleep 30"
    ));
    let (job, _) = start_job(&r).await;
    let mut cli = Wire::connect(&r.core, Surface::Cli, 1 << 20);
    let p = peek_until(&mut cli, &job, |p| {
        p.lines.last().is_some_and(|l| l == "row 23")
    })
    .await;
    assert_eq!(p.id, job);
    assert!(!p.complete, "it still runs");
    assert_eq!(
        p.lines.len(),
        PEEK_LINES,
        "the last twenty of twenty-five lines"
    );
    assert_eq!(p.lines[0], "row 5");
    assert!(
        p.lines.iter().any(|l| l.starts_with("leak ")),
        "{:?}",
        p.lines
    );
    let words = p.lines.join("\n");
    assert!(!words.contains(PLANTED), "the scrubber ran: {words}");
    let on_disk = std::fs::metadata(r.core.spool.result_path(&job))
        .unwrap()
        .len();
    assert_eq!(p.bytes, on_disk, "its size is the file's");
    assert!(p.started_at_ms.is_some());
    // No such job, and a job another id names, are not found.
    let none = cli
        .call(method::WORK_PEEK, json!({"id": "act_nothing"}))
        .await;
    assert_eq!(none.error.unwrap().code, error_code::NOT_FOUND);
}

/// A follow from the start sends the job's output in order, each chunk at its
/// own offset, and then its end with the outcome.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn a_follow_sends_the_output_in_order_then_the_end() {
    let r = rig_with("i=1; while [ $i -le 12 ]; do echo \"leg $i\"; i=$((i+1)); sleep 0.05; done");
    let (job, _) = start_job(&r).await;
    let mut cli = Wire::connect(&r.core, Surface::Cli, 1 << 20);
    let a = cli
        .call(method::ACTION_OUTPUT, json!({"id": job, "from": 0}))
        .await;
    assert_eq!(a.result.as_ref().unwrap()["offset"], 0, "{a:?}");
    let end = cli
        .until("the end", |n| n.method == notify::ACTION_OUTPUT_END)
        .await;
    assert_eq!(end.params["id"], job.as_str());
    assert_eq!(end.params["outcome"], "succeeded");
    let mut at = 0u64;
    let mut text = String::new();
    for n in &cli.notes {
        if n.method != notify::ACTION_OUTPUT_CHUNK {
            continue;
        }
        let c: ActionOutputChunk = serde_json::from_value(n.params.clone()).unwrap();
        assert_eq!(c.offset, at, "a chunk begins where the last one ended");
        at += c.text.len() as u64;
        text.push_str(&c.text);
    }
    let want: String = (1..=12).map(|i| format!("leg {i}\n")).collect();
    assert_eq!(text, want);
    // Its follower is gone with its end.
    for _ in 0..100 {
        if r.core.bus.followers() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(r.core.bus.followers(), 0);
}

/// `action.unfollow` ends a follow, and the connection's end does too.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn an_unfollow_and_a_closed_connection_end_a_follower() {
    let r = rig_with("echo begun; sleep 20");
    let (job, _) = start_job(&r).await;
    let mut a = Wire::connect(&r.core, Surface::Cli, 1 << 20);
    a.call(method::ACTION_OUTPUT, json!({"id": job, "from": 0}))
        .await;
    assert_eq!(r.core.bus.followers(), 1);
    let off = a.call(method::ACTION_UNFOLLOW, json!({"id": job})).await;
    assert_eq!(off.result.unwrap()["was_following"], true);
    assert_eq!(r.core.bus.followers(), 0);
    let again = a.call(method::ACTION_UNFOLLOW, json!({"id": job})).await;
    assert_eq!(again.result.unwrap()["was_following"], false);
    let mut b = Wire::connect(&r.core, Surface::Cli, 1 << 20);
    b.call(method::ACTION_OUTPUT, json!({"id": job, "from": 0}))
        .await;
    assert_eq!(r.core.bus.followers(), 1);
    drop(b);
    for _ in 0..200 {
        if r.core.bus.followers() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        r.core.bus.followers(),
        0,
        "a closed connection follows nothing"
    );
}

/// A client that stops reading is told what it lost (`events.lost` naming the
/// job's stream), and recovers by peeking: the peek's size is where it
/// follows from, the follow's answer reports an offset at or past that size,
/// and a chunk arrives beginning exactly at that offset. What the client hears
/// *before* its peek's end is not checked here, and is not promised: the
/// product lets a client drop a chunk that begins before it. `cap` lowers the
/// connection's own backlog cap (the queue then drops what it cannot hold);
/// with none, the follower's own limit (`ROOM` messages) is what holds.
async fn a_slow_client_recovers(job: &str, cap: Option<usize>) {
    let r = rig_with(job);
    let (job, _) = start_job(&r).await;
    if let Some(cap) = cap {
        r.core
            .push
            .backlog_cap
            .store(cap, std::sync::atomic::Ordering::Relaxed);
    }
    let mut slow = Wire::connect(&r.core, Surface::Cli, 2048);
    slow.call(method::ACTION_OUTPUT, json!({"id": job, "from": 0}))
        .await;
    // It reads nothing while the job prints, until the daemon has counted
    // what it could not queue for it (a loaded machine prints slowly, so
    // this waits for the loss, not for a time).
    let until = Instant::now() + Duration::from_secs(40);
    while r.core.push.status(0).lost == 0 {
        assert!(Instant::now() < until, "nothing was lost in 40 s");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let stream = format!("output:{job}");
    let lost = slow
        .until("events.lost", |n| n.method == notify::EVENTS_LOST)
        .await;
    assert!(
        lost.params["streams"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == stream.as_str()),
        "{:?}",
        lost.params
    );
    assert!(lost.params["dropped"].as_u64().unwrap() > 0);
    // It peeks, and follows on from what the peek says.
    let p = peek_until(&mut slow, &job, |p| p.bytes > 0).await;
    let follow = slow
        .call(method::ACTION_OUTPUT, json!({"id": job, "from": p.bytes}))
        .await;
    let from = follow.result.unwrap()["offset"].as_u64().unwrap();
    assert!(from >= p.bytes, "{from} < {}", p.bytes);
    // The old follower is ended by the new follow, but an abort takes effect
    // at its next await: a chunk it was already sending can land after the
    // answer, so no clearing of the notes separates the two followers by
    // time. What is told apart is instead the chunk at the answer's own
    // offset. No stale chunk can carry it: every chunk the old follower
    // queued began strictly before the length the peek observed. That chunk
    // is the new follower's in every ordinary interleaving, though not
    // necessarily -- if the old follower had skipped to EOF (`ROOM`) at
    // exactly the peek's end, its next chunk begins there too. It carries the
    // same bytes either way, so the property holds whichever sent it.
    // Waiting for it cannot pass with no chunk: `until` fails when none comes.
    slow.until("the new follower's first chunk", |n| {
        n.method == notify::ACTION_OUTPUT_CHUNK && n.params["offset"].as_u64() == Some(from)
    })
    .await;
}

const CHATTY: &str =
    "i=0; while [ $i -lt 4000 ]; do echo \"line $i of a long and chatty job output\"; \
     i=$((i+1)); [ $((i % 40)) -eq 0 ] && sleep 0.01; done; \
     j=0; while [ $j -lt 300 ]; do echo tick; sleep 0.1; j=$((j+1)); done";

/// The connection's own cap drops what a stuck client cannot hold.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn a_slow_client_hears_events_lost_and_recovers_by_a_peek() {
    a_slow_client_recovers(CHATTY, Some(3)).await;
}

/// A follower stops queueing for a client whose queue holds `ROOM` messages
/// (the real cap, 4,096, is far off): it counts the stream as lost and skips
/// ahead, so a stuck client costs the daemon a bounded queue.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn a_follower_does_not_fill_a_stuck_clients_queue() {
    a_slow_client_recovers(
        "yes 'a line that repeats for the size' | head -c 8000000; \
         j=0; while [ $j -lt 300 ]; do echo tick; sleep 0.1; j=$((j+1)); done",
        None,
    )
    .await;
}

/// A follow never runs more than `FOLLOW_BEHIND` behind: asked from 0 of a
/// longer output, it starts at the newest 4 MiB, and says where.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn a_follow_from_far_behind_skips_to_the_newest_four_mib() {
    let r = rig_with("yes 'a line that repeats for the size' | head -c 6000000; sleep 30");
    let (job, _) = start_job(&r).await;
    let mut cli = Wire::connect(&r.core, Surface::Cli, 1 << 20);
    let p = peek_until(&mut cli, &job, |p| p.bytes >= 6_000_000).await;
    let a = cli
        .call(method::ACTION_OUTPUT, json!({"id": job, "from": 0}))
        .await;
    let offset = a.result.unwrap()["offset"].as_u64().unwrap();
    assert_eq!(offset, p.bytes - FOLLOW_BEHIND);
}

/// A job's `last_line` is sampled, scrubbed, and said at most once a second
/// however much it prints (10,000 lines a second here): no two samples are
/// less than 900 ms apart, and none holds what the scrubber knows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn last_line_goes_out_at_most_once_a_second_under_a_chatty_job() {
    let r = rig_with(&format!(
        "end=$(( $(date +%s) + 4 )); i=0; while [ $(date +%s) -lt $end ]; do \
         j=0; while [ $j -lt 500 ]; do echo \"chatty $i {PLANTED}\"; j=$((j+1)); i=$((i+1)); done; \
         done"
    ));
    r.core.sample_last_lines_after_serving();
    r.core.push.ensure(&r.core).await.unwrap();
    let (tx, mut rx) = unbounded_channel();
    r.core.bus.watch_work("w", tx, WorkScope::default(), false);
    let (job, _) = start_job(&r).await;
    let mut seen: Vec<(u64, String)> = Vec::new();
    let mut frames_with_a_job = 0;
    let until = Instant::now() + Duration::from_secs(7);
    while Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(50)).await;
        while let Ok(m) = rx.try_recv() {
            let Message::Notification(n) = m else {
                continue;
            };
            if n.method != notify::WORK_CHANGED {
                continue;
            }
            let c: WorkChanged = serde_json::from_value(n.params).unwrap();
            for v in c.views.into_iter().filter(|v| v.id == job) {
                frames_with_a_job += 1;
                if let Some(l) = v.doing.and_then(|d| d.last_line) {
                    seen.push((v.at_ms, l));
                }
            }
        }
    }
    assert!(frames_with_a_job > 0);
    // A sample goes out only when the line changed, so each sample is a new line.
    // Any other frame that touches the job's view (the warm-up's, which lands when
    // the machine lets it) sends the line it holds again with that frame's time
    // (`settle`'s stale views): the same line, not another sample (theseus-1b2zm).
    seen.dedup_by(|later, first| later.1 == first.1);
    assert!(seen.len() >= 2, "it was sampled while it ran: {seen:?}");
    assert!(
        seen.len() <= 6,
        "about one a second for four seconds: {seen:?}"
    );
    for w in seen.windows(2) {
        assert!(
            w[1].0 >= w[0].0 + 900,
            "two samples {} ms apart: {seen:?}",
            w[1].0 - w[0].0
        );
    }
    assert!(seen.iter().all(|(_, l)| !l.contains(PLANTED)), "{seen:?}");
    assert!(
        seen.iter().all(|(_, l)| l.starts_with("chatty ")),
        "{seen:?}"
    );
}

/// A surface that reads no private text is told no job exists for a private
/// session's, to peek and to follow; the owner's surface reads it. A guarded
/// scope hands over no `last_line`, and a job of a shared place's session is
/// never sampled.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn a_shared_place_reads_no_private_job_and_gets_no_last_line() {
    use crate::places::BoundPlace;
    let r = rig_with("echo private work; sleep 20");
    let (job, _) = start_job(&r).await;
    let mut cli = Wire::connect(&r.core, Surface::Cli, 1 << 20);
    peek_until(&mut cli, &job, |p| p.bytes > 0).await;
    for surface in [Surface::Discord, Surface::Unnamed] {
        let mut shared = Wire::connect(&r.core, surface, 1 << 20);
        let p = shared.call(method::WORK_PEEK, json!({"id": job})).await;
        assert_eq!(p.error.expect("refused").code, error_code::NOT_FOUND);
        let f = shared.call(method::ACTION_OUTPUT, json!({"id": job})).await;
        assert_eq!(f.error.expect("refused").code, error_code::NOT_FOUND);
    }
    // The MCP server's own allow-list never offers either method.
    let mut mcp = Wire::connect(&r.core, Surface::Mcp, 1 << 20);
    let p = mcp.call(method::WORK_PEEK, json!({"id": job})).await;
    assert!(p.error.is_some(), "{p:?}");
    assert_eq!(r.core.bus.followers(), 0, "nothing followed");

    // A job a shared place's session is said to run is never sampled.
    r.core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{}", crate::tests_places::LAB),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    let shared_sid = crate::tests_places::session(
        &r.core,
        Some(&format!("channel:{}", crate::tests_places::LAB)),
    );
    r.core.push.ensure(&r.core).await.unwrap();
    let cache = Default::default();
    r.core
        .sample_last_lines(&[(job.clone(), shared_sid)], &cache);
    let held = |core: &Core| -> Option<WorkView> {
        let (_, views, _) = core.push.work_snapshot(200, &WorkScope::default());
        views.into_iter().find(|v| v.id == job)
    };
    let said = held(&r.core).and_then(|v| v.doing?.last_line);
    assert_eq!(said, None, "a shared place's job has no last line");

    // The guarded scope drops it from a view that has one.
    let mut v: WorkView = serde_json::from_value(json!({
        "id": job, "kind": "job", "position": 5, "at_ms": 9, "root": "exe_1", "title": "x",
        "state": "running", "attention": {"level": "working", "label": "x", "since_ms": 1},
        "started_at_ms": 1, "state_since_ms": 1,
        "doing": {"what": "job", "label": "x", "since_ms": 1, "turn": 1, "loop_index": 0,
                  "last_line": "leg 9"}
    }))
    .unwrap();
    assert_eq!(v.kind, WorkKind::Job);
    let scrubber = crate::scrub::Scrubber::default();
    let guarded = WorkScope {
        roots: None,
        guarded: true,
    };
    assert_eq!(guarded.show(&v, &scrubber).doing.unwrap().last_line, None);
    assert_eq!(
        WorkScope::default()
            .show(&v, &scrubber)
            .doing
            .unwrap()
            .last_line
            .as_deref(),
        Some("leg 9")
    );
    v.doing = None;
}

fn stop_of(r: Response) -> theseus_protocol::JobStopResult {
    serde_json::from_value(r.result.expect("a result")).unwrap()
}

/// `job.stop` stops a running job as the model's `job_stop` does: the action
/// settles, the answer says it ended, and a second call says it had ended
/// already. An id no job has is not found.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn job_stop_stops_a_running_job_and_a_second_call_says_it_had_ended() {
    let r = rig_with("echo up; sleep 60");
    let (job, _) = start_job(&r).await;
    let mut cli = Wire::connect(&r.core, Surface::Cli, 1 << 20);
    peek_until(&mut cli, &job, |p| p.bytes > 0).await;
    let first = stop_of(cli.call(method::JOB_STOP, json!({"id": job})).await);
    assert_eq!(first.id, job);
    assert!(first.ended, "{first:?}");
    assert!(first.words.contains(&job), "{first:?}");
    let a = r.core.kernel.action(&job).unwrap().unwrap();
    assert!(a.state.is_settled(), "the action settled: {:?}", a.state);
    let again = stop_of(cli.call(method::JOB_STOP, json!({"id": job})).await);
    assert!(again.ended && !again.verified, "{again:?}");
    assert!(again.words.contains("already ended"), "{again:?}");
    let none = cli
        .call(method::JOB_STOP, json!({"id": "act_nothing"}))
        .await;
    assert_eq!(none.error.unwrap().code, error_code::NOT_FOUND);
}

/// A connection that reads no private text is told no job exists for a
/// private session's, as `work.peek` does, and the job runs on.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn job_stop_obeys_the_place_rule() {
    let r = rig_with("echo private work; sleep 60");
    let (job, _) = start_job(&r).await;
    let mut cli = Wire::connect(&r.core, Surface::Cli, 1 << 20);
    peek_until(&mut cli, &job, |p| p.bytes > 0).await;
    // The owner's own Discord account is trusted, but Discord reads no private
    // text, so the place rule still hides a private session's job from it.
    // (An untrusted surface is refused first: `untrusted_channels_may_not_stop_a_job`.)
    let mut shared = Wire::connect(&r.core, Surface::Discord, 1 << 20);
    let s = shared
        .call(
            method::JOB_STOP,
            json!({"id": job, "discord": owner_in("1", Some("2"))}),
        )
        .await;
    assert_eq!(s.error.expect("refused").code, error_code::NOT_FOUND);
    let a = r.core.kernel.action(&job).unwrap().unwrap();
    assert!(!a.state.is_settled(), "still running: {:?}", a.state);
    stop_of(cli.call(method::JOB_STOP, json!({"id": job})).await);
}

/// The owner's Discord account, speaking in `channel` of guild `guild`.
fn owner_in(channel: &str, guild: Option<&str>) -> Value {
    json!({"user_id": crate::tests_places::OWNER.to_string(), "channel_id": channel,
           "guild_id": guild})
}

/// A job in a session of the shared place `#lab`.
async fn shared_job(r: &Rig) -> String {
    r.core.bind_places(vec![crate::places::BoundPlace {
        target: format!("discord:channel:{}", crate::tests_places::LAB),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    // A shared place's own turn may not run a program, so the job starts in a
    // private session, which then speaks in the shared place.
    let (job, sid) = start_job(r).await;
    let place = format!("channel:{}", crate::tests_places::LAB);
    r.core.outbox.bind_place(&place, &sid).unwrap();
    job
}

/// A channel Theseus does not trust as the owner's may not stop a job, even
/// one in a shared place that it may read: an MCP client, a connection no
/// listener named, Discord naming nobody, and a Discord user who is not an
/// owner are each refused, with why, and the job runs on. One refusal is one
/// `approval.refused` row naming the act.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn untrusted_channels_may_not_stop_a_job() {
    let r = rig_with("echo lab work; sleep 60");
    let job = shared_job(&r).await;
    let stranger = json!({"user_id": crate::tests_places::ALICE.to_string(),
                          "channel_id": "1", "guild_id": "2"});
    let cases = [
        (Surface::Unnamed, json!({"id": job})),
        (Surface::Discord, json!({"id": job})),
        (Surface::Discord, json!({"id": job, "discord": stranger})),
    ];
    for (n, (surface, params)) in cases.into_iter().enumerate() {
        let mut w = Wire::connect(&r.core, surface, 1 << 20);
        let e = w
            .call(method::JOB_STOP, params)
            .await
            .error
            .expect("refused");
        assert_eq!(e.code, error_code::REFUSED, "{surface:?}: {}", e.message);
        assert!(e.message.contains("The job runs on."), "{}", e.message);
        let why = e.data["why"].as_str().unwrap_or("");
        assert!(!why.is_empty(), "{surface:?}: {e:?}");
        let a = r.core.kernel.action(&job).unwrap().unwrap();
        assert!(
            !a.state.is_settled(),
            "{surface:?} stopped it: {:?}",
            a.state
        );
        let refused = crate::test_support::rows(&r.core, "approval.refused");
        assert_eq!(refused.len(), n + 1, "one row a refusal: {refused:?}");
        assert_eq!(refused[n]["act"], method::JOB_STOP);
        assert_eq!(refused[n]["job"], job.as_str());
    }
    // A refusal tells nothing of which jobs exist.
    let mut w = Wire::connect(&r.core, Surface::Unnamed, 1 << 20);
    let e = w.call(method::JOB_STOP, json!({"id": "act_nothing"})).await;
    assert_eq!(e.error.unwrap().code, error_code::REFUSED);
    // The MCP server's own allow-list never offers the method at all.
    let mut mcp = Wire::connect(&r.core, Surface::Mcp, 1 << 20);
    let e = mcp.call(method::JOB_STOP, json!({"id": job})).await;
    assert!(e.error.is_some(), "{e:?}");
    let a = r.core.kernel.action(&job).unwrap().unwrap();
    assert!(!a.state.is_settled(), "{:?}", a.state);
}

/// A trusted channel stops a job: the owner's own Discord account in a shared
/// channel (the job's place), and the CLI a job of a private session.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn a_trusted_channel_stops_a_job() {
    let r = rig_with("echo lab work; sleep 60");
    let job = shared_job(&r).await;
    let mut d = Wire::connect(&r.core, Surface::Discord, 1 << 20);
    let params = json!({"id": job, "discord": owner_in("1", Some("2"))});
    let s = stop_of(d.call(method::JOB_STOP, params).await);
    assert!(s.ended, "{s:?}");
    let a = r.core.kernel.action(&job).unwrap().unwrap();
    assert!(a.state.is_settled(), "{:?}", a.state);
    assert!(crate::test_support::rows(&r.core, "approval.refused").is_empty());

    let r = rig_with("echo private work; sleep 60");
    let (job, _) = start_job(&r).await;
    let mut cli = Wire::connect(&r.core, Surface::Cli, 1 << 20);
    peek_until(&mut cli, &job, |p| p.bytes > 0).await;
    let s = stop_of(cli.call(method::JOB_STOP, json!({"id": job})).await);
    assert!(s.ended, "{s:?}");
}
