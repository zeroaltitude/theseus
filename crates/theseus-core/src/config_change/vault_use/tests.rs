//! `vault.use` played through the whole core (issue 26, line 1): the owner's
//! turn is told a key is in the vault under a name and asked to set a provider
//! up, and ends with ONE card that names the item and field and says the
//! daemon read it, a provider that loads with its key, and no value anywhere;
//! a name that looks like a reference, two items of one title, a name with a
//! slash, an account that cannot see the vault, and every caller that may not
//! are each played too.

use std::sync::Arc;

use serde_json::{json, Value};

use super::*;
use crate::config_change::tests::{rig, Rig};
use crate::config_change::tests_look::everything;
use crate::node::{Body, ResultStatus};
use crate::secrets::look::tests::{logged, stub};
use crate::secrets::OpReader;

const VALUE: &str = "INVENTED-FIELD-VALUE-9";

fn rig_vault(mode: &str) -> (Rig, std::path::PathBuf, tempfile::TempDir) {
    rig_vault_at(mode, crate::content::OutsideMode::default())
}

/// [`rig_vault`] at `[content_based_security] outside = outside`.
fn rig_vault_at(
    mode: &str,
    outside: crate::content::OutsideMode,
) -> (Rig, std::path::PathBuf, tempfile::TempDir) {
    let r = crate::config_change::tests::rig_with(move |c, _| {
        c.content_based_security.outside = outside;
    });
    let dir = tempfile::tempdir().unwrap();
    r.core
        .tools
        .look
        .set_vault(Arc::new(stub(dir.path(), mode)));
    let path = dir.path().to_path_buf();
    (r, path, dir)
}

fn provider() -> Value {
    json!([{"path": "providers.lantern", "value": {
        "api_base": "https://lantern.example.test", "api_key_secret": "lantern_key"}}])
}

fn call(also: Value) -> Value {
    json!({"vault": "Harbor", "item": "Lantern key", "field": "credential",
           "name": "lantern_key", "also": also,
           "reason": "the key is in the vault as Lantern key"})
}

fn candidate(r: &Rig) -> String {
    let kept: Vec<_> = std::fs::read_dir(crate::config_change::write::dir(&r.state))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(kept.len(), 1, "one candidate");
    std::fs::read_to_string(kept[0].path()).unwrap()
}

/// "The key for lantern is in the vault as Lantern key; set that provider up":
/// one card, the item and field as the owner would say them, that the daemon
/// read it, what it is given; the candidate loads with a provider that has its
/// key; and the value reaches no turn, card, record or ledger row.
#[tokio::test]
async fn a_provider_from_a_vault_item_is_one_card_and_no_value() {
    let (r, dir, _d) = rig_vault("ok");
    let sid = r.session();
    r.turn_with("vault_use", &sid, call(provider())).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "one card: {asks:?}");
    let q = &asks[0];
    assert_eq!(q.tool, crate::config_change::ACK);
    for part in [
        "Use a secret from the vault?",
        "the item \"Lantern key\", field \"credential\", in the vault \"Harbor\"",
        "Theseus read it just now to check",
        "its account can read them",
        "never goes to the model, a record or this card",
        "kept under the name `lantern_key`, read from op://Harbor/Lantern key/credential",
        "Given to: the provider `lantern` (its key).",
        "- secrets.lantern_key: not set (its default) → \"op://Harbor/Lantern key/credential\"",
        "- providers.lantern.api_key_secret: not set (its default) → \"lantern_key\"",
        "second proof",
    ] {
        assert!(q.reason.contains(part), "{part} in {}", q.reason);
    }
    assert!(q.needs_proof, "a secret's source is sensitive");
    assert!(
        !q.reason.contains("do not read the secret"),
        "the provider's address is its own sibling: {}",
        q.reason
    );
    let plain = q.plain.as_ref().expect("plain words");
    assert!(
        plain
            .what
            .starts_with("use the vault's item \"Lantern key\" (field \"credential\") as `lantern_key`, given to the provider `lantern` (its key): change 3 settings"),
        "{}",
        plain.what
    );
    // What the yes would write loads, and the provider has its key.
    let (loaded, _) = crate::Config::parse(&candidate(&r)).unwrap();
    assert_eq!(loaded.provider_without_key("lantern"), None);
    assert_eq!(
        loaded.secrets.get("lantern_key").map(String::as_str),
        Some("op://Harbor/Lantern key/credential")
    );
    // Nothing is written by a proposal.
    assert_eq!(r.file_text(), crate::config_change::tests::FILE);
    // No value anywhere the model, the owner or a log can read.
    let all = format!("{text}{q:?}{}", everything(&r, &sid));
    for leaked in [
        VALUE,
        "INVENTED-USER-VALUE",
        "INVENTED-NOTE-TEXT",
        "secret-path",
    ] {
        assert!(!all.contains(leaked), "{leaked} reached the record");
    }
    // The daemon's own steps: lists, one get, and one read of the reference it built.
    assert_eq!(
        logged(&dir),
        [
            "vault list --format json",
            "item list --format json --vault vaultid00000000000000000001",
            "item get itemid00000000000000000001 --vault vaultid00000000000000000001 --format json",
            "read --no-newline op://Harbor/Lantern key/credential"
        ]
    );
}

/// A name that looks like a reference, a flag, a path or a query finds
/// nothing: the daemon reads only a reference it built from names the vault
/// listed.
#[tokio::test]
async fn a_name_that_looks_like_a_reference_finds_nothing_and_reads_nothing() {
    let cases = [
        (
            "vault",
            "op://Harbor/Lantern key/credential",
            "does not see a vault",
        ),
        ("vault", "--reveal", "does not see a vault"),
        ("vault", "Harbor/../Dock", "does not see a vault"),
        (
            "item",
            "op://Harbor/Lantern key/credential",
            "has no item of that title",
        ),
        (
            "item",
            "Lantern key/credential",
            "has no item of that title",
        ),
        ("item", "../etc", "has no item of that title"),
        ("field", "credential?attribute=otp", "has no field called"),
        ("field", "credential/extra", "has no field called"),
        ("field", "--reveal", "has no field called"),
        ("field", "credential#line", "has no field called"),
        ("section", "Recovery/credential", "has no field called"),
    ];
    for (key, value, says) in cases {
        let (r, dir, _d) = rig_vault("ok");
        let sid = r.session();
        let mut input = call(json!([]));
        input[key] = json!(value);
        r.turn_with("vault_use", &sid, input).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{key}={value}: {text}");
        assert!(text.contains(says), "{key}={value}: {text}");
        assert!(r.asked().is_empty(), "{key}={value}: no card");
        for line in logged(&dir) {
            assert!(!line.starts_with("read"), "{key}={value}: ran `{line}`");
            assert!(
                !line.contains("--reveal") && !line.contains("attribute"),
                "{key}={value}: ran `{line}`"
            );
        }
    }
}

/// A field that two sections share is told apart by its section, not guessed;
/// a field in a section is written with its section.
#[tokio::test]
async fn a_field_in_a_section_is_written_with_its_section() {
    let (r, dir, _d) = rig_vault("ok");
    let sid = r.session();
    let mut input = call(json!([]));
    input["field"] = json!("backup code");
    r.turn_with("vault_use", &sid, input).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(
        asks[0]
            .reason
            .contains("field \"backup code\", in the section \"Recovery\""),
        "{}",
        asks[0].reason
    );
    assert!(asks[0]
        .reason
        .contains("op://Harbor/Lantern key/Recovery/backup code"));
    assert!(logged(&dir)
        .contains(&"read --no-newline op://Harbor/Lantern key/Recovery/backup code".to_string()));
    assert!(asks[0].reason.contains("Nothing is given it yet"));
}

/// Two items of one title are not guessed between: their ids are given, and
/// nothing is read.
#[tokio::test]
async fn two_items_of_one_title_are_not_guessed_between() {
    let (r, dir, _d) = rig_vault("dup");
    let sid = r.session();
    r.turn_with("vault_use", &sid, call(json!([]))).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("2 items in \"Harbor\" are titled \"Lantern key\"")
            && text.contains("id itemid00000000000000000009"),
        "{text}"
    );
    assert!(r.asked().is_empty());
    assert!(!logged(&dir)
        .iter()
        .any(|l| l.starts_with("read") || l.starts_with("item get")));
}

/// A title or a label with a slash is written by its id, so the reference
/// cannot mean another thing; the card says so.
#[tokio::test]
async fn a_name_with_a_slash_is_written_by_its_id() {
    let (r, dir, _d) = rig_vault("slash");
    let sid = r.session();
    let mut input = call(json!([]));
    input["item"] = json!("Lantern/key");
    input["field"] = json!("api/key");
    r.turn_with("vault_use", &sid, input).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(
        asks[0]
            .reason
            .contains("read from op://Harbor/itemid00000000000000000001/apikey"),
        "{}",
        asks[0].reason
    );
    assert!(asks[0].reason.contains("written by the vault's own ids"));
    assert!(logged(&dir)
        .contains(&"read --no-newline op://Harbor/itemid00000000000000000001/apikey".to_string()));
    assert!(!everything(&r, &sid).contains(VALUE));
}

/// Where the account cannot see the vault, the result says so and says what the
/// owner can do; the vault's program is asked for the list and nothing more.
#[tokio::test]
async fn a_vault_the_account_cannot_see_is_told_with_what_to_do() {
    let (r, dir, _d) = rig_vault("ok");
    let sid = r.session();
    let mut input = call(json!([]));
    input["vault"] = json!("Personal");
    r.turn_with("vault_use", &sid, input).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    for part in [
        "does not see a vault called \"Personal\"; it sees Harbor, Dock",
        "grant the account that vault",
        "move the item to one it sees",
        "does not let a service account into a personal vault",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    assert_eq!(logged(&dir), ["vault list --format json"]);
    assert!(r.asked().is_empty());
}

/// A read that fails is its class and never the program's words; no card is
/// offered for a secret the daemon could not read.
#[tokio::test]
async fn a_secret_that_cannot_be_read_makes_no_card_and_says_its_class() {
    let (r, _, _d) = rig_vault("fail-read");
    let sid = r.session();
    r.turn_with("vault_use", &sid, call(json!([]))).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("refused the daemon's token"), "{text}");
    assert!(!text.contains("INVENTED-WORDS-IN-STDERR"), "{text}");
    assert!(r.asked().is_empty());
}

/// An install with no vault session says so and names the other ways.
#[tokio::test]
async fn with_no_vault_session_it_says_so() {
    let r = rig();
    r.core
        .tools
        .look
        .set_vault(Arc::new(OpReader::absent("no token".into())));
    let sid = r.session();
    r.turn_with("vault_use", &sid, call(json!([]))).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("no vault session") && text.contains("env:NAME"),
        "{text}"
    );
    assert!(!text.contains("no token"), "{text}");
}

/// `name` is a name, `also` does not write `[secrets]`: both are refused before
/// the vault is read.
#[tokio::test]
async fn a_bad_name_or_an_also_on_secrets_is_refused_before_the_vault_is_read() {
    let mut inputs = Vec::new();
    for name in ["", "1key", "a/b", "a b", "a.b", "x\ny", &"k".repeat(60)] {
        let mut i = call(json!([]));
        i["name"] = json!(name);
        inputs.push(i);
    }
    inputs.push(call(
        json!([{"path": "secrets.other", "value": "env:OTHER_FOR_TESTS"}]),
    ));
    inputs.push(call(
        json!([{"path": "secrets.lantern_key", "value": "env:OTHER_FOR_TESTS"}]),
    ));
    inputs.push(call(
        json!([{"path": "secrets", "value": {"x": "env:OTHER_FOR_TESTS"}}]),
    ));
    inputs.push(call(
        json!([{"path": "secrets.lantern_key", "remove": true}]),
    ));
    for input in inputs {
        let (r, dir, _d) = rig_vault("ok");
        let sid = r.session();
        r.turn_with("vault_use", &sid, input.clone()).await;
        let (status, _) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{input}");
        assert!(r.asked().is_empty(), "{input}");
        assert!(logged(&dir).is_empty(), "{input}: nothing was read");
    }
}

/// A vault reference written as text through `config.propose` is refused, whatever
/// it looks like, and a local one is not; the tool is the one way.
#[tokio::test]
async fn a_reference_written_by_a_turn_is_refused_but_a_local_one_is_not() {
    for rf in [
        "op://Harbor/Lantern key/credential",
        "op://Harbor/A/B/credential",
        "op://Harbor/Lantern key/credential?attribute=otp",
    ] {
        let r = rig();
        let sid = r.session();
        r.turn(
            &sid,
            super::super::tests::change("secrets.lantern_key", json!(rf)),
        )
        .await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Error, "{rf}: {text}");
        assert!(
            text.contains("not written as text") && text.contains("vault.use"),
            "{text}"
        );
        assert!(r.asked().is_empty(), "{rf}");
    }
    // Written through a whole table, the same.
    let r = rig();
    let sid = r.session();
    r.turn(
        &sid,
        super::super::tests::change(
            "secrets",
            json!({"anthropic_api_key": "op://v/i/f", "lantern_key": "op://Harbor/Lantern key/credential"}),
        ),
    )
    .await;
    assert_eq!(r.result(&sid).0, ResultStatus::Error);
    for local in ["env:LANTERN_FOR_TESTS", "file:/etc/lantern-for-tests"] {
        let r = rig();
        let sid = r.session();
        r.turn(
            &sid,
            super::super::tests::change("secrets.lantern_key", json!(local)),
        )
        .await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Ok, "{local}: {text}");
        assert_eq!(r.asked().len(), 1);
    }
}

/// An undo that puts back a reference an earlier card showed is not refused:
/// it is the owner's own earlier approved text.
#[tokio::test]
async fn an_undo_may_put_back_a_reference() {
    let r = rig();
    // Before: the reference was there; after: it was removed. Undo puts it back.
    let before = crate::config_change::tests::FILE.replace(
        "[secrets]\n",
        "[secrets]\nlantern_key = \"op://Harbor/Lantern key/credential\"\n",
    );
    r.seed(
        &before,
        crate::config_change::tests::FILE,
        &["secrets.lantern_key"],
    );
    let sid = r.session();
    r.turn_with("config_undo", &sid, json!({"reason": "put it back"}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(r.asked().len(), 1);
}

/// A turn that read text from outside since the owner's last message is asked
/// at the gate before anything is read from the vault, at `[content_based_security]
/// outside = "ask"`.
#[tokio::test]
async fn a_steered_turn_is_asked_before_the_vault_is_read() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    use theseus_judge::band::Band;
    let (r, dir, _d) = rig_vault_at("ok", crate::content::OutsideMode::Ask);
    let sid = r.session();
    let mut rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Injection,
        node: Some("nod_fetched".into()),
        kind: ItemKind::FetchedPage,
        source: "http.fetch https://elsewhere.example/page".into(),
        confidence: 0.95,
        band: Band::Act,
        at_ms: 1,
        judgment: String::new(),
    });
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(&sid, &rec).unwrap();
    r.turn_with("vault_use", &sid, call(provider())).await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0].tool, USE);
    assert!(
        asks[0].reason.contains(crate::config_change::FLAG_HOLD)
            && !asks[0].reason.contains("read text from outside"),
        "the flag, not outside text: {}",
        asks[0].reason
    );
    assert!(logged(&dir).is_empty(), "the vault was not read yet");
}

/// Both vault calls are held for each cause in its own words, and no other tool is touched.
#[test]
fn each_vault_call_says_why_it_is_held() {
    use crate::config_change::tests_hold::{each_cause_is_said, ways};
    let plan = || Plan {
        summary: "vault".into(),
        ..Default::default()
    };
    for tool in [USE, crate::config_change::vault_keep::KEEP] {
        each_cause_is_said(tool, |o, s, way| held(tool, o, s, way, plan()).authority);
    }
    assert!(held("look.vault", true, true, &ways()[1], plan())
        .authority
        .is_none());
}

/// A task's session is refused in the tool's words, and the vault is not read.
#[tokio::test]
async fn a_task_is_not_given_the_tool() {
    let (r, dir, _d) = rig_vault("ok");
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
    r.turn_with("vault_use", &rec.session_id, call(provider()))
        .await;
    let (status, text) = r.result(&rec.session_id);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("vault.use") && text.contains("not for a task"),
        "{text}"
    );
    assert!(logged(&dir).is_empty());
    assert!(r.asked().is_empty());
}

/// The words of the check: what a name may carry, a secret's name, and what a
/// reading setting is called.
#[test]
fn names_are_checked_and_readers_are_worded() {
    for ok in ["Lantern key", "a-b_c.d", "x"] {
        assert!(check::writable(ok), "{ok}");
    }
    for bad in [
        "",
        " x",
        "x ",
        "a/b",
        "a#b",
        "a?b",
        "a=b",
        "a\nb",
        "caf\u{e9}",
        &"x".repeat(65),
    ] {
        assert!(!check::writable(bad), "{bad:?}");
    }
    assert!(valid_name("lantern_key") && valid_name("K2"));
    assert!(!valid_name("2k") && !valid_name("a-b") && !valid_name(""));
    let p = |s: &str| s.split('.').map(str::to_string).collect::<Vec<_>>();
    // Settings with no such provider or server: the words alone, with no address to give.
    let (cfg, _) = crate::Config::parse(crate::config_change::tests::FILE).unwrap();
    assert_eq!(
        describe(&cfg, &p("providers.lantern.api_key_secret")),
        "the provider `lantern` (its key)"
    );
    assert_eq!(
        describe(&cfg, &p("broker.programs.deployer.env.DEPLOY_KEY")),
        "the program `deployer` (its variable `DEPLOY_KEY`)"
    );
    assert_eq!(
        describe(&cfg, &p("mcp.servers.docs.auth_secret")),
        "the attached server `docs` (its key)"
    );
    assert_eq!(
        describe(&cfg, &p("tools.web.search_key_secret")),
        "the web search (its key)"
    );
}

/// A change that rides on the same card but has nothing to do with the secret
/// is named on it, so the heading cannot hide it.
#[tokio::test]
async fn a_card_names_the_settings_that_do_not_read_the_secret() {
    let (r, _, _d) = rig_vault("ok");
    let sid = r.session();
    let mut also = provider();
    also.as_array_mut()
        .unwrap()
        .push(json!({"path": "import.notes.enabled", "value": false}));
    r.turn_with("vault_use", &sid, call(also)).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(
        asks[0].reason.contains(
            "It also changes settings that do not read the secret: `import.notes.enabled`."
        ),
        "{}",
        asks[0].reason
    );
}

/// Following the advice for two items of one title: called again with the id,
/// the reference is written by that id, not by the shared title, and the daemon
/// reads that.
#[tokio::test]
async fn a_shared_title_is_written_by_the_id_that_was_asked_for() {
    let (r, dir, _d) = rig_vault("dup");
    let sid = r.session();
    let mut input = call(json!([]));
    input["item"] = json!("itemid00000000000000000009");
    r.turn_with("vault_use", &sid, input).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(
        asks[0]
            .reason
            .contains("read from op://Harbor/itemid00000000000000000009/credential"),
        "{}",
        asks[0].reason
    );
    assert!(asks[0].reason.contains("written by the vault's own ids"));
    assert!(logged(&dir).contains(
        &"read --no-newline op://Harbor/itemid00000000000000000009/credential".to_string()
    ));
}

/// A field label two fields share is not guessed between; named by its id it is
/// written by that id. A field whose label is another field's id is told apart
/// by the exact id, and either is written by its id.
#[tokio::test]
async fn a_shared_field_label_is_written_by_its_id() {
    let (r, _, _d) = rig_vault("dupfield");
    let sid = r.session();
    let mut input = call(json!([]));
    input["field"] = json!("CREDENTIAL");
    r.turn_with("vault_use", &sid, input).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(
        text.contains("has 2 fields called \"CREDENTIAL\""),
        "{text}"
    );
    assert!(r.asked().is_empty());
    // `password` is the id of the field labelled "keyword": it is that field, and its own
    // label is unique, so it is written by that; `f9` is labelled "password", which is
    // also an id, so it is written by its id.
    // `credential` is A's label and id, and B's label: nothing that names A alone is
    // left, so it is refused, not written as a segment B also answers to.
    let (r, dir, _d) = rig_vault("dupfield");
    let sid = r.session();
    r.turn_with("vault_use", &sid, call(json!([]))).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("answer to the same name"), "{text}");
    assert!(r.asked().is_empty());
    assert!(!logged(&dir).iter().any(|l| l.starts_with("read")));
    for (asked, id) in [("f2id", "f2id"), ("password", "keyword"), ("f9", "f9")] {
        let (r, dir, _d) = rig_vault("dupfield");
        let sid = r.session();
        let mut input = call(json!([]));
        input["field"] = json!(asked);
        r.turn_with("vault_use", &sid, input).await;
        let (status, text) = r.result(&sid);
        assert_eq!(status, ResultStatus::Ok, "{asked}: {text}");
        let want = format!("read --no-newline op://Harbor/Lantern key/{id}");
        assert!(logged(&dir).contains(&want), "{asked}: {:?}", logged(&dir));
        let by_id = id != "keyword";
        assert_eq!(
            r.asked()[0]
                .reason
                .contains("written by the vault's own ids"),
            by_id,
            "{asked}"
        );
    }
}

/// A field with no section is known among every field of the item, so a
/// sectionless field that shares its label with a sectioned one is written by an
/// id nothing else answers to; the sectioned one is written with its section.
#[tokio::test]
async fn a_sectionless_field_is_known_among_all_the_items_fields() {
    let play = |field: &'static str, section: Option<&'static str>| async move {
        let (r, dir, _d) = rig_vault("dupsection");
        let sid = r.session();
        let mut input = call(json!([]));
        input["field"] = json!(field);
        if let Some(s) = section {
            input["section"] = json!(s);
        }
        r.turn_with("vault_use", &sid, input).await;
        let (status, text) = r.result(&sid);
        (status, text, logged(&dir))
    };
    let (status, text, _) = play("password", None).await;
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("has 2 fields called"), "{text}");
    let (status, text, ran) = play("f7", None).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        ran.contains(&"read --no-newline op://Harbor/Lantern key/f7".to_string()),
        "{ran:?}"
    );
    let (status, text, ran) = play("password", Some("Extra")).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        ran.contains(&"read --no-newline op://Harbor/Lantern key/Extra/password".to_string()),
        "{ran:?}"
    );
}

/// A provider's address and protocol that the maker publishes may come with
/// their source on the same one card as the secret, and the source is the
/// model's word: the vault's own lines of the card are the daemon's, as before.
#[tokio::test]
async fn a_published_address_beside_a_vault_secret_is_on_the_one_card_with_its_source() {
    let (r, _dir, _d) = rig_vault("ok");
    let sid = r.session();
    let mut also = provider();
    also[0]["source"] =
        json!("https://docs.lantern.example.test says the address is lantern.example.test");
    r.turn_with("vault_use", &sid, call(also)).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "one card: {asks:?}");
    let card = &asks[0].reason;
    let lines: Vec<&str> = card.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("- providers.lantern.api_base: "))
        .expect("the address is on the card");
    assert!(
        lines[at + 1]
            .starts_with("  Theseus's source for this, in its own words (nothing here checked it)")
            && lines[at + 1].contains("docs.lantern.example.test"),
        "{card}"
    );
    assert!(
        at < lines
            .iter()
            .position(|l| l.starts_with("- secrets.lantern_key"))
            .unwrap(),
        "an address a secret is sent to, with a source, comes first: {card}"
    );
    assert!(card.contains("Theseus read it just now to check"), "{card}");
    assert!(asks[0].needs_proof, "the second proof stays");
    assert_eq!(
        card.matches("Theseus's source for this").count(),
        2,
        "under each setting of the table (the address and the key's name): {card}"
    );
}

/// The settings file of [`crate::config_change::tests::FILE`] with an existing provider and an
/// existing MCP server that has a URL, for the readers on the list.
const WITH_READERS: &str = r#"# Theseus, on this machine.
narrative = false

[secrets]
anthropic_api_key = "op://v/i/f"

[providers.lantern]
api_base = "https://lantern.example.test"
api_key_secret = "anthropic_api_key"

[mcp.servers.wharf]
url = "https://wharf.example.test/mcp"

# The notes import.
[import.notes]
enabled = true   # turn it off to stop the import
"#;

fn rig_readers() -> (Rig, std::path::PathBuf, tempfile::TempDir) {
    let (r, dir, d) = rig_vault("ok");
    std::fs::write(&r.file, WITH_READERS).unwrap();
    (r, dir, d)
}

/// Case 2: the secret and each reader on the list (the main model's key, an existing provider's
/// key, an existing URL server's `auth_secret`) is ONE settings card with no second proof, which
/// says what is given the secret; nothing is written before the yes, and a plain yes writes it.
#[tokio::test]
async fn each_listed_reader_is_one_card_with_no_proof_and_a_yes_writes_it() {
    // Each reader with where it sends the key today: the daemon's own address for it, as a
    // scheme and host.
    for (path, given) in [
        (
            "model.api_key_secret",
            "the main model (its key, which sends it to https://",
        ),
        (
            "providers.lantern.api_key_secret",
            "the provider `lantern` (its key, which sends it to https://lantern.example.test)",
        ),
        (
            "mcp.servers.wharf.auth_secret",
            "the attached server `wharf` (its key, which sends it to https://wharf.example.test)",
        ),
    ] {
        let (r, _dir, _d) = rig_readers();
        let sid = r.session();
        r.turn_with(
            "vault_use",
            &sid,
            call(json!([{"path": path, "value": "lantern_key"}])),
        )
        .await;
        let asks = r.asked();
        assert_eq!(asks.len(), 1, "{path}: {asks:?}");
        let q = &asks[0];
        assert!(!q.needs_proof && !q.needs_fresh_proof, "{path}: {q:?}");
        assert!(
            q.reason.starts_with("Use a secret from the vault?"),
            "{path}: {}",
            q.reason
        );
        assert!(q.reason.contains(given), "{path}: {}", q.reason);
        assert!(!q.reason.contains("second proof"), "{path}: {}", q.reason);
        assert_eq!(
            r.file_text(),
            WITH_READERS,
            "{path}: nothing before the yes"
        );
        r.core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap();
        let after = r.file_text();
        assert!(
            after.contains("lantern_key = \"op://Harbor/Lantern key/credential\""),
            "{path}: {after}"
        );
        assert_eq!(r.rows("config.changed").len(), 1, "{path}");
    }
}

/// Case 2, a re-point: the card, with no second proof, says the name is re-pointed, what it was
/// read from until now, and that every setting already reading it will read the new item.
#[tokio::test]
async fn a_repoint_is_one_card_that_says_what_it_read_and_who_reads_it() {
    let (r, _dir, _d) = rig_readers();
    let sid = r.session();
    let mut c = call(json!([]));
    c["name"] = json!("anthropic_api_key");
    r.turn_with("vault_use", &sid, c).await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(!asks[0].needs_proof, "{:?}", asks[0]);
    for part in [
        "This RE-POINTS the name `anthropic_api_key`: until now it was read from op://v/i/f.",
        "These settings already read `anthropic_api_key` and will read the new item from the next start:",
        "the provider `lantern` (its key, which sends it to https://lantern.example.test)",
        "a setting that reads this name by default may use it too.",
    ] {
        assert!(asks[0].reason.contains(part), "{part}: {}", asks[0].reason);
    }
    assert_eq!(r.file_text(), WITH_READERS, "nothing before the yes");
}

/// A new name with nothing else is ONE settings card with no second proof (there is no use
/// without a card): it says plainly what it is, and does not claim that nothing reads the name.
/// Nothing is written before the yes; a plain yes writes it.
#[tokio::test]
async fn a_new_name_alone_is_one_card_with_no_proof() {
    let (r, _dir, _d) = rig_vault("ok");
    let sid = r.session();
    r.turn_with("vault_use", &sid, call(json!([]))).await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    let q = &asks[0];
    assert!(!q.needs_proof && !q.needs_fresh_proof, "{q:?}");
    for part in [
        "This gives a secret from the vault a name in the settings.",
        "No setting is set to read it; a setting that reads this name by default may use it.",
    ] {
        assert!(q.reason.contains(part), "{part}: {}", q.reason);
    }
    assert!(!q.reason.contains("second proof"), "{}", q.reason);
    assert_eq!(
        r.file_text(),
        crate::config_change::tests::FILE,
        "nothing before the yes"
    );
    r.core
        .confirm_action(&q.correlation_id, true, None, "cli")
        .unwrap();
    assert!(
        r.file_text()
            .contains("lantern_key = \"op://Harbor/Lantern key/credential\""),
        "{}",
        r.file_text()
    );
}

/// The reviewer's case: no voice section in the file, and a new name equal to the voice's default
/// key. Not inert (an audition reads it, even with voice off): ONE card, no second proof, nothing
/// written before the yes, and the card names the voice as a reader.
#[tokio::test]
async fn the_voices_default_name_is_one_card_that_names_the_voice() {
    let (r, _dir, _d) = rig_vault("ok");
    assert!(!r.file_text().contains("[voice]"));
    let sid = r.session();
    let mut c = call(json!([]));
    c["name"] = json!("deepgram_api_key");
    r.turn_with("vault_use", &sid, c).await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(!asks[0].needs_proof, "{:?}", asks[0]);
    assert!(
        asks[0]
            .reason
            .contains("Given to: the voice (its key, which an audition uses even with voice off)."),
        "{}",
        asks[0].reason
    );
    assert_eq!(
        r.file_text(),
        crate::config_change::tests::FILE,
        "nothing before the yes"
    );
}

/// Case 3: anything off the list takes main's path for the WHOLE change, with main's proof: the
/// reviewer's new server whose argv carries the name and whose env reads it, a map named `key` or
/// ending `_secret`, a new provider, an ordinary string equal to the name, an array holding it, an
/// address beside the key, and an ordinary setting riding along. Nothing is written or told.
#[tokio::test]
async fn everything_off_the_list_takes_mains_path_with_its_proof() {
    let cases = [
        json!([
            {"path": "mcp.servers.key.command",
             "value": ["/bin/sh", "-c", "send \"$TOKEN\" elsewhere", "lantern_key"]},
            {"path": "mcp.servers.key.env.TOKEN", "value": "lantern_key"}
        ]),
        json!([{"path": "mcp.servers.key.command", "value": ["/bin/sh", "lantern_key"]}]),
        json!([{"path": "providers.key.api_key_secret", "value": "lantern_key"},
               {"path": "providers.key.api_base", "value": "https://elsewhere.example.test"}]),
        json!([{"path": "mcp.servers.x_secret.auth_secret", "value": "lantern_key"},
               {"path": "mcp.servers.x_secret.url", "value": "https://elsewhere.example.test/mcp"}]),
        json!([{"path": "mcp.servers.wharf.env.TOKEN", "value": "lantern_key"}]),
        json!([{"path": "model.model", "value": "lantern_key"}]),
        json!([{"path": "mcp.servers.wharf.read", "value": ["lantern_key"]}]),
        json!([{"path": "providers.lantern.api_base", "value": "https://elsewhere.example.test"},
               {"path": "providers.lantern.api_key_secret", "value": "lantern_key"}]),
        json!([{"path": "narrative", "value": true}]),
    ];
    for also in cases {
        let (r, dir, _d) = rig_readers();
        let sid = r.session();
        r.turn_with("vault_use", &sid, call(also.clone())).await;
        let (status, text) = r.result(&sid);
        assert_eq!(r.file_text(), WITH_READERS, "{also}: {text}");
        let asks = r.asked();
        match status {
            ResultStatus::Ok => {
                assert_eq!(asks.len(), 1, "{also}: {asks:?}");
                assert!(
                    asks[0].needs_proof,
                    "{also}: as on main, the proof: {}",
                    asks[0].reason
                );
                assert!(
                    asks[0]
                        .reason
                        .contains("need a second proof as well as your yes: "),
                    "{also}: {}",
                    asks[0].reason
                );
                assert!(asks[0].reason.contains("secrets.lantern_key ("), "{also}");
            }
            // A change whose candidate does not load is refused before any card: nothing asked.
            _ => assert!(asks.is_empty(), "{also}: {text}"),
        }
        let _ = dir;
    }
}

/// A provider `kind` that equals the name is not a reader either: refused or carded with the
/// proof, never told, never written.
#[tokio::test]
async fn a_provider_kind_equal_to_the_name_is_not_a_reader() {
    let (r, _dir, _d) = rig_readers();
    let sid = r.session();
    let mut c = call(json!([{"path": "providers.lantern.kind", "value": "anthropic"}]));
    c["name"] = json!("anthropic");
    r.turn_with("vault_use", &sid, c).await;
    assert_eq!(r.file_text(), WITH_READERS);
    assert!(r.asked().iter().all(|q| q.needs_proof), "{:?}", r.asked());
}

/// The classifier itself, on the path's shape: a freely named map (`key`, `x_secret`) changes
/// nothing, a scalar must equal the name, an array never counts, and a server or provider must
/// already exist.
#[test]
fn the_list_is_decided_by_the_paths_shape_and_the_value() {
    let (base, _) = crate::Config::parse(WITH_READERS).unwrap();
    let set = |path: &str, v: Value| crate::config_change::diff::Setting {
        path: crate::config_change::edit::parse_path(path).unwrap(),
        before: None,
        after: Some(v),
    };
    let own = set(
        "secrets.lantern_key",
        json!("op://Harbor/Lantern key/credential"),
    );
    use classify::Case::{Other, Pure};
    for (settings, want) in [
        (vec![own.clone()], Pure),
        (
            vec![
                own.clone(),
                set("model.api_key_secret", json!("lantern_key")),
            ],
            Pure,
        ),
        (
            vec![
                own.clone(),
                set("providers.lantern.api_key_secret", json!("lantern_key")),
            ],
            Pure,
        ),
        (
            vec![
                own.clone(),
                set("mcp.servers.wharf.auth_secret", json!("lantern_key")),
            ],
            Pure,
        ),
        (
            vec![
                own.clone(),
                set("providers.key.api_key_secret", json!("lantern_key")),
            ],
            Other,
        ),
        (
            vec![
                own.clone(),
                set("mcp.servers.key.auth_secret", json!("lantern_key")),
            ],
            Other,
        ),
        (
            vec![
                own.clone(),
                set("mcp.servers.wharf.env.TOKEN", json!("lantern_key")),
            ],
            Other,
        ),
        (
            vec![
                own.clone(),
                set("mcp.servers.key.command", json!(["lantern_key"])),
            ],
            Other,
        ),
        (
            vec![
                own.clone(),
                set("model.api_key_secret", json!(["lantern_key"])),
            ],
            Other,
        ),
        (
            vec![own.clone(), set("model.api_key_secret", json!("other_key"))],
            Other,
        ),
        (vec![own, set("model.model", json!("lantern_key"))], Other),
        (vec![], Other),
    ] {
        assert_eq!(
            classify::case_of(&base, &settings, "lantern_key"),
            want,
            "{settings:?}"
        );
    }
}

/// The wall, for each kind of session: executions opened with the job's, the self loop's and an
/// MCP client's principals (constructed here as those producers open them, not by running the
/// producers), a task's session (a real kernel task), and a shared place are each refused, the
/// call answered with an error; nothing is read or written.
#[tokio::test]
async fn a_use_keeps_the_wall_for_each_kind_of_session() {
    use theseus_kernel::Authority;
    // The loop's turn is refused its budget while self-improvement is off: it is on, so that the
    // refusal is the wall's.
    let r = crate::config_change::tests::rig_with(|c, _| {
        c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
    });
    let d = tempfile::tempdir().unwrap();
    r.core.tools.look.set_vault(Arc::new(stub(d.path(), "ok")));
    let dir = d.path().to_path_buf();
    let mut sessions = Vec::new();
    for principal in [
        "job",
        crate::rsi::backlog::PRINCIPAL,
        crate::mcp_server::PRINCIPAL,
    ] {
        let sid = r.session();
        let rec: crate::session::SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
        let e = r
            .core
            .kernel
            .open_execution(
                &sid,
                rec.kind,
                Authority {
                    principal: principal.into(),
                    ..Default::default()
                },
                None,
                None,
            )
            .unwrap();
        r.core
            .store
            .update_session(&sid, |rec: &mut crate::session::SessionRecord| {
                rec.execution_id = Some(e.id.clone());
                Ok(vec![])
            })
            .unwrap();
        sessions.push((principal.to_string(), sid));
    }
    let parent = crate::config_change::tests_work_global::conversation(&r).await;
    let task = crate::config_change::tests_work_global::task(&r, &parent, "Chart the reef", None);
    sessions.push(("task".into(), task.session_id.clone()));
    sessions.push(("shared".into(), r.session_at("channel:2")));
    for (who, sid) in sessions {
        r.turn_with("vault_use", &sid, call(json!([]))).await;
        // Every result the turn wrote: the call is refused, in the wall's words when the tool was
        // offered, and as a tool that is not offered (an error result under its wire name) when
        // the producer's place offers none. Either way the call is answered with an error.
        let results: Vec<(String, ResultStatus, String)> = r
            .core
            .store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match n.body {
                Body::ToolResult {
                    tool,
                    status,
                    content,
                    ..
                } => Some((tool, status, content)),
                _ => None,
            })
            .collect();
        assert!(!results.is_empty(), "{who}: the call was answered");
        for (tool, status, text) in &results {
            assert_ne!(*status, ResultStatus::Ok, "{who}: {tool}: {text}");
            if tool == USE {
                assert!(
                    text.contains("vault.use is "),
                    "{who}: the wall's words: {text}"
                );
            }
        }
        eprintln!("{who}: {results:?}");
    }
    assert_eq!(r.file_text(), crate::config_change::tests::FILE);
    assert!(logged(&dir).is_empty(), "the vault was not read");
}

/// At `[content_based_security] outside = "notify"` a flagged turn's use is not held first: its
/// own settings card comes, with no second proof. At `ask` the hold's card comes first, with two
/// answers only.
#[tokio::test]
async fn a_flagged_use_is_told_at_the_default_and_asked_at_ask() {
    use crate::content::{Flag, OutsideMode};
    for outside in [OutsideMode::Notify, OutsideMode::Ask] {
        let (r, dir, _d) = rig_vault_at("ok", outside);
        let sid = r.session();
        crate::test_support::label_session(&r.core, &sid, Flag::Injection);
        r.turn_with("vault_use", &sid, call(json!([]))).await;
        let asks = r.asked();
        match outside {
            OutsideMode::Notify => {
                assert_eq!(asks.len(), 1, "{asks:?}");
                assert_ne!(asks[0].tool, USE, "not the hold's card: {asks:?}");
                assert!(!asks[0].needs_proof, "{asks:?}");
            }
            OutsideMode::Ask => {
                assert_eq!(asks.len(), 1, "{asks:?}");
                assert_eq!(asks[0].tool, USE);
                assert_eq!(asks[0].grant_offer, None, "no third answer");
                assert!(logged(&dir).is_empty(), "the vault was not read yet");
            }
        }
    }
}
