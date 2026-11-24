//! The sampler (own-benchmark): a few turns per cell, one rung down the
//! routing table, judged, under the day's cap; a shared place's turn too;
//! and a turn with an attachment replays on another provider like any other.

use std::sync::Arc;

use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::own_bench::TurnReplayParams;
use theseus_protocol::LedgerKind;

use super::sampler::{pick, Candidate, PER_CELL};
use super::tests::{asked, rig, rig_compare_only, script};
use crate::fact::own_bench::RouteReplayJudged;
use crate::provider::FakeProvider;
use crate::tests_recall::{session, turn, Rig, PIER};

fn cand(turn: &str, model: &str, effort: &str, at_ms: u64) -> Candidate {
    Candidate {
        session: "ses_a".into(),
        turn: turn.into(),
        model: model.into(),
        effort: effort.into(),
        at_ms,
        cost_usd: 0.1,
    }
}

/// Newest first, at most `PER_CELL` a cell, a turn already judged left out.
#[test]
fn the_pick_is_the_newest_few_per_cell_and_none_twice() {
    let mut all: Vec<Candidate> = (0..6)
        .map(|i| cand(&format!("t{i}"), "sonnet", "high", i))
        .collect();
    all.push(cand("t9", "sonnet", "", 9));
    all.push(cand("t8", "fable", "xhigh", 8));
    let done = ["t5".to_string()].into_iter().collect();
    let got = pick(all, &done, PER_CELL);
    let turns = |m: &str, e: &str| -> Vec<String> {
        got[&(m.to_string(), e.to_string())]
            .iter()
            .map(|c| c.turn.clone())
            .collect()
    };
    assert_eq!(turns("sonnet", "high"), ["t4", "t3", "t2"]);
    assert_eq!(turns("sonnet", ""), ["t9"]);
    assert_eq!(turns("fable", "xhigh"), ["t8"]);
}

fn jev_same() -> FakeJev {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "compare.v1/verdict",
        Jev::Choice {
            option: "same".into(),
            confidence: 0.9,
        },
    );
    jev
}

fn judged_rows(r: &Rig) -> Vec<crate::ledger::LedgerRow> {
    r.core.rows_since(LedgerKind::RouteReplayJudged, 0).unwrap()
}

fn now() -> u64 {
    theseus_protocol::now_unix_ms()
}

/// Four turns on one cell: three are replayed (the cell's share), each on the
/// rung down the routing table (Haiku under Sonnet), each judged, a row each;
/// a second pass replays the fourth and no turn twice.
#[tokio::test]
async fn the_sampler_replays_a_cells_turns_one_rung_down_and_judges_each() {
    let jev = jev_same();
    // This test counts judged replays, so nothing else may judge on the
    // client: see `rig_compare_only`.
    let r = rig_compare_only(&jev);
    script(&r, &["a", "b", "c", "d", "r1", "r2", "r3", "r4"]);
    let mut turns = Vec::new();
    for _ in 0..4 {
        turns.push(asked(&r).await);
    }
    let out = r.core.replay_sample(now()).await.unwrap();
    // A replay that was not judged says why in its own row's `unjudged`;
    // `Sampled`'s debug does not carry it, so a bare `(3, 2)` names no
    // mechanism at all and reads like a miscount.
    let unjudged: Vec<String> = judged_rows(&r)
        .iter()
        .map(|row| {
            format!(
                "verdict={} unjudged={}",
                row.data["verdict"], row.data["unjudged"]
            )
        })
        .collect();
    assert_eq!(
        (out.replayed, out.judged),
        (3, 3),
        "{out:?} rows={unjudged:?}"
    );
    assert_eq!(out.stopped, None);
    let rows = judged_rows(&r);
    assert_eq!(rows.len(), 3);
    for row in &rows {
        assert_eq!(row.data["to_model"], "claude-haiku-5-5", "{}", row.data);
        assert_eq!(row.data["from_model"], "claude-sonnet-5-5");
        assert_eq!(row.data["verdict"], "same");
        assert_eq!(row.data["by"], "sampler");
        assert!(row.data["same_or_better"].as_f64().unwrap() > 0.8);
    }
    // The newest three; the oldest waits for the next night.
    let replayed: Vec<&str> = rows
        .iter()
        .filter_map(|r| r.data["turn"].as_str())
        .collect();
    assert!(!replayed.contains(&turns[0].1.as_str()), "{replayed:?}");
    let again = r.core.replay_sample(now()).await.unwrap();
    assert_eq!(again.replayed, 1, "the fourth, and no turn twice");
    assert_eq!(judged_rows(&r).len(), 4);
    // And the report reads them.
    let rep = r.core.route_report_now(true, now()).unwrap();
    let cell = rep
        .cells
        .iter()
        .find(|c| c.model == "claude-sonnet-5-5")
        .unwrap();
    assert_eq!((cell.turns, cell.replayed, cell.matched), (4, 4, 4));
    assert_eq!(
        cell.rung_down.as_deref().map(|p| p.starts_with("haiku")),
        Some(true)
    );
    assert!(cell.match_floor.unwrap() > 0.5 && cell.replay_usd > 0.0);
    assert!(rep.replay_today_usd > 0.0 && (rep.replay_day_usd - 1.0).abs() < 1e-9);
}

/// The cap is checked before the call: a day's cap under one replay's
/// reservation makes no call at all and says why.
#[tokio::test]
async fn the_cap_is_checked_before_the_call() {
    let jev = jev_same();
    let r = crate::tests_recall::rig_with_secrets(
        crate::config::MemoryMode::Off,
        super::tests::board(),
        |c| {
            c.judge.enabled = true;
            c.judge.api_base = jev.base();
            c.route_bench.replay_day_usd = 0.000_001;
        },
    );
    script(&r, &["a", "r1"]);
    asked(&r).await;
    let calls = r.model.requests().len();
    let out = r.core.replay_sample(now()).await.unwrap();
    assert_eq!(out.replayed, 0);
    assert!(
        out.stopped
            .as_deref()
            .unwrap_or("")
            .contains("under the cap"),
        "{out:?}"
    );
    assert_eq!(
        r.model.requests().len(),
        calls,
        "no model was called past the cap"
    );
    assert!(judged_rows(&r).is_empty());
}

/// A day whose rows already spent the cap makes no replay.
#[tokio::test]
async fn a_day_that_spent_its_cap_replays_nothing() {
    let jev = jev_same();
    let r = rig(Some(&jev));
    script(&r, &["a", "r1"]);
    let (sid, t) = asked(&r).await;
    r.core
        .rec(Some(&sid))
        .in_turn(Some(&t))
        .record(&RouteReplayJudged {
            session: &sid,
            turn: "trn_other",
            from_model: "claude-sonnet-5-5",
            from_effort: "",
            to_profile: "haiku",
            to_model: "claude-haiku-5-5",
            to_effort: "",
            verdict: None,
            p: None,
            same_or_better: None,
            judgment: None,
            unjudged: Some("earlier"),
            orig_usd: 0.0,
            replay_usd: 0.6,
            judge_usd: 0.4,
        });
    let calls = r.model.requests().len();
    let out = r.core.replay_sample(now()).await.unwrap();
    assert_eq!(out.replayed, 0);
    assert!(
        out.stopped
            .as_deref()
            .unwrap_or("")
            .contains("is spent for the day"),
        "{out:?}"
    );
    assert_eq!(r.model.requests().len(), calls);
}

/// `replay_day_usd = 0` turns it off.
#[tokio::test]
async fn a_zero_cap_turns_the_sampler_off() {
    let r = crate::tests_recall::rig_with(crate::config::MemoryMode::Off, |c| {
        c.route_bench.replay_day_usd = 0.0;
    });
    let out = r.core.replay_sample(now()).await.unwrap();
    assert!(out.stopped.unwrap().contains("off"));
}

/// Anything goes (2026-11-22): a shared place's turn is a candidate like a private one; both are sampled.
#[tokio::test]
async fn the_sampler_takes_a_shared_places_turn_like_any_other() {
    let jev = jev_same();
    let r = rig(Some(&jev));
    script(&r, &["a", "b", "r1", "r2"]);
    let shared = session(&r.core, Some(&format!("channel:{PIER}")), &[]);
    let st = turn(&r.core, &shared, "Which port?").await.turn_id;
    let (private, t) = asked(&r).await;
    let out = r.core.replay_sample(now()).await.unwrap();
    assert_eq!(out.replayed, 2, "{out:?}");
    assert_eq!(out.skipped.get("shared_place"), None);
    let rows = judged_rows(&r);
    let turns: Vec<_> = rows.iter().filter_map(|r| r.turn_id.clone()).collect();
    assert!(turns.contains(&st), "the shared place's turn: {turns:?}");
    assert!(turns.contains(&t), "the private one: {turns:?}");
    let sessions: Vec<_> = rows.iter().filter_map(|r| r.session_id.clone()).collect();
    assert!(sessions.contains(&private) && sessions.contains(&shared));
}

/// A rig whose `glm` profile has a provider of its own.
fn two_providers() -> (Rig, Arc<FakeProvider>) {
    let zai = Arc::new(FakeProvider::default());
    let z = zai.clone();
    let r = crate::tests_recall::rig_built(
        crate::config::MemoryMode::Off,
        // Only a second provider here: a replay to a restricted-origin
        // profile is refused earlier (theseus-a1wvq.10).
        |c| {
            // Dearer than Haiku and cheaper than Sonnet, so it is the rung
            // down from the turn's model.
            c.serve_glm_as("claude-sonnet-5");
            c.catalog.insert(
                "claude-sonnet-5".into(),
                crate::catalog::CatalogRow {
                    input_per_mtok: Some(1.0),
                    output_per_mtok: Some(5.0),
                    ..Default::default()
                },
            );
        },
        move |p| {
            p.providers.insert("zai".into(), z);
        },
    );
    (r, zai)
}

fn png_turn(core: &Arc<crate::Core>, sid: &str) -> crate::turn::TurnRequest {
    let rec: crate::session::SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let bytes = crate::attach::tests::png(8, 8, 0);
    crate::turn::TurnRequest {
        prompt: None,
        session: rec,
        input: Some("What is in this picture?".into()),
        target,
        sink: crate::bus::EventSink::new(core.bus.clone(), sid, None),
        author: "test".into(),
        attachments: vec![theseus_protocol::Attachment {
            name: "shot.png".into(),
            media_type: "image/png".into(),
            size: bytes.len() as u64,
            data: Some(crate::blobs::encode(&bytes)),
            ..Default::default()
        }],
        arrived: None,
        reply_to: None,
        surface: None,
        answer_at: None,
    }
}

/// Anything goes (2026-11-22): a turn with an attachment replays on a profile of another provider, the
/// attachment sent with it, as on one of its own.
#[tokio::test]
async fn an_attachment_replays_on_another_provider_like_any_other() {
    let (r, zai) = two_providers();
    script(&r, &["It is a screenshot.", "A picture."]);
    let sid = session(&r.core, None, &[]);
    let t = r
        .core
        .runner
        .run(png_turn(&r.core, &sid))
        .await
        .unwrap()
        .turn_id;
    let p = |profile: &str| TurnReplayParams {
        session: sid.clone(),
        turn: t.clone(),
        profile: profile.into(),
        compare: Some(false),
        ..Default::default()
    };
    let other = r.core.turn_replay(p("glm"), "cli").await.unwrap();
    assert_eq!(other.profile, "glm");
    assert_eq!(other.turn, t);
    let sent = zai.requests();
    assert_eq!(sent.len(), 1, "the other provider was sent the replay");
    assert!(
        serde_json::to_string(&sent[0].messages)
            .unwrap()
            .contains("\"image\""),
        "with its attachment"
    );
    let ok = r.core.turn_replay(p("haiku"), "cli").await.unwrap();
    assert_eq!(ok.replay.text, "A picture.");
}

/// A report over nothing says $0.00, never a negative zero.
#[tokio::test]
async fn an_empty_report_totals_a_positive_zero() {
    let r = rig(None);
    let rep = r.core.route_report_now(false, now()).unwrap();
    assert!(rep.cells.is_empty());
    for v in [
        rep.total_spend_usd,
        rep.total_movable_usd,
        rep.replay_today_usd,
    ] {
        assert!(v == 0.0 && v.is_sign_positive(), "{v}");
    }
}

/// The report reads a draft and a skip from the ledger (theseus-a1wvq.12):
/// a `task.draft` row is a cell, and a turn's `route.decided` `draft_skip`
/// is a count.
#[tokio::test]
async fn the_report_reads_a_draft_from_the_ledger() {
    let r = rig(None);
    script(&r, &["fine"]);
    let (sid, t) = asked(&r).await;
    let put = |kind: &str, turn: Option<&str>, data: serde_json::Value| {
        let row = crate::ledger::LedgerRow::named(kind, Some(&sid), turn, data);
        r.core.store.append_ledger(&row).unwrap();
    };
    put(
        "task.draft",
        None,
        serde_json::json!({
            "draft_model": "deepseek-flash", "review_model": "claude-sonnet-5-5",
            "draft_cost_usd": 0.05, "review_cost_usd": 0.25,
            "lines_changed": 40, "applied": true, "draft_outcome": "complete",
        }),
    );
    put(
        "route.decided",
        Some(&t),
        serde_json::json!({"draft_skip": "dir_not_open"}),
    );
    let rep = r.core.route_report_now(false, now()).unwrap();
    assert_eq!(rep.drafts.len(), 1, "{:?}", rep.drafts);
    assert_eq!(rep.drafts[0].draft_model, "deepseek-flash");
    assert_eq!(rep.drafts[0].lines_changed, 40);
    assert!((rep.drafts[0].total_usd - 0.30).abs() < 1e-9);
    let skips: Vec<_> = rep.draft_skips.iter().map(|g| (&*g.why, g.turns)).collect();
    assert_eq!(skips, [("dir_not_open", 1)]);
}

/// Admission and settlement share the replay's allowance, including failed judgments.
#[tokio::test]
async fn comparison_respects_allowance_and_retains_failed_charges() {
    use theseus_judge::builders::{CompareInput, CompareReply};
    let jev = jev_same();
    let r = rig_compare_only(&jev);
    let input = || CompareInput {
        request: "compare".into(),
        a: CompareReply {
            text: "a".into(),
            calls: vec![],
        },
        b: CompareReply {
            text: "b".into(),
            calls: vec![],
        },
    };
    let mut charged = 99;
    let denied = r
        .core
        .runner
        .judge
        .compare_replies("s", "t", input(), Some(0), &mut charged)
        .await;
    assert!(denied.unwrap_err().contains("replay allowance"));
    assert_eq!(charged, 0);
    assert!(jev.seen().is_empty(), "refused before dispatch");
    let good = r
        .core
        .runner
        .judge
        .compare_replies("s", "t", input(), Some(1_000_000), &mut charged)
        .await
        .unwrap();
    assert!(charged > 0);
    assert_eq!(theseus_kernel::usd_to_micros(good.cost_usd), charged);
    jev.set_mode(theseus_judge::fake::FakeMode::Malformed);
    let bad = r
        .core
        .runner
        .judge
        .compare_replies("s", "t", input(), Some(1_000_000), &mut charged)
        .await;
    assert!(bad.is_err());
    assert!(charged > 0, "failed paid comparison must remain charged");
}

#[tokio::test]
async fn sampled_failed_comparison_is_charged_in_the_replay_day() {
    let jev = jev_same();
    let r = rig_compare_only(&jev);
    script(&r, &["original", "replayed"]);
    asked(&r).await;
    jev.set_mode(theseus_judge::fake::FakeMode::Malformed);
    let sampled = r.core.replay_sample(now()).await.unwrap();
    assert_eq!(sampled.replayed, 1);
    assert_eq!(sampled.judged, 0);
    let rows = judged_rows(&r);
    assert_eq!(rows.len(), 1);
    let judge = rows[0].data["judge_usd"].as_f64().unwrap();
    let replay = rows[0].data["replay_usd"].as_f64().unwrap();
    assert!(
        judge > 0.0,
        "charged failed judgment must reach the day ledger"
    );
    assert!((sampled.spent_usd - judge - replay).abs() < 0.000_002);
    let report = r.core.route_report_now(true, now()).unwrap();
    assert!((report.replay_today_usd - sampled.spent_usd).abs() < 0.000_002);
}

#[tokio::test]
async fn replay_that_spends_its_allowance_does_not_dispatch_comparison() {
    let jev = jev_same();
    let r = rig_compare_only(&jev);
    script(&r, &["original"]);
    let (session, turn) = asked(&r).await;
    let p = TurnReplayParams {
        session: session.clone(),
        turn: turn.clone(),
        profile: "haiku".into(),
        effort: None,
        scope: None,
        compare: Some(true),
    };
    let original = r.core.rebuild_first(&session, &turn).unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, Some("haiku"), None, None)
        .unwrap();
    let request = r.core.replay_request(&p, &original, &target, target.effort);
    let price = r.core.runner.catalog.get(&request.model).unwrap();
    let estimate = crate::compiler::estimate(&request, price.bytes_per_token, None);
    let need = r.core.audit_reserve(&request).unwrap();
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(crate::provider::Scripted::Billed {
            usage: theseus_protocol::Usage {
                // The reservation prices input at Haiku's highest rate: a one-hour cache write.
                cache_creation_input_tokens: estimate.tokens,
                cache_creation_1h_input_tokens: estimate.tokens,
                output_tokens: request.max_tokens as u64,
                ..Default::default()
            },
            then: Box::new(crate::provider::Scripted::text("replayed")),
        });
    let mut spent = 0;
    let done = r
        .core
        .replay_turn(&p, super::BY_SAMPLER, Some(need), &mut spent)
        .await
        .unwrap();
    assert_eq!(spent, need);
    assert!(done.result.unjudged.unwrap().contains("replay allowance"));
    assert!(
        jev.seen().is_empty(),
        "no comparison may dispatch after exhausting the allowance"
    );
    assert_eq!(theseus_kernel::usd_to_micros(done.result.cost_usd), need);
}
