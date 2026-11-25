//! `config.show` and the one card (issue 22): what the read gives and does not,
//! its wall, its gate, and a turn that plays the owner's words to the end.

use serde_json::json;

use super::*;
use crate::config_change::tests::{change, rig, FILE};
use crate::node::ResultStatus;

fn path(s: &str) -> Vec<String> {
    edit::parse_path(s).unwrap()
}

fn read(text: &str, paths: &[&str]) -> String {
    let paths: Vec<Vec<String>> = paths.iter().map(|p| path(p)).collect();
    words(text, &paths).unwrap()
}

/// A setting says its value now, where it comes from, whether a change is
/// sensitive, and whether it restarts: the file's value when the file sets it,
/// the built-in one when it does not.
#[test]
fn a_setting_says_its_value_its_source_its_weight_and_its_restart() {
    let said = read(FILE, &["import.notes.enabled"]);
    assert!(
        said.contains("- import.notes.enabled: on · set in the settings file · an ordinary change"),
        "{said}"
    );
    assert!(said.contains("takes effect after a restart"), "{said}");
    let default = crate::Config::parse(FILE)
        .unwrap()
        .0
        .discord
        .task_channel_cap;
    let said = read(FILE, &["discord.task_channel_cap"]);
    assert!(
        said.contains(&format!(
            "- discord.task_channel_cap: {default} · the default (the file does not set it) · an ordinary change"
        )),
        "{said}"
    );
    // A table the file never mentions has its defaults too.
    let said = read(FILE, &["memory.mode"]);
    assert!(
        said.contains("the default (the file does not set it)"),
        "{said}"
    );
    assert!(!said.contains("no value"), "{said}");
    let said = read(FILE, &["self.budget.day_usd"]);
    assert!(
        said.contains("a sensitive change (") && said.contains("second proof"),
        "{said}"
    );
    let said = read(FILE, &["places.owner"]);
    assert!(said.contains("fresh code"), "{said}");
}

/// A name in no table says so and names the tables, so a typo finds its table;
/// one in a table that has no value for it (an optional setting, or a typo) says
/// that, and where to look.
#[test]
fn a_name_that_is_no_setting_says_so() {
    let said = read(FILE, &["nope.x"]);
    assert!(
        said.contains("no such setting") && said.contains("discord"),
        "{said}"
    );
    let said = read(FILE, &["discord.nope"]);
    assert!(
        said.contains("or no setting has this name") && said.contains("`discord`"),
        "{said}"
    );
}

/// What the read never gives: a vault reference, a value under a key that
/// names where a secret is read from, a URL that carries a credential, a value
/// too long for a card.
#[test]
fn a_secrets_value_and_a_vault_items_name_are_never_given() {
    let said = read(FILE, &["secrets", "secrets.anthropic_api_key"]);
    assert!(
        !said.contains("op://") && !said.contains("anthropic_api_key: \""),
        "{said}"
    );
    assert!(said.contains("not shown"), "{said}");
    for (p, v) in [
        (&["mcp", "servers", "x", "env", "A"][..], json!("plain")),
        (&["mcp", "servers", "x", "auth_secret"][..], json!("plain")),
        (&["google", "credentials"][..], json!("plain")),
        (&["anything", "else"][..], json!("op://vault/item/field")),
        (&["anything", "else"][..], json!("https://user:pw@host/x")),
        (&["anything", "else"][..], json!("https://host/x?token=abc")),
        (
            &["anything", "else"][..],
            json!(["https://host/ok", "op://v/i/f"]),
        ),
        (&["broker", "anything"][..], json!(1)),
    ] {
        let path: Vec<String> = p.iter().map(|s| s.to_string()).collect();
        assert!(withheld(&path, &v).is_some(), "{p:?} {v}");
    }
    for (p, v) in [
        (&["discord", "task_channel_cap"][..], json!(26)),
        (&["anything", "else"][..], json!("https://host/ok")),
        (&["mcp", "servers", "x", "command"][..], json!("run-it")),
    ] {
        let path: Vec<String> = p.iter().map(|s| s.to_string()).collect();
        assert!(withheld(&path, &v).is_none(), "{p:?} {v}");
    }
    // A long text says how long it is, and not what it starts with.
    let long = "z".repeat(500);
    let text = format!("{FILE}\n[model]\nsystem = \"{long}\"\n");
    let said = read(&text, &["model.system"]);
    assert!(
        said.contains("a long value (502 characters") && !said.contains("zzzzzz"),
        "{said}"
    );
}

/// A table lists its settings, at most [`MAX_LISTED`], and says it stopped.
#[test]
fn a_table_lists_its_settings_and_stops_at_the_room() {
    let said = read(FILE, &["kernel"]);
    assert!(said.contains("- kernel."), "{said}");
    let said = read(FILE, &["kernel", "discord", "memory", "model", "tools"]);
    assert!(
        said.lines().count() <= MAX_LISTED + 1,
        "{}",
        said.lines().count()
    );
}

/// The result of a proposal says each setting's value before, set or default,
/// as the daemon read it: a plain change needs no look at all.
#[tokio::test]
async fn a_proposals_result_says_each_value_before_set_or_default() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("discord.task_channel_cap", json!(26)))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let default = crate::Config::parse(FILE)
        .unwrap()
        .0
        .discord
        .task_channel_cap;
    assert!(
        text.contains("Before this change, as Theseus read the settings:")
            && text.contains(&format!(
                "- discord.task_channel_cap: {default} · the default (the file does not set it)"
            )),
        "{text}"
    );
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let (_, text) = r.result(&sid);
    assert!(
        text.contains("- import.notes.enabled: on · set in the settings file"),
        "{text}"
    );
}

/// The owner says "set X to Y" in their own place and the model proposes: the
/// questions put to the owner are exactly one, the card. And the same model,
/// reaching for the shell on the settings file, is held by the floor (which is
/// not loosened): that is the question the owner was asked three times.
#[tokio::test]
async fn a_plain_change_is_one_card_and_the_shell_on_the_file_is_still_held() {
    let r = rig();
    let sid = r.session();
    r.turn(&sid, change("discord.task_channel_cap", json!(26)))
        .await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "exactly one question: {asks:?}");
    assert_eq!(asks[0].tool, crate::config_change::ACK);
    assert!(
        asks[0].reason.contains("discord.task_channel_cap"),
        "{}",
        asks[0].reason
    );

    // The look that used to come first: the shell on the file.
    let r = rig();
    let sid = r.session();
    let file = r.file.display().to_string();
    r.turn_with(
        "proc_run",
        &sid,
        json!({"argv": ["bash", "-c", format!("ls -l {file}; grep cap {file}")]}),
    )
    .await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(asks[0].floor, "held by the floor: {asks:?}");
    assert_eq!(asks[0].tool, "proc.run");
}

/// The read is free in the owner's own clean turn: it answers, and nothing is
/// asked, and the change that follows in the same turn is one card.
#[tokio::test]
async fn the_read_is_free_and_the_change_after_it_is_still_one_card() {
    let r = rig();
    let sid = r.session();
    r.steps(
        &sid,
        vec![
            (
                "config_show",
                json!({"paths": ["discord.task_channel_cap"]}),
            ),
            (
                "config_propose",
                change("discord.task_channel_cap", json!(26)),
            ),
        ],
    )
    .await;
    let (status, shown) = r.result_of(&sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "{shown}");
    assert!(shown.contains("discord.task_channel_cap"), "{shown}");
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "the read asked nothing: {asks:?}");
    assert_eq!(asks[0].tool, crate::config_change::ACK);
}

/// The read has `config.history`'s wall: a task's session and a shared place
/// are refused, in the tool's own name.
#[tokio::test]
async fn a_task_cannot_read_the_settings() {
    let r = rig();
    let parent = r.session();
    let mut rec =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.task = Some(crate::session::TaskOf {
        parent_session: parent,
        parent_execution: "exe_x".into(),
        by: "tool_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.turn_with(
        "config_show",
        &rec.session_id,
        json!({"paths": ["narrative"]}),
    )
    .await;
    let (status, text) = r.result(&rec.session_id);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("config.show") && text.contains("not for a task"),
        "{text}"
    );
}

/// A read in a turn that read outside text since the owner's last message, or
/// whose session is flagged, is put to the owner, as any call that asks is, each
/// cause in its own words; a clean one is not.
#[test]
fn a_read_after_outside_text_is_asked_and_a_clean_one_is_free() {
    use crate::config_change::tests_hold::{each_cause_is_said, ways};
    let plan = || Plan {
        summary: "read".into(),
        ..Default::default()
    };
    each_cause_is_said(SHOW, |o, s, way| held(SHOW, o, s, way, plan()).authority);
    // No other tool is touched.
    assert!(held("config.history", true, true, &ways()[1], plan())
        .authority
        .is_none());
}

/// What the family costs a request that loads it. The settings are deferred in
/// a direct message (`PlaceKind::Dm.defaults()`), so the model finds them by the
/// family's summary line; loading them in the owner's private places by default
/// would add this many tokens to every request there.
#[test]
fn the_settings_family_has_the_read_and_a_bounded_cost() {
    let d = rig().core.tool_defer();
    let dm = d
        .places
        .iter()
        .find(|p| p.place == "dm")
        .expect("the dm place");
    let fam = dm
        .families
        .iter()
        .find(|f| f.family == "settings")
        .expect("the settings family");
    assert!(
        fam.tools.iter().any(|t| t == "config_show"),
        "{:?}",
        fam.tools
    );
    assert!(!fam.loaded, "deferred in a direct message: {}", fam.why);
    // Measured at 3,095 with the read (seven tools). A bound, not a budget: the
    // family stays deferred, and a tool added to it that costs more fails here.
    assert!(
        (1..=3_500).contains(&fam.tokens),
        "the family costs {} tokens",
        fam.tokens
    );
}
