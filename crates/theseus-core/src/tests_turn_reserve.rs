//! A turn's own provider call is reserved on the input estimate's upper
//! bound, as a compaction summary's is (theseus-ps9i):
//! - the call's reservation is the price of its `max_tokens` plus its
//!   request's `upper`, not its `tokens`, when the request has an estimated
//!   tail (a new session: nothing is counted yet);
//! - a provider that counts the input 11 % over the estimate's tokens and
//!   writes the most it may settles within the reservation.

use std::sync::Arc;

use theseus_protocol::{SessionKind, Usage};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const MODEL: &str = "claude-sonnet-5-5";

fn core_with(dir: &std::path::Path, model: Arc<FakeProvider>) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let store = Store::open(&dir.join("store")).unwrap();
    Core::build(crate::rpc::Parts::for_tests(cfg, model, store)).unwrap()
}

/// What a provider that counts 11 % more input than the estimate's tokens,
/// and writes all `max_tokens`, bills a turn's call.
fn over_the_estimate(q: &ProviderRequest) -> Usage {
    let est = crate::compiler::estimate(q, rates(), None);
    Usage {
        input_tokens: (est.tokens * 111).div_ceil(100),
        output_tokens: u64::from(q.max_tokens),
        ..Default::default()
    }
}

fn rates() -> crate::catalog::TokenRates {
    crate::catalog::Catalog::builtin()
        .get(MODEL)
        .unwrap()
        .bytes_per_token
}

/// One turn of a new session on `model`; returns the core and its session.
async fn first_turn(model: Scripted) -> (Arc<Core>, Arc<FakeProvider>, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![model]));
    let core = core_with(dir.path(), fake.clone());
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec.clone(),
            input: Some("What did the keeper log before the lamp was lit?".into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap();
    (core, fake, rec.session_id, dir)
}

/// The kernel's one model call: what it reserved, and what the execution
/// spent in all.
fn the_call(core: &Core, sid: &str) -> (u64, u64) {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let exec = core
        .kernel
        .execution(rec.execution_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(exec.budget.reserved_micros, 0, "nothing is left held");
    let actions = core.kernel.actions().unwrap();
    let call = actions
        .iter()
        .find(|a| a.session_id == sid && a.reserved_micros > 0)
        .expect("the turn's call");
    (call.reserved_micros, exec.budget.spent_micros)
}

#[tokio::test]
async fn a_turns_call_is_reserved_on_the_estimates_upper_bound() {
    let (core, fake, sid, _dir) = first_turn(Scripted::text("The lamp was trimmed.")).await;
    let q = fake.requests().pop().unwrap();
    let est = crate::compiler::estimate(&q, rates(), None);
    assert!(est.upper > est.tokens, "an estimated tail: {est:?}");
    let price = crate::catalog::Catalog::builtin()
        .get(MODEL)
        .unwrap()
        .clone();
    let (reserved, _) = the_call(&core, &sid);
    assert_eq!(reserved, price.reserve_micros(q.max_tokens, est.upper));
    assert!(reserved > price.reserve_micros(q.max_tokens, est.tokens));
}

#[tokio::test]
async fn a_provider_counting_over_the_estimate_settles_within_the_reservation() {
    let (core, _fake, sid, _dir) = first_turn(Scripted::BilledBy {
        usage: over_the_estimate,
        then: Box::new(Scripted::text("The lamp was trimmed.")),
    })
    .await;
    let (reserved, spent) = the_call(&core, &sid);
    println!("reserved {reserved} micros, settled {spent}");
    assert!(
        spent <= reserved,
        "settled {spent} past reserved {reserved}"
    );
    assert!(spent > 0);
}
