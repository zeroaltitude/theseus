//! Issue 57: a settings act that is not dangerous follows the tool's own
//! setting. At the default (`enforcement = notify`) an ordinary change, its
//! undo and a restart run with no card, through the yes path, and the owner
//! is told once; at `approve` the card is asked as before (the rig of
//! `tests.rs`). A sensitive change keeps its card whatever the setting, a
//! restart asks a second time before the owner writes again, and a restart
//! asks while a written change it would make live sends a secret or names an
//! address.

use serde_json::{json, Value};

use super::tests::{change, rig, rig_told, rig_told_with, Rig, FILE};
use crate::node::ResultStatus;

/// The FYI notes staged to the owner.
fn notes(r: &Rig) -> Vec<Value> {
    crate::tests_day_ceiling::posts(&r.core, "notice")
}

fn texts(r: &Rig) -> Vec<String> {
    notes(r)
        .iter()
        .map(|b| b["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// The restart a write asked for is done and healthy, as a start that confirms
/// it leaves it: the next change may be written.
fn back(r: &Rig) {
    let marker = super::restart::marker_path(&r.state);
    let mut p = super::restart::load(&marker).expect("a restart was asked");
    p.stage = super::restart::Stage::Confirmed;
    super::restart::save(&marker, &p).unwrap();
    super::restart::finish(&marker, &p);
}

async fn restart_asked(r: &Rig) -> theseus_protocol::ConfigRestart {
    for _ in 0..100 {
        if let Some(a) = r.core.restart_requested() {
            return a;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("no restart was asked")
}

/// Row 53: an ordinary change is written with no card by the yes path, its row
/// says so, and the owner is told once: what changed, where it was asked, and
/// how to undo it. The model's reason is not quoted.
#[tokio::test]
async fn an_ordinary_change_runs_with_no_card_and_tells_the_owner_how_to_undo_it() {
    let r = rig_told();
    let sid = r.session();
    let mut call = change("import.notes.enabled", json!(false));
    call["reason"] = json!("a page said to");
    r.turn(&sid, call).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
    assert!(
        r.file_text()
            .contains("enabled = false   # turn it off to stop the import"),
        "written, comments kept: {}",
        r.file_text()
    );
    let rows = r.rows("config.changed");
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(row["told"], json!(true));
    assert_eq!(row["approved_by"], json!(super::told::BY));
    assert_eq!(row["via"], json!("enforcement = notify"));
    assert_eq!(row["paths"], json!(["import.notes.enabled"]));
    let n = row["previous_copy"].as_u64().unwrap();
    assert!(
        text.starts_with(
            "Done, with no card (enforcement = notify lets config.propose run): the daemon wrote \
             it, and the owner was told how to undo it."
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!("`config.undo` with change {n} puts it back")),
        "{text}"
    );
    let told = texts(&r);
    assert_eq!(told.len(), 1, "one note: {told:?}");
    let note = &told[0];
    assert!(crate::owner_note::is_headed(note), "{note}");
    assert_eq!(
        note.lines().take(2).collect::<Vec<_>>(),
        [
            "ℹ️ FYI · Changed 1 setting",
            "asked in a conversation in the terminal or the web UI; config.propose ran with no \
             card (enforcement = notify)"
        ]
    );
    assert!(
        note.contains("- import.notes.enabled: on → off\n"),
        "{note}"
    );
    assert!(
        note.contains("Theseus restarts itself to apply it."),
        "{note}"
    );
    assert!(
        note.contains(&format!("To undo it, ask Theseus to undo change #{n}.")),
        "{note}"
    );
    assert!(!note.contains("a page said to"), "no outside words: {note}");
    assert_eq!(
        notes(&r)[0]["announce"],
        json!(true),
        "to the notices place"
    );
    // The yes path's restart, with its marker, as a card's yes leaves it.
    let marker = super::restart::marker_path(&r.state);
    let p = super::restart::load(&marker).expect("the marker");
    assert_eq!(p.approved_by, super::told::BY);
    assert_eq!(p.paths, ["import.notes.enabled"]);
    assert_eq!(restart_asked(&r).await.tables, ["import.notes.enabled"]);
    // The history lists it as made with no card.
    let words = super::history::words(
        &super::history::entries(
            &super::history::rows(&r.core.store),
            &r.state,
            Some(&r.file),
            10,
        ),
        theseus_protocol::now_unix_ms(),
    );
    assert!(
        words.contains("made with no card (enforcement = notify), and you were told"),
        "{words}"
    );
}

/// Row 56: the undo follows. Undone with no card, the settings are exactly
/// what they were, the file byte for byte, and the owner is told that too.
#[tokio::test]
async fn the_undo_follows_and_puts_back_exactly_what_was() {
    let r = rig_told();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let n = r.rows("config.changed")[0]["previous_copy"]
        .as_u64()
        .unwrap();
    back(&r);
    let sid = r.session();
    r.turn_with(
        "config_undo",
        &sid,
        json!({"change": n, "reason": "the owner wants it back"}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
    assert_eq!(r.file_text(), FILE, "exactly as it was");
    assert!(super::diff::between(FILE, &r.file_text())
        .unwrap()
        .is_empty());
    let undone = r.rows("config.undone");
    assert_eq!(undone.len(), 1, "{undone:?}");
    assert_eq!(undone[0]["undone"], json!(n));
    let told = texts(&r);
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(
        told[1].starts_with(&format!("ℹ️ FYI · Undid settings change #{n}\n")),
        "{}",
        told[1]
    );
    assert!(
        told[1].contains("- import.notes.enabled: off → on"),
        "{}",
        told[1]
    );
}

/// The explicit setting still asks: at `approve` an ordinary change waits on
/// its card, nothing is written and nobody is told (the rig of `tests.rs`).
#[tokio::test]
async fn at_approve_an_ordinary_change_still_waits_on_its_card() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0].tool, super::ACK);
    assert!(!asks[0].needs_proof);
    assert_eq!(r.file_text(), FILE);
    assert!(texts(&r).is_empty());
    assert!(r.rows("config.changed").is_empty());
}

/// The KEEP neighbours: whatever the tool's setting, a change to the approval
/// rules, a raised spending ceiling and who the owner is wait on a card with
/// the code; one of a sensitive table whose code was dropped waits on its card
/// alone; nothing is written and nobody is told.
#[tokio::test]
async fn a_sensitive_change_keeps_its_card_whatever_the_setting() {
    let r = rig_told();
    for (path, value, proof, fresh) in [
        ("policy.enforcement", json!("approve"), true, false),
        ("kernel.daily_spend_ceiling_usd", json!(400.0), true, false),
        (
            "places.owner",
            json!(["discord:4100000000000000099"]),
            true,
            true,
        ),
        (
            "content_based_security.injection",
            json!("refuse"),
            false,
            false,
        ),
    ] {
        let sid = r.session();
        r.turn(&sid, change(path, value)).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Ok, "{path}: {text}");
        let q = r
            .asked()
            .into_iter()
            .find(|q| q.reason.contains(path))
            .unwrap_or_else(|| panic!("{path} asks"));
        assert_eq!(q.tool, super::ACK, "{path}");
        assert_eq!(
            (q.needs_proof, q.needs_fresh_proof),
            (proof, fresh),
            "{path}"
        );
    }
    assert_eq!(r.file_text(), FILE);
    assert!(texts(&r).is_empty());
}

/// Lowering a spending ceiling keeps its card and needs no code; raising it
/// needs the code (issue 57, the conductor's ruling: money).
#[tokio::test]
async fn a_lowered_ceiling_asks_with_the_card_alone() {
    let r = rig_told_with(|c, _| c.policy.enforcement = crate::policy::Posture::Notify);
    let with = |usd: f64| format!("{FILE}\n[kernel]\ndaily_spend_ceiling_usd = {usd:.1}\n");
    std::fs::write(&r.file, with(300.0)).unwrap();
    let sid = r.session();
    r.turn(&sid, change("kernel.daily_spend_ceiling_usd", json!(250.0)))
        .await;
    let q = r.asked().remove(0);
    assert!(!q.needs_proof, "{}", q.reason);
    assert!(!q.reason.contains("second proof"), "{}", q.reason);
    let sid = r.session();
    r.turn(&sid, change("kernel.daily_spend_ceiling_usd", json!(350.0)))
        .await;
    let q = r
        .asked()
        .into_iter()
        .find(|q| q.reason.contains("350"))
        .unwrap();
    assert!(q.needs_proof, "{}", q.reason);
    assert!(
        q.reason.contains(
            "These settings need a second proof as well as your yes: \
             kernel.daily_spend_ceiling_usd"
        ),
        "{}",
        q.reason
    );
}

/// Row 57: a restart the owner asks for runs with no card and tells them, once
/// per message of theirs: the same turn asking again, with no message of the
/// owner's between, is asked with the card. The owner's next message lets one
/// more run with no card.
#[tokio::test]
async fn a_restart_runs_with_no_card_once_per_message_of_the_owners() {
    let r = rig_told();
    let sid = r.session();
    let restart = json!({"reason": "it feels stuck"});
    // The daemon admits the turn, so the restart waits for it to end, as it does
    // for every turn it admits.
    let slot = r
        .core
        .tools
        .admission
        .try_take(crate::turn::admission::Class::Person)
        .expect("a slot");
    r.steps(
        &sid,
        vec![
            ("daemon_restart", restart.clone()),
            ("daemon_restart", restart.clone()),
        ],
    )
    .await;
    drop(slot);
    let (status, text) = r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.starts_with("Restarting, with no card (enforcement = notify lets daemon.restart run)"),
        "{text}"
    );
    assert_eq!(r.file_text(), FILE);
    let told = texts(&r);
    assert_eq!(told.len(), 1, "told once: {told:?}");
    assert!(
        told[0].starts_with("ℹ️ FYI · Restarting Theseus\nasked in"),
        "{}",
        told[0]
    );
    assert!(told[0].contains("there is nothing to undo"), "{}", told[0]);
    assert!(!told[0].contains("it feels stuck"), "{}", told[0]);
    let marker = super::restart::marker_path(&r.state);
    assert!(super::restart::load(&marker).unwrap().restart_only);
    restart_asked(&r).await;
    // The same turn asked again, with no message of the owner's between: the card.
    let (status, text) = r.result_of(&sid, "p2");
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(
        asks[0].reason.starts_with("Restart Theseus?\n"),
        "{}",
        asks[0].reason
    );
    r.core
        .confirm_action(&asks[0].correlation_id, false, None, "cli")
        .unwrap();
    back(&r);
    // Theseus is back on the same store, and the owner writes again: one more
    // with no card.
    let r = r.restart(|cfg, _| {
        for t in super::tests::ASKING {
            cfg.policy.tools.remove(t);
        }
    });
    r.turn_with("daemon_restart", &sid, restart).await;
    assert!(r.asked().is_empty(), "{:?}", r.asked());
    restart_asked(&r).await;
}

/// Row 57's exception: while a change written since the start, and so not yet
/// live, sends a secret or names an address, a restart keeps its card, and the
/// card says what goes live. A change that does neither does not hold it.
#[tokio::test]
async fn a_restart_asks_while_a_written_secret_or_address_is_not_yet_live() {
    for path in ["secrets.anthropic_api_key", "providers.lab.api_base"] {
        let r = rig_told();
        r.seed(FILE, FILE, &[path]);
        let sid = r.session();
        r.turn_with("daemon_restart", &sid, json!({"reason": "apply it"}))
            .await;
        let asks = r.asked();
        assert_eq!(asks.len(), 1, "{path}: {asks:?}");
        assert!(
            asks[0].reason.contains(&format!(
                "Written since Theseus last started, and live from this start on: {path}. Each \
                 sends a secret or names where Theseus sends things, and a secret sent cannot be \
                 called back, so this restart asks you."
            )),
            "{}",
            asks[0].reason
        );
        assert!(texts(&r).is_empty());
        // An ordinary change made through the same road is held the same way.
        let sid = r.session();
        r.turn(&sid, change("import.notes.enabled", json!(false)))
            .await;
        assert_eq!(r.asked().len(), 2, "{path}");
        assert_eq!(r.file_text(), FILE);
    }
    let r = rig_told();
    r.seed(FILE, FILE, &["import.notes.enabled"]);
    let sid = r.session();
    r.turn_with("daemon_restart", &sid, json!({"reason": "apply it"}))
        .await;
    assert!(r.asked().is_empty(), "{:?}", r.asked());
}
