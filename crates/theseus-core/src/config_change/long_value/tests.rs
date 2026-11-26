//! A setting too long for its card, through the whole core (issue 23, line 2):
//! it goes on the card in a prompt file's form instead of being refused, the
//! whole value is readable before the yes, and a short value keeps its card.

use serde_json::json;
use theseus_protocol::ConfigWholeParams;

use super::*;
use crate::config_change::tests::{change, rig, FILE};
use crate::node::ResultStatus;

fn prompt(n: usize) -> String {
    (1..=n).map(|i| format!("Rule {i}: be careful. ")).collect()
}

/// A value between 400 characters and the proposal's limit is not refused: its
/// card gives the size and fingerprint before and after, the whole value is
/// readable before the yes, and the second proof is asked as for any prompt.
#[tokio::test]
async fn a_long_value_goes_on_a_card_instead_of_being_refused() {
    let r = rig();
    let sid = r.session();
    let value = prompt(60);
    assert!((400..4_096).contains(&value.len()));
    r.turn(&sid, change("model.system", json!(value))).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let q = r.asked().remove(0);
    assert!(q.reason.starts_with("Change 1 setting?\n"), "{}", q.reason);
    assert!(
        q.reason
            .contains("- model.system: a long text, shown below (after a restart)"),
        "{}",
        q.reason
    );
    assert!(
        q.reason.contains("  Before: not set (its default)"),
        "{}",
        q.reason
    );
    let after = shown::facts(&value);
    assert!(
        q.reason.contains(&format!("  After: {}", after.words())),
        "{}",
        q.reason
    );
    assert!(q.reason.contains("theseus config show"), "{}", q.reason);
    // Prompt text keeps its card and needs no code (issue 57).
    assert!(q.input["whole"] == true && !q.needs_proof, "{q:?}");
    assert!(
        q.reason.chars().count() <= super::super::prompt_file::card::CARD_CHARS + 200,
        "{}",
        q.reason.chars().count()
    );
    // The whole value is readable before the yes, and nothing is written.
    let words = r
        .core
        .config_whole(&ConfigWholeParams::default(), "cli")
        .unwrap()
        .words;
    assert!(words.contains("===== model.system, after: "), "{words}");
    assert!(words.contains("Rule 60: be careful."), "{words}");
    assert_eq!(r.file_text(), FILE);
}

/// A small change to a long value shows its changed lines whole, and a whole
/// different one is a summary that says so.
#[tokio::test]
async fn a_small_change_to_a_long_value_shows_its_changed_lines_and_a_big_one_is_a_summary() {
    let r = rig();
    let lines: String = (1..=40)
        .map(|i| format!("Rule {i}: be careful.\n"))
        .collect();
    let file = format!(
        "{FILE}\n[model]\nsystem = {}\n",
        serde_json::to_string(&lines).unwrap()
    );
    std::fs::write(&r.file, &file).unwrap();
    let sid = r.session();
    r.turn(
        &sid,
        change(
            "model.system",
            json!(lines.replace("Rule 20: be careful.", "Rule 20: be kind.")),
        ),
    )
    .await;
    assert_eq!(r.result(&sid).0, ResultStatus::Ok);
    let q = r.asked().remove(0);
    assert!(
        q.reason.contains("```diff\n@@ -18,5 +18,5 @@\n Rule 18: be careful.\n Rule 19: be careful.\n-Rule 20: be careful.\n+Rule 20: be kind.\n Rule 21: be careful.\n Rule 22: be careful.\n```\n"),
        "{}",
        q.reason
    );
    assert!(
        q.reason.contains("1 line added, 1 line removed."),
        "{}",
        q.reason
    );
    // Every line different: a summary, and the whole is there to read.
    r.core
        .confirm_action(&q.correlation_id, false, None, "cli")
        .unwrap();
    let sid = r.session();
    let rewritten: String = (1..=40).map(|i| format!("Law {i}: be gentle.\n")).collect();
    r.turn(&sid, change("model.system", json!(rewritten))).await;
    assert_eq!(r.result(&sid).0, ResultStatus::Ok);
    let q = r.asked().pop().unwrap();
    assert!(
        q.reason.contains("a summary and not the text"),
        "{}",
        q.reason
    );
    assert!(!q.reason.contains("Law 3:"), "{}", q.reason);
    let words = r
        .core
        .config_whole(
            &ConfigWholeParams {
                question: Some(q.correlation_id),
                ..Default::default()
            },
            "cli",
        )
        .unwrap()
        .words;
    assert!(
        words.contains("Law 40: be gentle.") && words.contains("Rule 40: be careful."),
        "{words}"
    );
}

/// A yes with a good code writes the long value, and the texts the card kept
/// are forgotten when it settles.
#[tokio::test]
async fn a_yes_writes_the_long_value_and_the_kept_texts_go() {
    let r = rig();
    let secret = r.core.proofs.enrol_stand_in();
    let sid = r.session();
    r.turn(&sid, change("model.system", json!(prompt(60))))
        .await;
    let q = r.asked().remove(0);
    let code = crate::owner_proof::Proofs::stand_in_code(&secret, 0);
    let client = crate::tests_m3::surface("sock#1", crate::approval::Surface::Cli)
        .behind(crate::peer_proc::Behind::Owner);
    crate::tests_m3::rpc_as(
        &r.core,
        client,
        theseus_protocol::method::ACTION_CONFIRM,
        json!({"correlation_id": q.correlation_id, "approve": true, "code": code}),
    )
    .await
    .unwrap();
    assert!(
        r.file_text().contains("Rule 60: be careful."),
        "{}",
        r.file_text()
    );
    let left: Vec<String> = std::fs::read_dir(crate::config_change::write::dir(&r.state))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("settings-base-") || n.starts_with("candidate-"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// A value past the proposal's own limit is still refused, in words that say why.
#[tokio::test]
async fn a_document_is_still_refused() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("model.system", json!("p".repeat(5_000))))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("longer than a card can show whole") && text.contains("4096 bytes"),
        "{text}"
    );
    assert!(r.asked().is_empty());
}

/// A short value keeps today's card: no stub, no extra argument.
#[tokio::test]
async fn a_short_value_keeps_its_card_exactly() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let q = r.asked().remove(0);
    assert!(
        q.reason
            .contains("- import.notes.enabled: on → off (after a restart)"),
        "{}",
        q.reason
    );
    assert!(
        !q.reason.contains("a long text") && !q.reason.contains("config show"),
        "{}",
        q.reason
    );
    assert!(
        q.input.get("whole").is_none() && q.input.get("base_kept").is_none(),
        "{}",
        q.input
    );
}

/// One card carries at most three long values, each with its own room; three
/// go on it, a fourth is refused in words that say how many.
#[tokio::test]
async fn one_card_carries_at_most_three_long_values() {
    let r = rig();
    let card = "c".repeat(500);
    let ask = |n: usize| {
        let changes: Vec<_> = (0..n)
            .map(|i| json!({"path": format!("personas.p{i}.card"), "value": card}))
            .collect();
        json!({"changes": changes, "reason": "x"})
    };
    let sid = r.session();
    r.turn(&sid, ask(4)).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("4 long values") && text.contains("at most 3"),
        "{text}"
    );
    assert!(r.asked().is_empty());
    let sid = r.session();
    r.turn(&sid, ask(3)).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let q = r.asked().remove(0);
    assert_eq!(
        q.reason.matches(": a long text, shown below").count(),
        3,
        "{}",
        q.reason
    );
}
