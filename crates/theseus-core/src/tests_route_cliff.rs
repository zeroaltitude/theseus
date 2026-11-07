//! The prompt cliff (theseus-ataas): which model a turn gets must not turn on
//! the system prompt's byte count.
//!
//! The cache hold ([`crate::routing::model::decide`]) weighs the session's own
//! context against `[routing] cold_switch_tokens`. Until 2026-11-07 it weighed
//! the whole compile's estimate instead, and the universal prefix — the tool
//! definitions and the header block, which every session of every place class
//! carries identically — is most of that estimate. An ordinary routed turn
//! therefore sat four tokens under the bound, and eighteen more characters in
//! the model-facing tools note (`toolrun.rs`, where the approve list's line was
//! replaced at theseus-c4i8m) moved the turn from Opus to Sonnet. Two unrelated
//! tests had been tuned to ride that line, and `tests_route_keep`'s constants
//! record four earlier re-tunings for the same cause: a tool joining the
//! catalog, a persona's line, `routine.create`'s schema, `task_fan_out`.
//!
//! So: growing the header must move no model. These tests grow it the way the
//! owner grows it — a standing order, which `compiler::layers` renders into
//! layer 1 beside the tools note — and hold every model the route pack can
//! pick to the answer it gave without it.

use theseus_judge::fake::FakeJev;

use crate::tests_route::{mode, rig, turn, Rig};
use theseus_protocol::{StandingAddParams, StandingKind, StandingScope};

/// A message long enough that an ordinary turn's estimate is near the bound,
/// as the home-size rig's is (`tests_home_size::long`).
fn long() -> String {
    "the design chat goes on and on. ".repeat(1_000)
}

/// Roughly 600 characters of header, about 180 tokens: far more than the
/// eighteen that moved a model, and far less than any sane bound.
fn more_header(r: &Rig) {
    r.core
        .standing_add(
            &StandingAddParams {
                kind: StandingKind::Order,
                text: "when the tide gauges disagree, say which one you trusted and why, \
                       and name the reading you set aside, so the next reader can check it \
                       without opening the log; the same goes for a wind reading taken \
                       while the lamp was being lit, which the keeper's own notes have \
                       always treated as provisional rather than as a measurement"
                    .into(),
                scope: StandingScope::Everywhere,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
}

/// The routed profile and why, for one message on a fresh session.
async fn routed(m: &str, grow: bool) -> (String, String) {
    let jev = FakeJev::start().unwrap();
    mode(&jev, m, 0.95);
    let r = rig(Some(&jev), 2, |_| {});
    if grow {
        more_header(&r);
    }
    let res = turn(&r.core, None, &long(), None).await;
    let why = res
        .route
        .as_ref()
        .map_or_else(|| "none".to_string(), |x| x.reason.clone());
    (res.profile, why)
}

/// Every model the pack can pick, with and without the extra header: the same
/// answer both times. Red before theseus-ataas for `sophisticated` (opus) and
/// `trivial` (haiku): the grown header put the estimate over
/// `cold_switch_tokens` and both came back `sonnet`/`cache_hold`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_longer_system_prompt_changes_no_model() {
    // `chat` stays on the session's own sonnet: it is here so a reader can see
    // that the no-switch case is held too, and that the set is every answer
    // `tests_route::mode` can give.
    for m in ["sophisticated", "trivial", "chat"] {
        let plain = routed(m, false).await;
        let grown = routed(m, true).await;
        assert_eq!(
            plain, grown,
            "{m}: 600 characters of header moved the model"
        );
    }
}

/// The measurement the fix rests on, so the numbers in `routing::model`'s doc
/// are not folklore: the universal prefix is most of an ordinary request, and
/// what a one-message session actually carries of its own is a small part of
/// `cold_switch_tokens`.
///
/// The figures on 2026-11-07, on this rig: the whole estimate 29,996 against a
/// bound of 30,000 — four tokens, which is what made the cliff — and the
/// session's own context about 9,000. Those are recorded here and not
/// asserted: an assertion on the request's size is the very thing this row
/// exists to take out of the suite.
///
/// It is not a bound on how big the prefix may grow — it is the statement that
/// the prefix and the session's context are different sizes and must not be
/// weighed as one number. If a later release makes the prefix small, the
/// `assert!(fixed > context)` here is the line to re-read rather than to
/// delete: it would mean the cliff of 2026-11-07 can no longer happen.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ordinary_turn_carries_far_less_context_than_the_bound() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 2, |_| {});
    let res = turn(&r.core, None, &long(), None).await;
    assert_eq!(res.profile, "opus", "it switched at once");

    let rows = crate::tests_route::decided(&r.core.store);
    let est = rows[0]["est_tokens"].as_u64().expect("the row's estimate");
    let cold = r.core.cfg.routing.cold_switch_tokens;

    let q = r.claude.requests().pop().expect("the request it sent");
    let len = |v: &serde_json::Value| serde_json::to_string(v).map_or(0, |s| s.len() as u64);
    // The universal prefix as `CacheLayout::universal_prefix_bytes` takes it —
    // every tool definition and the first system block, the header — in tokens
    // as `TurnRunner::fixed_prefix_tokens` converts it.
    let bytes: u64 =
        q.tools.iter().map(len).sum::<u64>() + q.system.first().map_or(0, |b| len(b));
    let rates = crate::catalog::TokenRates::of(&q.model);
    let fixed = (bytes as f64 / rates.json.max(rates.text)) as u64;
    let context = est.saturating_sub(fixed);
    assert!(
        fixed > context,
        "the universal prefix is most of the request: prefix {fixed}, the session's {context}"
    );
    assert!(
        context * 2 < cold,
        "one message is nowhere near the bound: {context} of {cold}"
    );
}
