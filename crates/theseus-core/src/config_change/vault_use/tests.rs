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
use crate::node::ResultStatus;
use crate::secrets::look::tests::{logged, stub};
use crate::secrets::OpReader;

const VALUE: &str = "INVENTED-FIELD-VALUE-9";

fn rig_vault(mode: &str) -> (Rig, std::path::PathBuf, tempfile::TempDir) {
    let r = rig();
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
/// at the gate before anything is read from the vault.
#[tokio::test]
async fn a_steered_turn_is_asked_before_the_vault_is_read() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    use theseus_judge::band::Band;
    let (r, dir, _d) = rig_vault("ok");
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
    assert_eq!(
        describe(&p("providers.lantern.api_key_secret")),
        "the provider `lantern` (its key)"
    );
    assert_eq!(
        describe(&p("broker.programs.deployer.env.DEPLOY_KEY")),
        "the program `deployer` (its variable `DEPLOY_KEY`)"
    );
    assert_eq!(
        describe(&p("mcp.servers.docs.auth_secret")),
        "the attached server `docs` (its key)"
    );
    assert_eq!(
        describe(&p("tools.web.search_key_secret")),
        "the setting `tools.web.search_key_secret`"
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
