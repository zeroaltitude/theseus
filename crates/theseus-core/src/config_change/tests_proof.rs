//! The owner's second proof, wired into the cards of issue 22 (`config.propose`
//! and `config.undo` here; an MCP server's card and the install card in their
//! own tests), on a scratch settings file and a scratch state directory, with a
//! stand-in authenticator, answered over real protocol connections:
//!
//! - an ordinary change needs the card alone; a sensitive one cannot be
//!   approved without a paired app, and its card says how to pair one;
//! - a good code writes it and opens a window for the place it came from; the
//!   next sensitive change from there goes on the yes alone, and the card says
//!   so; from another place, after `lock`, and after a rollback it asks;
//! - a wrong code writes nothing and is recorded;
//! - who the owner is, and the proof itself, ask for a fresh code inside a window;
//! - an undo of a sensitive change is held to the same rules;
//! - an agent's process and a job's process approve none of it, with a good code
//!   and an open window both in hand.

use serde_json::{json, Value};
use theseus_protocol::{method, RpcError};

use super::tests::{change, rig, Rig, FILE};
use crate::approval::{Client, Surface};
use crate::node::ResultStatus;
use crate::owner_proof::totp::Secret;
use crate::owner_proof::Proofs;
use crate::peer_proc::Behind;
use crate::tests_m3::{rpc_as, surface};

const CEILING: &str = "kernel.daily_spend_ceiling_usd";

fn cli() -> Client {
    surface("sock#1", Surface::Cli).behind(Behind::Owner)
}

fn web() -> Client {
    surface("web#1", Surface::Web).behind(Behind::Owner)
}

async fn answer(
    r: &Rig,
    client: Client,
    corr: &str,
    approve: bool,
    code: Option<&str>,
) -> Result<Value, RpcError> {
    rpc_as(
        &r.core,
        client,
        method::ACTION_CONFIRM,
        json!({"correlation_id": corr, "approve": approve, "code": code}),
    )
    .await
}

/// The card as `client` is shown it by `confirm.list`.
async fn card(r: &Rig, client: Client, corr: &str) -> Value {
    let list = rpc_as(&r.core, client, method::CONFIRM_LIST, json!({}))
        .await
        .unwrap();
    list["confirms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["correlation_id"] == corr)
        .expect("the card is listed")
        .clone()
}

fn code(secret: &Secret, ahead: u64) -> String {
    Proofs::stand_in_code(secret, ahead)
}

/// A proposal, asked: the question's id.
async fn ask(r: &Rig, path: &str, value: Value) -> String {
    let sid = r.session();
    r.turn(&sid, change(path, value)).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    r.asked().pop().expect("a card is waiting").correlation_id
}

fn waiting(r: &Rig, corr: &str) -> bool {
    r.asked().iter().any(|c| c.correlation_id == corr)
}

fn outcomes(r: &Rig) -> Vec<String> {
    r.rows("owner.proof.checked")
        .iter()
        .map(|d| d["outcome"].as_str().unwrap_or("?").to_string())
        .collect()
}

fn windows(r: &Rig) -> Vec<String> {
    r.core
        .proofs
        .status()
        .windows
        .into_iter()
        .map(|w| w.place)
        .collect()
}

/// An ordinary change needs the card alone: no mark, no code, written, and a
/// restart asked so that it takes effect.
#[tokio::test]
async fn an_ordinary_change_needs_the_card_alone() {
    let r = rig();
    let q = ask(&r, "import.notes.enabled", json!(false)).await;
    let c = card(&r, cli(), &q).await;
    assert_ne!(c["needs_proof"], true, "{c}");
    answer(&r, cli(), &q, true, None).await.unwrap();
    assert!(r.file_text().contains("enabled = false"));
    let p = super::restart::load(&super::restart::marker_path(&r.state))
        .expect("the change asks for its restart");
    assert_eq!(p.paths, ["import.notes.enabled"]);
    assert!(outcomes(&r).is_empty(), "the proof was not consulted");
}

/// A sensitive change with no app paired: the card says how to pair one, no
/// yes (with or without a code) approves it, and nothing is written.
#[tokio::test]
async fn a_sensitive_change_with_no_proof_enrolled_cannot_be_approved_and_says_how() {
    let r = rig();
    let q = ask(&r, CEILING, json!(300)).await;
    let c = card(&r, cli(), &q).await;
    assert_eq!(c["needs_proof"], true, "{c}");
    assert_eq!(c["proof"]["not_enrolled"], true, "{c}");
    assert!(
        c["proof"]["line"]
            .as_str()
            .unwrap()
            .contains("theseus owner proof enroll"),
        "{c}"
    );
    for with in [None, Some("123456")] {
        let e = answer(&r, cli(), &q, true, with).await.unwrap_err();
        assert!(e.message.contains("theseus owner proof enroll"), "{e:?}");
    }
    assert_eq!(r.file_text(), FILE);
    assert!(waiting(&r, &q), "it still waits");
    assert!(r.rows("config.changed").is_empty() && !super::restart::marker_path(&r.state).exists());
}

/// The way in: paired, a good code writes the change and opens a window for
/// the place it came from.
#[tokio::test]
async fn a_good_code_writes_a_sensitive_change_and_opens_a_window() {
    let r = rig();
    let secret = r.core.proofs.enrol_stand_in();
    let q = ask(&r, CEILING, json!(300)).await;
    answer(&r, cli(), &q, true, Some(&code(&secret, 0)))
        .await
        .unwrap();
    assert!(r.file_text().contains("daily_spend_ceiling_usd"));
    assert_eq!(windows(&r), ["cli"]);
    assert_eq!(outcomes(&r), ["ok"]);
    assert_eq!(r.rows("owner.proof.window_opened").len(), 1);
}

/// Inside the window the same place goes on the yes alone, and the card says
/// so before it is pressed; another place is asked for a code.
#[tokio::test]
async fn inside_the_window_the_same_place_needs_the_yes_alone_and_another_is_asked() {
    let r = rig();
    let secret = r.core.proofs.open_stand_in_window("cli");
    let q = ask(&r, CEILING, json!(300)).await;
    let here = card(&r, cli(), &q).await;
    assert_eq!(here["proof"]["code_needed"], false, "{here}");
    assert!(
        here["proof"]["line"]
            .as_str()
            .unwrap()
            .starts_with("second proof: given here"),
        "{here}"
    );
    let there = card(&r, web(), &q).await;
    assert_eq!(there["proof"]["code_needed"], true, "{there}");
    let e = answer(&r, web(), &q, true, None).await.unwrap_err();
    assert!(e.message.contains("second proof"), "{e:?}");
    assert_eq!(r.file_text(), FILE);
    answer(&r, cli(), &q, true, None).await.unwrap();
    assert!(r.file_text().contains("daily_spend_ceiling_usd"));
    assert_eq!(r.rows("owner.proof.window_used").len(), 1);
    let _ = secret;
}

/// A wrong code writes nothing, leaves the question waiting, and is recorded.
#[tokio::test]
async fn a_wrong_code_changes_nothing_and_is_recorded() {
    let r = rig();
    let secret = r.core.proofs.enrol_stand_in();
    let q = ask(&r, CEILING, json!(300)).await;
    // Not the code the app shows now, nor the step either side of it.
    let wrong = code(&secret, 0)
        .chars()
        .map(|c| if c == '7' { '3' } else { '7' })
        .collect::<String>();
    let e = answer(&r, cli(), &q, true, Some(&wrong)).await.unwrap_err();
    assert!(
        e.message.contains("second proof") || e.message.contains("code"),
        "{e:?}"
    );
    assert_eq!(r.file_text(), FILE);
    assert!(waiting(&r, &q));
    assert_eq!(outcomes(&r), ["wrong"]);
    assert_eq!(r.rows("approval.refused").len(), 1);
    assert!(windows(&r).is_empty(), "a wrong code opens nothing");
}

/// `lock` ends the window: the next sensitive change asks for a code again.
#[tokio::test]
async fn after_lock_a_sensitive_change_asks_again() {
    let r = rig();
    let _ = r.core.proofs.open_stand_in_window("cli");
    r.core.proofs.lock("cli", "cli").unwrap();
    assert!(windows(&r).is_empty());
    let q = ask(&r, CEILING, json!(300)).await;
    assert_eq!(card(&r, cli(), &q).await["proof"]["code_needed"], true);
    let e = answer(&r, cli(), &q, true, None).await.unwrap_err();
    assert!(e.message.contains("second proof"), "{e:?}");
    assert_eq!(r.file_text(), FILE);
}

/// A change to who the owner is, or to the proof itself, asks for a fresh code
/// inside a window, and its code opens no window and ends none.
#[tokio::test]
async fn who_the_owner_is_and_the_proof_itself_ask_for_a_fresh_code_inside_a_window() {
    for (path, value) in [
        ("places.owner", json!(["discord:4100000000000000999"])),
        ("owner_proof.window_hours", json!(1)),
    ] {
        let r = rig();
        let secret = r.core.proofs.open_stand_in_window("cli");
        let opened = r.core.proofs.status().windows;
        let q = ask(&r, path, value).await;
        let c = card(&r, cli(), &q).await;
        assert_eq!(c["needs_fresh_proof"], true, "{path}: {c}");
        assert_eq!(c["proof"]["code_needed"], true, "{path}: {c}");
        let e = answer(&r, cli(), &q, true, None).await.unwrap_err();
        assert!(e.message.contains("second proof"), "{path}: {e:?}");
        assert_eq!(r.file_text(), FILE, "{path}");
        // The window's own code is spent; the next step's is a fresh one.
        answer(&r, cli(), &q, true, Some(&code(&secret, 1)))
            .await
            .unwrap_or_else(|e| panic!("{path}: {e:?}"));
        assert_ne!(r.file_text(), FILE, "{path}");
        assert_eq!(
            r.core.proofs.status().windows,
            opened,
            "{path}: no window moved"
        );
    }
}

/// The settings file after a change to the day's ceiling, as seeded for an undo.
fn with_ceiling() -> String {
    format!("{FILE}\n[kernel]\ndaily_spend_ceiling_usd = 300.0\n")
}

/// An undo of a sensitive change is a sensitive change: marked, and inside an
/// open window it goes on the yes alone from the place that has the window
/// and from no other.
#[tokio::test]
async fn an_undo_of_a_sensitive_change_rides_the_window_of_its_own_place_only() {
    let r = rig();
    let _ = r.core.proofs.open_stand_in_window("cli");
    r.seed(FILE, &with_ceiling(), &[CEILING]);
    let sid = r.session();
    r.turn_with(
        "config_undo",
        &sid,
        json!({"reason": "the owner wants it back"}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let u = r.asked().pop().unwrap().correlation_id;
    assert_eq!(card(&r, cli(), &u).await["needs_proof"], true);
    let e = answer(&r, web(), &u, true, None).await.unwrap_err();
    assert!(e.message.contains("second proof"), "{e:?}");
    assert_eq!(r.file_text(), with_ceiling());
    answer(&r, cli(), &u, true, None).await.unwrap();
    assert!(!r.file_text().contains("daily_spend_ceiling_usd"));
    assert_eq!(r.rows("config.undone").len(), 1);
}

/// After `lock` the same undo asks for a code, and a code does it.
#[tokio::test]
async fn after_lock_an_undo_of_a_sensitive_change_asks_for_a_code() {
    let r = rig();
    let secret = r.core.proofs.open_stand_in_window("cli");
    r.core.proofs.lock("cli", "cli").unwrap();
    r.seed(FILE, &with_ceiling(), &[CEILING]);
    let sid = r.session();
    r.turn_with(
        "config_undo",
        &sid,
        json!({"reason": "the owner wants it back"}),
    )
    .await;
    let u = r.asked().pop().unwrap().correlation_id;
    assert_eq!(card(&r, cli(), &u).await["proof"]["code_needed"], true);
    let e = answer(&r, cli(), &u, true, None).await.unwrap_err();
    assert!(e.message.contains("second proof"), "{e:?}");
    assert_eq!(r.file_text(), with_ceiling());
    answer(&r, cli(), &u, true, Some(&code(&secret, 1)))
        .await
        .unwrap();
    assert!(!r.file_text().contains("daily_spend_ceiling_usd"));
}

/// A rollback's restart ends every open window: the settings a window was
/// given under are gone, so the next sensitive change asks for a code.
#[tokio::test]
async fn a_rollback_ends_every_open_window() {
    let r = rig();
    let secret = r.core.proofs.enrol_stand_in();
    let q = ask(&r, CEILING, json!(300)).await;
    answer(&r, cli(), &q, true, Some(&code(&secret, 0)))
        .await
        .unwrap();
    assert_eq!(windows(&r), ["cli"]);
    // The daemon comes back unable to load, or unhealthy: the settings go
    // back, and the start that follows tells the owner.
    let marker = super::restart::marker_path(&r.state);
    let mut p = super::restart::load(&marker).expect("the marker");
    super::restart::roll_back(&marker, &mut p, "it was not healthy", &r.file).unwrap();
    assert_eq!(r.file_text(), FILE);
    assert_eq!(windows(&r), ["cli"], "nothing has ended them yet");
    r.core.tell_rolled_back(&p);
    assert!(windows(&r).is_empty());
    let told = crate::tests_day_ceiling::posts(&r.core, "notice")
        .iter()
        .map(|b| b["text"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        told.contains("went back to the settings") && told.contains("windows of your second proof"),
        "{told}"
    );
    assert_eq!(
        r.rows("owner.proof.locked").len(),
        1,
        "ended by the daemon, and recorded"
    );
    // The next sensitive change asks for a code again.
    assert!(r.core.proofs.standing("cli", false).code_needed);
}

/// An agent's process and a job's process approve no card of this module, not
/// with a good code and not inside an open window of the owner's own place.
#[tokio::test]
async fn an_agents_and_a_jobs_process_approve_neither_a_proposal_nor_an_undo() {
    let r = rig();
    let secret = r.core.proofs.open_stand_in_window("cli");
    r.seed(FILE, &with_ceiling(), &[CEILING]);
    let after = r.file_text();
    // Three cards waiting: an ordinary proposal, a sensitive one, an undo.
    let ordinary = ask(&r, "import.notes.enabled", json!(false)).await;
    let sensitive = ask(&r, CEILING, json!(400)).await;
    let sid = r.session();
    r.turn_with(
        "config_undo",
        &sid,
        json!({"reason": "the owner wants it back"}),
    )
    .await;
    let undo = r.asked().pop().unwrap().correlation_id;
    let windows_before = r.core.proofs.status().windows;
    for who in [
        Behind::Agent("claude".into()),
        Behind::Agent(crate::peer_proc::JOB.into()),
    ] {
        for q in [&ordinary, &sensitive, &undo] {
            for with in [None, Some(code(&secret, 1))] {
                let client = surface("sock#9", Surface::Cli).behind(who.clone());
                let e = answer(&r, client, q, true, with.as_deref())
                    .await
                    .unwrap_err();
                assert!(
                    e.message.contains("only the owner") || e.message.contains("agent"),
                    "{who:?}: {e:?}"
                );
            }
        }
    }
    // An agent is shown no button on any of them.
    let agent = surface("sock#9", Surface::Cli).behind(Behind::Agent("claude".into()));
    for q in [&ordinary, &sensitive, &undo] {
        assert_eq!(card(&r, agent.clone(), q).await["may_answer"], false);
    }
    assert_eq!(r.file_text(), after, "nothing was written");
    assert!(waiting(&r, &ordinary) && waiting(&r, &sensitive) && waiting(&r, &undo));
    assert_eq!(r.core.proofs.status().windows, windows_before);
    // The code was never spent on them: the owner's own hand, inside the window, can.
    answer(&r, cli(), &sensitive, true, None).await.unwrap();
}

/// The restart card counts what is parked on a question to the owner, not only
/// the turns that are running: such an execution (a self build among them) is
/// not "building", so a card that counted turns alone made a restart look safe.
#[tokio::test]
async fn the_restart_card_counts_what_is_waiting_on_a_question() {
    let r = rig();
    let sid = r.session();
    r.turn_with("daemon_restart", &sid, json!({"reason": "it feels stuck"}))
        .await;
    let quiet = r.asked().remove(0).reason;
    assert!(!quiet.contains("waiting on a question"), "{quiet}");
    // A setting's card now waits on the owner too, beside the first restart card: two.
    let _ = ask(&r, "import.notes.enabled", json!(false)).await;
    let sid = r.session();
    r.turn_with("daemon_restart", &sid, json!({"reason": "it feels stuck"}))
        .await;
    let card = r
        .asked()
        .into_iter()
        .map(|c| c.reason)
        .find(|t| t.starts_with("Restart Theseus?") && t.contains("waiting on a question"))
        .expect("a restart card that counts the waiting execution");
    assert!(card.contains("2 executions are waiting"), "{card}");
    assert!(
        card.contains("self-improvement build")
            && card.contains("shows it as building, waiting on you"),
        "{card}"
    );
}
