//! The form for a secret's value (issue 29): the card drawn in the owner's direct messages and
//! nowhere else, the form as drawn, the press, and the submit's request to the daemon.
//!
//! The values here are invented. The vault's own program is not in this crate's reach; the whole
//! path to a stand-in vault is `crates/theseusd/tests/keep_form.rs`.

use serde_json::{json, Value};
use theseus_core::config_change::vault_keep::target::{State, Target};
use theseus_core::config_change::{vault_keep, words};
use theseus_protocol::work::QuestionKind;
use theseus_protocol::ConfirmRequest;
use twilight_model::application::interaction::Interaction;

use super::*;
use crate::render::{Buttons, Route};
use crate::runtime::tests::{core_with, OWNER};
use crate::runtime::{shared_for_tests, PlaceMsg};

/// Invented, and never in any output.
const VALUE: &str = "INVENTED-FORM-VALUE-5513";

fn target(title: &str, field: &str) -> Target {
    Target {
        vault: "Harbor".into(),
        vault_id: "vaultid00000000000000000001".into(),
        title: title.into(),
        item_id: None,
        field: field.into(),
        field_id: None,
        state: State::New,
        protected: false,
    }
}

/// The keep card as the daemon lists it: the decline alone, the words from the daemon.
fn keep_card(t: &Target) -> ConfirmRequest {
    let q = vault_keep::question(t, false, false, "the lantern provider's key");
    let args = json!({"question": q, "settled": vault_keep::settled(t), "keep": t.args()});
    let mut req = ConfirmRequest {
        correlation_id: "act_0000feed".into(),
        session_id: "ses_00000000harbour".into(),
        tool: "config.ack".into(),
        reason: q,
        plain: words::plain_of(&args),
        input: args,
        needs_proof: true,
        ..Default::default()
    };
    req.options = req
        .question(QuestionKind::Approval, true)
        .options
        .into_iter()
        .filter(|o| o.id == "decline")
        .collect();
    req
}

fn ids_of(c: &[Component]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for row in c {
        if let Component::ActionRow(r) = row {
            for b in &r.components {
                if let Component::Button(b) = b {
                    out.push((
                        b.label.clone().unwrap_or_default(),
                        b.custom_id.clone().unwrap_or_default(),
                    ));
                }
            }
        }
    }
    out
}

/// In the owner's direct messages with the setting on, the card says the form is there and what
/// it costs, before the press, and draws its button beside Decline.
#[test]
fn the_dm_card_says_what_the_form_costs_and_draws_its_button() {
    let mut req = keep_card(&target("Lantern new", "credential"));
    assert!(offer(&mut req, true, true));
    let said = &req.plain.as_ref().unwrap().detail;
    assert!(
        said.starts_with("Type the value in your own terminal (run `theseus vault keep`"),
        "{said}"
    );
    // One plain line: through the chat service, shown as typed, and the terminal neither.
    assert!(
        said.contains(
            "A value typed in the form goes through Discord's servers on its way to Theseus, and \
             the form shows what you type; the terminal does neither."
        ),
        "{said}"
    );
    // The terminal and the cockpit are still named; the old "no Approve here" words are gone.
    assert!(said.contains("`theseus vault keep`") && said.contains("cockpit"));
    assert!(!said.contains("offers only Decline"), "{said}");
    assert!(has_form(&req.options));
    // The card as Discord draws it, and its buttons.
    let card = crate::render::card(&req, &Route::Here, "");
    assert!(
        card.content
            .starts_with("Type the value in your own terminal"),
        "{}",
        card.content
    );
    assert_eq!(
        crate::courier::ask::card_buttons(&req, "act_0000feed"),
        Buttons::KeepForm("act_0000feed".into())
    );
    assert_eq!(
        ids_of(&buttons("act_0000feed")),
        [
            ("Type the value here".into(), "kf:act_0000feed".into()),
            ("Decline".into(), "confirm:decline:act_0000feed".into())
        ]
    );
    // The V2 card carries the same two answers.
    let v2 = crate::render::card_v2::waiting("act_0000feed", &card.content, &req.options);
    let ids = crate::render::v2::custom_ids(&v2);
    assert!(ids.iter().any(|i| i.ends_with(":keep_form")), "{ids:?}");
    assert!(ids.iter().any(|i| i.ends_with(":decline")), "{ids:?}");
}

/// Every other card is what it was: not in a direct message, the setting off, a card that is not
/// a keep card, and words this build did not write (a card drawn by another build).
#[test]
fn every_other_card_is_drawn_as_it_was() {
    let t = target("Lantern new", "credential");
    let as_was = keep_card(&t);
    for (what, in_dm, on) in [
        ("a channel", false, true),
        ("the setting off", true, false),
        ("both", false, false),
    ] {
        let mut req = as_was.clone();
        assert!(!offer(&mut req, in_dm, on), "{what}");
        assert_eq!(
            format!("{req:?}"),
            format!("{as_was:?}"),
            "{what}: not a word changed"
        );
        let card = crate::render::card(&req, &Route::Here, "");
        // Discord draws markdown literally here: every special character has a backslash before it.
        let plain = card.content.replace('\\', "");
        assert!(
            plain.starts_with("Type the value in your own terminal: run `theseus vault keep`"),
            "{what}: {}",
            card.content
        );
        assert!(plain.contains("offers only Decline"), "{what}");
        assert!(!card.content.contains("Discord's servers"), "{what}");
        assert_eq!(
            crate::courier::ask::card_buttons(&req, "act_0000feed"),
            Buttons::Decline("act_0000feed".into()),
            "{what}"
        );
    }
    // A card that is not a keep card.
    let mut settings = as_was.clone();
    settings.input = json!({"paths": []});
    assert!(!offer(&mut settings, true, true));
    // Words that are not this build's: no form, and no line about one.
    let mut old = as_was;
    old.plain.as_mut().unwrap().detail = "Keep a secret?\nType it in your terminal.".into();
    let before = old.clone();
    assert!(!offer(&mut old, true, true));
    assert_eq!(format!("{old:?}"), format!("{before:?}"));
}

/// The needs-you post is the owner's direct messages alone, and draws the form the same way: the
/// words swapped, the last line naming the button. Anything that is not a keep card is left.
#[test]
fn the_needs_you_post_draws_the_form_with_the_same_words() {
    let t = target("Lantern new", "credential");
    let q = vault_keep::question(&t, false, false, "the lantern provider's key");
    let body = json!({"settings": {"detail": q, "needs_proof": true, "short": "feed",
                                   "no_yes": true, "keep": true}});
    let drawn = needs_you_body(&body).expect("a keep card");
    let (before, last) = crate::render::settings::needs_you(&drawn).unwrap();
    let all = format!("{}{last}", before.concat());
    assert!(
        all.starts_with("Type the value in your own terminal"),
        "{all}"
    );
    assert!(all.contains("goes through Discord"), "{all}");
    assert!(all.contains("tap Type the value here"), "{all}");
    // Not a keep card, or not this build's words: nothing to draw.
    let mut other = body.clone();
    other["settings"]["keep"] = json!(false);
    assert!(needs_you_body(&other).is_none());
    let mut old = body;
    old["settings"]["detail"] = json!("Keep a secret?");
    assert!(needs_you_body(&old).is_none());
}

/// The form's label names the item and its field and is what a person sees while typing, clipped
/// as the proof form clips; a code is a field of its own, asked only when the daemon says so.
#[test]
fn the_form_is_labelled_with_the_item_and_never_puts_a_code_and_a_value_in_one_field() {
    let keep = target("Lantern new", "credential").args();
    let plain = modal("act_0000feed", &keep, false);
    let rows = plain["data"]["components"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["label"], "Value for Lantern new, field credential");
    assert!(rows[0]["description"]
        .as_str()
        .unwrap()
        .contains("Goes through Discord, and is shown as you type."));
    assert_eq!(rows[0]["component"]["custom_id"], "value");
    assert_eq!(plain["data"]["custom_id"], "kf:act_0000feed");
    // Needing a code: a field of its own, before the value, six or seven digits only.
    let proved = modal("act_0000feed", &keep, true);
    let rows = proved["data"]["components"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["component"]["custom_id"], "code");
    assert_eq!(rows[0]["component"]["max_length"], 7);
    assert_eq!(rows[1]["component"]["custom_id"], "value");
    assert_eq!(rows[1]["component"]["max_length"], 4000);
    // A long title and field are clipped with an ellipsis, inside Discord's limits.
    let long = target(&"t".repeat(64), &"f".repeat(64)).args();
    let m = modal("act_0000feed", &long, false);
    let label = m["data"]["components"][0]["label"].as_str().unwrap();
    let desc = m["data"]["components"][0]["description"].as_str().unwrap();
    assert!(
        label.chars().count() <= 45 && label.ends_with('…'),
        "{label}"
    );
    assert!(desc.chars().count() <= 100, "{desc}");
}

/// A `kf:` id names a question by the characters a question's id has, and nothing else.
#[test]
fn a_forms_question_is_read_from_its_id_and_only_a_questions_characters_count() {
    assert_eq!(question_of("kf:act_0000feed"), Some("act_0000feed"));
    for bad in [
        "kf:",
        "kf:act 1",
        "kf:act:1",
        "kf:../x",
        "pf:act_1",
        "confirm:decline:act_1",
    ] {
        assert_eq!(question_of(bad), None, "{bad}");
    }
    assert_eq!(question_of(&format!("kf:{}", "a".repeat(91))), None);
}

/// The card and the form as drawn, kept as files: a change to a word of either shows here.
/// `THESEUS_GOLDEN=write` rewrites the file, for a change that is meant.
#[test]
fn the_card_and_the_form_are_drawn_as_the_golden_says() {
    let t = target("Lantern new", "credential");
    let mut req = keep_card(&t);
    assert!(offer(&mut req, true, true));
    let card = crate::render::card(&req, &Route::Here, "");
    let mut out = format!(
        "=== the card in the owner's direct messages\n{}\n--- buttons\n",
        card.content
    );
    for (label, id) in ids_of(&buttons("act_0000feed")) {
        out.push_str(&format!("{label} -> {id}\n"));
    }
    let as_was = crate::render::card(&keep_card(&t), &Route::Here, "");
    out.push_str(&format!(
        "\n=== the same card anywhere else\n{}\n",
        as_was.content
    ));
    for (name, code) in [("without a code", false), ("with a code", true)] {
        out.push_str(&format!(
            "\n=== the form {name}\n{}\n",
            serde_json::to_string_pretty(&modal("act_0000feed", &t.args(), code)).unwrap()
        ));
    }
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/runtime/keep_form/golden/keep_form.txt"
    );
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, &out).unwrap();
    }
    let golden = std::fs::read_to_string(path).expect("the golden: THESEUS_GOLDEN=write writes it");
    assert_eq!(
        out, golden,
        "THESEUS_GOLDEN=write rewrites it, for a change you mean"
    );
}

/// The id every interaction here has; its token is the stand-in's, so that an edit finds its interaction.
const ID: &str = "1000000000000000011";

/// A press (`pressed`) or a submit (`submitted`), as the gateway delivers one, from `user` in
/// `channel` of `guild` (none: a direct message).
fn interaction(kind: u8, guild: Option<&str>, channel: u64, user: u64, data: Value) -> Interaction {
    let mut v = json!({
        "id": ID, "application_id": "1000000000000000002", "type": kind,
        "token": format!("fake-interaction-token-{ID}"), "version": 1, "authorizing_integration_owners": {},
        "channel": {"id": channel.to_string(), "type": if guild.is_some() { 0 } else { 1 }},
        "user": {"id": user.to_string(), "username": if user == OWNER { "dockhand" } else { "mallory" },
                 "discriminator": "0", "avatar": null},
        "data": data,
    });
    if let Some(g) = guild {
        v["guild_id"] = json!(g);
    }
    serde_json::from_value(v).unwrap()
}

fn pressed(guild: Option<&str>, channel: u64, user: u64, custom_id: &str) -> Interaction {
    interaction(
        3,
        guild,
        channel,
        user,
        json!({"custom_id": custom_id, "component_type": 2}),
    )
}

fn submitted(
    guild: Option<&str>,
    channel: u64,
    user: u64,
    custom_id: &str,
    fields: &[(&str, &str)],
) -> Interaction {
    let rows: Vec<Value> = fields
        .iter()
        .enumerate()
        .map(|(n, (k, v))| {
            json!({"type": 18, "id": 2 * n + 1,
                   "component": {"type": 4, "id": 2 * n + 2, "custom_id": k, "value": v}})
        })
        .collect();
    interaction(
        5,
        guild,
        channel,
        user,
        json!({"custom_id": custom_id, "components": rows}),
    )
}

/// A shared state over a core whose Discord is the fake, the owner's DM bound.
fn rig(
    keep_by_discord: bool,
) -> (
    std::sync::Arc<Shared>,
    std::sync::Arc<theseus_sim::fake_discord::FakeDiscord>,
    tempfile::TempDir,
    tokio::sync::mpsc::UnboundedReceiver<PlaceMsg>,
) {
    let fake = theseus_sim::fake_discord::FakeDiscord::start();
    let d = tempfile::tempdir().unwrap();
    let addr = fake.addr.clone();
    let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
        c.discord.rest_proxy = Some(addr);
        c.vault.keep_by_discord = keep_by_discord;
    });
    let shared = shared_for_tests(&core);
    let (dm_tx, dm_rx) = tokio::sync::mpsc::unbounded_channel();
    shared
        .routes
        .lock()
        .unwrap()
        .by_dm_user
        .insert(OWNER, dm_tx);
    (shared, fake, d, dm_rx)
}

const DM: u64 = 444_444_444_444_444_444;
/// A guild channel this daemon binds.
const CHANNEL: u64 = 900_000_000_000_000_001;

fn said(fake: &theseus_sim::fake_discord::FakeDiscord) -> Vec<theseus_sim::fake_discord::Reply> {
    fake.replies()
}

/// Each answer to the interaction as (kind, callback type, private): a submit is acknowledged at once with a
/// deferred message (type 5) that is private, and its words arrive as an edit of it, private too.
type Kinds<'a> = Vec<(&'a str, Option<u64>, bool)>;

fn private_kinds(replies: &[theseus_sim::fake_discord::Reply]) -> Kinds<'_> {
    let mut kinds = Vec::new();
    for r in replies {
        kinds.push((r.kind.as_str(), r.response_type, r.ephemeral));
    }
    kinds
}

const PRIVATE_THEN_EDITED: [(&str, Option<u64>, bool); 2] =
    [("callback", Some(5), true), ("original", None, true)];

/// With the setting off, a press says so to the presser alone, opens no form and asks the daemon
/// nothing; in a channel it says where a value is typed, and opens none either.
#[tokio::test]
async fn a_press_opens_no_form_when_the_setting_is_off_or_the_place_is_a_channel() {
    let (off, fake, _d, _rx) = rig(false);
    off.clone()
        .on_interaction(pressed(None, DM, OWNER, "kf:act_0000feed"))
        .await;
    let replies = said(&fake);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(
        replies[0].ephemeral && replies[0].modal.is_none(),
        "{replies:?}"
    );
    assert!(
        replies[0]
            .content
            .as_deref()
            .unwrap()
            .contains("keep_by_discord"),
        "{replies:?}"
    );
    assert!(
        !crate::rpc_client::calls()
            .iter()
            .any(|m| m == theseus_protocol::method::CONFIRM_LIST),
        "the daemon was not asked: {:?}",
        crate::rpc_client::calls()
    );

    // A channel this daemon binds is a place of ours, and a form is not opened there.
    let (on, fake, _d, _rx) = rig(true);
    let (ch_tx, _ch_rx) = tokio::sync::mpsc::unbounded_channel();
    {
        let mut r = on.routes.lock().unwrap();
        r.by_channel.insert(CHANNEL, ch_tx);
        r.users.insert(CHANNEL, vec![OWNER]);
    }
    on.clone()
        .on_interaction(pressed(
            Some("314159265358979323"),
            CHANNEL,
            OWNER,
            "kf:act_0000feed",
        ))
        .await;
    let replies = said(&fake);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(
        replies[0].modal.is_none() && replies[0].ephemeral,
        "{replies:?}"
    );
    assert!(
        replies[0]
            .content
            .as_deref()
            .unwrap()
            .contains("only in your direct messages"),
        "{replies:?}"
    );
}

/// A press for a card the daemon does not list as a keep card opens no form: the daemon's list is
/// what decides, not the id on the button.
#[tokio::test]
async fn a_press_for_a_question_that_is_not_a_waiting_keep_card_opens_no_form() {
    let (on, fake, _d, _rx) = rig(true);
    on.clone()
        .on_interaction(pressed(None, DM, OWNER, "kf:act_0000feed"))
        .await;
    let replies = said(&fake);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(
        replies[0].modal.is_none() && replies[0].ephemeral,
        "{replies:?}"
    );
    assert!(
        replies[0]
            .content
            .as_deref()
            .unwrap()
            .contains("not waiting for a value"),
        "{replies:?}"
    );
}

/// A card the daemon lists for the presser and the presser may answer, that is not a keep card
/// (a turn's write waiting for a yes): its id on a `kf:` button opens no form.
#[tokio::test]
async fn a_press_for_another_kind_of_card_opens_no_form() {
    use theseus_core::provider::Scripted;
    use theseus_protocol::{TurnSubmitParams, TurnSubmitResult};
    let fake = theseus_sim::fake_discord::FakeDiscord::start();
    let d = tempfile::tempdir().unwrap();
    let core = crate::runtime::tests_why::core_at_with(
        d.path(),
        &fake.addr,
        vec![
            Scripted::tools(
                "Writing.",
                &[("w1", "fs_write", json!({"path": "a.txt", "content": "x\n"}))],
            ),
            Scripted::text("Done."),
        ],
        |c| c.places.owner = Some(vec![format!("discord:{OWNER}")]),
    );
    let rec = theseus_core::session::SessionRecord::new(
        theseus_protocol::SessionKind::Conversation,
        None,
    );
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    core.outbox
        .bind_place(&format!("dm:{OWNER}"), &sid)
        .unwrap();
    core.bind_places(vec![theseus_core::places::BoundPlace {
        target: format!("discord:dm:{OWNER}"),
        name: "DM".into(),
        private: false,
        ..Default::default()
    }]);
    let shared = shared_for_tests(&core);
    // The turn is the owner's own, from the terminal; its call asks first in a place that is not theirs.
    let (cli, _notes) = crate::rpc_client::RpcClient::connect(
        core.clone(),
        theseus_core::approval::Client::new("test", theseus_core::approval::Surface::Cli),
    );
    let first: TurnSubmitResult = cli
        .call(
            theseus_protocol::method::TURN_SUBMIT,
            TurnSubmitParams {
                session_id: Some(sid),
                input: "write a file".into(),
                author: Some("test".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let corr = first
        .awaiting_confirm
        .clone()
        .unwrap_or_else(|| panic!("its call asks first: {first:?}"));
    let listed: ConfirmListResult = shared
        .rpc
        .call(
            theseus_protocol::method::CONFIRM_LIST,
            ConfirmListParams {
                discord: origin_of(Some(OWNER), Some(DM), None),
            },
        )
        .await
        .unwrap();
    let card = listed
        .confirms
        .iter()
        .find(|c| c.correlation_id == corr)
        .expect("the card is listed for the owner");
    assert_eq!(card.may_answer, Some(true), "{card:?}");
    assert!(!keeps(card));
    let (dm_tx, _dm_rx) = tokio::sync::mpsc::unbounded_channel();
    shared
        .routes
        .lock()
        .unwrap()
        .by_dm_user
        .insert(OWNER, dm_tx);
    shared
        .clone()
        .on_interaction(pressed(None, DM, OWNER, &format!("kf:{corr}")))
        .await;
    let replies = fake.replies();
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(
        replies[0].modal.is_none() && replies[0].ephemeral,
        "{replies:?}"
    );
    assert!(replies[0]
        .content
        .as_deref()
        .unwrap()
        .contains("not waiting for a value"));
}

/// The submit: one request to the daemon with the question from the form's id, the value in the
/// value field alone, and the presser; its refusal is told to the presser and the value is found
/// nowhere: not in what the binding said, not in its rows, not in the store, not in a trace.
#[tokio::test]
async fn a_submit_is_one_request_and_the_value_is_nowhere_else() {
    let log = tracing_capture::start();
    let (shared, fake, d, _rx) = rig(true);
    shared
        .clone()
        .on_interaction(submitted(
            None,
            DM,
            OWNER,
            "kf:act_0000feed",
            &[("code", "123456"), ("value", VALUE)],
        ))
        .await;
    let lines = crate::rpc_client::keep_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let sent: Value = serde_json::from_str(&lines[0]).unwrap();
    let p = &sent["params"];
    assert_eq!(p["question_id"], "act_0000feed");
    assert_eq!(p["value"], VALUE);
    assert_eq!(p["code"], "123456");
    assert_eq!(p["author"], "discord:dockhand");
    assert_eq!(p["discord"]["user_id"], OWNER.to_string());
    assert_eq!(p["discord"]["channel_id"], DM.to_string());
    assert!(p["discord"]["guild_id"].is_null(), "a direct message: {p}");
    // The value is in that one field of that one request.
    assert_eq!(lines[0].matches(VALUE).count(), 1, "{}", lines[0]);

    // The presser is answered at once, privately, and told what the daemon said by the edit that completes
    // that answer, still private; it holds no value.
    let replies = said(&fake);
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(replies[1].ephemeral, "{replies:?}");
    assert_eq!(private_kinds(&replies), PRIVATE_THEN_EDITED, "{replies:?}");
    let told = replies[1].content.clone().unwrap();
    assert!(
        told.starts_with("🔐 Nothing was kept") || told.starts_with("⚠️ Nothing was kept"),
        "{told}"
    );
    assert!(!told.contains(VALUE), "{told}");
    // Nothing the binding sent to Discord, wrote to the daemon's rows, or logged holds it.
    let rest = format!("{:?}{:?}", fake.seen(), fake.all_messages());
    assert!(!rest.contains(VALUE), "in what Discord was sent");
    shared.core.store.flush_waiting();
    let rows = format!(
        "{:?}",
        shared
            .core
            .store
            .ledger_tail::<theseus_core::ledger::LedgerRow>(100_000)
            .unwrap()
    );
    assert!(!rows.contains(VALUE), "in a ledger row");
    assert!(
        rows.contains("keep_form"),
        "the attempt left its row: {rows}"
    );
    assert!(!log.text().contains(VALUE), "in a trace");
    let on_disk = tracing_capture::files_holding(d.path(), VALUE);
    assert!(on_disk.is_empty(), "on disk: {on_disk:?}");
}

/// The daemon is slow to answer (it writes to the vault): past Discord's three seconds. The first response has
/// gone already, as a private deferral, so the service does not refuse it; the kept words complete it, private
/// still, and no value is in what the presser sees, in the row, or in the log.
#[tokio::test]
async fn a_submit_the_daemon_is_slow_to_answer_still_shows_the_kept_words_privately() {
    let log = tracing_capture::start();
    let (shared, fake, _d, _rx) = rig(true);
    let kept = json!({"vault": "Harbor", "title": "Lantern new", "field": "credential",
                      "replaced": false, "words": "Kept in Harbor: Lantern new, field credential."});
    let held = crate::rpc_client::slow_once(method::VAULT_KEEP_ANSWER, "act_0000slow", kept);
    let submit = shared.clone().on_interaction(submitted(
        None,
        DM,
        OWNER,
        "kf:act_0000slow",
        &[("value", VALUE)],
    ));
    // Three and a half seconds go by while the daemon call is held: the stand-in is told they did (a test
    // does not wait them), after the deferral and before the words.
    let takes_its_time = async {
        held.reached.notified().await;
        fake.backdate_interaction(ID, 3_500);
        held.release.notify_one();
    };
    tokio::join!(submit, takes_its_time);
    let replies = said(&fake);
    assert!(replies.iter().all(|r| r.kind != "refused"), "{replies:?}");
    assert_eq!(private_kinds(&replies), PRIVATE_THEN_EDITED, "{replies:?}");
    assert_eq!(
        replies[1].content.as_deref(),
        Some("✅ Kept in Harbor: Lantern new, field credential."),
        "{replies:?}"
    );
    assert!(!format!("{:?}{:?}", fake.seen(), replies).contains(VALUE));
    shared.core.store.flush_waiting();
    let rows = format!(
        "{:?}",
        shared
            .core
            .store
            .ledger_tail::<theseus_core::ledger::LedgerRow>(100_000)
            .unwrap()
    );
    assert!(
        rows.contains("keep_form") && !rows.contains(VALUE),
        "{rows}"
    );
    assert!(!log.text().contains(VALUE), "in a trace");
}

/// The daemon's error is completed privately as well (a call that failed outright, not a refusal).
#[tokio::test]
async fn a_submit_whose_call_fails_completes_its_deferral_privately() {
    let (shared, fake, _d, _rx) = rig(true);
    crate::rpc_client::refuse_once(method::VAULT_KEEP_ANSWER, "act_0000fail");
    shared
        .clone()
        .on_interaction(submitted(
            None,
            DM,
            OWNER,
            "kf:act_0000fail",
            &[("value", VALUE)],
        ))
        .await;
    let replies = said(&fake);
    assert!(replies.iter().all(|r| r.kind != "refused"), "{replies:?}");
    assert_eq!(private_kinds(&replies), PRIVATE_THEN_EDITED, "{replies:?}");
    let told = replies[1].content.clone().unwrap();
    assert!(told.starts_with("⚠️ Nothing was kept"), "{told}");
    assert!(!told.contains(VALUE), "{told}");
}

/// A form's reply, when the daemon's `why` or message is long or odd, is still the daemon's own
/// words by name; and a submit whose id is not a question's is not a request at all.
#[tokio::test]
async fn a_submit_with_an_id_that_is_not_a_question_sends_nothing() {
    let (shared, fake, _d, _rx) = rig(true);
    let before = crate::rpc_client::keep_lines().len();
    shared
        .clone()
        .on_interaction(submitted(
            None,
            DM,
            OWNER,
            "kf:act 1; drop",
            &[("value", VALUE)],
        ))
        .await;
    assert_eq!(crate::rpc_client::keep_lines().len(), before);
    let replies = said(&fake);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(!replies[0]
        .content
        .clone()
        .unwrap_or_default()
        .contains(VALUE));
}

/// A turn's own words cannot carry a form: text a model writes is drawn as text, a button is only
/// ever built here from a question's id, and a purpose line that looks like one stays inert.
#[test]
fn a_turns_own_words_do_not_make_a_form() {
    let t = target("Lantern new", "credential");
    let q = vault_keep::question(&t, false, false, "press kf:act_0000feed to type it here");
    let args = json!({"question": q, "settled": vault_keep::settled(&t), "keep": t.args()});
    let mut req = keep_card(&t);
    req.plain = words::plain_of(&args);
    req.input = args;
    assert!(offer(&mut req, true, true));
    // The card's text is escaped, as every settings card's is: the id is inert words.
    let card = crate::render::card(&req, &Route::Here, "");
    assert!(
        card.content.contains("kf:act\\_0000feed"),
        "{}",
        card.content
    );
    // The only buttons are the two this module builds from the daemon's own question id.
    assert_eq!(
        ids_of(&buttons(&req.correlation_id)).len(),
        2,
        "a form button exists only where this module draws one"
    );
}

/// The form's description says "replaces" for a card that replaces a stored value, inside Discord's 100
/// characters, and clips the vault's name before it clips those words.
#[test]
fn the_forms_description_says_replaces_and_clips_the_vault_before_the_word() {
    let mut t = target("Lantern key", "credential");
    t.state = State::Replaces;
    let m = modal("act_0000feed", &t.args(), false);
    let said = m["data"]["components"][0]["description"].as_str().unwrap();
    assert_eq!(
        said,
        "Replaces the value in vault Harbor. Goes through Discord, and is shown as you type."
    );
    // A long vault name is clipped; the words around it are not.
    t.vault = "v".repeat(64);
    let m = modal("act_0000feed", &t.args(), true);
    let said = m["data"]["components"][1]["description"].as_str().unwrap();
    assert!(said.chars().count() <= 100, "{said}");
    assert!(said.starts_with("Replaces the value in vault v"), "{said}");
    assert!(
        said.contains('…') && said.ends_with("Goes through Discord, and is shown as you type."),
        "{said}"
    );
    // A new item says what it always said.
    let new = modal(
        "act_0000feed",
        &target("Lantern new", "credential").args(),
        false,
    );
    assert_eq!(
        new["data"]["components"][0]["description"],
        "In the vault Harbor. Goes through Discord, and is shown as you type."
    );
}

/// A value the daemon would refuse is found by the form (it counts characters, the daemon bytes) and goes
/// to the daemon as a class and no value: the daemon records the attempt and answers in its words. The
/// request holds no value, and what the presser is told holds none.
#[tokio::test]
async fn a_value_the_daemon_would_refuse_is_sent_as_a_class_and_no_value() {
    let (shared, fake, _d, _rx) = rig(true);
    let before = crate::rpc_client::keep_lines().len();
    let long = format!("{VALUE}{}", "é".repeat(2100)); // 2,100 characters, over 4,096 bytes
    for value in [long.as_str(), "   "] {
        shared
            .clone()
            .on_interaction(submitted(
                None,
                DM,
                OWNER,
                "kf:act_0000feed",
                &[("value", value)],
            ))
            .await;
    }
    let lines = crate::rpc_client::keep_lines();
    assert_eq!(lines.len(), before + 2, "{lines:?}");
    for (line, class) in lines[before..].iter().zip(["not_a_value", "blank"]) {
        let sent: Value = serde_json::from_str(line).unwrap();
        assert_eq!(sent["params"]["refused"], class, "{line}");
        assert_eq!(sent["params"]["value"], "", "{line}");
        assert!(!line.contains("é") && !line.contains(VALUE), "{line}");
    }
    let told: Vec<String> = fake
        .replies()
        .into_iter()
        .filter_map(|r| r.content)
        .collect();
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(
        told.iter().all(|t| !t.contains("é") && !t.contains(VALUE)),
        "{told:?}"
    );
    // (This daemon has no such question waiting, so its answer is the error arm, through the real daemon.)
    assert!(
        told.iter().all(|t| t.starts_with("⚠️ Nothing was kept")),
        "{told:?}"
    );
    let replies = said(&fake);
    assert!(
        replies.iter().all(|r| r.ephemeral && r.kind != "refused"),
        "{replies:?}"
    );
}

/// `/answer` in the owner's direct messages draws the form's words and its button as the card does; in a
/// channel, or with the setting off, it draws what it drew. (The Details press has no keep card to explain:
/// a keep card is drawn with Decline and the form's button alone, never a Details button, see the first
/// test.)
#[test]
fn the_answer_view_draws_the_form_where_the_card_does() {
    let t = target("Lantern new", "credential");
    let mut req = keep_card(&t);
    req.may_answer = Some(true);
    let as_was = req.clone();
    assert!(offer(&mut req, true, true));
    let view = crate::runtime::answer_cmd::view_of(&req);
    let ids = crate::render::v2::custom_ids(&view);
    assert!(ids.iter().any(|i| i.ends_with(":keep_form")), "{ids:?}");
    assert!(ids.iter().any(|i| i.ends_with(":decline")), "{ids:?}");
    let said = crate::render::v2::texts(&view).join("\n");
    assert!(said.contains("Discord's servers"), "{said}");
    // Not offered: the view is what it was.
    let view = crate::runtime::answer_cmd::view_of(&as_was);
    let ids = crate::render::v2::custom_ids(&view);
    assert!(!ids.iter().any(|i| i.ends_with(":keep_form")), "{ids:?}");
    assert!(!crate::render::v2::texts(&view)
        .join("\n")
        .contains("Discord's servers"));
}

/// A tiny capture of everything logged, at trace level, for the test that plants a value.
mod tracing_capture {
    use std::sync::{Arc, Mutex};

    /// What was logged, while the subscriber that wrote it is the thread's own.
    pub struct Log {
        buf: Arc<Mutex<Vec<u8>>>,
        _guard: tracing::subscriber::DefaultGuard,
    }

    #[derive(Clone)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
        type Writer = Sink;
        fn make_writer(&'a self) -> Sink {
            self.clone()
        }
    }

    pub fn start() -> Log {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sub = tracing_subscriber::fmt()
            .with_writer(Sink(buf.clone()))
            .with_max_level(tracing::Level::TRACE)
            .finish();
        Log {
            buf,
            _guard: tracing::subscriber::set_default(sub),
        }
    }

    impl Log {
        pub fn text(&self) -> String {
            String::from_utf8_lossy(&self.buf.lock().unwrap()).into_owned()
        }
    }

    pub fn files_holding(root: &std::path::Path, needle: &str) -> Vec<String> {
        let mut hits = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(p) = stack.pop() {
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                stack.extend(
                    std::fs::read_dir(&p)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.path()),
                );
            } else if let Ok(bytes) = std::fs::read(&p) {
                if bytes.windows(needle.len()).any(|w| w == needle.as_bytes()) {
                    hits.push(p.display().to_string());
                }
            }
        }
        hits
    }
}
