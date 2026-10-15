//! The context meter (theseus-wy6v.2): `ctx N%` on the status line at 80 and
//! 120 columns, `/context` by part, and `/cost` turn by turn against the day.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::json;
use theseus_protocol::{Event, TurnSubmitResult};

use super::tests::{script, Rig, SID};
use crate::tests::Script;

/// The default script, and a `context.explain` and a `health` with a day.
fn metered() -> Script {
    let base = script();
    Arc::new(move |m, p| match m {
        "context.explain" => Ok(json!({
            "session_id": SID, "kind": "conversation", "class": "private", "place": "CLI",
            "turns": [], "turn_id": "trn_2",
            "compiled": {"session_id": SID, "turn_id": "trn_2", "loop": 0, "decision": "append",
                "messages": 9},
            "profile": "f10", "model": "claude-sonnet-5-5", "window": 200000,
            "parts": [
                {"block": "header", "name": "persona", "bytes": 4800, "tokens": 1200},
                {"block": "context", "name": "AGENTS.md", "bytes": 9000, "tokens": 2250},
                {"block": "tools", "name": "31 tools", "bytes": 12000, "tokens": 3000},
                {"block": "recall", "name": "a note", "bytes": 800, "tokens": 210},
                {"block": "conversation", "name": "6 nodes", "bytes": 20000, "tokens": 7550}],
            "digest_now": "d1", "recalls": [], "sources": {}, "ms": 4.0
        })),
        "health" => Ok(
            json!({"name": "theseusd", "model": "claude-sonnet-5-5", "profile": "f10",
            "day_spend": {"day": "2026-10-14", "ceiling_usd": 200.0, "spent_usd": 12.5,
                "held_usd": 0.0, "reached": false, "turns_at_ms": 1, "turns_at": "2026-10-15 00:00",
                "mode": "notify", "runaway_usd": 2000.0}}),
        ),
        _ => base(m, p),
    })
}

impl Rig {
    /// A turn ends with this cost and this fill.
    fn ends_with(&self, turn: &str, cost: f64, session: f64, fill: Option<(u64, u64)>) {
        self.tell(Event::TurnEnded(TurnSubmitResult {
            session_id: SID.into(),
            turn_id: turn.into(),
            model: "claude-sonnet-5-5".into(),
            elapsed_ms: 1000,
            cost_usd: Some(cost),
            session_cost_usd: Some(session),
            context_tokens: fill.map(|f| f.0),
            context_window: fill.map(|f| f.1),
            stop_reason: "done".into(),
            ..Default::default()
        }));
    }
}

#[tokio::test(start_paused = true)]
async fn the_status_line_shows_the_share_at_80_and_120_columns() {
    for w in [80u16, 120] {
        let mut r = Rig::new(w, 24).await;
        r.ready().await;
        assert!(
            !r.screen().last().unwrap().contains("ctx"),
            "none before a request"
        );
        r.ends_with("trn_1", 0.0042, 0.0042, Some((14_210, 200_000)));
        r.until("the share", |r| {
            r.screen().last().unwrap().contains("ctx 7%")
        })
        .await;
        let last = r.screen().last().unwrap().clone();
        assert!(last.chars().count() <= usize::from(w), "{last:?}");
        assert!(last.contains("$0.0042"), "the cost stays: {last:?}");
        // Past the band, it warns.
        r.ends_with("trn_2", 0.0042, 0.0084, Some((170_000, 200_000)));
        r.until("the warning", |r| {
            r.screen().last().unwrap().contains("ctx 85%")
        })
        .await;
        let last = r.screen().last().unwrap().clone();
        assert!(last.chars().count() <= usize::from(w), "{last:?}");
        if w == 120 {
            assert!(last.contains("compacts soon"), "{last:?}");
        }
    }
}

#[tokio::test(start_paused = true)]
async fn context_lists_the_parts_with_amounts_and_the_free_space() {
    let mut r = Rig::with(100, 40, metered()).await;
    r.ready().await;
    r.type_text("/context").await;
    r.press(KeyCode::Enter).await;
    r.asked("context.explain", 1).await;
    assert_eq!(r.daemon.asked("context.explain")[0]["session_id"], SID);
    r.shows("`theseus why` names every part").await;
    let t = r.text();
    assert!(t.contains("14,210 of 200,000 tokens (7.1%)"), "{t}");
    for want in [
        "system and persona",
        "tools (31)",
        "context files",
        "recalled (1)",
        "conversation (9 messages)",
        "free space",
        "185,790",
    ] {
        assert!(t.contains(want), "{want} is missing:\n{t}");
    }
    // The meter takes the explained total, so the status line has the share.
    assert!(r.screen().last().unwrap().contains("ctx 7%"), "{t}");
}

#[tokio::test(start_paused = true)]
async fn cost_sums_the_turns_to_the_sessions_cost_and_names_the_day() {
    let mut r = Rig::with(100, 40, metered()).await;
    r.ready().await;
    r.ends_with("trn_1", 0.01, 0.01, None);
    r.ends_with("trn_2", 0.02, 0.03, None);
    r.ends_with("trn_3", 0.0125, 0.0425, None);
    r.shows("$0.0125 this reply").await;
    r.type_text("/cost").await;
    r.press(KeyCode::Enter).await;
    r.shows("today: $12.50 of the $200.00 daily ceiling (6%)")
        .await;
    let t = r.text();
    assert!(t.contains("this conversation: $0.0425"), "{t}");
    for turn in ["turn 1", "turn 2", "turn 3"] {
        assert!(t.contains(turn), "{turn}:\n{t}");
    }
    assert!(t.contains("$0.0100") && t.contains("$0.0200"), "{t}");
}

#[test]
fn cost_lines_sum_to_the_sessions_cost_with_the_rest_in_one_line() {
    use super::usage::cost_lines;
    // Twelve turns seen, a conversation that began before the chat opened.
    let turns: Vec<Option<f64>> = (1..=12).map(|i| Some(f64::from(i) * 0.5)).collect();
    let seen: f64 = turns.iter().flatten().sum();
    let total = seen + 3.0;
    let lines = cost_lines(&turns, total, None);
    let dollars = |l: &String| -> Option<f64> {
        let t = l.trim_start();
        t.strip_prefix('$')?.split_whitespace().next()?.parse().ok()
    };
    let rows: Vec<f64> = lines.iter().skip(1).filter_map(dollars).collect();
    assert_eq!(
        rows.len(),
        11,
        "ten listed turns and the rest in one:\n{lines:#?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("2 earlier turns")),
        "{lines:#?}"
    );
    let sum: f64 = rows.iter().sum();
    assert!((sum - total).abs() < 0.01, "{sum} vs {total}\n{lines:#?}");
    assert!(lines.last().unwrap().starts_with("today:"));
}
