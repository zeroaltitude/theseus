//! Money (theseus-3p36, theseus-jse4): a call a stop cut is booked with its
//! input split as its request was sent (`turn/cut_step.rs`), the previous
//! call's cache at the read price, under the old all-plain-input figure and
//! never past its reservation, and with nothing cached as before; a limit
//! that notifies stops at its hard multiple with its own words
//! (`[kernel] spend_limit_hard_multiple`); and a multiple is noticed at the
//! settle that crosses it, a cut call's too.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult, Usage};

use crate::bus::EventSink;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const PLACE: &str = "dm:5521";
const TARGET: &str = "discord:dm:5521";
const STOPPER: &str = "discord:zeroaltitude";

/// The scripted answers, in order, except the call numbered `hold` (from 0),
/// which says it began and never answers: a stop cuts it.
struct Holds {
    fake: FakeProvider,
    hold: usize,
    calls: AtomicUsize,
    entered: tokio::sync::mpsc::UnboundedSender<()>,
}

impl Provider for Holds {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::Relaxed) == self.hold {
                let _ = self.entered.send(());
                std::future::pending::<()>().await;
            }
            self.fake.stream_message(req, on_delta).await
        })
    }
}

/// A call that diffs, billed as `usage`.
fn diffing(n: usize, usage: Usage) -> Scripted {
    Scripted::Billed {
        usage,
        then: Box::new(Scripted::tools(
            "Diffing.",
            &[(
                &format!("t{n}"),
                "text_diff",
                json!({"a": "x\n", "b": "y\n"}),
            )],
        )),
    }
}

/// 2,000 tokens in and 30,000 out: about $0.304 on the template's live
/// profile, whose reservation is past $1.28.
fn plain() -> Usage {
    Usage {
        input_tokens: 2_000,
        output_tokens: 30_000,
        ..Usage::default()
    }
}

struct Rig {
    core: Arc<Core>,
    sid: String,
    _dir: tempfile::TempDir,
}

fn rig(provider: Arc<dyn Provider>, cfg: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Config::example();
    c.server.state_dir = dir.path().to_string_lossy().into_owned();
    c.tools.roots = vec![];
    cfg(&mut c);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let core = Core::build(crate::rpc::Parts::for_tests(c, provider, store)).unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place(PLACE, &rec.session_id).unwrap();
    Rig {
        sid: rec.session_id,
        core,
        _dir: dir,
    }
}

async fn turn(core: Arc<Core>, sid: String) -> anyhow::Result<TurnSubmitResult> {
    let session: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            session,
            input: Some("diff these, again and again".into()),
            target,
            sink: EventSink::new(core.bus.clone(), &sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

impl Rig {
    fn rows(&self, kind: &str) -> Vec<Value> {
        let rows: Vec<(u64, crate::ledger::LedgerRow)> = self.core.store.ledger_tail(5000).unwrap();
        rows.into_iter()
            .filter(|(_, r)| r.kind == kind && r.session_id.as_deref() == Some(self.sid.as_str()))
            .map(|(_, r)| r.data)
            .collect()
    }

    fn exec(&self) -> theseus_kernel::Execution {
        let rec: SessionRecord = self.core.store.get_session(&self.sid).unwrap().unwrap();
        let id = rec.execution_id.unwrap();
        self.core.kernel.execution(&id).unwrap().unwrap()
    }

    fn notices(&self) -> Vec<String> {
        self.core
            .outbox
            .open_for(TARGET)
            .iter()
            .filter(|a| crate::outbox::kind_of(a) == "notice")
            .map(|a| {
                crate::outbox::body_of(a)["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    }

    /// What the cut call's whole input estimate would have cost as plain
    /// input, the old figure, at its model's prices.
    fn plain_figure(&self, cut: &Value) -> u64 {
        let price = self
            .core
            .runner
            .catalog
            .get(cut["model"].as_str().unwrap())
            .unwrap();
        let est = cut["input_tokens"].as_u64().unwrap()
            + cut["cache_read_input_tokens"].as_u64().unwrap()
            + cut["cache_creation_input_tokens"].as_u64().unwrap();
        price.cost_micros(&Usage {
            input_tokens: est,
            output_tokens: cut["output_tokens"].as_u64().unwrap(),
            ..Usage::default()
        })
    }
}

/// Run a turn whose call numbered `hold` is cut by a stop; the cut's
/// `provider.cut` row.
async fn cut_at(hold: usize, script: Vec<Scripted>, cfg: impl FnOnce(&mut Config)) -> (Rig, Value) {
    let (tx, mut entered) = tokio::sync::mpsc::unbounded_channel();
    let provider = Arc::new(Holds {
        fake: FakeProvider::scripted(script),
        hold,
        calls: AtomicUsize::new(0),
        entered: tx,
    });
    let r = rig(provider, cfg);
    let running = tokio::spawn(turn(r.core.clone(), r.sid.clone()));
    entered.recv().await.unwrap();
    let eid = r.exec().id;
    let stop = r.core.stop_execution(&eid, STOPPER).await.unwrap();
    assert!(stop.stopped && stop.turn_running, "{stop:?}");
    let res = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("the stop cut the call")
        .unwrap()
        .unwrap();
    assert_eq!(res.stop_reason, "stopped");
    let cut = r.rows("provider.cut");
    assert_eq!(cut.len(), 1, "{cut:?}");
    let cut = cut[0].clone();
    (r, cut)
}

/// theseus-3p36: a turn's second call cut after its first read 20,000
/// tokens from the cache and wrote 3,000. The cut's input is the cached
/// prefix at the read price (the whole estimate here, a small request), so
/// it is booked well under the old figure, the estimate as plain input, and
/// within its reservation; the row and the completion say how it was split.
#[tokio::test]
async fn a_cut_call_whose_prefix_was_cached_is_booked_at_the_split_price() {
    let cached = Usage {
        input_tokens: 40,
        output_tokens: 200,
        cache_read_input_tokens: 20_000,
        cache_creation_input_tokens: 3_000,
        ..Usage::default()
    };
    let (r, cut) = cut_at(1, vec![diffing(1, cached)], |_| {}).await;
    let split = cut["split"].as_str().unwrap();
    assert!(split.starts_with("cache_read"), "{cut}");
    assert_eq!(cut["estimated"], true);
    let read = cut["cache_read_input_tokens"].as_u64().unwrap();
    assert!(read > 0 && read <= 23_000, "{cut}");
    let cost = theseus_kernel::usd_to_micros(cut["cost_usd"].as_f64().unwrap());
    let reserved = theseus_kernel::usd_to_micros(cut["reserved_usd"].as_f64().unwrap());
    let plain = r.plain_figure(&cut);
    assert!(
        cost > 0 && cost * 5 < plain,
        "{cost} against {plain}: {cut}"
    );
    assert!(cost <= reserved);
    // The session's books hold the answered call and the cut one.
    assert!(r.exec().budget.spent_micros > cost);
}

/// A cut with no cache read (the turn's first call) is booked as before:
/// the whole estimate as plain input, capped at the reservation.
#[tokio::test]
async fn a_cut_with_no_cache_read_prices_as_before() {
    let (r, cut) = cut_at(0, vec![], |_| {}).await;
    assert_eq!(cut["split"], "uncached", "{cut}");
    assert_eq!(
        (
            &cut["cache_read_input_tokens"],
            &cut["cache_creation_input_tokens"]
        ),
        (&json!(0), &json!(0))
    );
    let cost = theseus_kernel::usd_to_micros(cut["cost_usd"].as_f64().unwrap());
    let reserved = theseus_kernel::usd_to_micros(cut["reserved_usd"].as_f64().unwrap());
    assert_eq!(cost, r.plain_figure(&cut).min(reserved));
    assert_eq!(r.exec().budget.spent_micros, cost);
}

/// theseus-jse4: a $0.50 limit that notifies, and a model that diffs for
/// ever. The calls go out past the limit, each multiple noticed, until the
/// next call's reservation would take the spend past ten times the limit:
/// that call is not made, the turn fails `spend_hard_stop` with the stop's
/// words, and the spend never passes $5.
#[tokio::test]
async fn a_notifying_session_stops_at_ten_times_its_limit() {
    let script: Vec<Scripted> = (1..=40).map(|n| diffing(n, plain())).collect();
    let fake = Arc::new(FakeProvider::scripted(script));
    let r = rig(fake.clone(), |c| c.kernel.spend_limit_usd = 0.50);
    let err = turn(r.core.clone(), r.sid.clone()).await.unwrap_err();
    let failed = err.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(failed.class, "spend_hard_stop");
    let words = format!("{err:#}");
    assert!(
        words.contains("hard stop, 10 times its $0.50 limit ($5)")
            && words.contains("spend_limit_hard_multiple")
            && words.contains("the call was not made"),
        "{words}"
    );
    let spent = r.exec().budget.spent_micros;
    assert!(spent <= 5_000_000, "{spent}");
    let calls = fake.requests().len();
    assert!(calls >= 10, "{calls} calls went out first");
    // The refused call's reservation was what would have passed the stop.
    let planned = r.rows("action.planned");
    let reserve = theseus_kernel::usd_to_micros(
        planned
            .iter()
            .rev()
            .find(|p| p["tool"] == "provider.messages")
            .unwrap()["reserved_usd"]
            .as_f64()
            .unwrap(),
    );
    assert!(spent + reserve > 5_000_000, "{spent} + {reserve}");
    assert_eq!(
        planned
            .iter()
            .filter(|p| p["tool"] == "provider.messages")
            .count(),
        calls
    );
    // Every multiple below the stop was noticed, and each notice names the stop.
    let multiples: Vec<u64> = r
        .rows("budget.reached")
        .iter()
        .map(|x| x["multiple"].as_u64().unwrap())
        .collect();
    assert_eq!(multiples, (1..=(spent / 500_000)).collect::<Vec<_>>());
    let notices = r.notices();
    assert!(
        notices[0].contains("At 10 times the limit, $5, its calls stop"),
        "{}",
        notices[0]
    );
    // The record says why the turn ended.
    let failed = r.rows("turn.failed");
    assert!(
        failed
            .iter()
            .any(|f| f.to_string().contains("spend_hard_stop")),
        "{failed:?}"
    );
}

/// theseus-jse4: a multiple is noticed at the settle that crosses it, a cut
/// call's too, not at the next multiple: a limit far under one call's input
/// (and no hard stop), the turn's first call cut by a stop. Its estimate
/// crosses the limit many times over, and the cut's own settle posts the
/// notice.
#[tokio::test]
async fn a_multiple_crossed_by_a_cut_calls_settle_is_noticed_then() {
    let (r, cut) = cut_at(0, vec![], |c| {
        c.kernel.spend_limit_usd = 0.000_1;
        c.kernel.spend_limit_hard_multiple = 0;
    })
    .await;
    let spent = r.exec().budget.spent_micros;
    assert_eq!(
        spent,
        theseus_kernel::usd_to_micros(cut["cost_usd"].as_f64().unwrap())
    );
    let reached = r.rows("budget.reached");
    assert_eq!(reached.len(), 1, "{reached:?}");
    assert_eq!(reached[0]["multiple"].as_u64().unwrap(), spent / 100);
    assert_eq!(r.notices().len(), 1);
}
