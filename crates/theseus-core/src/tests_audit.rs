//! The audit (M5 25d; design §3, "25d"), on seeded stores with a scripted
//! provider: labels from a seeded sample, answers outside the options
//! dropped, the per-run cap, a second run writing nothing, the run the
//! owner's alone, and its requests polled off its low thread.

use serde_json::json;
use theseus_judge::fake::FakeJev;
use theseus_judge::price::micros_to_usd;
use theseus_protocol::judge_runs::JudgeAuditParams;
use theseus_protocol::LedgerKind;

use crate::approval::{Answerer, Surface};
use crate::learning::audit::{request_for, sample};
use crate::provider::Scripted;
use crate::rpc::Core;
use crate::tests_replay::{call_row, judgment, loop_state, rig, rows};

/// What the audit model answers: two of loop.v1's questions rightly, one
/// outside its options, and one the pack does not ask.
fn audit_answer() -> Scripted {
    Scripted::text(
        r#"Here: {"work_state": "progressing", "announced_unfinished": false,
        "stopping_point_defined": "maybe", "invented": true}"#,
    )
}

/// Five answered `loop.v1` judgments, each with its state's blob.
fn five(c: &Core) {
    let mut records = Vec::new();
    for (i, name) in ["heron", "wren", "gull", "tern", "kite"].iter().enumerate() {
        let (state, blob) = loop_state(c, &format!("Count the {name}s at dusk."));
        let j = judgment(
            &format!("jdg_{name}"),
            "loop.v1",
            &state,
            vec![],
            json!({"blob": blob, "session": "ses_x", "turn": format!("trn_{name}")}),
        );
        records.push(call_row(&j, 1_000 + i as u64));
    }
    c.store.append(&records).unwrap();
}

fn audit(c: &Core, sample: u32) -> JudgeAuditParams {
    JudgeAuditParams {
        pack: "loop".into(),
        sample,
        profile: c.live_profile().0,
        seed: Some(7),
    }
}

/// An audit: a seeded sample, one request per state with Jev's
/// instructions and criteria, each answer inside its options an audit
/// label (weight 0.5, keyed by judgment, question, and run), the rest
/// dropped; the next run draws only what is unaudited, and a run with
/// nothing left writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_audit_labels_a_seeded_sample_once() {
    let jev = FakeJev::start().unwrap();
    let r = rig(vec![audit_answer(); 8], &jev, |_| {});
    let c = &r.core;
    five(c);
    let out = c.judge_audit(audit(c, 3), "cli").await.unwrap();
    assert_eq!(
        (
            out.pack.as_str(),
            out.eligible,
            out.sampled,
            out.asked,
            out.failed
        ),
        ("loop.v1", 5, 3, 3, 0)
    );
    assert_eq!((out.labels, out.dropped), (6, 6));
    assert!(out.stopped.is_none());
    assert!(out.cost_usd > 0.0);
    // The request: the state, and each question as Jev reads it.
    let sent = r.fake.requests();
    assert_eq!(sent.len(), 3);
    let body = serde_json::to_string(&sent[0].messages).unwrap();
    assert!(body.contains("Count the"), "{body}");
    assert!(
        body.contains("\\\"progressing\\\" (Work toward the ask remains"),
        "{body}"
    );
    assert!(
        body.contains("True when: The final message announces"),
        "{body}"
    );
    let labels = rows(c, "judge:loop", LedgerKind::JudgeLabel);
    assert_eq!(labels.len(), 6);
    assert!(labels.iter().all(|l| l.data["source"] == "audit"
        && l.data["weight"] == json!(0.5)
        && l.data["rule"] == json!(out.id)));
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeAudit).len(), 1);
    // The report counts them as audit labels.
    let rep = c
        .run_learning(theseus_protocol::now_unix_ms(), "on_demand", |_| {})
        .unwrap();
    assert_eq!(rep.labels.audit, 6);
    // The next run draws the two left; one more finds none and writes
    // nothing.
    let next = c.judge_audit(audit(c, 5), "cli").await.unwrap();
    assert_eq!((next.eligible, next.asked, next.labels), (2, 2, 4));
    let none = c.judge_audit(audit(c, 5), "cli").await.unwrap();
    assert_eq!((none.eligible, none.asked), (0, 0));
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeLabel).len(), 10);
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeAudit).len(), 2);
    assert_eq!(r.fake.requests().len(), 5);
}

/// The same seed draws the same sample; another seed, another order.
#[test]
fn a_seed_draws_one_sample() {
    let seen: Vec<crate::learning::Seen> = (0..20)
        .map(|i| crate::learning::Seen {
            position: i,
            at_ms: i,
            judgment: serde_json::from_value(json!({
                "id": format!("jdg_{i}"), "pack": "loop.v1", "version": 1, "pack_sha256": "",
                "point": "loop_end", "mode": "shadow", "model": "jev-1.13.0",
                "answered_by": null, "model_drift": false,
                "state": {"sha256": "", "bytes": 0, "tokens": 0, "cap_tokens": 1,
                    "builder": "loop", "builder_version": 1, "truncated": [], "dropped": []},
                "questions": 0, "answers": [], "call": null,
                "timing": {"queued_ms": 0, "http_ms": 0, "total_ms": 0}, "usage": null,
                "cost_micros": null, "reserve_micros": null,
                "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {},
                "context": {},
            }))
            .unwrap(),
        })
        .collect();
    let ids = |seed| -> Vec<String> {
        sample(&seen, seed, 5)
            .iter()
            .map(|s| s.judgment.id.clone())
            .collect()
    };
    assert_eq!(ids(7), ids(7));
    assert_ne!(ids(7), ids(8));
    assert_eq!(ids(7).len(), 5);
}

/// The run stops before a request would pass `[judge] audit_limit_usd`
/// (what it spent and that request's reservation), saying so; what it
/// spent stays inside it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_audit_stops_at_its_per_run_cap() {
    let jev = FakeJev::start().unwrap();
    // One request's reservation, on a rig like the capped one.
    let probe = rig(vec![], &jev, |_| {});
    let (state, _) = loop_state(&probe.core, "Count the herons at dusk.");
    let _ = state;
    let pack = theseus_judge::pack::by_name("loop.v1").unwrap();
    let (live, _) = probe.core.live_profile();
    let target = probe
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let json_state = {
        let (_, blob) = loop_state(&probe.core, "Count the herons at dusk.");
        let b = probe.core.store.blobs().base64(&blob).unwrap();
        String::from_utf8(crate::blobs::decode(&b).unwrap()).unwrap()
    };
    let need = probe
        .core
        .audit_reserve(&request_for(
            &pack,
            &json_state,
            &target.model,
            target.max_tokens,
        ))
        .unwrap();
    // Room for one request's reservation: the second, after the first
    // spent anything, would pass it.
    let limit = micros_to_usd(need) + 0.000_000_5;
    let r = rig(vec![audit_answer(); 8], &jev, |c| {
        c.judge.audit_limit_usd = limit
    });
    let c = &r.core;
    five(c);
    let out = c.judge_audit(audit(c, 5), "cli").await.unwrap();
    assert_eq!(out.asked, 1, "{out:?}");
    let why = out.stopped.as_deref().unwrap();
    assert!(why.contains("past [judge] audit_limit_usd"), "{why}");
    assert!(out.cost_usd <= limit, "{} > {limit}", out.cost_usd);
    assert_eq!(r.fake.requests().len(), 1);
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeLabel).len(), 2);
}

/// An audit is the owner's: from a shared place it is refused, ledgered,
/// and nothing is asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_audit_from_a_shared_place_is_refused() {
    let jev = FakeJev::start().unwrap();
    let r = rig(vec![audit_answer()], &jev, |_| {});
    let c = &r.core;
    five(c);
    let stranger = Answerer {
        label: "discord".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "42".into(),
            channel_id: "1001".into(),
            guild_id: Some("7".into()),
        }),
    };
    let e = c.judge_audit(audit(c, 1), stranger).await.unwrap_err();
    assert!(
        e.downcast_ref::<crate::approval::Refusal>().is_some(),
        "{e:#}"
    );
    let refused = c.store.ledger_tail::<crate::ledger::LedgerRow>(50).unwrap();
    assert!(refused
        .iter()
        .any(|(_, r)| r.kind == "approval.refused" && r.data["act"] == "judge.audit"));
    assert!(r.fake.requests().is_empty());
    assert!(rows(c, "judge:loop", LedgerKind::JudgeLabel).is_empty());
}

/// A stand-in provider that answers as the fake does and records the nice
/// value of each thread that polls its request.
struct Niced {
    inner: crate::provider::FakeProvider,
    /// Each polling thread's nice value and name.
    nices: std::sync::Mutex<Vec<(i32, String)>>,
}

/// This thread's own nice value (Linux's nice is per thread).
fn this_threads_nice() -> i32 {
    // SAFETY: gettid and getpriority on this thread's own id read nothing
    // but its scheduling priority.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::getpriority(libc::PRIO_PROCESS, tid)
    }
}

impl crate::provider::Provider for Niced {
    fn name(&self) -> &str {
        "niced"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a crate::provider::ProviderRequest,
        on_delta: crate::provider::DeltaSink<'a>,
    ) -> crate::provider::ProviderFuture<'a> {
        Box::pin(async move {
            let name = std::thread::current()
                .name()
                .unwrap_or_default()
                .to_string();
            self.nices.lock().unwrap().push((this_threads_nice(), name));
            self.inner.stream_message(req, on_delta).await
        })
    }
}

/// The audit's requests are sent on the runtime, never polled on its
/// `learning` thread at nice 19, so no pool thread a request starts (a host
/// name's lookup) takes that thread's priority for life (theseus-bgg5). A
/// request polled by `block_on` on the low thread reads 19; one spawned on
/// a worker reads the worker's own nice value (the test's, 0 unless the
/// test itself runs niced), whatever the pool holds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_audits_requests_are_polled_off_its_low_thread() {
    let base = this_threads_nice();
    let jev = FakeJev::start().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::tests_judge::judge_config(dir.path(), Some(&jev));
    for (p, _) in crate::judge::WIRED {
        cfg.judge
            .packs
            .insert((*p).into(), crate::tests_judge::off());
    }
    cfg.validate().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let niced = std::sync::Arc::new(Niced {
        inner: crate::provider::FakeProvider::scripted(vec![audit_answer(); 3]),
        nices: Default::default(),
    });
    let mut p = crate::rpc::Parts::for_tests(cfg, niced.clone(), store);
    p.secrets = crate::tests_judge::board();
    let c = &Core::build(p).unwrap();
    five(c);
    let out = c.judge_audit(audit(c, 3), "cli").await.unwrap();
    assert_eq!((out.asked, out.failed, out.labels), (3, 0, 6), "{out:?}");
    let polled = niced.nices.lock().unwrap().clone();
    assert_eq!(polled.len(), 3, "{polled:?}");
    // The thread's name holds at any nice value: a test process itself run
    // at nice 19 (a gate under `nice -n 19`) reads 19 on the low thread too
    // (theseus-fner).
    for (nice, name) in &polled {
        assert_ne!(name, "learning", "a request polled on the low thread");
        if base < 19 {
            assert_eq!(*nice, base, "each request polled on a worker");
        }
    }
}
