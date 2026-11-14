//! The binding's own unasked messages start with a kind and a title, then a
//! trigger hint (theseus-x8b9k), as the core's do (its `tests_owner_note.rs`).
//! The voice notices are held in `runtime/voice/deaf.rs`'s tests.

use serde_json::json;
use theseus_core::owner_note::is_headed;

#[test]
fn every_unasked_discord_message_starts_with_a_kind_and_a_title() {
    let disk = |state: &str, left: &str| {
        crate::diskwords::disk_note(&json!({"kind": "disk", "state": state, "left": left,
            "free_mb": 4000, "total_mb": 100000, "warn_mb": 5120, "floor_mb": 1024}))
    };
    let families = [
        (
            "restarted",
            crate::render::restarted(1_790_000_000_000, &["model".into()]),
        ),
        ("disk low", disk("low", "ok")),
        ("disk below the floor", disk("below_floor", "low")),
        ("disk back over the floor", disk("low", "below_floor")),
        ("disk back to normal", disk("ok", "low")),
        (
            "mcp tools",
            crate::courier::mcp_note(&json!({"server": "tide", "summary": "+1 tool"})),
        ),
        (
            "mcp prompt",
            crate::courier::mcp_note(&json!({"server": "tide", "prompt": "p", "summary": "s"})),
        ),
        (
            "jev notice",
            crate::runtime::jev_notice_text(&json!({"tool": "proc.run", "line": "Jev: 95% risky",
                "summary": "rm -r build", "judgment": "jdg_1"})),
        ),
        (
            "reminder",
            crate::runtime::remind_text(&json!({"text": "stretch"})),
        ),
        (
            "new persona card",
            theseus_core::personas::note::text("pilot", "Pilot", "the cockpit", 2000),
        ),
    ];
    let bad: Vec<String> = families
        .iter()
        .filter(|(_, t)| !is_headed(t))
        .map(|(n, t)| format!("{n}: {t:?}"))
        .collect();
    assert!(
        bad.is_empty(),
        "families without a head:\n{}",
        bad.join("\n")
    );
}

/// Theseus is the one who flags a call (theseus-nzwa5): the notice's heading
/// never names the small model. The detail line is the one known exception
/// (`Jev: 95% risky`, `JudgeNoticed::line`, shared with the cockpit and the
/// CLI), which waits on the owner (ask i9-remembers-5): the test says so,
/// so it changes the day that line does.
#[test]
fn the_notice_of_a_flagged_call_says_theseus_in_its_heading() {
    let text =
        crate::runtime::jev_notice_text(&json!({"tool": "proc.run", "line": "Jev: 95% risky",
        "summary": "rm -r build", "judgment": "jdg_1"}));
    assert!(
        text.starts_with("⚠️ Alert · Theseus flagged a call that already ran: `proc.run`"),
        "{text}"
    );
    let heading = text.lines().next().unwrap_or_default();
    assert!(!heading.contains("Jev"), "{heading}");
    assert!(
        text.contains("notified after it ran: Jev: 95% risky"),
        "the score line is the one place left that names it: {text}"
    );
}
