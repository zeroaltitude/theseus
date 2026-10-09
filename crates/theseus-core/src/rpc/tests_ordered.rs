//! One connection's requests that change a session apply in their arrival
//! order (theseus-klo2): over a real protocol connection, the order of fifty
//! quick inputs, an answer after an input, a question answered on the
//! connection that asked it, a `/stop` while the turn it stops runs, reads
//! while the lane waits, and two connections apart.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{method, Id, Message, Request, Response};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};
use tokio::sync::watch;

use crate::node::{Body, ResultStatus};
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::rpc::Parts;
use crate::store::Store;
use crate::{Config, Core};

/// Long enough for anything that should answer; a lane that holds what it
/// must not hold misses it.
const ANSWERS: Duration = Duration::from_secs(10);

/// A model that answers as `fake` does, except that a request whose
/// messages hold `hold-model` waits until the test lets it go.
struct Gated {
    fake: Arc<FakeProvider>,
    open: watch::Sender<bool>,
}

impl Provider for Gated {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let held = req
                .messages
                .last()
                .is_some_and(|m| m.to_string().contains("hold-model"));
            if held {
                let mut open = self.open.subscribe();
                let _ = open.wait_for(|o| *o).await;
            }
            self.fake.stream_message(req, on_delta).await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    gate: Arc<Gated>,
    root: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

/// A core whose writes wait for the operator (as tests_m3's rig), on the
/// gated model with `script`.
fn rig(script: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Approve;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let gate = Arc::new(Gated {
        fake: Arc::new(FakeProvider::scripted(script)),
        open: watch::channel(false).0,
    });
    let core = Core::build(Parts::for_tests(cfg, gate.clone(), store)).unwrap();
    Rig {
        core,
        gate,
        root,
        _dir: dir,
    }
}

impl Rig {
    fn release(&self) {
        self.gate.open.send_replace(true);
    }

    /// The text of the session's user messages, in the order stored.
    fn inputs(&self, sid: &str) -> Vec<String> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .iter()
            .filter_map(|(_, n)| match &n.body {
                Body::UserMessage { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}

/// One protocol connection to the core, as a client holds it.
struct Wire {
    w: WriteHalf<DuplexStream>,
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    /// What arrived while waiting for something else.
    early: Vec<Message>,
}

impl Wire {
    fn open(core: &Arc<Core>, label: &str) -> Self {
        let (ours, theirs) = tokio::io::duplex(1 << 20);
        let (sr, sw) = tokio::io::split(theirs);
        // The CLI's surface: a private place, whose answers count.
        let client = crate::approval::Client::new(label, crate::approval::Surface::Cli);
        tokio::spawn(core.clone().serve_connection(sr, sw, client));
        let (r, w) = tokio::io::split(ours);
        Self {
            w,
            lines: BufReader::new(r).lines(),
            early: Vec::new(),
        }
    }

    /// Send every request in one write, as a paste's lines leave a client.
    async fn send(&mut self, reqs: &[(u64, &str, Value)]) {
        let mut out = String::new();
        for (id, m, params) in reqs {
            out.push_str(
                &serde_json::to_string(&Request::new(Id::Num(*id), m, params.clone())).unwrap(),
            );
            out.push('\n');
        }
        self.w.write_all(out.as_bytes()).await.unwrap();
    }

    async fn next(&mut self) -> Message {
        let l = self
            .lines
            .next_line()
            .await
            .unwrap()
            .expect("the connection is open");
        serde_json::from_str(&l).unwrap()
    }

    /// The answer to request `id`.
    async fn response(&mut self, id: u64) -> Response {
        let is = |m: &Message| matches!(m, Message::Response(r) if r.id == Id::Num(id));
        if let Some(i) = self.early.iter().position(is) {
            let Message::Response(r) = self.early.remove(i) else {
                unreachable!()
            };
            return r;
        }
        loop {
            let m = self.next().await;
            if is(&m) {
                let Message::Response(r) = m else {
                    unreachable!()
                };
                return r;
            }
            self.early.push(m);
        }
    }

    /// The answer to `id` within `ANSWERS`, or a failure naming `what`.
    async fn answered(&mut self, id: u64, what: &str) -> Response {
        tokio::time::timeout(ANSWERS, self.response(id))
            .await
            .unwrap_or_else(|_| panic!("{what} did not answer"))
    }

    /// The next notification named `name`'s params.
    async fn notified(&mut self, name: &str) -> Value {
        let is = |m: &Message| matches!(m, Message::Notification(n) if n.method == name);
        if let Some(i) = self.early.iter().position(is) {
            let Message::Notification(n) = self.early.remove(i) else {
                unreachable!()
            };
            return n.params;
        }
        loop {
            let m = self.next().await;
            if is(&m) {
                let Message::Notification(n) = m else {
                    unreachable!()
                };
                return n.params;
            }
            self.early.push(m);
        }
    }
}

fn ok(r: &Response) -> &Value {
    assert!(r.error.is_none(), "{:?}", r.error);
    r.result.as_ref().unwrap()
}

async fn new_session(wire: &mut Wire) -> String {
    wire.send(&[(1, method::SESSION_OPEN, json!({}))]).await;
    ok(&wire.answered(1, "session.open").await)["session_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn submit(sid: &str, input: &str) -> Value {
    json!({"session_id": sid, "input": input})
}

fn write_call(file: &str) -> Scripted {
    Scripted::tools(
        "",
        &[("t1", "fs_write", json!({"path": file, "content": "x"}))],
    )
}

/// A paste's lines, sent back to back on one connection, are stored in the
/// order they were sent: the bug stored five as 1, 3, 5, 4, 2.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_inputs_sent_back_to_back_are_stored_in_send_order() {
    let r = rig(vec![]);
    let mut wire = Wire::open(&r.core, "paste");
    let sid = new_session(&mut wire).await;
    let lines: Vec<String> = (0..50).map(|i| format!("line {i:02}")).collect();
    let reqs: Vec<(u64, &str, Value)> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| (10 + i as u64, method::TURN_SUBMIT, submit(&sid, l)))
        .collect();
    wire.send(&reqs).await;
    for i in 0..50 {
        ok(&wire.answered(10 + i, "a submit").await);
    }
    assert_eq!(r.inputs(&sid), lines);
}

/// An answer sent after an input applies after it: the input supersedes the
/// question it would have answered, so the write never runs, whichever task
/// the daemon happened to start first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_answer_sent_after_an_input_applies_after_it() {
    let r = rig(vec![
        write_call("b.txt"),
        Scripted::text("Never mind, then."),
    ]);
    let mut wire = Wire::open(&r.core, "ordered");
    let sid = new_session(&mut wire).await;
    wire.send(&[(2, method::TURN_SUBMIT, submit(&sid, "write b"))])
        .await;
    let asked = ok(&wire.answered(2, "the asking turn").await).clone();
    let corr = asked["awaiting_confirm"]
        .as_str()
        .expect("the write waits")
        .to_string();
    wire.send(&[
        (3, method::TURN_SUBMIT, submit(&sid, "actually, don't")),
        (
            4,
            method::ACTION_CONFIRM,
            json!({"correlation_id": corr, "approve": true}),
        ),
    ])
    .await;
    let answer = wire.answered(4, "the answer").await;
    ok(&wire.answered(3, "the input").await);
    // Refused as an answer to a question no longer asked, never for its place.
    let refused = answer.error.as_ref().expect("the answer applied first");
    assert!(!refused.message.contains("does not count"), "{refused:?}");
    assert!(!r.root.join("b.txt").exists());
    assert_eq!(r.inputs(&sid), ["write b", "actually, don't"]);
}

/// A question answered on the connection that asked it, as soon as the
/// client sees it and before the turn's own answer is read: the answer
/// applies, and the turn runs on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_question_answered_on_its_own_connection_lets_the_turn_run_on() {
    let r = rig(vec![write_call("out.txt"), Scripted::text("Written.")]);
    let mut wire = Wire::open(&r.core, "asker");
    let sid = new_session(&mut wire).await;
    wire.send(&[(2, method::TURN_SUBMIT, submit(&sid, "write out.txt"))])
        .await;
    let asked = wire
        .notified(theseus_protocol::notify::CONFIRM_REQUESTED)
        .await;
    let corr = asked["correlation_id"].as_str().unwrap().to_string();
    wire.send(&[(
        3,
        method::ACTION_CONFIRM,
        json!({"correlation_id": corr, "approve": true}),
    )])
    .await;
    assert_eq!(ok(&wire.answered(3, "the answer").await)["approved"], true);
    let turn = ok(&wire.answered(2, "the asking turn").await).clone();
    let exec = turn["execution_id"].as_str().unwrap();
    // The turn runs on: the answer may have landed before the asking turn
    // parked (its end then wakes the execution), so take the continuation
    // whenever it is ready, until the write has run.
    let ran = async {
        while !r.root.join("out.txt").exists() {
            if let Ok(Some(cont)) = r.core.continue_execution(exec).await {
                assert_eq!(cont.output, "Written.");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    if tokio::time::timeout(ANSWERS, ran).await.is_err() {
        let e = r.core.kernel.execution(exec).unwrap().unwrap();
        let a = r.core.kernel.action(&corr).unwrap().unwrap();
        panic!(
            "the turn did not run on: execution {:?} waking on {:?} (resume {}, queued {:?}); its question {:?}, answered {:?}",
            e.state,
            e.wake,
            e.resume_pending,
            e.queued_results,
            a.state,
            a.confirm.is_some()
        );
    }
    assert_eq!(
        std::fs::read_to_string(r.root.join("out.txt")).unwrap(),
        "x"
    );
    let stored = r.core.store.session_nodes(&sid).unwrap();
    assert!(stored.iter().any(|(_, n)| matches!(
        &n.body,
        Body::ToolResult {
            status: ResultStatus::Ok,
            ..
        }
    )));
}

/// A `/stop` sent after an input applies once the input is stored, never
/// after its whole turn: it reaches the turn while its model call runs, and
/// cuts it. A lane that waited for the turn would wait for the model.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_after_an_input_reaches_its_running_turn() {
    let r = rig(vec![Scripted::text("first"), Scripted::text("too late")]);
    let mut wire = Wire::open(&r.core, "stopper");
    let sid = new_session(&mut wire).await;
    wire.send(&[(2, method::TURN_SUBMIT, submit(&sid, "first"))])
        .await;
    let first = ok(&wire.answered(2, "the first turn").await).clone();
    let exec = first["execution_id"].as_str().unwrap().to_string();
    wire.send(&[
        (3, method::TURN_SUBMIT, submit(&sid, "hold-model, please")),
        (4, method::EXECUTION_STOP, json!({"execution_id": exec})),
    ])
    .await;
    let stop = wire.answered(4, "the stop").await;
    assert_eq!(ok(&stop)["turn_running"], true, "{stop:?}");
    let held = wire.answered(3, "the stopped turn").await;
    r.release();
    let out = held
        .result
        .map_or(String::new(), |v| v["output"].to_string());
    assert!(
        !out.contains("too late"),
        "the model's answer was kept: {out}"
    );
    assert_eq!(r.inputs(&sid), ["first", "hold-model, please"]);
}

/// The fault hook, holding the frame that stores `marker` until `go` is
/// sent: the request that writes it has not taken effect meanwhile.
fn hold_frame(core: &Core, marker: &'static str) -> std::sync::mpsc::Sender<()> {
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let wait = Mutex::new(Some(wait));
    core.store.fail_turn_frame(move |records| {
        let hit = records.iter().any(|r| {
            r.payload
                .windows(marker.len())
                .any(|w| w == marker.as_bytes())
        });
        // Once: the frames after it pass.
        if let Some(wait) = hit.then(|| wait.lock().unwrap().take()).flatten() {
            let _ = wait.recv_timeout(ANSWERS * 3);
        }
        false
    });
    go
}

/// A session with one turn behind it, so a held input frame holds no
/// session record's lock (a first turn writes its target in that frame).
async fn warm_session(wire: &mut Wire) -> String {
    let sid = new_session(wire).await;
    wire.send(&[(90, method::TURN_SUBMIT, submit(&sid, "warm"))])
        .await;
    ok(&wire.answered(90, "the warm turn").await);
    sid
}

/// Reads never wait behind the lane: `health` and `session.list` answer on
/// the connection whose input waits, and so does a change to another
/// session; the input behind the held one waits.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reads_and_other_sessions_answer_while_an_input_waits() {
    let r = rig(vec![]);
    let mut wire = Wire::open(&r.core, "busy");
    let sid = warm_session(&mut wire).await;
    wire.send(&[(2, method::SESSION_OPEN, json!({}))]).await;
    let other = ok(&wire.answered(2, "session.open").await)["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let go = hold_frame(&r.core, "hold-frame");
    wire.send(&[
        (3, method::TURN_SUBMIT, submit(&sid, "hold-frame")),
        (4, method::TURN_SUBMIT, submit(&sid, "behind it")),
        (5, method::HEALTH, json!({})),
        (6, method::SESSION_LIST, json!({})),
        (
            7,
            method::SESSION_RECOMPILE,
            json!({"session_id": other, "strategy": "fresh"}),
        ),
    ])
    .await;
    ok(&wire.answered(5, "health").await);
    ok(&wire.answered(6, "session.list").await);
    ok(&wire.answered(7, "another session's recompile").await);
    assert_eq!(
        r.inputs(&sid),
        ["warm"],
        "nothing more of the held session is stored"
    );
    go.send(()).unwrap();
    ok(&wire.answered(3, "the held input").await);
    ok(&wire.answered(4, "the input behind it").await);
    assert_eq!(r.inputs(&sid), ["warm", "hold-frame", "behind it"]);
}

/// Two connections are not ordered against each other: each has its own
/// lane. A recompile asked on the connection whose input is held waits for
/// it; the same ask on another connection does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_connections_are_not_ordered_against_each_other() {
    let r = rig(vec![]);
    let mut one = Wire::open(&r.core, "one");
    let mut two = Wire::open(&r.core, "two");
    let sid = warm_session(&mut one).await;
    let go = hold_frame(&r.core, "hold-frame");
    let recompile = json!({"session_id": sid, "strategy": "fresh"});
    one.send(&[
        (2, method::TURN_SUBMIT, submit(&sid, "hold-frame")),
        (3, method::SESSION_RECOMPILE, recompile.clone()),
    ])
    .await;
    two.send(&[(1, method::SESSION_RECOMPILE, recompile)]).await;
    ok(&two.answered(1, "the other connection's recompile").await);
    let early = tokio::time::timeout(Duration::from_millis(300), one.response(3)).await;
    assert!(
        early.is_err(),
        "the recompile behind the held input ran first: {early:?}"
    );
    go.send(()).unwrap();
    ok(&one.answered(2, "the held input").await);
    ok(&one.answered(3, "the recompile behind it").await);
}

/// The lane itself: a lone request runs at once, a later one on its session
/// waits for it, one on another session or none does not, and the lane
/// forgets each as it takes effect.
#[tokio::test]
async fn a_lane_waits_only_for_its_sessions_earlier_requests() {
    use super::ordered::Lane;
    let lane = Arc::new(Lane::default());
    let a = lane.enter();
    let b = lane.enter();
    let c = lane.enter();
    let d = lane.enter();
    a.wait(Some("s1".into())).await;
    let soon = Duration::from_millis(50);
    // Not named yet, `b` holds `c`: it may be for `c`'s session.
    assert!(tokio::time::timeout(soon, c.wait(Some("s2".into())))
        .await
        .is_err());
    assert!(tokio::time::timeout(soon, b.wait(Some("s1".into())))
        .await
        .is_err());
    assert!(tokio::time::timeout(soon, c.wait(Some("s2".into())))
        .await
        .is_ok());
    assert!(tokio::time::timeout(soon, d.wait(None)).await.is_ok());
    let mark = a.applied();
    super::ordered::scope(mark, async { super::ordered::applied() }).await;
    assert!(tokio::time::timeout(soon, b.wait(Some("s1".into())))
        .await
        .is_ok());
    drop((a, b, c, d));
    assert_eq!(lane.open(), 0);
}

/// A `/stop` behind two inputs, the first one's turn on a held model and the
/// second waiting for admission behind it: the stop goes ahead of the input
/// that waits, reaches the running turn, and stops the waiting one at its
/// admission. A stop that waited for that input to be stored would wait for
/// the held model (the review's join fix, theseus-klo2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_behind_an_input_waiting_for_admission_reaches_the_running_turn() {
    let r = rig(vec![
        Scripted::text("first"),
        Scripted::text("too late"),
        Scripted::text("never asked"),
    ]);
    let mut wire = Wire::open(&r.core, "stopper");
    let sid = new_session(&mut wire).await;
    wire.send(&[(2, method::TURN_SUBMIT, submit(&sid, "first"))])
        .await;
    let first = ok(&wire.answered(2, "the first turn").await).clone();
    let exec = first["execution_id"].as_str().unwrap().to_string();
    wire.send(&[
        (3, method::TURN_SUBMIT, submit(&sid, "hold-model, please")),
        (4, method::TURN_SUBMIT, submit(&sid, "and this")),
        (5, method::EXECUTION_STOP, json!({"execution_id": exec})),
    ])
    .await;
    let stop = wire
        .answered(5, "the stop behind an input waiting for admission")
        .await;
    assert_eq!(ok(&stop)["turn_running"], true, "{stop:?}");
    let held = wire.answered(3, "the stopped turn").await;
    let behind = wire.answered(4, "the input behind it").await;
    r.release();
    for resp in [held, behind] {
        let out = resp
            .result
            .map_or(String::new(), |v| v["output"].to_string());
        assert!(
            !out.contains("too late"),
            "the model's answer was kept: {out}"
        );
        assert!(!out.contains("never asked"), "the stopped input ran: {out}");
    }
    assert_eq!(r.inputs(&sid), ["first", "hold-model, please", "and this"]);
}

/// An answer behind an input whose frame is held waits for it, whichever
/// task the daemon starts first: the lane's key for `action.confirm` is the
/// action's session. A wrong key lets the answer through at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_answer_waits_for_the_input_before_it_on_its_session() {
    let r = rig(vec![
        Scripted::text("warm"),
        write_call("c.txt"),
        Scripted::text("Fine."),
    ]);
    let mut wire = Wire::open(&r.core, "ordered");
    let sid = warm_session(&mut wire).await;
    wire.send(&[(2, method::TURN_SUBMIT, submit(&sid, "write c"))])
        .await;
    let asked = ok(&wire.answered(2, "the asking turn").await).clone();
    let corr = asked["awaiting_confirm"]
        .as_str()
        .expect("the write waits")
        .to_string();
    let go = hold_frame(&r.core, "hold-frame");
    wire.send(&[
        (3, method::TURN_SUBMIT, submit(&sid, "hold-frame")),
        (
            4,
            method::ACTION_CONFIRM,
            json!({"correlation_id": corr, "approve": true}),
        ),
    ])
    .await;
    let early = tokio::time::timeout(Duration::from_millis(300), wire.response(4)).await;
    assert!(early.is_err(), "the answer ran before the input: {early:?}");
    go.send(()).unwrap();
    ok(&wire.answered(3, "the held input").await);
    assert!(wire.answered(4, "the answer").await.error.is_some());
    assert!(!r.root.join("c.txt").exists());
}

/// The Discord binding speaks for every channel on one connection: a burst
/// on one session keeps its order and answers while another session's turn
/// waits on its model, and that session's own second input waits for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_sessions_on_one_connection_keep_their_own_order_and_neither_waits_for_the_other() {
    let r = rig(vec![]);
    let mut wire = Wire::open(&r.core, "binding");
    let x = warm_session(&mut wire).await;
    wire.send(&[(2, method::SESSION_OPEN, json!({}))]).await;
    let y = ok(&wire.answered(2, "session.open").await)["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let ys: Vec<String> = (1..=5).map(|i| format!("y line {i}")).collect();
    let mut reqs = vec![
        (3, method::TURN_SUBMIT, submit(&x, "hold-model x")),
        (4, method::TURN_SUBMIT, submit(&x, "x two")),
    ];
    for (i, l) in ys.iter().enumerate() {
        reqs.push((10 + i as u64, method::TURN_SUBMIT, submit(&y, l)));
    }
    wire.send(&reqs).await;
    for i in 0..5 {
        ok(&wire.answered(10 + i, "the other session's input").await);
    }
    assert_eq!(r.inputs(&y), ys);
    assert_eq!(r.inputs(&x), ["warm", "hold-model x"]);
    r.release();
    ok(&wire.answered(3, "the held turn").await);
    ok(&wire.answered(4, "its session's next input").await);
    assert_eq!(r.inputs(&x), ["warm", "hold-model x", "x two"]);
}

/// A recompile, a retire and a reopen behind an input whose turn waits on a
/// held model each answer while the model is still held: the lane frees at
/// the input's frame, never at its turn's end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_owners_changes_behind_a_running_input_answer_before_its_model() {
    let r = rig(vec![]);
    let mut wire = Wire::open(&r.core, "owner");
    let sid = warm_session(&mut wire).await;
    wire.send(&[
        (2, method::TURN_SUBMIT, submit(&sid, "hold-model")),
        (
            3,
            method::SESSION_RECOMPILE,
            json!({"session_id": sid, "strategy": "fresh"}),
        ),
        (4, method::SESSION_RETIRE, json!({"session_id": sid})),
        (5, method::SESSION_REOPEN, json!({"session_id": sid})),
    ])
    .await;
    // Each answers, as it may (a refusal is an answer); none waits for the model.
    wire.answered(3, "the recompile").await;
    wire.answered(4, "the retire").await;
    wire.answered(5, "the reopen").await;
    let early = tokio::time::timeout(Duration::from_millis(100), wire.response(2)).await;
    assert!(early.is_err(), "the held turn ended unreleased: {early:?}");
    r.release();
    wire.answered(2, "the held turn").await;
}

/// A request refused before it takes effect frees its place at its answer:
/// the input behind an empty one runs, and is stored.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_request_frees_the_lane_for_the_next() {
    let r = rig(vec![]);
    let mut wire = Wire::open(&r.core, "refused");
    let sid = warm_session(&mut wire).await;
    wire.send(&[
        (2, method::TURN_SUBMIT, submit(&sid, "  ")),
        (3, method::TURN_SUBMIT, submit(&sid, "after it")),
    ])
    .await;
    assert!(wire.answered(2, "the empty input").await.error.is_some());
    ok(&wire.answered(3, "the input behind it").await);
    assert_eq!(r.inputs(&sid), ["warm", "after it"]);
}

/// A connection that closes with requests in its lane leaves none waiting
/// forever: the held input and the one behind it are both stored once the
/// frame goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_connection_closed_mid_lane_leaves_no_request_waiting() {
    let r = rig(vec![]);
    let mut wire = Wire::open(&r.core, "gone");
    let sid = warm_session(&mut wire).await;
    let go = hold_frame(&r.core, "hold-frame");
    wire.send(&[
        (2, method::TURN_SUBMIT, submit(&sid, "hold-frame")),
        (3, method::TURN_SUBMIT, submit(&sid, "behind it")),
    ])
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(wire);
    go.send(()).unwrap();
    let stored = async {
        while r.inputs(&sid).len() < 3 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    assert!(
        tokio::time::timeout(ANSWERS, stored).await.is_ok(),
        "stored: {:?}",
        r.inputs(&sid)
    );
    assert_eq!(r.inputs(&sid), ["warm", "hold-frame", "behind it"]);
}
