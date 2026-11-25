//! `look.vault`'s field names played through the whole core (issue 26): the
//! owner's own turn asks which fields an item has, is asked nothing, and is
//! given each field's label and kind and never a value; the daemon's command
//! line for it is one get of an id the program listed, whatever the turn
//! says.

use serde_json::json;

use super::tests_look::{everything, rig_with_a_vault};
use crate::node::ResultStatus;
use crate::secrets::look::tests::logged;

/// Every planted value in the vault's answer: none may reach the turn, the
/// record or the ledger.
const VALUES: [&str; 6] = [
    "INVENTED-USER-VALUE",
    "INVENTED-FIELD-VALUE-9",
    "INVENTED-NOTE-TEXT",
    "INVENTED-SECTION-VALUE",
    "INVENTED-ODD-VALUE",
    "secret-path",
];

/// "Which fields does Lantern key have?": the labels, sections and kinds, no
/// card, the three program calls, and none of the values the program printed.
#[tokio::test]
async fn the_owner_asks_for_an_items_field_names_and_gets_no_value() {
    let (r, dir, _d) = rig_with_a_vault("ok");
    let sid = r.session();
    r.turn_with(
        "look_vault",
        &sid,
        json!({"fields_of": "lantern KEY", "vault": "Harbor", "what": ["fields"]}),
    )
    .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    for part in [
        "Fields of \"Lantern key\" (API_CREDENTIAL, in Harbor)",
        "never a value",
        "- username · text",
        "- credential · concealed",
        "- notesPlain · a note",
        "- Recovery › backup code · concealed",
        "- odd · other",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    assert!(r.asked().is_empty(), "no card: {:?}", r.asked());
    let all = format!("{text}{}", everything(&r, &sid));
    for leaked in VALUES {
        assert!(
            !all.contains(leaked),
            "{leaked} reached the turn or the record"
        );
    }
    assert_eq!(
        logged(&dir),
        [
            "vault list --format json",
            "item list --format json --vault vaultid00000000000000000001",
            "item get itemid00000000000000000001 --vault vaultid00000000000000000001 --format json"
        ]
    );
}

/// Whatever the item is called in the call, the one get is of an id the
/// program listed: a title that looks like a flag, a reference or a path finds
/// nothing and runs no get.
#[tokio::test]
async fn no_fields_of_makes_the_daemon_get_anything_but_a_listed_id() {
    for hostile in [
        "--reveal",
        "op://Harbor/Lantern key/credential",
        "Lantern key --fields credential",
        "../../etc/passwd",
        "itemid00000000000000000001 --reveal",
        "itemid0000000000000000000",
        "\n--format json",
        "*",
    ] {
        let (r, dir, _d) = rig_with_a_vault("ok");
        let sid = r.session();
        r.turn_with("look_vault", &sid, json!({"fields_of": hostile}))
            .await;
        let (_, text) = r.result(&sid);
        for line in logged(&dir) {
            assert!(
                line == "vault list --format json" || line.starts_with("item list --format json"),
                "{hostile:?}: the daemon ran `{line}`"
            );
        }
        for leaked in VALUES {
            assert!(!text.contains(leaked), "{hostile:?}: {text}");
        }
        assert!(
            text.contains("no item of that title or id"),
            "{hostile:?}: {text}"
        );
    }
    // By its listed id it is found, and the get is of that id alone.
    let (r, dir, _d) = rig_with_a_vault("ok");
    let sid = r.session();
    r.turn_with(
        "look_vault",
        &sid,
        json!({"fields_of": "itemid00000000000000000001", "what": ["fields"]}),
    )
    .await;
    assert!(logged(&dir).contains(
        &"item get itemid00000000000000000001 --vault vaultid00000000000000000001 --format json"
            .to_string()
    ));
}

/// Two items of one title are not guessed between: their ids are given and no
/// get runs, until the model names one by id.
#[tokio::test]
async fn two_items_of_one_title_are_not_guessed_between() {
    let (r, dir, _d) = rig_with_a_vault("dup");
    let sid = r.session();
    r.turn_with("look_vault", &sid, json!({"fields_of": "Lantern key"}))
        .await;
    let (status, text) = r.result(&sid);
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert!(
        text.contains("2 items are titled \"Lantern key\"")
            && text.contains("id itemid00000000000000000001")
            && text.contains("id itemid00000000000000000009"),
        "{text}"
    );
    assert!(!logged(&dir).iter().any(|l| l.starts_with("item get")));
    let sid = r.session();
    r.turn_with(
        "look_vault",
        &sid,
        json!({"fields_of": "itemid00000000000000000009"}),
    )
    .await;
    // The second of the two is in the other vault, and is asked for there.
    assert!(logged(&dir).contains(
        &"item get itemid00000000000000000009 --vault vaultid00000000000000000002 --format json"
            .to_string()
    ));
}

/// A failed get is its class, never the program's words.
#[tokio::test]
async fn a_failed_get_says_its_class_and_nothing_of_its_own() {
    let (r, _, _d) = rig_with_a_vault("fail-get");
    let sid = r.session();
    r.turn_with("look_vault", &sid, json!({"fields_of": "Lantern key"}))
        .await;
    let (_, text) = r.result(&sid);
    assert!(
        text.contains("Fields: not available: the vault refused"),
        "{text}"
    );
    assert!(!text.contains("INVENTED-WORDS-IN-STDERR"), "{text}");
}

/// The fields of an item are asked first when the turn has read outside text,
/// like every look at the vault: nothing is run until the owner says yes.
#[tokio::test]
async fn field_names_are_asked_first_after_outside_text() {
    use crate::content::{ContentFlags, Flag, ItemKind, Label};
    use theseus_judge::band::Band;
    let (r, dir, _d) = rig_with_a_vault("ok");
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
    r.turn_with("look_vault", &sid, json!({"fields_of": "Lantern key"}))
        .await;
    assert_eq!(r.asked().len(), 1, "{:?}", r.asked());
    assert!(
        r.asked()[0]
            .reason
            .contains(crate::config_change::FLAG_HOLD),
        "the flag: {}",
        r.asked()[0].reason
    );
    assert!(
        logged(&dir).is_empty(),
        "nothing was asked of the vault yet"
    );
}
