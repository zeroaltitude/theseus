//! Egress for L1 jobs through the whole core (M4 18c): the gate's step for
//! hosts beyond `[sandbox] egress`, the list a confirm binds, and a result
//! whose job reached a host beyond that list as outside text (T1's hold, its
//! label, its rows; theseus-gyin), on every path a job's result takes: read
//! while the turn waits, read late by the next turn, and swept after a
//! cancel. A job that reached only listed hosts holds nothing. A test
//! launcher keeps each
//! job's `WrapperArgs` and settles the job itself, as an L1 job's wrapper
//! does, with `detail.egress` as its proxy would record it: these tests are
//! of the gate and of what the core makes of a completion, and the daemon's
//! own (`theseusd/tests/sandbox.rs`) run real L1 jobs through a real proxy.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_kernel::job::WrapperArgs;
use theseus_kernel::{Completion, Outcome, Spool};
use theseus_protocol::{ExternalText, SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::{Body, Node};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// What the stand-in proxy recorded for each job: the hosts it reached, or
/// none, and the refusals; and how long the job takes to settle.
#[derive(Clone)]
struct Ran {
    egress: Value,
    after: Duration,
}

/// Keeps every job's arguments, and settles each job itself: its output in
/// the spool's file, and a completion whose `detail` says it ran in L1 with
/// `egress` as its proxy recorded it.
#[derive(Default)]
struct Kept {
    args: Mutex<Vec<WrapperArgs>>,
    ran: Mutex<Option<Ran>>,
}

impl crate::toolrun::JobLauncher for Kept {
    fn launch(&self, spool: &Spool, args: &WrapperArgs) -> anyhow::Result<u32> {
        self.args.lock().unwrap().push(args.clone());
        let ran = self.ran.lock().unwrap().clone().expect("a job's run");
        let (dir, id) = (spool.dir().to_path_buf(), args.correlation_id.clone());
        let settle = move || -> anyhow::Result<()> {
            let spool = Spool::open(&dir)?;
            let out = spool.result_path(&id);
            std::fs::write(&out, "{\"tide\": \"high at 06:12\"}\n")?;
            let now = theseus_protocol::now_unix_ms();
            let detail = json!({"exit_code": 0, "sandbox": {"class": "l1"}, "egress": ran.egress});
            spool.write(&Completion {
                correlation_id: id,
                outcome: Outcome::Succeeded,
                result_ref: Some(out.to_string_lossy().into_owned()),
                external_op_id: None,
                started_at_ms: now,
                finished_at_ms: now,
                producer: "test".into(),
                signature: None,
                cost_micros: None,
                detail: Some(detail),
            })?;
            Ok(())
        };
        if ran.after.is_zero() {
            settle()?;
        } else {
            std::thread::spawn(move || {
                std::thread::sleep(ran.after);
                let _ = settle();
            });
        }
        Ok(std::process::id())
    }
}

/// A job whose proxy opened two tunnels to `api.tides.test:443`.
fn connected() -> Value {
    json!({"allow": ["api.tides.test:443"],
        "hosts": [{"host": "api.tides.test", "port": 443, "connections": 2, "up": 517,
            "down": 4096, "ms": 41}]})
}

/// A job whose call named `pypi.test:443`, beyond the rig's list, approved,
/// and whose proxy opened a tunnel to it.
fn beyond() -> Value {
    json!({"allow": ["api.tides.test:443", "*.crates.test:443", "pypi.test:443"],
        "hosts": [{"host": "pypi.test", "port": 443, "connections": 1, "up": 90,
            "down": 2048, "ms": 12}]})
}

/// The rig's list without `api.tides.test:443`: a job the launcher settles as
/// having reached it reached a host beyond the list, as an approved call's
/// job does.
fn unlisted(cfg: &mut Config, _: &Path) {
    cfg.sandbox.egress = vec!["*.crates.test:443".into()];
}

/// A job whose proxy refused its one `CONNECT`, and reached nothing.
fn refused() -> Value {
    json!({"allow": ["api.tides.test:443"],
        "refused": [{"host": "pypi.test", "port": 443,
            "why": "pypi.test:443 is not on this job's egress list", "count": 1}]})
}

struct Rig {
    core: Arc<Core>,
    kept: Arc<Kept>,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// The template, with `[sandbox] egress` listing `api.tides.test:443`
    /// and every name under `crates.test`, and jobs that settle as `ran`.
    fn new(script: Vec<Scripted>, ran: Ran, tweak: impl FnOnce(&mut Config, &Path)) -> Self {
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
        cfg.sandbox.egress = vec!["api.tides.test:443".into(), "*.crates.test:443".into()];
        tweak(&mut cfg, &root);
        let store = Store::open(&dir.path().join("store")).unwrap();
        let fake = Arc::new(FakeProvider::scripted(script));
        let mut p = crate::rpc::Parts::for_tests(cfg, fake, store);
        let kept = Arc::new(Kept::default());
        *kept.ran.lock().unwrap() = Some(ran);
        p.launcher = kept.clone();
        Self {
            core: Core::build(p).unwrap(),
            kept,
            _dir: dir,
        }
    }

    async fn turn(&self, session: Option<SessionRecord>, input: &str) -> TurnSubmitResult {
        let rec = match session {
            Some(r) => r,
            None => SessionRecord::new(SessionKind::Conversation, None),
        };
        if self
            .core
            .store
            .get_session::<SessionRecord>(&rec.session_id)
            .unwrap()
            .is_none()
        {
            self.core.store.put_session(&rec.session_id, &rec).unwrap();
        }
        let rec: SessionRecord = self
            .core
            .store
            .get_session(&rec.session_id)
            .unwrap()
            .unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        let sink = EventSink::new(self.core.bus.clone(), &rec.session_id, None);
        self.core
            .runner
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
            .unwrap()
    }

    fn hold(&self, sid: &str) -> Option<ExternalText> {
        crate::external::held(&self.core.store, sid).unwrap()
    }

    /// Each job's egress list, as the wrapper was given it.
    fn lists(&self) -> Vec<Vec<String>> {
        let kept = self.kept.args.lock().unwrap();
        kept.iter()
            .map(|a| a.sandbox.as_ref().unwrap().egress.clone())
            .collect()
    }

    fn results(&self, sid: &str) -> Vec<Node> {
        let nodes = self.core.store.session_nodes(sid).unwrap();
        let results = nodes.into_iter().map(|(_, n)| n);
        results
            .filter(|n| matches!(n.body, Body::ToolResult { .. }))
            .collect()
    }

    fn reasons(&self, sid: &str) -> Vec<String> {
        let nodes = self.core.store.session_nodes(sid).unwrap();
        nodes
            .into_iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolCall { gate: Some(g), .. } => {
                    g.decision.as_ref().map(|d| d.reason.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        let tail = self
            .core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(2000)
            .unwrap();
        tail.into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    /// A turn in a new session, and the frames it wrote.
    async fn counted(&self, input: &str) -> (TurnSubmitResult, u64) {
        let frames = || self.core.store.stats().unwrap().frames_appended;
        let before = frames();
        let res = self.turn(None, input).await;
        (res, frames() - before)
    }
}

fn run(input: Value) -> Scripted {
    Scripted::tools("", &[("t1", "proc_run", input)])
}

fn now() -> Ran {
    Ran {
        egress: connected(),
        after: Duration::ZERO,
    }
}

/// The gate's step for hosts beyond the list (design §2.4): a call that
/// names one waits, as a path outside the roots does, and its reason names
/// it; one whose hosts the list covers runs at L1's notify. The job's whole
/// list is in its proposal, so the digest a confirm binds covers it: the
/// same call with another host is refused, and once approved the job gets
/// the hosts its call named, and no other. What it brought back from the
/// host beyond the list holds its session; a job that reached only listed
/// hosts holds nothing (theseus-gyin).
#[tokio::test]
async fn a_call_naming_a_host_beyond_the_list_waits_and_its_approval_reaches_it_alone() {
    let r = Rig::new(
        vec![
            run(json!({"argv": ["true"], "sandbox": {"egress": ["pypi.test:443"]}})),
            Scripted::text("Ran."),
            run(json!({"argv": ["true"], "sandbox": {"egress": ["index.crates.test:443"]}})),
            Scripted::text("Done."),
        ],
        now(),
        |_, _| {},
    );
    let res = r.turn(None, "beyond the list").await;
    assert_eq!(res.stop_reason, "awaiting_confirm", "{res:?}");
    let why = r.reasons(&res.session_id).remove(0);
    assert!(
        why.contains(
            "it names hosts beyond [sandbox] egress: pypi.test:443; an approval lets \
             this job reach them, and no other"
        ),
        "{why}"
    );
    assert!(r.lists().is_empty(), "nothing runs before the approval");
    let corr = r.core.pending_confirms(&res.session_id).unwrap()[0]
        .correlation_id
        .clone();
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    let bound = a.proposal.clone().unwrap();
    let list = ["api.tides.test:443", "*.crates.test:443", "pypi.test:443"];
    assert_eq!(crate::egress::bound(&bound), list);
    // The same call reaching one more host is another proposal.
    let mut wider = bound.clone();
    wider.policy_context["egress"]
        .as_array_mut()
        .unwrap()
        .push(json!("evil.test:443"));
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    let refused = r
        .core
        .kernel
        .authorize(&corr, &wider, Some(crate::turn::OPERATOR))
        .unwrap_err()
        .to_string();
    assert!(refused.contains("digest"), "{refused}");
    *r.kept.ran.lock().unwrap() = Some(Ran {
        egress: beyond(),
        after: Duration::ZERO,
    });
    let exec = res.execution_id.clone().unwrap();
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Ran.");
    assert_eq!(r.lists(), [list.to_vec()]);
    let h = r
        .hold(&res.session_id)
        .expect("the host beyond the list holds its session");
    assert_eq!(
        (h.url.as_str(), h.via.as_deref()),
        ("pypi.test:443", Some("egress"))
    );
    // A host the list covers: no wait, the list as the operator wrote it,
    // and what the job brought back from it holds nothing.
    *r.kept.ran.lock().unwrap() = Some(now());
    let covered = r.turn(None, "covered").await;
    assert_eq!(covered.stop_reason, "no_tool_calls", "{covered:?}");
    assert_eq!(r.lists()[1], ["api.tides.test:443", "*.crates.test:443"]);
    assert!(
        r.hold(&covered.session_id).is_none(),
        "a listed host is not outside text"
    );
    let notified = r.rows("tool.notified");
    let rule = notified.last().unwrap()["rule"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        rule.contains("egress: api.tides.test:443, *.crates.test:443"),
        "{rule}"
    );
}

/// A job that reached a host beyond the list returns outside text (T1): its
/// session holds it, with the reason naming the egress, in the frame that
/// writes the result; the result is untrusted, `via: egress`, and the
/// owner's (note 3); and its `sandbox.egress` row rides in that frame too,
/// so the turn writes no frame more than one whose job connected nowhere,
/// which leaves its session clear and its result trusted.
#[tokio::test]
async fn a_job_that_connected_out_holds_its_session_and_one_that_did_not_leaves_it_clear() {
    let r = Rig::new(
        vec![
            run(json!({"argv": ["true"], "sandbox": true})),
            Scripted::text("Fetched."),
            run(json!({"argv": ["true"], "sandbox": true})),
            Scripted::text("Fetched."),
            run(json!({"argv": ["true"], "sandbox": true})),
            Scripted::text("Ran."),
        ],
        now(),
        unlisted,
    );
    let (out, connected) = r.counted("fetch the tides").await;
    assert_eq!(out.stop_reason, "no_tool_calls", "{out:?}");
    let h = r
        .hold(&out.session_id)
        .expect("the session holds outside text");
    assert_eq!(
        (h.tool.as_str(), h.url.as_str(), h.via.as_deref()),
        ("proc.run", "api.tides.test:443", Some("egress"))
    );
    let why = crate::external::why(&h, crate::external::Mode::Ask);
    assert!(
        why.starts_with(
            "this session read external text (proc.run's egress to api.tides.test:443, at "
        ),
        "{why}"
    );
    let result = r.results(&out.session_id).remove(0);
    // DD5's marker: the result is outside text, which the latch reads.
    let Body::ToolResult {
        content, external, ..
    } = &result.body
    else {
        unreachable!()
    };
    assert!(external.is_some(), "the job connected out: {external:?}");
    assert!(
        content.contains("[ran in L1, the sandbox: egress: api.tides.test:443, no secret; "),
        "{content}"
    );
    assert!(
        content
            .contains("[L1: it reached api.tides.test:443 (2 connections, 517 B up, 4.1 KB down)"),
        "{content}"
    );
    let read = r.rows("session.external_read");
    assert_eq!(
        (read.len(), read[0]["via"].as_str()),
        (1, Some("egress")),
        "{read:?}"
    );
    let egress = r.rows("sandbox.egress");
    assert_eq!(egress.len(), 1, "{egress:?}");
    assert_eq!(
        (
            egress[0]["host"].as_str(),
            egress[0]["connections"].as_u64()
        ),
        (Some("api.tides.test"), Some(2))
    );
    let health = r.core.tools.sandbox.health();
    assert_eq!((health.egress_connections, health.egress_down), (2, 4096));

    *r.kept.ran.lock().unwrap() = Some(Ran {
        egress: refused(),
        after: Duration::ZERO,
    });
    let (clear, refused) = r.counted("fetch, refused").await;
    assert!(
        r.hold(&clear.session_id).is_none(),
        "a job that connected nowhere holds nothing"
    );
    let result = r.results(&clear.session_id).remove(0);
    let Body::ToolResult {
        content, external, ..
    } = &result.body
    else {
        unreachable!()
    };
    assert!(
        external.is_none(),
        "a job that connected nowhere: {external:?}"
    );
    assert!(
        content.contains("[L1: egress refused: pypi.test:443 is not on this job's egress list]"),
        "{content}"
    );
    let refusals = r.rows("sandbox.egress_refused");
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(r.core.tools.sandbox.health().egress_refused, 1);
    // A job whose completion has no egress at all: the frames of a turn with
    // none of 18c's records, against which the two above are counted.
    *r.kept.ran.lock().unwrap() = Some(Ran {
        egress: Value::Null,
        after: Duration::ZERO,
    });
    let (_, none) = r.counted("no egress").await;
    assert_eq!(
        (connected, refused),
        (none, none),
        "the hold and the rows ride in frames the turn writes anyway"
    );
}

/// The operator chose `[sandbox] egress`'s hosts, so what a job brought back
/// from them alone is not outside text (theseus-gyin, cut-list 4.1): its
/// session holds nothing, its result is not marked, and its rows and
/// health's counts are written as for any job that connected out.
#[tokio::test]
async fn a_job_that_reached_only_listed_hosts_leaves_its_session_clear() {
    let r = Rig::new(
        vec![
            run(json!({"argv": ["true"], "sandbox": true})),
            Scripted::text("Fetched."),
        ],
        now(),
        |_, _| {},
    );
    let out = r.turn(None, "fetch the tides").await;
    assert_eq!(out.stop_reason, "no_tool_calls", "{out:?}");
    assert!(
        r.hold(&out.session_id).is_none(),
        "a listed host is not outside text"
    );
    let result = r.results(&out.session_id).remove(0);
    assert!(!crate::external::is_outside(&result), "{result:?}");
    assert!(r.rows("session.external_read").is_empty());
    assert_eq!(r.rows("sandbox.egress").len(), 1);
    assert_eq!(r.core.tools.sandbox.health().egress_connections, 2);
}

/// A session that already holds outside text holds an L1 call with egress
/// too, whatever its list: 20a's exemption covers only jobs with no egress
/// and no secret, and this keeps it honest.
#[tokio::test]
async fn in_a_session_that_holds_outside_text_an_l1_call_with_egress_waits() {
    let r = Rig::new(
        vec![
            run(json!({"argv": ["true"], "sandbox": {"egress": ["api.tides.test:443"]}})),
            run(json!({"argv": ["true"], "sandbox": true})),
        ],
        now(),
        |_, _| {},
    );
    for input in ["with its own hosts", "with the operator's list"] {
        let mut rec = SessionRecord::new(SessionKind::Conversation, None);
        let page = crate::external::read("nod_page", "http.fetch", "https://tides.test/", None, 1);
        rec.external = Some(page);
        let res = r.turn(Some(rec), input).await;
        assert_eq!(res.stop_reason, "awaiting_confirm", "{input}: {res:?}");
        let why = r.reasons(&res.session_id).pop().unwrap();
        assert!(
            why.contains("this session read external text (http.fetch https://tides.test/"),
            "{why}"
        );
        let corr = r.core.pending_confirms(&res.session_id).unwrap()[0]
            .correlation_id
            .clone();
        r.core.confirm_action(&corr, false, None, "test").unwrap();
    }
    assert!(r.lists().is_empty());
}

/// A job read late, by the next turn (theseus-kol): its hold rides in the
/// frame that takes it, as its rows do.
#[tokio::test]
async fn a_late_result_that_connected_out_holds_its_session_in_the_frame_that_takes_it() {
    let r = Rig::new(
        vec![
            run(json!({"argv": ["true"], "sandbox": true})),
            Scripted::text("It runs in the background."),
            Scripted::text("Read it."),
        ],
        Ran {
            egress: connected(),
            after: Duration::from_millis(300),
        },
        |cfg, root| {
            cfg.tools.proc_sync_secs = 0;
            unlisted(cfg, root);
        },
    );
    let first = r.turn(None, "start it").await;
    assert_eq!(first.stop_reason, "no_tool_calls", "{first:?}");
    assert!(r.hold(&first.session_id).is_none(), "nothing read yet");
    let spool = Spool::open(r.core.tools.spool.as_ref().unwrap().dir()).unwrap();
    let corr = r.kept.args.lock().unwrap()[0].correlation_id.clone();
    let t0 = std::time::Instant::now();
    while !spool.has_completion(&corr) {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the job never settled"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    r.core.drain_spool();
    // Its execution was woken, and the turn it takes reads the result.
    let exec = first.execution_id.clone().unwrap();
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Read it.");
    let h = r
        .hold(&first.session_id)
        .expect("the late result holds the session");
    assert_eq!(h.via.as_deref(), Some("egress"));
    let late = r.results(&first.session_id);
    assert!(late
        .iter()
        .any(|n| matches!(&n.body, Body::ToolResult { late: true, .. })
            && crate::external::is_outside(n)));
    assert_eq!(r.rows("sandbox.egress").len(), 1);
}
