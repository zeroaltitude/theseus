//! The form in the owner's Discord direct messages (issue 29): `vault.keep.answer` as the
//! binding sends it, from the Discord surface with the presser's ids, through the whole core
//! against the stub of the vault's program.
//!
//! A value planted here (`VALUE`, invented) is searched for where it must not be, as the
//! other places' tests do: what was said, the nodes and the ledger, the outbox and the whole
//! state directory on disk, a trace at trace level, the list of what waits. It is on the stub's
//! standard input, once. Each refusal leaves the vault's program asked for nothing but its lists.

use serde_json::{json, Value};
use theseus_protocol::{error_code, method, RpcError};

use super::*;
use crate::config_change::tests::{change, OWNER};
use crate::places::BoundPlace;

/// A user who is not an owner.
const MALLORY: u64 = 4_100_000_000_000_000_999;

fn discord() -> Client {
    surface("discord#1", Surface::Discord)
}

/// What the binding reads off a press: the user, the channel, and the guild (none in a DM).
fn origin(user: u64, channel: &str, guild: Option<&str>) -> Value {
    json!({"user_id": user.to_string(), "channel_id": channel, "guild_id": guild})
}

fn dm() -> Value {
    origin(OWNER, "7001", None)
}

/// The submit, as the binding sends it.
async fn submit(
    r: &Rig,
    q: &str,
    value: &str,
    code: Option<&str>,
    from: Value,
) -> Result<Value, RpcError> {
    let mut p = json!({"question_id": q, "value": value, "author": "discord:dockhand",
                       "discord": from});
    if let Some(c) = code {
        p["code"] = json!(c);
    }
    rpc_as(&r.core, discord(), method::VAULT_KEEP_ANSWER, p).await
}

/// The rig with the setting as the test wants it.
fn rig_form(keep_by_discord: bool) -> (Rig, tempfile::TempDir) {
    let r = rig_with(|cfg, _| {
        cfg.vault.keep_by_discord = keep_by_discord;
        cfg.secrets.insert(
            "settings_key".into(),
            "op://Harbor/Settings key/credential".into(),
        );
    });
    let dir = tempfile::tempdir().unwrap();
    r.core
        .tools
        .look
        .set_vault(Arc::new(stub(dir.path(), "ok")));
    (r, dir)
}

/// The class the last row of attempts that stopped carries.
fn stopped(r: &Rig) -> Vec<String> {
    r.rows("vault.keep")
        .iter()
        .filter(|row| row["outcome"] == "stopped")
        .map(|row| row["why"].as_str().unwrap().to_string())
        .collect()
}

/// The owner, in their direct messages, gives the value through the form: it is kept, the card
/// settles, and the value is found nowhere but the stub's standard input.
#[tokio::test]
async fn the_value_goes_from_the_dm_form_to_the_vault_and_nowhere_else() {
    let (log, _guard) = trace();
    let (r, dir) = rig_form(true);
    let secret = secret_of(&r);
    let sid = r.session();
    let q = ask(&r, input("Lantern new", "credential")).await;
    let params = format!(
        "{:?}",
        VaultKeepAnswerParams {
            question_id: q.clone(),
            value: SecretText::new(VALUE),
            ..Default::default()
        }
    );
    assert!(!params.contains(VALUE), "{params}");
    let said = submit(&r, &q, VALUE, Some(&code(&secret, 0)), dm())
        .await
        .unwrap();
    let words = said["words"].as_str().unwrap().to_string();
    assert!(
        words.starts_with("Kept in the vault \"Harbor\", item \"Lantern new\", field \"credential\": it reads back as written."),
        "{words}"
    );
    assert!(!waiting(&r, &q), "the card is settled");
    let rows = r.rows("vault.keep");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["outcome"], "kept");
    assert_eq!(rows[0]["question"], q);
    assert!(
        rows[0].to_string().contains("discord:dm"),
        "the row says where it was typed: {:?}",
        rows[0]
    );
    // What the card was settled with, and every error string, are the names and the outcome.
    assert_value_nowhere(&r, &sid, dir.path(), &[&said.to_string(), &words], &log);
    let kept = r.core.kernel.action(&q).unwrap().unwrap();
    assert!(!format!("{kept:?}").contains(VALUE));
}

/// A submit for a question that is not open, and a second submit: each is refused, and nothing is
/// written for either.
#[tokio::test]
async fn a_question_that_is_not_open_is_not_a_form_to_submit() {
    let (r, dir) = rig_form(true);
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    // Not a question at all.
    let none = submit(&r, "act_00000000nothing", VALUE, None, dm())
        .await
        .unwrap_err();
    assert_eq!(none.code, error_code::NOT_FOUND, "{none:?}");
    assert!(!wrote(dir.path()));
    // Given once, it is single use.
    let first = submit(&r, &q, VALUE, Some(&code(&secret, 0)), dm()).await;
    assert!(first.is_ok(), "the first submit: {first:?}");
    let again = submit(&r, &q, VALUE, Some(&code(&secret, 1)), dm())
        .await
        .unwrap_err();
    assert!(again.message.contains("not waiting"), "{again:?}");
    assert_eq!(stopped(&r).len(), 2, "{:?}", r.rows("vault.keep"));
    let stdin = std::fs::read_to_string(dir.path().join("op.stdin")).unwrap();
    assert_eq!(stdin.matches(VALUE).count(), 1, "written once");
    for e in [&none, &again] {
        assert!(!format!("{e:?}").contains(VALUE));
    }
}

/// A form whose `custom_id` names another kind of question (a settings change waiting for a yes)
/// is refused: only a keep card takes a value, and the other question is untouched.
#[tokio::test]
async fn a_form_for_another_kind_of_question_is_refused() {
    let (r, dir) = rig_form(true);
    let sid = r.session();
    r.turn(&sid, change("kernel.daily_spend_ceiling_usd", json!(300)))
        .await;
    let other = r.asked().pop().expect("a settings card waits");
    assert!(!other.input["keep"].is_object(), "{other:?}");
    let wrong = submit(&r, &other.correlation_id, VALUE, None, dm())
        .await
        .unwrap_err();
    assert!(wrong.message.contains("not a secret to keep"), "{wrong:?}");
    assert!(!format!("{wrong:?}").contains(VALUE));
    assert!(!wrote(dir.path()));
    assert!(
        waiting(&r, &other.correlation_id),
        "the question is untouched"
    );
    assert_eq!(stopped(&r), ["not_a_keep_card"]);
}

/// A submit from anywhere but the owner's direct messages is refused: a shared channel, a
/// channel bound private (which the other answers accept: a value is stricter), another user in a
/// direct message, and a client that is not the binding naming a Discord place.
#[tokio::test]
async fn only_the_owner_in_their_direct_messages_gives_a_value_by_discord() {
    let (r, dir) = rig_form(true);
    let secret = secret_of(&r);
    r.core.bind_places(vec![BoundPlace {
        target: "discord:channel:6002".into(),
        name: "private channel".into(),
        private: true,
        ..Default::default()
    }]);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let places = [
        ("a shared channel", origin(OWNER, "6001", Some("888"))),
        (
            "a channel bound private",
            origin(OWNER, "6002", Some("888")),
        ),
        ("another user in a DM", origin(MALLORY, "7002", None)),
    ];
    for (what, from) in places {
        let e = submit(&r, &q, VALUE, Some(&code(&secret, 0)), from)
            .await
            .unwrap_err();
        assert_eq!(e.code, error_code::REFUSED, "{what}: {e:?}");
        assert!(!format!("{e:?}").contains(VALUE), "{what}");
        assert!(!wrote(dir.path()), "{what}");
        assert!(waiting(&r, &q), "{what}: the card still waits");
    }
    // The CLI naming a Discord place is not the binding.
    let from_cli = rpc_as(
        &r.core,
        cli(),
        method::VAULT_KEEP_ANSWER,
        json!({"question_id": q, "value": VALUE, "discord": dm()}),
    )
    .await
    .unwrap_err();
    assert_eq!(from_cli.code, error_code::REFUSED, "{from_cli:?}");
    assert!(!wrote(dir.path()));
    assert_eq!(stopped(&r).len(), 4, "{:?}", stopped(&r));
    let all = format!("{:?}", r.rows("vault.keep"));
    assert!(!all.contains(VALUE), "{all}");
    // And the owner in their DM can.
    let kept = submit(&r, &q, VALUE, Some(&code(&secret, 1)), dm()).await;
    assert!(kept.is_ok(), "{kept:?}");
    assert!(wrote(dir.path()));
}

/// With `[vault] keep_by_discord` off, a value sent from Discord is refused with the setting's
/// name in the words and a row of its own class; the terminal and the cockpit are as they were.
#[tokio::test]
async fn with_the_setting_off_a_value_from_discord_is_refused_and_the_terminal_is_not() {
    let (r, dir) = rig_form(false);
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let e = submit(&r, &q, VALUE, Some(&code(&secret, 0)), dm())
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(format!("{e:?}").contains("keep_by_discord"), "{e:?}");
    assert!(!format!("{e:?}").contains(VALUE));
    assert!(!wrote(dir.path()));
    assert!(waiting(&r, &q));
    assert_eq!(stopped(&r), ["setting_off"]);
    let ok = give(&r, cli(), &q, VALUE, Some(&code(&secret, 1))).await;
    assert!(ok.is_ok(), "{ok:?}");
    assert!(wrote(dir.path()));
}

/// A blank value, one over 4 KiB, and a missing or wrong code: each is refused before the vault
/// is asked to write, the card keeps waiting, and no value is in what is said.
#[tokio::test]
async fn a_blank_an_over_long_or_unproved_value_is_refused_by_the_form_as_by_the_terminal() {
    let (r, dir) = rig_form(true);
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    super::old_card(&r, &q, false);
    let long = "x".repeat(4097);
    let tries = [
        ("blank", "   ".to_string(), Some(code(&secret, 0))),
        ("too long", long, Some(code(&secret, 0))),
        ("no code", VALUE.to_string(), None),
        (
            "a wrong code",
            VALUE.to_string(),
            Some("000000".to_string()),
        ),
    ];
    let spent = checks(&r);
    for (what, value, c) in tries {
        let e = submit(&r, &q, &value, c.as_deref(), dm())
            .await
            .unwrap_err();
        assert!(
            [error_code::REFUSED, error_code::INVALID_PARAMS].contains(&e.code),
            "{what}: {e:?}"
        );
        assert!(!format!("{e:?}").contains(VALUE), "{what}");
        assert!(!wrote(dir.path()), "{what}");
        assert!(waiting(&r, &q), "{what}: the card keeps waiting");
    }
    assert_eq!(
        stopped(&r).len(),
        4,
        "every attempt left a row: {:?}",
        stopped(&r)
    );
    // The value was trimmed as every secret is: a pasted newline is not part of it.
    submit(&r, &q, &format!("{VALUE}\n"), Some(&code(&secret, 0)), dm())
        .await
        .unwrap();
    let stdin = std::fs::read_to_string(dir.path().join("op.stdin")).unwrap();
    assert_eq!(stdin.matches(VALUE).count(), 1);
    assert!(!stdin.contains(&format!("{VALUE}\\n")), "{stdin}");
    assert!(checks(&r) > spent);
}

/// An overwrite by the form takes the value with no code (issue 54: "the form to ask for it, yes,
/// the 2fa, no"); a card asked before that, which carried the fresh mark, still needs the fresh code
/// the terminal asks for, which a window of the place does not cover.
#[tokio::test]
async fn a_replace_by_the_form_takes_the_value_and_an_old_card_its_fresh_code() {
    let (r, dir) = rig_form(true);
    let q = ask(&r, input("Lantern key", "credential")).await;
    let card = r.asked().pop().unwrap();
    assert!(!card.needs_proof && !card.needs_fresh_proof, "{card:?}");
    let kept = submit(&r, &q, VALUE, None, dm()).await;
    assert!(kept.is_ok(), "{kept:?}");
    assert!(wrote(dir.path()));

    let (r, dir) = rig_form(true);
    // The pairing, and a window of the place the form is typed in, which does not cover a replace.
    let secret = r.core.proofs.open_stand_in_window("discord:dm");
    let q = ask(&r, input("Lantern key", "credential")).await;
    super::old_card(&r, &q, true);
    let e = submit(&r, &q, VALUE, None, dm()).await.unwrap_err();
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(!wrote(dir.path()));
    let kept = submit(&r, &q, VALUE, Some(&code(&secret, 1)), dm()).await;
    assert!(kept.is_ok(), "{kept:?}");
    assert!(wrote(dir.path()));
}

/// A keep that arrives without the second proof (the window lapsed between the press and the submit) is
/// told in the keep card's own words: the code is asked with the value, so ask again, by the form or the
/// terminal. `theseus answer ... approve --code` is never offered: a keep card refuses an Approve.
#[tokio::test]
async fn a_missing_code_is_told_in_the_keep_cards_own_words() {
    let (r, dir) = rig_form(true);
    let _secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    super::old_card(&r, &q, false);
    let by_form = submit(&r, &q, VALUE, None, dm()).await.unwrap_err();
    let said = format!("{by_form:?}");
    assert!(
        said.contains("press \\\"Type the value here\\\" again"),
        "{said}"
    );
    assert!(said.contains("theseus vault keep"), "{said}");
    assert!(
        !said.contains("theseus answer") && !said.contains("approve"),
        "{said}"
    );
    assert!(!said.contains(VALUE));
    // From the terminal there is no form to press again.
    let by_terminal = give(&r, cli(), &q, VALUE, None).await.unwrap_err();
    let said = format!("{by_terminal:?}");
    assert!(
        said.contains("run `theseus vault keep`") && !said.contains("press"),
        "{said}"
    );
    assert!(!said.contains("theseus answer"), "{said}");
    assert!(!wrote(dir.path()));
    assert!(waiting(&r, &q));
}

/// A value the binding's form refused before sending it (`refused`, with no value) is recorded as the
/// keep path's own row, by class, and told in the daemon's words; nothing is written. The row holds the
/// class, who, where and the card, and never a value or its length. A value carried beside `refused` is
/// not read.
#[tokio::test]
async fn a_value_the_form_refused_leaves_the_keep_paths_own_row_and_no_value() {
    let (r, dir) = rig_form(true);
    let secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    for (class, words) in [
        ("blank", "no value was given: type it where only you see it"),
        (
            "not_a_value",
            "that is not a secret's value (a key is at most 4096 bytes",
        ),
    ] {
        let mut p = json!({"question_id": q, "value": VALUE, "refused": class,
                           "author": "discord:dockhand", "discord": dm()});
        p["code"] = json!(code(&secret, 0));
        let e = rpc_as(&r.core, discord(), method::VAULT_KEEP_ANSWER, p)
            .await
            .unwrap_err();
        assert!(format!("{e:?}").contains(words), "{class}: {e:?}");
        assert!(!format!("{e:?}").contains(VALUE), "{class}");
        assert!(!wrote(dir.path()), "{class}");
        assert!(waiting(&r, &q), "{class}: the card keeps waiting");
    }
    assert_eq!(stopped(&r), ["blank", "not_a_value"]);
    let rows = r.rows("vault.keep");
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in &rows {
        assert_eq!(row["outcome"], "stopped");
        assert_eq!(row["question"], q.as_str());
        assert!(row["via"].as_str().unwrap().contains("discord:dm"), "{row}");
        let keys: Vec<&str> = row
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            keys.iter()
                .all(|k| ["outcome", "why", "approved_by", "via", "pid", "question"].contains(k)),
            "a row holds names and a class only: {keys:?}"
        );
    }
    assert!(!format!("{rows:?}").contains(VALUE));
    // A refusal is judged first: from a channel it is the place that refuses, not a class of the value.
    let mut p = json!({"question_id": q, "value": "", "refused": "blank",
                       "author": "discord:dockhand", "discord": origin(OWNER, "6001", Some("888"))});
    p["code"] = json!(code(&secret, 1));
    let e = rpc_as(&r.core, discord(), method::VAULT_KEEP_ANSWER, p)
        .await
        .unwrap_err();
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert_eq!(stopped(&r).last().map(String::as_str), Some("place"));
}

/// A yes, or a code, given to a keep card from a conversation is told the keep card's own way: the owner
/// gives the value (the card's button in their direct messages, or `theseus vault keep`). The chat's answer
/// tool, a typed yes where an answer becomes a decision, and the needs-you line all say it; none sends the
/// owner to `theseus answer ... approve`.
#[tokio::test]
async fn a_yes_to_a_keep_card_from_a_conversation_is_told_how_the_value_is_given() {
    use crate::config_change::tests_answer::owner_in_dm;
    use crate::rpc::Rides;
    let (r, _dir) = rig_form(true);
    let _secret = secret_of(&r);
    let q = ask(&r, input("Lantern new", "credential")).await;
    let req = r
        .core
        .confirm_list()
        .unwrap()
        .into_iter()
        .find(|c| c.correlation_id == q)
        .unwrap();
    let e = r
        .core
        .answer_plain_act(&req, "yes", None, owner_in_dm())
        .unwrap_err();
    for said in [
        e.clone(),
        format!(
            "{:#}",
            r.core
                .confirm_action_with(
                    &q,
                    true,
                    None,
                    owner_in_dm(),
                    Rides {
                        typed: true,
                        ..Default::default()
                    },
                )
                .unwrap_err()
        ),
    ] {
        assert!(said.contains("a secret to keep"), "{said}");
        assert!(said.contains("theseus vault keep"), "{said}");
        assert!(
            !said.contains("approve --code") && !said.contains("answered by its own button"),
            "{said}"
        );
    }
    assert!(e.contains("Type the value here"), "{e}");
    // The needs-you line says how the value is given, and that only a decline is an answer.
    let a = r.core.kernel.action(&q).unwrap().unwrap();
    let body = crate::needs_you::body_of(&a, &req, "Theseus");
    let line = body.to_string();
    assert!(
        line.contains("theseus vault keep") && line.contains("decline"),
        "{line}"
    );
    assert!(!line.contains("approve"), "{line}");
    assert!(waiting(&r, &q));
}
