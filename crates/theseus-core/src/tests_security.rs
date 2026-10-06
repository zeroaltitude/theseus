//! `security.v1` and `security.v3` in shadow at the gate (M5 step 24; design
//! §3's "24"), against the fake Jev: every call that acts is judged after the
//! gate decides, and nothing the gate decides changes with it. A call Jev
//! scores at 1% in a holding session still waits; the floor still asks; a
//! search is judged only in a session that holds external text; a "should
//! have asked" press labels the call's judgments; a notified call's score
//! follows its notice; a slow Jev never delays `tool.started`; and a judged
//! tool loop keeps its frame budget.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::{ExternalText, Message, SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::config::WebToolsConfig;
use crate::judge::gate::{judgment_id, SECURITY_CANDIDATE, SECURITY_PACK};
use crate::ledger::LedgerRow;
use crate::node::Body;
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::rpc::{Core, Parts};
use crate::secrets::{Secret, SecretBoard};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::web::tests::{serve, web, Server};
use crate::Config;

/// A board with the Jev key ready, as it is once the secrets settle.
pub(crate) fn board() -> Arc<SecretBoard> {
    let b = SecretBoard::new(["jev_api_key".to_string()], Instant::now());
    b.publish(
        BTreeMap::from([(
            "jev_api_key".to_string(),
            Ok(Secret::new("jev-test-key-0123456789".into())),
        )]),
        "test",
    );
    b
}

/// A model that runs `argv` once per turn: a call when the turn's last
/// message is the operator's, and `Done.` after its result. A call that
/// waits ends its turn, so a list of answers would fall out of step.
pub(crate) struct RunsOnce {
    pub(crate) argv: Vec<String>,
}

impl Provider for RunsOnce {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            let answered = req
                .messages
                .last()
                .and_then(|m| m["content"].as_array())
                .is_some_and(|b| b.iter().any(|b| b["type"] == "tool_result"));
            let argv: Vec<&str> = self.argv.iter().map(String::as_str).collect();
            let answer = match answered {
                true => Scripted::text("Done."),
                false => run("r1", &argv),
            };
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) root: PathBuf,
    pub(crate) _server: Arc<Server>,
    pub(crate) _dir: tempfile::TempDir,
    pub(crate) _work: tempfile::TempDir,
}

/// A core over a fresh store, its tools rooted at `root` (shared, so two
/// cores' reasons read alike), the web tools reaching `server`, and the
/// judge on at `jev` when given. `notify` is the operator's own posture.
pub(crate) fn core(
    root: &Path,
    server: &Arc<Server>,
    model: Arc<dyn Provider>,
    jev: Option<&FakeJev>,
    tweak: impl FnOnce(&mut Config),
) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.policy.enforcement = Posture::Notify;
    if let Some(j) = jev {
        cfg.judge.enabled = true;
        cfg.judge.api_base = j.base();
        cfg.judge.connect_secs = 1;
        cfg.judge.total_secs = 2;
    }
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let mut p = Parts {
        toollets: web(server.port, WebToolsConfig::default(), true).tools(),
        ..Parts::for_tests(cfg, model, store)
    };
    p.secrets = board();
    let core = Core::build(p).unwrap();
    crate::tests_judge::warm(&core);
    (core, dir)
}

pub(crate) async fn rig(
    script: Vec<Scripted>,
    jev: Option<&FakeJev>,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    rig_with(Arc::new(FakeProvider::scripted(script)), jev, tweak).await
}

pub(crate) async fn rig_with(
    model: Arc<dyn Provider>,
    jev: Option<&FakeJev>,
    tweak: impl FnOnce(&mut Config),
) -> Rig {
    let server = Arc::new(serve().await);
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let (core, dir) = core(&root, &server, model, jev, tweak);
    Rig {
        core,
        root,
        _server: server,
        _dir: dir,
        _work: work,
    }
}

pub(crate) fn session(core: &Core, hold: Option<ExternalText>) -> String {
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.external = hold;
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// What a session that read a page holds.
pub(crate) fn a_hold() -> ExternalText {
    ExternalText {
        since_ms: 1_700_000_000_000,
        tool: "http.fetch".into(),
        url: "http://site.test/page.html".into(),
        node_id: "nod_page".into(),
        ..Default::default()
    }
}

/// Every notification the turn's connection got, with when it came.
pub(crate) type Heard = Arc<Mutex<Vec<(Instant, String, Value)>>>;

pub(crate) async fn turn(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
    heard: Option<&Heard>,
) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let direct = heard.map(|h| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
        let h = h.clone();
        tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                if let Message::Notification(n) = m {
                    let v = serde_json::to_value(&n).unwrap();
                    let method = v["method"].as_str().unwrap_or_default().to_string();
                    h.lock()
                        .unwrap()
                        .push((Instant::now(), method, v["params"].clone()));
                }
            }
        });
        ("conn_test".to_string(), tx.into())
    });
    let sink = EventSink::new(core.bus.clone(), sid, direct);
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
            prompt: None,
        })
        .await
        .unwrap()
}

pub(crate) fn run(id: &str, argv: &[&str]) -> Scripted {
    Scripted::tools("", &[(id, "proc_run", json!({ "argv": argv }))])
}

/// The `judge:security` rows of `kind`, decoded.
pub(crate) fn security_rows(store: &Store, kind: &str) -> Vec<LedgerRow> {
    store
        .scope_after("judge:security", 0)
        .unwrap()
        .into_iter()
        .map(|r| {
            let row: LedgerRow = r.decode().unwrap();
            assert_eq!(r.key.as_deref(), row.data["id"].as_str(), "keyed by its id");
            row
        })
        .filter(|r| r.kind == kind)
        .collect()
}

/// Wait, on the runtime's timer, until `n` gate judgments are recorded.
pub(crate) async fn until_judged(store: &Store, n: usize) -> Vec<LedgerRow> {
    let t0 = Instant::now();
    loop {
        let rows = security_rows(store, "judge.call");
        if rows.len() >= n {
            return rows;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} of {n} judgments recorded",
            rows.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The session's call of `tool`: its correlation id and the gate's record.
pub(crate) fn the_call(core: &Core, sid: &str, tool: &str) -> (String, Value) {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolCall {
                tool: t,
                correlation_id,
                gate,
                ..
            } if t == tool => Some((
                correlation_id.unwrap_or_default(),
                serde_json::to_value(gate).unwrap(),
            )),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {tool} call"))
}

/// What the gate decided about a call, as a clean judge-off core would:
/// its verdict and its decision whole.
fn decided(gate: &Value, res: &TurnSubmitResult) -> Value {
    json!({"gate": gate["result"]["gate"], "decision": gate["decision"],
        "waits": res.awaiting_confirm.is_some()})
}

/// The property (design §3's 24): over the posture, the hold, the floor,
/// and the score Jev gives, the gate decides as the judge-off gate does,
/// for the call it judges and for one after its judgment has landed.
fn same_decision(posture: Posture, held: bool, floor: bool, risky: f64) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let jev = FakeJev::start().unwrap();
        jev.script("risky", Jev::Noul(risky));
        jev.script("steered", Jev::Noul(risky));
        let argv: &[&str] = if floor {
            &["op", "whoami"]
        } else {
            &["echo", "hi"]
        };
        let model = || -> Arc<dyn Provider> {
            Arc::new(RunsOnce {
                argv: argv.iter().map(|a| a.to_string()).collect(),
            })
        };
        let lines = |c: &mut Config| {
            c.policy.tools.insert("proc.run".into(), posture);
        };
        let on = rig_with(model(), Some(&jev), lines).await;
        let (off, _off_dir) = core(&on.root, &on._server, model(), None, lines);
        let hold = || held.then(a_hold);
        let mut decisions = Vec::new();
        for (core, judged) in [(&on.core, true), (&off, false)] {
            let mut seen = Vec::new();
            for word in ["first", "second"] {
                let sid = session(core, hold());
                let res = turn(core, &sid, word, None).await;
                let (_, gate) = the_call(core, &sid, "proc.run");
                seen.push(decided(&gate, &res));
                if judged && word == "first" {
                    // The second call is gated after the first's judgment
                    // has landed.
                    until_judged(&core.store, 2).await;
                }
            }
            decisions.push(seen);
        }
        assert_eq!(
            decisions[0], decisions[1],
            "{posture:?} held {held} floor {floor} risky {risky}"
        );
        let rows = security_rows(&on.core.store, "judge.call");
        assert!(rows.len() >= 2);
        let v1 = rows
            .iter()
            .find(|r| r.data["pack"] == SECURITY_PACK)
            .unwrap();
        assert_eq!(v1.data["outcome"]["outcome"], "answered", "{}", v1.data);
        let p = v1.data["answers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["question"] == "risky")
            .unwrap()["band"]["value"]
            .as_f64()
            .unwrap();
        assert!((p - risky).abs() < 1e-6, "{p} for {risky}");
    });
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 8, ..ProptestConfig::default() })]

    #[test]
    fn with_jev_the_gate_decides_as_it_does_without(
        posture in prop::sample::select(vec![Posture::Open, Posture::Notify, Posture::Approve]),
        held in any::<bool>(),
        floor in any::<bool>(),
        risky in prop::sample::select(vec![0.0, 0.01, 0.5, 0.75, 0.99, 1.0]),
    ) {
        same_decision(posture, held, floor, risky);
    }
}

/// In a holding session, a call Jev scores at 1% still waits: the hold is
/// the deterministic floor, and the record says the hold made it wait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn in_a_holding_session_a_call_scored_one_percent_still_waits() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.01));
    let r = rig(
        vec![run("r1", &["echo", "hi"]), Scripted::text("Done.")],
        Some(&jev),
        |_| {},
    )
    .await;
    let sid = session(&r.core, Some(a_hold()));
    let res = turn(&r.core, &sid, "run it", None).await;
    let corr = res.awaiting_confirm.clone().expect("the run waits");
    let rows = until_judged(&r.core.store, 2).await;
    let v1 = rows
        .iter()
        .find(|r| r.data["pack"] == SECURITY_PACK)
        .unwrap();
    let c = &v1.data["context"];
    assert_eq!(c["call"], corr.as_str());
    assert_eq!(
        (
            c["posture"].as_str(),
            c["hold"].as_bool(),
            c["hold_raised"].as_bool(),
            c["waited_on_hold"].as_bool()
        ),
        (Some("approve"), Some(true), Some(true), Some(true)),
        "{c}"
    );
    assert_eq!(v1.data["id"], judgment_id(SECURITY_PACK, &corr).as_str());
    let risky = &v1.data["answers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["question"] == "risky")
        .unwrap()["band"]["value"];
    assert_eq!(risky.as_f64(), Some(0.01));
    // Still waiting, whatever Jev said.
    assert_eq!(r.core.pending_confirms(&sid).unwrap().len(), 1);
    assert!(rows.iter().any(|r| r.data["pack"] == SECURITY_CANDIDATE));
}

/// The floor still asks, whatever the score: `op` waits at 1%.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_floor_still_asks() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.01));
    let r = rig(
        vec![run("r1", &["op", "whoami"]), Scripted::text("Done.")],
        Some(&jev),
        |c| {
            c.policy.tools.insert("proc.run".into(), Posture::Open);
        },
    )
    .await;
    let sid = session(&r.core, None);
    let res = turn(&r.core, &sid, "who am i", None).await;
    let corr = res.awaiting_confirm.clone().expect("the floor asks");
    let rows = until_judged(&r.core.store, 2).await;
    let c = &rows[0].data["context"];
    assert_eq!(
        (
            c["call"].as_str(),
            c["floor"].as_bool(),
            c["posture"].as_str()
        ),
        (Some(corr.as_str()), Some(true), Some("approve"))
    );
    assert_eq!(c["waited_on_hold"], false);
    assert!(r.core.pending_confirms(&sid).unwrap()[0].floor);
}

/// Q12: a `web.search` in a clean session is a read, and is not judged; in a
/// session that holds external text it is, as the exfiltration path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_search_is_judged_only_in_a_holding_session() {
    let jev = FakeJev::start().unwrap();
    let search = |id: &str| {
        Scripted::tools(
            "",
            &[(id, "web_search", json!({"query": "rust ignore crate"}))],
        )
    };
    let script = vec![
        search("s1"),
        run("r1", &["echo", "after"]),
        Scripted::text("Done."),
        search("s2"),
        Scripted::text("Done."),
    ];
    let r = rig(script, Some(&jev), |_| {}).await;
    // Clean: the search is not judged, the run after it is.
    let clean = session(&r.core, None);
    turn(&r.core, &clean, "look it up, then run", None).await;
    let rows = until_judged(&r.core.store, 2).await;
    assert_eq!(rows.len(), 2, "the run's two judgments, and no more");
    assert!(
        rows.iter().all(|r| r.data["context"]["tool"] == "proc.run"),
        "{rows:?}"
    );
    let (search_call, _) = the_call(&r.core, &clean, "web.search");
    assert!(r.core.runner.judge.of_call(&search_call).is_empty());
    // Holding: the search is judged, by both packs.
    let held = session(&r.core, Some(a_hold()));
    turn(&r.core, &held, "look it up", None).await;
    let rows = until_judged(&r.core.store, 4).await;
    let searched: Vec<&LedgerRow> = rows
        .iter()
        .filter(|r| r.data["context"]["tool"] == "web.search")
        .collect();
    assert_eq!(searched.len(), 2, "{rows:?}");
    for r in &searched {
        let c = &r.data["context"];
        assert_eq!(
            (
                c["tool_class"].as_str(),
                c["class"].as_str(),
                c["hold"].as_bool()
            ),
            (Some("read"), Some("tools"), Some(true))
        );
        assert_eq!(c["hold_raised"], false, "a read keeps its posture");
    }
    let (search_call, _) = the_call(&r.core, &held, "web.search");
    assert_eq!(r.core.runner.judge.of_call(&search_call).len(), 2);
}

/// "Should have asked" with `--call` labels each of the call's judgments in
/// the press's frame: inside the sink's window (the judgment not even
/// answered yet) as after it, when the tool is tightened already.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_press_with_a_call_labels_its_judgments_in_its_frame() {
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Slow(Duration::from_millis(800)));
    let script = vec![
        run("r1", &["echo", "one"]),
        run("r2", &["echo", "two"]),
        Scripted::text("Done."),
    ];
    let r = rig(script, Some(&jev), |_| {}).await;
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "run twice", None).await;
    let calls: Vec<String> = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolCall { correlation_id, .. } => correlation_id,
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 2);
    // Pressed before Jev has answered: the rows are not written yet.
    assert!(security_rows(&r.core.store, "judge.call").is_empty());
    let frames = || r.core.store.stats().unwrap().frames_appended;
    let f0 = frames();
    r.core.tighten("proc.run", Some(&calls[0]), "test").unwrap();
    assert_eq!(
        frames() - f0,
        1,
        "the tightening and its labels are one frame"
    );
    let labels = security_rows(&r.core.store, "judge.label");
    assert_eq!(labels.len(), 2, "one for each pack's judgment");
    // After the window: the tool is tightened already, and the press still
    // labels the second call's judgments, in a frame of their own.
    let judged = until_judged(&r.core.store, 4).await;
    let f1 = frames();
    let again = r.core.tighten("proc.run", Some(&calls[1]), "test").unwrap();
    assert!(again.already);
    assert_eq!(frames() - f1, 1);
    let labels = security_rows(&r.core.store, "judge.label");
    assert_eq!(labels.len(), 4);
    for (call, l) in [&calls[0], &calls[0], &calls[1], &calls[1]]
        .into_iter()
        .zip(&labels)
    {
        let d = &l.data;
        assert!(d["id"].as_str().unwrap().starts_with("lbl_"), "{d}");
        assert_eq!(d["correlation_id"], call.as_str());
        assert_eq!(
            (
                d["source"].as_str(),
                d["weight"].as_f64(),
                d["question"].as_str(),
                d["label"].as_bool()
            ),
            (Some("operator"), Some(1.0), Some("risky"), Some(true))
        );
        let row = judged
            .iter()
            .find(|j| j.data["id"] == d["judgment"])
            .expect("its judgment");
        assert_eq!(row.data["context"]["call"], call.as_str());
        assert_eq!(row.data["pack"], d["pack"]);
    }
}

/// A notified call's score follows its notice to the turn's clients, as
/// `risk N% (shadow)`; an open call's is recorded and never told.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_notified_calls_score_follows_its_notice() {
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.37));
    let script = vec![run("r1", &["echo", "hi"]), Scripted::text("Done.")];
    let r = rig(script, Some(&jev), |_| {}).await;
    let sid = session(&r.core, None);
    let heard: Heard = Arc::default();
    turn(&r.core, &sid, "say hi", Some(&heard)).await;
    let t0 = Instant::now();
    let scored = loop {
        let h = heard.lock().unwrap().clone();
        if let Some(i) = h.iter().position(|(_, m, _)| m == "judge.scored") {
            break (h, i);
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "no score");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let (h, i) = scored;
    let s: theseus_protocol::judge::JudgeScored = serde_json::from_value(h[i].2.clone()).unwrap();
    assert_eq!(s.line(), "risk 37% (shadow)");
    assert_eq!(
        (s.pack.as_str(), s.tool.as_str()),
        (SECURITY_PACK, "proc.run")
    );
    let notice = h
        .iter()
        .position(|(_, m, p)| m == "policy.notified" && p["tool_use_id"] == "r1");
    assert!(
        notice.is_some_and(|n| n < i),
        "the score follows its notice"
    );
    assert_eq!(s.judgment, judgment_id(SECURITY_PACK, &s.correlation_id));
    // Only the incumbent's score is told.
    assert_eq!(h.iter().filter(|(_, m, _)| m == "judge.scored").count(), 1);
}

/// When `tool.started` comes after the turn began: the dispatch never waits
/// on the judgment, so a Jev that takes 5 s changes nothing.
async fn started_after(jev: Option<&FakeJev>) -> (Duration, Duration) {
    let r = rig(
        vec![run("r1", &["echo", "hi"]), Scripted::text("Done.")],
        jev,
        |c| {
            c.judge.total_secs = 6;
        },
    )
    .await;
    let sid = session(&r.core, None);
    let heard: Heard = Arc::default();
    let t0 = Instant::now();
    turn(&r.core, &sid, "say hi", Some(&heard)).await;
    let took = t0.elapsed();
    let started = heard
        .lock()
        .unwrap()
        .iter()
        .find(|(_, m, _)| m == "tool.started")
        .map(|(at, _, _)| at.duration_since(t0))
        .expect("tool.started");
    (started, took)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_jev_slow_tool_started_comes_as_fast_as_with_the_judge_off() {
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Slow(Duration::from_secs(5)));
    let (off, _) = started_after(None).await;
    let (on, took) = started_after(Some(&jev)).await;
    // A dispatch that waited on the judgment would start past 5 s; load
    // stretches a turn's own time, never to 3 s.
    assert!(
        on < off + Duration::from_millis(1500),
        "on {on:?}, off {off:?}"
    );
    assert!(
        on < Duration::from_secs(3) && took < Duration::from_secs(4),
        "{on:?} {took:?}"
    );
    assert!(jev.connections() >= 1, "it was judged");
}

/// A judged tool loop keeps its frame budget: the turn's own count of its
/// frames (its trace's) is the judge-off turn's, and the trace marks each
/// judgment under the call's span.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judged_tool_loop_keeps_its_frame_budget_and_marks_its_trace() {
    let jev = FakeJev::start().unwrap();
    let write = |id: &str| {
        Scripted::tools(
            "",
            &[(
                id,
                "fs_write",
                json!({"path": "note.txt", "content": "hi\n"}),
            )],
        )
    };
    let script = || {
        vec![
            Scripted::text("first"),
            write("w1"),
            Scripted::text("Written."),
        ]
    };
    let mut frames = Vec::new();
    let mut traces = Vec::new();
    for judged in [false, true] {
        let r = rig(script(), judged.then_some(&jev), |_| {}).await;
        let sid = session(&r.core, None);
        turn(&r.core, &sid, "warm up", None).await;
        let res = turn(&r.core, &sid, "write the note", None).await;
        assert_eq!((res.loops, res.tool_calls), (2, 1));
        let trace = r
            .core
            .store
            .ledger_tail::<LedgerRow>(10_000)
            .unwrap()
            .into_iter()
            .map(|(_, row)| row)
            .find(|row| {
                row.kind == "turn.trace" && row.turn_id.as_deref() == Some(res.turn_id.as_str())
            })
            .expect("the turn's trace")
            .data;
        frames.push(trace["attrs"]["frames"].as_u64().expect("frames"));
        traces.push((trace, r, sid));
    }
    assert_eq!(frames[0], frames[1], "the judge adds no frame to the turn");
    assert!(
        frames[1] <= 9,
        "a judged tool loop wrote {} frames",
        frames[1]
    );
    let (trace, r, sid) = &traces[1];
    // The gate's two marks; 23b's loop.v1 mark at the turn's end is a third.
    let marks = spans(trace, "mark")
        .into_iter()
        .filter(|s| s["name"] == "judge" && s["attrs"]["point"] == "gate")
        .collect::<Vec<_>>();
    assert_eq!(marks.len(), 2, "{trace}");
    let (call, _) = the_call(&r.core, sid, "fs.write");
    // security.v3 is live as notices (step 24's notices); v1 in shadow.
    for ((m, pack), mode) in marks
        .iter()
        .zip([SECURITY_PACK, SECURITY_CANDIDATE])
        .zip(["shadow", "live"])
    {
        let a = &m["attrs"];
        assert_eq!(
            (a["pack"].as_str(), a["point"].as_str(), a["mode"].as_str()),
            (Some(pack), Some("gate"), Some(mode))
        );
        assert_eq!(a["judgment"], judgment_id(pack, &call).as_str());
        assert_eq!(m["start_us"], m["end_us"], "zero-length");
    }
    let rows = until_judged(&r.core.store, 2).await;
    for m in &marks {
        assert!(rows
            .iter()
            .any(|row| row.data["id"] == m["attrs"]["judgment"]));
    }
    assert!(
        traces[0].0.to_string().find("\"judge\"").is_none(),
        "judge off, no mark"
    );
}

/// Every span of `kind` in a trace, depth first.
fn spans<'a>(s: &'a Value, kind: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    if s["kind"] == kind {
        out.push(s);
    }
    for c in s["children"].as_array().into_iter().flatten() {
        out.extend(spans(c, kind));
    }
    out
}
