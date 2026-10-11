//! Money (theseus-3p36): a call a stop cut is booked with its input split as
//! its request was sent (`turn/cut_step.rs`), the previous call's cache at
//! the read price, under the old all-plain-input figure and never past its
//! reservation, and with nothing cached as before.

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
