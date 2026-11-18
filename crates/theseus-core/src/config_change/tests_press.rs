//! A press the owner's earlier proof covers goes through with no code (issue 22):
//! the window's place is the one the Discord press is judged by, the list says
//! so before the press, and the press decides at press time. Every case is the
//! owner's direct message, as the Discord binding asks and answers it.

use serde_json::{json, Value};
use theseus_protocol::{method, RpcError};

use super::tests::{change, rig, Rig, FILE};
use crate::approval::{Client, Surface};
use crate::node::ResultStatus;
use crate::owner_proof::{Footing, Proofs};
use crate::tests_m3::{rpc_as, surface};

/// A sensitive setting that asks for a code, or a window of the place.
const CEILING: &str = "kernel.daily_spend_ceiling_usd";
/// The owner's Discord id, as the rig's `[places] owner` has it.
const OWNER: &str = "4100000000000000001";

fn discord() -> Client {
    surface("discord#1", Surface::Discord)
}

/// The owner in a direct message (no guild) or in a guild channel.
fn origin(guild: bool) -> Value {
    json!({"user_id": OWNER, "channel_id": "777",
           "guild_id": guild.then_some("888")})
}

/// The card as the DM presser's `confirm.list` shows it.
async fn listed(r: &Rig, corr: &str, guild: bool) -> Value {
    let list = rpc_as(
        &r.core,
        discord(),
        method::CONFIRM_LIST,
        json!({"discord": origin(guild)}),
    )
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

/// The press: Approve, with the code the form held if there was one.
async fn press(r: &Rig, corr: &str, code: Option<&str>) -> Result<Value, RpcError> {
    rpc_as(
        &r.core,
        discord(),
        method::ACTION_CONFIRM,
        json!({"correlation_id": corr, "approve": true, "code": code,
               "discord": origin(false)}),
    )
    .await
}

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

/// The window the owner's code opened in the DM is the one the DM's list and
/// press are judged by, and a guild channel of the same owner is another place.
#[tokio::test]
async fn the_window_a_dm_code_opens_is_the_dms_list_and_press_place() {
    let r = rig();
    r.core.proofs.open_stand_in_window("discord:dm");
    let q = ask(&r, CEILING, json!(300)).await;
    let dm = listed(&r, &q, false).await;
    assert_eq!(dm["proof"]["code_needed"], false, "{dm}");
    assert!(dm["proof"]["line"]
        .as_str()
        .unwrap()
        .starts_with("second proof: given here"));
    let channel = listed(&r, &q, true).await;
    assert_eq!(channel["proof"]["code_needed"], true, "{channel}");
}

/// A press inside the window, with no code: the change is made, the use is a
/// row, no window is opened or lengthened, and the footing is the earlier proof.
#[tokio::test]
async fn a_press_inside_the_window_makes_the_change_with_no_code() {
    let r = rig();
    r.core.proofs.open_stand_in_window("discord:dm");
    let before = r.core.proofs.status().windows;
    let q = ask(&r, CEILING, json!(300)).await;
    press(&r, &q, None).await.unwrap();
    assert!(r.file_text().contains("daily_spend_ceiling_usd"));
    assert!(!waiting(&r, &q));
    assert_eq!(r.rows("owner.proof.window_used").len(), 1);
    assert_eq!(r.core.proofs.status().windows, before, "no window moved");
    assert!(matches!(
        r.core.proofs.footing_kept(&q),
        Some(Footing::Earlier(_))
    ));
}

/// A press with no window and no code changes nothing: the form is what asks.
#[tokio::test]
async fn a_press_with_no_window_and_no_code_changes_nothing() {
    let r = rig();
    r.core.proofs.enrol_stand_in();
    let q = ask(&r, CEILING, json!(300)).await;
    let card = listed(&r, &q, false).await;
    assert_eq!(card["proof"]["code_needed"], true, "{card}");
    assert!(press(&r, &q, None).await.is_err());
    assert_eq!(r.file_text(), FILE);
    assert!(waiting(&r, &q));
}

/// A question that takes a fresh code every time is not covered by a window:
/// the list says a code is needed, and a press without one changes nothing.
#[tokio::test]
async fn a_fresh_question_inside_a_window_still_needs_its_code() {
    let r = rig();
    let secret = r.core.proofs.open_stand_in_window("discord:dm");
    let q = ask(&r, "owner_proof.window_hours", json!(1)).await;
    let card = listed(&r, &q, false).await;
    assert_eq!(card["needs_fresh_proof"], true, "{card}");
    assert_eq!(card["proof"]["code_needed"], true, "{card}");
    assert!(press(&r, &q, None).await.is_err());
    assert_eq!(r.file_text(), FILE);
    press(&r, &q, Some(&Proofs::stand_in_code(&secret, 1)))
        .await
        .unwrap();
    assert_ne!(r.file_text(), FILE);
    assert_eq!(r.core.proofs.footing_kept(&q), Some(Footing::Code));
}

/// A window that ended between the card and the press: the press, which the
/// card's words had promised would go through, changes nothing, and the list
/// now says a code is needed (the form then opens).
#[tokio::test]
async fn a_window_that_ended_before_the_press_asks_for_the_code_then() {
    let r = rig();
    r.core.proofs.open_stand_in_window("discord:dm");
    let q = ask(&r, CEILING, json!(300)).await;
    assert_eq!(listed(&r, &q, false).await["proof"]["code_needed"], false);
    r.core.proofs.end_windows("test").unwrap();
    assert!(press(&r, &q, None).await.is_err());
    assert_eq!(r.file_text(), FILE);
    assert!(waiting(&r, &q));
    assert_eq!(listed(&r, &q, false).await["proof"]["code_needed"], true);
}

/// A good code with the press opens a window of the DM and says so: the
/// settled card's footing is a fresh code; the echo too.
#[tokio::test]
async fn a_press_with_a_code_says_a_fresh_code_was_the_footing() {
    let r = rig();
    let secret = r.core.proofs.enrol_stand_in();
    let q = ask(&r, CEILING, json!(300)).await;
    press(&r, &q, Some(&Proofs::stand_in_code(&secret, 0)))
        .await
        .unwrap();
    let footing = r.core.proofs.footing_kept(&q).expect("a footing");
    assert_eq!(footing.note(), "with a fresh code");
    assert!(footing.detail(false).contains("with a fresh code"));
    assert_eq!(r.core.proofs.status().windows.len(), 1);
}

/// The footing as the settled card says it.
#[test]
fn the_footing_is_said_in_words() {
    assert_eq!(Footing::Code.note(), "with a fresh code");
    assert_eq!(
        Footing::Earlier("3 minutes".into()).note(),
        "on the second proof given here 3 minutes earlier"
    );
    assert_eq!(
        Footing::Earlier("3 minutes".into()).detail(false),
        "second proof: given here 3 minutes earlier, so no code was asked"
    );
}
