//! "self digest" and "self log" (theseus-pw1q.19): the owner's DM gets the
//! digest the weekly post carries and the last ten changes; a shared channel
//! is told the log is private, and nothing more; with the mode off both say
//! so plainly and still list the rows so far.

use std::sync::Arc;

use serde_json::json;
use theseus_core::config::rsi::SelfMode;
use theseus_core::ledger::LedgerRow;
use theseus_core::provider::{FakeProvider, Provider};
use theseus_protocol::LedgerKind;

use super::{bindings, guild, Rig, ANA, ANA_DM, LAB};

type Model = fn(&std::path::Path, Arc<theseus_sim::fake_discord::FakeDiscord>) -> Arc<dyn Provider>;

fn model(_: &std::path::Path, _: Arc<theseus_sim::fake_discord::FakeDiscord>) -> Arc<dyn Provider> {
    Arc::new(FakeProvider::scripted(vec![]))
}

/// A rig with the bindings of the gateway's tests: `#lab` shared (`open`)
/// beside ana's DM, with the mode `mode`.
async fn rig(mode: SelfMode) -> Rig {
    let start: Model = model;
    Rig::start_tweaked(start, guild(true), &bindings(false), &[], |c| {
        c.self_improve.mode = mode;
    })
    .await
}

/// `n` changes the self steps wrote, oldest first: `change 1` … `change n`.
fn changes(r: &Rig, n: usize) {
    for i in 1..=n {
        let data = json!({"what": format!("change {i}"), "why": null,
            "numbers": {"cost_usd": 0.5}, "undo": "theseus self veto x", "item": format!("tsk_{i}")});
        r.core
            .store
            .append_ledger(&LedgerRow::new(
                LedgerKind::SelfBranchBuilt,
                None,
                None,
                data,
            ))
            .unwrap();
    }
}

fn said(r: &Rig, channel: u64, needle: &str) -> Option<String> {
    r.posted(channel)
        .into_iter()
        .map(|m| m.content)
        .find(|c| c.contains(needle))
}

#[tokio::test]
async fn self_digest_in_the_owners_dm_posts_the_weekly_posts_text() {
    let r = rig(SelfMode::Act).await;
    changes(&r, 3);
    r.say((ANA, "ana"), Some(ANA_DM), "self digest");
    r.until("the digest in the DM", || {
        said(&r, ANA_DM, "change 3").is_some()
    })
    .await;
    let got = said(&r, ANA_DM, "change 3").unwrap();
    let want = theseus_core::rsi::digest_capped(
        &r.core
            .self_digest(&theseus_protocol::rsi::SelfDigestParams::default())
            .unwrap()
            .text,
    );
    assert_eq!(got, want, "the same text the weekly post carries");
    assert!(
        got.starts_with("What Theseus changed about itself this week"),
        "{got}"
    );
    assert!(
        !got.contains("Self-improvement is off"),
        "act mode says nothing of off: {got}"
    );
}

#[tokio::test]
async fn self_log_in_the_dm_lists_the_last_ten_newest_first() {
    let r = rig(SelfMode::Act).await;
    changes(&r, 12);
    r.say((ANA, "ana"), Some(ANA_DM), "Self Log");
    r.until("the log in the DM", || {
        said(&r, ANA_DM, "change 12").is_some()
    })
    .await;
    let got = said(&r, ANA_DM, "change 12").unwrap();
    let lines: Vec<&str> = got.lines().filter(|l| l.starts_with("- ")).collect();
    assert_eq!(lines.len(), 10, "{got}");
    assert!(
        lines[0].contains("change 12") && lines[9].contains("change 3"),
        "{got}"
    );
    assert!(
        !got.contains("change 2:") && !got.contains("change 1:"),
        "{got}"
    );
    assert!(got.contains("undo: `theseus self veto x`"), "{got}");
}

#[tokio::test]
async fn a_shared_channel_is_told_the_log_is_private_and_nothing_more() {
    let r = rig(SelfMode::Act).await;
    changes(&r, 3);
    for word in ["self digest", "self log"] {
        r.say((ANA, "ana"), Some(LAB), word);
    }
    r.until("both refusals", || {
        r.posted(LAB)
            .iter()
            .filter(|m| m.content == "The self log is private.")
            .count()
            == 2
    })
    .await;
    let all: String = r.posted(LAB).iter().map(|m| m.content.clone()).collect();
    assert!(
        !all.contains("change 3") && !all.contains("What Theseus changed"),
        "{all}"
    );
}

#[tokio::test]
async fn with_the_mode_off_both_say_so_and_give_the_rows_so_far() {
    let r = rig(SelfMode::Off).await;
    changes(&r, 2);
    for word in ["self digest", "self log"] {
        r.say((ANA, "ana"), Some(ANA_DM), word);
    }
    r.until("both answers", || {
        r.posted(ANA_DM)
            .iter()
            .filter(|m| {
                m.content
                    .starts_with("Self-improvement is off. Nothing has run.")
                    && m.content.contains("change 2")
            })
            .count()
            == 2
    })
    .await;
}
