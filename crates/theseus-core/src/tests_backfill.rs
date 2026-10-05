//! The backfill (M5 25d; design §3, "25d"), on scripted histories against
//! the fake Jev: its states equal the live builder's, byte for byte; each
//! event judged once, keyed by its event, its holdout time the event's; and
//! the run refused without the owner's consent, for a pack whose input
//! can't be rebuilt, and from a shared place.

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_judge::learn::DAY_MS;
use theseus_protocol::judge_runs::{JudgeAuditParams, JudgeBackfillParams};
use theseus_protocol::LedgerKind;

use crate::approval::{Answerer, Surface};
use crate::learning::backfill::day_start;
use crate::rpc::Core;
use crate::tests_judge::{rig_with, texts, turn, until_judged};
use crate::tests_replay::{rig, rows};

fn audit(c: &Core, sample: u32) -> JudgeAuditParams {
    JudgeAuditParams {
        pack: "loop".into(),
        sample,
        profile: c.live_profile().0,
        seed: Some(7),
    }
}

/// Backfill's states, built from the recorded history, equal what the live
/// point built at each turn's end, byte for byte; and a backfill of those
/// turns finds each judged already, and writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backfills_states_equal_the_live_builders_byte_for_byte() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(3), Some(&jev), |c| {
        c.judge.backfill_consent = true;
        c.judge
            .packs
            .insert("categorize.v1".into(), crate::tests_judge::off());
    });
    let c = &r.core;
    // Each turn's judgment lands before the next turn begins: the live
    // point reads the session as it stands when its task runs.
    let a = turn(c, None, "Draft the release notes for the herons.").await;
    until_judged(&c.store, 1).await;
    turn(c, Some(&a.session_id), "Go on.").await;
    until_judged(&c.store, 2).await;
    turn(c, None, "Count the wrens.").await;
    let live = until_judged(&c.store, 3).await;
    let pack = theseus_judge::pack::by_name("loop.v1").unwrap();
    let events = c.backfill_events(&pack, 0).unwrap();
    assert_eq!(events.len(), 3);
    for (_, row) in &live {
        let turn = row.data["context"]["turn"].as_str().unwrap();
        let blob = row.data["context"]["blob"].as_str().unwrap();
        let bytes = crate::blobs::decode(&c.store.blobs().base64(blob).unwrap()).unwrap();
        let e = events.iter().find(|e| e.turn_id == turn).unwrap();
        let nodes: Vec<_> = c
            .store
            .session_nodes(&e.session_id)
            .unwrap()
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        let rebuilt = c.backfill_state(&pack, e, &nodes).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            rebuilt.state.json,
            "turn {turn}"
        );
    }
    let since = crate::judge::spend::local_day(theseus_protocol::now_unix_ms());
    let out = c
        .judge_backfill(
            JudgeBackfillParams {
                pack: "loop.v1".into(),
                since,
            },
            "cli",
        )
        .await
        .unwrap();
    assert_eq!((out.events, out.already, out.judged), (3, 3, 0));
    assert!(rows(c, "judge:loop", LedgerKind::JudgeBackfill).is_empty());
}

/// Under consent, a backfill judges each event once, in shadow, keyed by
/// its event, with the live point's context and the event's time, which
/// the report's split reads; a second run writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_backfill_judges_each_event_once_under_consent() {
    let jev = FakeJev::start().unwrap();
    let r = rig(texts(3), &jev, |c| c.judge.backfill_consent = true);
    let c = &r.core;
    let a = turn(c, None, "Draft the release notes for the herons.").await;
    turn(c, None, "Count the wrens.").await;
    let since = crate::judge::spend::local_day(theseus_protocol::now_unix_ms());
    let p = JudgeBackfillParams {
        pack: "loop".into(),
        since: since.clone(),
    };
    let out = c.judge_backfill(p.clone(), "cli").await.unwrap();
    assert_eq!(
        (out.events, out.already, out.judged, out.failed),
        (2, 0, 2, 0)
    );
    assert!(out.left_out.is_empty(), "{:?}", out.left_out);
    assert_eq!(
        out.consent,
        crate::config_copy::sha256(&serde_json::to_string(&*c.cfg).unwrap())
    );
    assert_eq!(jev.seen().len(), 2);
    let calls = rows(c, "judge:loop", LedgerKind::JudgeCall);
    assert_eq!(calls.len(), 2);
    let mine = calls
        .iter()
        .find(|r| r.data["context"]["turn"] == json!(a.turn_id))
        .unwrap();
    let ctx = &mine.data["context"];
    assert_eq!(
        (
            ctx["purpose"].as_str(),
            ctx["session"].as_str(),
            ctx["decision"].as_str(),
            ctx["class"].as_str(),
            mine.data["budget"].as_str()
        ),
        (
            Some("backfill"),
            Some(a.session_id.as_str()),
            Some("no_tool_calls"),
            Some("reply"),
            Some("backfill")
        )
    );
    assert_eq!(
        mine.data["id"],
        json!(crate::learning::backfill::event_id(
            "loop.v1",
            &a.session_id,
            &a.turn_id
        ))
    );
    // Its holdout time is the event's.
    let scope = crate::learning::read_scope(&c.store, "loop").unwrap();
    let seen = &scope.judgments["loop.v1"];
    assert!(seen
        .iter()
        .all(|s| Some(s.at_ms) == s.judgment.context["event_at_ms"].as_u64()));
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeBackfill).len(), 1);
    // A second run writes nothing.
    let again = c.judge_backfill(p, "cli").await.unwrap();
    assert_eq!((again.events, again.already, again.judged), (2, 2, 0));
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeCall).len(), 2);
    assert_eq!(rows(c, "judge:loop", LedgerKind::JudgeBackfill).len(), 1);
    assert_eq!(jev.seen().len(), 2);
}

/// Without the owner's consent a backfill is refused, naming the line;
/// a pack whose input the store can't rebuild is refused with the reason;
/// and every run is the owner's, from a private place.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_backfill_needs_consent_and_a_rebuildable_pack() {
    let jev = FakeJev::start().unwrap();
    let r = rig(texts(1), &jev, |_| {});
    let c = &r.core;
    turn(c, None, "Count the wrens.").await;
    let since = crate::judge::spend::local_day(theseus_protocol::now_unix_ms());
    let p = |pack: &str| JudgeBackfillParams {
        pack: pack.into(),
        since: since.clone(),
    };
    let e = c
        .judge_backfill(p("loop.v1"), "cli")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("`backfill_consent = true` under [judge] in your config note"),
        "{e}"
    );
    assert!(jev.seen().is_empty());
    let r2 = rig(texts(0), &jev, |c| c.judge.backfill_consent = true);
    let e = r2
        .core
        .judge_backfill(p("rerank.v1"), "cli")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("rerank.v1 can't be backfilled: rerank's input"),
        "{e}"
    );
    let e = r2
        .core
        .judge_backfill(p("continue.v1"), "cli")
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("CONTINUE's input"), "{e}");
    let stranger = Answerer {
        label: "discord".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "42".into(),
            channel_id: "1001".into(),
            guild_id: Some("7".into()),
        }),
    };
    for refused in [
        r2.core
            .judge_backfill(p("loop.v1"), stranger.clone())
            .await
            .unwrap_err(),
        r2.core
            .judge_audit(audit(&r2.core, 1), stranger)
            .await
            .unwrap_err(),
    ] {
        assert!(
            refused.downcast_ref::<crate::approval::Refusal>().is_some(),
            "{refused:#}"
        );
    }
    let acts: Vec<Value> = r2
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(50)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == "approval.refused")
        .map(|(_, r)| r.data["act"].clone())
        .collect();
    assert_eq!(acts, [json!("judge.backfill"), json!("judge.audit")]);
    assert!(jev.seen().is_empty());
}

/// `--since` is a local day, read as the midnight that begins it.
#[test]
fn since_is_a_local_day() {
    let now = theseus_protocol::now_unix_ms();
    let day = crate::judge::spend::local_day(now);
    assert_eq!(
        day_start(&day).unwrap(),
        crate::learning::local_midnight(now)
    );
    assert_eq!(
        day_start(&crate::judge::spend::local_day(now - DAY_MS)).unwrap(),
        crate::learning::local_midnight(now - DAY_MS)
    );
    assert!(day_start("yesterday").is_err());
    assert!(day_start("2026-13-01").is_err());
}
