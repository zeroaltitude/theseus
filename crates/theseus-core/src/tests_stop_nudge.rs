//! The stop nudge (theseus-fh0u; the owner's D8), through a whole core
//! against the fake Jev: a turn whose last answer announced work still to
//! come (`loop.v1`'s `announced_unfinished` act-true, live) goes on once,
//! after one line in the harness's voice; a second stop ends it. No nudge
//! for a verdict to ask or below the act band, in a shared place, with the
//! switch off (the daemon's or the session's), past the wait, or while a job
//! of the turn runs.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::nudge::SessionNudgeParams;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::node::{Body, Origin};
use crate::policy::Posture;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::rpc::{Core, Parts};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_judge::{board, judged, until_judged, warm};
use crate::turn::nudge_step::{NUDGE_AUTHOR, NUDGE_TEXT};
use crate::turn::TurnRequest;
use crate::Config;

/// The owner's Discord id, and someone else's.
const OWNER: u64 = 271_828_182_845_904_523;
const SOMEONE: u64 = 222_222_222_222_222_222;

const SAID: &str = "I read the module. Next I'll run the tests.";
const RAN: &str = "Ran them: 41 passed.";

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

/// A core with the judge on against `jev`, `loop.v1` judging alone, the
/// stop nudge on (the default), and `proc.run` in process with a notice;
/// then `tweak`.
fn rig(script: Vec<Scripted>, jev: &FakeJev, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = crate::scratch::tempdir();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.policy.tools.remove("proc.run");
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.judge.enabled = true;
    cfg.judge.api_base = jev.base();
    cfg.judge.connect_secs = 1;
    cfg.judge.total_secs = 3;
    for p in crate::judge::inbound::PACKS {
        cfg.judge.packs.insert(p.into(), crate::tests_judge::off());
    }
    assert!(cfg.judge.stop_nudge, "on by default");
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let mut p = Parts::for_tests(cfg, fake.clone(), store);
    p.secrets = board();
    let core = Core::build(p).unwrap();
    warm(&core);
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

/// A Jev sure the last message announced work still to come.
fn unfinished(p: f64) -> FakeJev {
    let jev = FakeJev::start().unwrap();
    jev.script("announced_unfinished", Jev::Noul(p));
    jev
}

fn script() -> Vec<Scripted> {
    vec![
        Scripted::text(SAID),
        Scripted::text(RAN),
        Scripted::text("A third answer no turn asks for."),
    ]
}

impl Rig {
    /// A session, posting to `place` (`dm:<user>`) when given.
    fn session(&self, place: Option<&str>) -> String {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        if let Some(p) = place {
            self.core.outbox.bind_place(p, &r.session_id).unwrap();
        }
        r.session_id
    }

    async fn turn(&self, sid: &str, input: &str) -> TurnSubmitResult {
        let rec = self
            .core
            .store
            .get_session::<SessionRecord>(sid)
            .unwrap()
            .unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        let sink = EventSink::new(self.core.bus.clone(), sid, None);
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                surface: None,
                session: rec,
                input: Some(input.into()),
                target,
                sink,
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
                answer_at: None,
            })
            .await
            .unwrap()
    }

    fn requests(&self) -> Vec<ProviderRequest> {
        self.fake.requests()
    }

    fn rows(&self, kind: &str) -> Vec<LedgerRow> {
        self.core
            .store
            .ledger_tail::<LedgerRow>(10_000)
            .unwrap()
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.kind == kind)
            .collect()
    }

    /// The session's harness nodes that are nudges.
    fn nudge_nodes(&self, sid: &str) -> Vec<crate::node::Node> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .map(|(_, n)| n)
            .filter(|n| n.author.as_deref() == Some(NUDGE_AUTHOR))
            .collect()
    }
}

/// The text of a request's last user message.
fn last_user(req: &ProviderRequest) -> String {
    let m = req
        .messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .expect("a user message");
    serde_json::to_string(&m["content"]).unwrap()
}

/// Wait out the judgments a turn's end spawns, then say how many there are.
async fn settled_judgments(store: &Store, at_least: usize) -> Vec<Value> {
    until_judged(store, at_least).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    judged(store).into_iter().map(|(_, r)| r.data).collect()
}

/// How long the turn's stop waited for the verdict: its `judge` `wait` span.
fn wait_span_ms(span: &theseus_protocol::Span) -> Option<u64> {
    if span.name == "judge" && span.kind == "wait" {
        return Some(span.end_us?.saturating_sub(span.start_us) / 1_000);
    }
    span.children.iter().find_map(wait_span_ms)
}

/// An announced stop: one nudge, one more model call in the same turn, its
/// line the harness's (a harness node, never the person's), its row naming
/// the judgment, and a second stop that ends the turn. The stop judged live
/// is not judged again after the turn; the second is, in shadow.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_announced_stop_is_nudged_once_and_a_second_stop_ends_the_turn() {
    let jev = unfinished(0.97);
    let r = rig(script(), &jev, |_| {});
    let sid = r.session(None);
    let res = r
        .turn(&sid, "Check the parser module and run its tests.")
        .await;
    assert_eq!(
        r.requests().len(),
        2,
        "one nudge, one more call, and no third"
    );
    assert_eq!((res.loops, res.stop_reason.as_str()), (2, "no_tool_calls"));
    assert!(
        res.output.contains(SAID) && res.output.contains(RAN),
        "{}",
        res.output
    );
    let n = res.nudged.clone().expect("nudged");
    assert_eq!(
        (n.by.as_str(), n.loop_index, n.acted),
        ("announced_unfinished", 0, false)
    );
    assert!((n.p - 0.97).abs() < 1e-9, "{}", n.p);
    // The model read the harness's line as the last thing before its second answer.
    let second = &r.requests()[1];
    assert!(
        last_user(second).contains(NUDGE_TEXT),
        "{}",
        last_user(second)
    );
    assert!(!last_user(&r.requests()[0]).contains(NUDGE_TEXT));
    // One harness node, and its row, naming the judgment.
    let nodes = r.nudge_nodes(&sid);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].origin, Origin::Harness);
    assert!(matches!(&nodes[0].body, Body::UserMessage { text, .. } if text == NUDGE_TEXT));
    let rows = r.rows("turn.nudged");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].data["judgment"], n.judgment);
    assert_eq!(rows[0].data["by"], "announced_unfinished");
    assert_eq!(rows[0].turn_id.as_deref(), Some(res.turn_id.as_str()));
    // The turn's end row carries it, for the prove.
    let ended = r.rows("turn.ended");
    assert_eq!(ended.last().unwrap().data["nudged"]["judgment"], n.judgment);
    // Two judgments: the stop's, live and taken; the second stop's after the turn, in shadow.
    let js = settled_judgments(&r.core.store, 2).await;
    assert_eq!(js.len(), 2, "{js:#?}");
    let live = js
        .iter()
        .find(|j| j["id"] == n.judgment)
        .expect("the live one");
    assert_eq!(live["mode"], "live");
    assert_eq!(live["context"]["stop"]["taken"], true);
    assert_eq!(live["context"]["stop"]["acts"], true);
    let after = js.iter().find(|j| j["id"] != n.judgment).unwrap();
    assert_eq!(after["mode"], "shadow");
    // Every surface's line.
    assert_eq!(
        res.harness_lines(),
        ["Nudged once: the reply said more work would follow; the model answered without acting."]
    );
}

/// A nudge the model answers with a tool call says `acted`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_nudge_the_model_acts_on_says_so() {
    let jev = unfinished(0.97);
    let run = json!({"argv": ["true"]});
    let r = rig(
        vec![
            Scripted::text(SAID),
            Scripted::tools("Running them.", &[("t1", "proc_run", run)]),
            Scripted::text(RAN),
        ],
        &jev,
        |_| {},
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Run the tests.").await;
    assert_eq!(r.requests().len(), 3);
    assert_eq!((res.loops, res.tool_calls), (3, 1));
    assert!(res.nudged.expect("nudged").acted);
}

/// A turn the stop nudge continued is not told by the end note too, though it
/// stops beside the files its command made: one harness line a turn
/// (theseus-53ujg.1). Its facts still reach the result.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_end_note_after_the_stop_nudge_continued() {
    let jev = unfinished(0.97);
    let r = rig(
        vec![
            Scripted::tools(
                "Writing.",
                &[(
                    "w1",
                    "fs_write",
                    json!({"path": "main.rs", "content": "fn main() {}"}),
                )],
            ),
            Scripted::tools(
                "Building.",
                &[(
                    "r1",
                    "proc_run",
                    json!({"argv": ["sh", "-c", "echo x > main"]}),
                )],
            ),
            Scripted::text(SAID),
            Scripted::text(RAN),
            Scripted::text("A fifth answer no turn asks for."),
        ],
        &jev,
        |_| {},
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Write main.rs and build it.").await;
    assert_eq!(
        r.requests().len(),
        4,
        "the nudge's one more call, and no end note's"
    );
    assert!(res.nudged.is_some());
    assert!(r.rows("turn.end_noted").is_empty());
    let files = res.files.expect("the facts stay");
    assert!(
        files.end_note.is_none() && files.left.paths.len() == 1,
        "{files:?}"
    );
}

/// A verdict to ask (between confirm and act), and a lean below it, nudge
/// nothing; the stop is judged once, live, and not again after the turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verdict_to_ask_or_below_the_act_band_nudges_nothing() {
    for p in [0.75, 0.2] {
        let jev = unfinished(p);
        let r = rig(script(), &jev, |_| {});
        let sid = r.session(None);
        let res = r.turn(&sid, "Run the tests.").await;
        assert_eq!((r.requests().len(), res.loops), (1, 1), "p {p}");
        assert!(res.nudged.is_none());
        assert!(r.rows("turn.nudged").is_empty() && r.nudge_nodes(&sid).is_empty());
        let js = settled_judgments(&r.core.store, 1).await;
        assert_eq!(js.len(), 1, "judged once: p {p}");
        assert_eq!(js[0]["mode"], "live");
        assert_eq!(js[0]["context"]["stop"]["acts"], false);
    }
}

/// A shared place is never nudged, and nothing waits on Jev there: its stop
/// is judged after the turn, in shadow. The owner's DM is private: nudged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_place_is_never_nudged_and_the_owners_dm_is() {
    let jev = unfinished(0.97);
    let r = rig(script(), &jev, |_| {});
    let shared = r.session(Some(&format!("dm:{SOMEONE}")));
    assert_eq!(
        r.core.runner.class_of(&shared),
        crate::places::PlaceClass::Shared
    );
    let res = r.turn(&shared, "Run the tests.").await;
    assert_eq!((r.requests().len(), res.loops), (1, 1));
    assert!(res.nudged.is_none() && r.rows("turn.nudged").is_empty());
    let js = settled_judgments(&r.core.store, 1).await;
    assert_eq!((js.len(), js[0]["mode"].as_str()), (1, Some("shadow")));
    let owners = r.session(Some(&format!("dm:{OWNER}")));
    let res = r.turn(&owners, "Run the tests.").await;
    assert!(res.nudged.is_some(), "the owner's DM is private");
}

/// The switch off: the daemon's (`[judge] stop_nudge = false`, which holds
/// loop.v1 in shadow), or the session's own (`session.nudge`), and back on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_switch_off_nudges_nothing_the_daemons_or_the_sessions() {
    let jev = unfinished(0.97);
    let r = rig(script(), &jev, |c| c.judge.stop_nudge = false);
    let sid = r.session(None);
    let res = r.turn(&sid, "Run the tests.").await;
    assert_eq!((r.requests().len(), res.loops), (1, 1));
    assert!(res.nudged.is_none());
    let js = settled_judgments(&r.core.store, 1).await;
    assert_eq!((js.len(), js[0]["mode"].as_str()), (1, Some("shadow")));

    let jev = unfinished(0.97);
    let mut s = script();
    s.extend(script());
    let r = rig(s, &jev, |_| {});
    let sid = r.session(None);
    let off = r
        .core
        .session_nudge(SessionNudgeParams {
            session_id: sid.clone(),
            on: false,
        })
        .unwrap();
    assert!(!off.on && off.daemon_on);
    let res = r.turn(&sid, "Run the tests.").await;
    assert_eq!((r.requests().len(), res.loops), (1, 1));
    assert!(res.nudged.is_none());
    r.core
        .session_nudge(SessionNudgeParams {
            session_id: sid.clone(),
            on: true,
        })
        .unwrap();
    let res = r.turn(&sid, "Run them again.").await;
    assert!(res.nudged.is_some(), "back on");
    assert!(r
        .core
        .session_nudge(SessionNudgeParams {
            session_id: "ses_none".into(),
            on: false
        })
        .is_err());
}

/// A verdict that lands after the wait nudges nothing, and the turn waits
/// no longer than the wait; the judgment is still recorded, untaken.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_verdict_after_the_wait_nudges_nothing() {
    let jev = unfinished(0.97);
    jev.set_mode(FakeMode::Slow(Duration::from_millis(3_000)));
    let r = rig(script(), &jev, |c| c.judge.total_secs = 6);
    let sid = r.session(None);
    let res = r.turn(&sid, "Run the tests.").await;
    assert_eq!((r.requests().len(), res.loops), (1, 1));
    assert!(res.nudged.is_none());
    // The wait's own span, so a loaded machine's slower turn is not the wait.
    let waited = wait_span_ms(res.trace.as_ref().expect("a trace"));
    assert!(
        waited.is_some_and(|ms| ms < crate::judge::stop::WAIT.as_millis() as u64 + 500),
        "the stop waited {waited:?} ms"
    );
    let js = settled_judgments(&r.core.store, 1).await;
    assert_eq!(js.len(), 1);
    assert_eq!(js[0]["context"]["stop"]["taken"], false);
    assert_eq!(js[0]["context"]["stop"]["acts"], true);
}

/// A job of the turn still running: the turn is waiting on it, not stopped,
/// so nothing is asked and nothing nudged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_job_still_running_is_not_nudged() {
    let jev = unfinished(0.97);
    let job = json!({"argv": ["sleep", "3"]});
    let r = rig(
        vec![
            Scripted::tools("Starting the suite.", &[("t1", "proc_run", job)]),
            Scripted::text("It runs in the background; next I'll read its result."),
            Scripted::text(RAN),
        ],
        &jev,
        |c| c.tools.proc_sync_secs = 1,
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Run the suite.").await;
    assert_eq!((res.loops, res.tool_calls), (2, 1), "{:?}", res.stop_reason);
    assert!(res.nudged.is_none());
    assert!(r.rows("turn.nudged").is_empty());
}

/// Jev off (the judge disabled, as a sparse install or the bench's fixed
/// arm): nothing waits, nothing is judged, nothing nudged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_judge_off_nothing_changes() {
    let jev = unfinished(0.97);
    let r = rig(script(), &jev, |c| c.judge.enabled = false);
    let sid = r.session(None);
    let res = r.turn(&sid, "Run the tests.").await;
    assert_eq!((r.requests().len(), res.loops), (1, 1));
    assert!(res.nudged.is_none());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(judged(&r.core.store).is_empty());
    assert_eq!(jev.connections(), 0);
}
