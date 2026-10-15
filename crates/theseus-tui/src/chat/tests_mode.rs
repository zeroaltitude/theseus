//! The chat's mode (theseus-lyxy): Shift+Tab asks for the next mode, the
//! status line names it (`edits (shift+tab)`) and counts the plan (`plan
//! 2/3`), the checklist goes into the transcript each time it moves, and a
//! plan's question shows its numbered steps and takes `e` for edits mode.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use theseus_protocol::grants::GrantOffer;
use theseus_protocol::{ConfirmPreview, ConfirmRequest, Event};

use super::tests::{Rig, SID, T0};
use crate::tests::Answer;

/// The daemon's script, `session.mode` echoing the mode it is asked for.
fn script() -> crate::tests::Script {
    Arc::new(|m, p| -> Answer {
        Ok(match m {
            "health" => json!({"name": "theseusd", "model": "claude-sonnet-5-5", "profile": "f10"}),
            "session.open" => json!({
                "session_id": SID, "kind": "conversation", "label": null,
                "created_at_unix_ms": T0, "turns": 0,
            }),
            "session.list" => json!({"sessions": []}),
            "session.watch" => json!({"watching": SID, "watchers": 1}),
            "session.mode" => json!({
                "session_id": SID,
                "mode": p.get("mode").cloned().unwrap_or(json!("ask")),
                "steps": [],
            }),
            _ => json!({}),
        })
    })
}

async fn shift_tab(r: &mut Rig) {
    r.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
        .await;
}

#[tokio::test(start_paused = true)]
async fn shift_tab_cycles_ask_edits_plan_auto_and_the_status_line_follows() {
    let mut r = Rig::with(100, 24, script()).await;
    r.ready().await;
    // The conversation's mode was read when it opened, with no mode named.
    r.asked("session.mode", 1).await;
    assert_eq!(
        r.daemon.asked("session.mode")[0],
        json!({"session_id": SID})
    );
    assert!(r.text().contains("ask (shift+tab)"), "{}", r.text());
    for (n, mode) in ["edits", "plan", "auto", "ask"].into_iter().enumerate() {
        shift_tab(&mut r).await;
        r.asked("session.mode", n + 2).await;
        assert_eq!(
            r.daemon.asked("session.mode")[n + 1],
            json!({"session_id": SID, "mode": mode})
        );
        r.shows(&format!("{mode} (shift+tab)")).await;
    }
}

#[tokio::test(start_paused = true)]
async fn the_plan_s_checklist_is_in_the_transcript_and_its_count_in_the_status_line() {
    let mut r = Rig::with(100, 24, script()).await;
    r.ready().await;
    let steps = |states: [&str; 3]| {
        let texts = ["add the function", "test it", "run all"];
        json!(states
            .iter()
            .zip(texts)
            .map(|(s, t)| json!({"text": t, "state": s}))
            .collect::<Vec<_>>())
    };
    for (states, by) in [
        (["todo", "todo", "todo"], "plan approved"),
        (["done", "doing", "todo"], "plan_step"),
    ] {
        r.daemon.notify(
            "session.mode",
            json!({"session_id": SID, "mode": "edits", "by": by, "steps": steps(states)}),
        );
    }
    r.shows("plan 1/3").await;
    let text = r.text();
    assert!(text.contains("edits (shift+tab)"), "{text}");
    for line in [
        "☐ 1. add the function",
        "✔ 1. add the function",
        "◼ 2. test it",
    ] {
        assert!(text.contains(line), "{line} in {text}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_plan_question_shows_its_steps_and_e_approves_it_into_edits_mode() {
    let mut r = Rig::with(100, 24, script()).await;
    r.ready().await;
    r.starts_turn("trn_1");
    let q = ConfirmRequest {
        correlation_id: "act_plan".into(),
        session_id: SID.into(),
        tool: "plan.present".into(),
        input: json!({"steps": ["add the function", "test it"]}),
        reason: "plan.present — approve (the plan is the question)".into(),
        preview: Some(ConfirmPreview {
            kind: ConfirmPreview::TEXT.into(),
            body: "1. add the function\n2. test it".into(),
        }),
        grant_offer: Some(GrantOffer::mode_edits()),
        ..Default::default()
    };
    r.tell(Event::ConfirmRequested(q));
    r.shows("[e] approve the plan + edits mode").await;
    let text = r.text();
    assert!(text.contains("Approve this plan (2 steps)?"), "{text}");
    assert!(text.contains("1. add the function"), "{text}");
    assert!(text.contains("2. test it"), "{text}");
    r.press(KeyCode::Char('e')).await;
    r.asked("work.answer", 1).await;
    let a = &r.daemon.asked("work.answer")[0];
    assert_eq!(
        (a["question_id"].as_str(), a["option"].as_str()),
        (Some("act_plan"), Some("approve_grant"))
    );
}
