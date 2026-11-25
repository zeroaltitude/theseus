//! `look.theseus` and `look.vault` played through the whole core (issue 26),
//! on the settings rig: the owner's own turn looks at Theseus and at the
//! vault's names and is asked nothing; the vault's program the daemon runs is
//! asked only for lists; no value, field or note reaches the result, the
//! ledger or an error; and a turn that has read outside text, a task, and a job
//! are not given the vault's names.

use std::sync::Arc;

use serde_json::json;

use super::tests::{rig, rig_with, Rig};
use crate::node::ResultStatus;
use crate::secrets::look::tests::{logged, stub};
use crate::secrets::{OpReader, Secret};

/// Invented, and never in any output.
const KEY_VALUE: &str = "INVENTED-KEY-VALUE-0001";

pub(super) fn rig_with_a_vault(mode: &str) -> (Rig, std::path::PathBuf, tempfile::TempDir) {
    rig_with_a_vault_at(mode, crate::content::OutsideMode::default())
}

/// [`rig_with_a_vault`] at `[content_based_security] outside = outside`.
pub(super) fn rig_with_a_vault_at(
    mode: &str,
    outside: crate::content::OutsideMode,
) -> (Rig, std::path::PathBuf, tempfile::TempDir) {
    let r = rig_with(move |cfg, _| {
        cfg.content_based_security.outside = outside;
        cfg.secrets.insert(
            "lantern_key".into(),
            "op://Harbor/Lantern key/credential".into(),
        );
        cfg.secrets
            .insert("tide_key".into(), "env:TIDE_KEY_FOR_TESTS".into());
    });
    let dir = tempfile::tempdir().unwrap();
    r.core
        .tools
        .look
        .set_vault(Arc::new(stub(dir.path(), mode)));
    r.core
        .secrets
        .hold("lantern_key", Secret::new(KEY_VALUE.into()));
    let path = dir.path().to_path_buf();
    (r, path, dir)
}

/// Everything the turn and the daemon wrote, as one string: the session's
/// nodes and the ledger.
pub(super) fn everything(r: &Rig, sid: &str) -> String {
    let nodes = format!("{:?}", r.core.store.session_nodes(sid).unwrap());
    let rows = format!(
        "{:?}",
        r.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
    );
    format!("{nodes}{rows}")
}

/// The owner's own clean turn: "which models can you use?" is a look at
/// Theseus, answered with no card, and says each provider's key state without
/// its value.
#[tokio::test]
async fn the_owner_looks_at_theseus_and_is_asked_nothing() {
    let (r, _, _d) = rig_with_a_vault("ok");
    let sid = r.session();
    r.turn_with("look_theseus", &sid, json!({})).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    for part in [
        "Profiles (the live one is",
        "(live)",
        "Providers",
        "key ",
        "Health:",
        "Build: Theseus",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    assert!(!text.contains(KEY_VALUE), "{text}");
    assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
    assert!(!everything(&r, &sid).contains(KEY_VALUE));
}

/// "Is the key in the vault?": a look at the vault's names, no card, the
/// titles and the settings that read them, and none of what the program lists
/// beside a title.
#[tokio::test]
async fn the_owner_looks_in_the_vault_and_is_asked_nothing() {
    let (r, dir, _d) = rig_with_a_vault("ok");
    let sid = r.session();
    r.turn_with("look_vault", &sid, json!({"filter": "lantern"}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    for part in [
        "names only, never a value",
        "Account: harbor.example.test (a service account)",
        "Harbor",
        "- Lantern key · API_CREDENTIAL · in Harbor",
        "secrets.lantern_key: item \"Lantern key\" in vault \"Harbor\" (in the vault's list) · resolved",
        "secrets.tide_key: read from the environment",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
    let all = everything(&r, &sid);
    for leaked in [
        KEY_VALUE,
        "INVENTED-VALUE-1234",
        "INVENTED-USERNAME",
        "INVENTED-NOTE",
        "INVENTED-PERSON",
        "invented-tag",
        "secret-path",
        "ACCT-UUID-1",
        "USER-UUID-1",
        "credential\"", // the reference's field name is never given
    ] {
        assert!(!all.contains(leaked), "{leaked} reached the record");
    }
    assert!(!text.contains("/credential"), "{text}");
    // The vaults are listed twice: all of them, and the ones the account may make items in (so the
    // look says for each whether Theseus may write there).
    assert_eq!(logged(&dir).len(), 4, "{:?}", logged(&dir));
}

/// Whatever the tool is given, the daemon's command line is one of the list
/// shapes: a vault named like a flag or a reference is not found, not passed;
/// a filter that looks like a flag only filters; a vault found by name is
/// passed by the id the program listed.
#[tokio::test]
async fn no_input_makes_the_daemon_ask_for_a_value() {
    let allowed = |line: &str| {
        line == "whoami --format json"
            || line == "vault list --format json"
            || line == "vault list --permission create_items --format json"
            || line == "item list --format json"
            || line == "item list --format json --vault vaultid00000000000000000001"
            || line == "item list --format json --vault vaultid00000000000000000002"
    };
    let hostile = [
        json!({"vault": "--reveal"}),
        json!({"vault": "op://Harbor/Lantern key/credential"}),
        json!({"vault": "Harbor --fields credential"}),
        json!({"vault": "vaultid00000000000000000001 --reveal"}),
        json!({"vault": "harbor"}),
        json!({"vault": "vaultid00000000000000000002"}),
        json!({"filter": "--reveal"}),
        json!({"filter": "op read op://Harbor/Lantern key/credential"}),
        json!({"filter": "\n--fields=credential"}),
        json!({"what": ["items"], "vault": "Dock", "filter": "-h", "limit": 1}),
        json!({"limit": 0}),
        json!({"limit": 1_000_000}),
    ];
    for input in hostile {
        let (r, dir, _d) = rig_with_a_vault("ok");
        let sid = r.session();
        r.turn_with("look_vault", &sid, input.clone()).await;
        let (_, text) = r.result(&sid);
        for line in logged(&dir) {
            assert!(allowed(&line), "{input}: the daemon ran `{line}`");
        }
        assert!(!text.contains("INVENTED-VALUE"), "{input}: {text}");
    }
    // Keys the tool does not take, and a part it does not know, are refused
    // before anything runs.
    for input in [
        json!({"item": "Lantern key"}),
        json!({"field": "credential"}),
        json!({"reveal": true}),
        json!({"what": ["value"]}),
        json!({"what": ["items; read"]}),
    ] {
        let (r, dir, _d) = rig_with_a_vault("ok");
        let sid = r.session();
        r.turn_with("look_vault", &sid, input.clone()).await;
        let (status, _) = r.result(&sid);
        assert_ne!(status, ResultStatus::Ok, "{input}");
        assert!(logged(&dir).is_empty(), "{input}: nothing ran");
    }
}

/// A vault named by its name is asked for by the id the program gave; one that
/// does not exist says which ones do, and runs no item list.
#[tokio::test]
async fn a_vault_is_found_by_name_and_passed_by_its_listed_id() {
    let (r, dir, _d) = rig_with_a_vault("ok");
    let sid = r.session();
    r.turn_with(
        "look_vault",
        &sid,
        json!({"vault": "harbor", "what": ["items"]}),
    )
    .await;
    assert!(logged(&dir)
        .contains(&"item list --format json --vault vaultid00000000000000000001".to_string()));
    let (r, dir, _d) = rig_with_a_vault("ok");
    let sid = r.session();
    r.turn_with(
        "look_vault",
        &sid,
        json!({"vault": "Nowhere", "what": ["items"]}),
    )
    .await;
    let (_, text) = r.result(&sid);
    assert!(
        text.contains("no vault of that name: the vaults are Harbor, Dock"),
        "{text}"
    );
    assert!(!logged(&dir).iter().any(|l| l.starts_with("item list")));
}

/// A failure is its class, and never the program's own words.
#[tokio::test]
async fn the_vault_failing_says_its_class_and_nothing_of_its_own() {
    for (mode, says) in [
        ("fail-denied", "refused the daemon's token"),
        ("fail-net", "could not be reached"),
        ("fail-other", "program failed"),
    ] {
        let (r, _, _d) = rig_with_a_vault(mode);
        let sid = r.session();
        r.turn_with("look_vault", &sid, json!({})).await;
        let (_, text) = r.result(&sid);
        assert!(text.contains(says), "{mode}: {text}");
        assert!(!text.contains("INVENTED-WORDS-IN-STDERR"), "{text}");
        assert!(!text.contains("nowhere.example"), "{text}");
        // The settings are still listed.
        assert!(text.contains("secrets.lantern_key"), "{text}");
    }
}

/// An install with no vault session says so, runs nothing, and still lists
/// where each secret is read from.
#[tokio::test]
async fn with_no_token_the_look_says_so_and_lists_the_sources() {
    let r = rig_with(|cfg, _| {
        cfg.secrets
            .insert("lantern_key".into(), "env:LANTERN_FOR_TESTS".into());
    });
    r.core
        .tools
        .look
        .set_vault(Arc::new(OpReader::absent("no token".into())));
    let sid = r.session();
    r.turn_with("look_vault", &sid, json!({})).await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(text.contains("No vault session"), "{text}");
    assert!(
        text.contains("secrets.lantern_key: read from the environment"),
        "{text}"
    );
    assert!(
        !text.contains("no token"),
        "the reason is not echoed: {text}"
    );
}

/// A turn that was steered (it read text from outside) is asked first, and the
/// vault's program is not run until the owner says yes: at `[content_based_security]
/// outside = "ask"` (at the default it runs and the owner is told, `tests_quiet`).
#[tokio::test]
async fn a_steered_turn_is_asked_before_the_vault_is_read() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    use theseus_judge::band::Band;
    let (r, dir, _d) = rig_with_a_vault_at("ok", crate::content::OutsideMode::Ask);
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
    r.turn_with("look_vault", &sid, json!({})).await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert_eq!(asks[0].tool, "look.vault");
    assert!(
        asks[0].reason.contains(crate::config_change::FLAG_HOLD)
            && !asks[0].reason.contains("read text from outside"),
        "the flag, not outside text: {}",
        asks[0].reason
    );
    assert!(
        logged(&dir).is_empty(),
        "nothing was asked of the vault yet"
    );
}

/// A task's session is refused, in the tool's own words, and the vault is not
/// asked.
#[tokio::test]
async fn a_task_is_not_given_either_look() {
    for tool in ["look_vault", "look_theseus"] {
        let (r, dir, _d) = rig_with_a_vault("ok");
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
        r.turn_with(tool, &rec.session_id, json!({})).await;
        let (status, text) = r.result(&rec.session_id);
        assert_eq!(status, ResultStatus::Error, "{text}");
        assert!(text.contains("not for a task"), "{text}");
        assert!(logged(&dir).is_empty());
    }
}

/// The floor is as it was: the shell naming the vault's program, or Theseus's
/// own, is still held, and held as a floor hold.
#[tokio::test]
async fn the_shell_on_the_vaults_program_is_still_held_by_the_floor() {
    let r = rig();
    let sid = r.session();
    r.turn_with(
        "proc_run",
        &sid,
        json!({"argv": ["bash", "-c", "op whoami; op vault list"]}),
    )
    .await;
    let asks = r.asked();
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(asks[0].floor, "{asks:?}");
    assert_eq!(asks[0].tool, "proc.run");
}

/// A core as a daemon has it: deferral on (a test's core has it off), started
/// from a vault's note when `vault_note` (the looks and no settings tools) and
/// from nothing when not.
pub(super) fn deferred_core(vault_note: bool) -> (Arc<crate::Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.index.enabled = false;
    if vault_note {
        cfg.config_copy = Some("/var/lib/theseus/config.copy".into());
    }
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![]));
    let mut parts = crate::rpc::Parts::for_tests(cfg, fake, store);
    // Tests build with deferral off; a daemon has it on.
    parts.cfg.tools.defer.enabled = true;
    (crate::Core::build(parts).unwrap(), dir)
}

/// What the family costs a private place's request, deferred as it is, and if a
/// place loaded it by default. A bound, not a budget: the owner's rule is that a
/// family is loaded by default only below 400 tokens, so the family stays
/// deferred and this fails when what it adds to a request grows past that.
#[test]
fn the_look_family_is_deferred_and_its_cost_is_measured() {
    let (with, _a) = deferred_core(true);
    let (without, _b) = deferred_core(false);
    let d = with.tool_defer();
    let dm = d.places.iter().find(|p| p.place == "dm").expect("dm");
    let fam = dm
        .families
        .iter()
        .find(|f| f.family == "look")
        .expect("the look family");
    assert_eq!(
        fam.tools,
        ["look_theseus", "look_vault", "look_work"],
        "{:?}",
        fam.tools
    );
    assert!(!fam.loaded, "deferred in a direct message: {}", fam.why);
    let added = |place: &str| {
        let (a, b) = (
            with.first_request(place).unwrap(),
            without.first_request(place).unwrap(),
        );
        assert_eq!(
            a.loaded, b.loaded,
            "{place}: the looks load nothing by default"
        );
        a.tokens - b.tokens
    };
    let (dm_added, cli_added) = (added("dm"), added("cli"));
    eprintln!(
        "MEASURED look family: loads at {} tokens; deferred adds {dm_added} to a dm request, {cli_added} to a cli one",
        fam.tokens
    );
    // Measured 2026-11-18, and again 2026-11-19: a deferred family adds nothing to the census's total
    // (a deferred tool is counted at a constant, and its family's summary is read
    // only in the answer to a search), and loading it would add 1,184 tokens (re-measured 2026-11-19, with `look_work`): more
    // than the 400 under which the owner's rule would load it, so it stays deferred.
    assert!(
        dm_added <= 400 && cli_added <= 400,
        "{dm_added} {cli_added}"
    );
    assert!((401..=1_200).contains(&fam.tokens), "{}", fam.tokens);
}
