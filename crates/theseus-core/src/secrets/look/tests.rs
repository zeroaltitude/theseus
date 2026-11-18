//! The vault's names (issue 26): the closed set of verbs, what is rebuilt from
//! an answer, and what a failure says. The program here is a script that logs
//! its arguments and answers a list; no real vault is read.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::*;

/// A fake vault program: logs each argument list to `op.argv`, and answers
/// `item list` with a list that has, beside the five named keys, every field
/// the real one lists that must not ride through.
pub(crate) fn stub(dir: &Path, mode: &str) -> OpReader {
    let script = format!(
        r#"#!/bin/sh
LOG='{d}/op.argv'; MODE='{mode}'
printf '%s\n' "$*" >> "$LOG"
[ "$MODE" = fail-denied ] && {{ echo '[ERROR] 2026/10/19 08:00:00 you are not authorized: the token is wrong' >&2; exit 1; }}
[ "$MODE" = fail-net ] && {{ echo '[ERROR] 2026/10/19 08:00:00 dial tcp: lookup nowhere.example: no such host' >&2; exit 1; }}
[ "$MODE" = fail-other ] && {{ echo '[ERROR] 2026/10/19 08:00:00 something with INVENTED-WORDS-IN-STDERR' >&2; exit 1; }}
[ "$MODE" = junk ] && {{ echo 'not json'; exit 0; }}
[ "$MODE" = big ] && {{ head -c 5000000 /dev/zero | tr '\0' 'x'; exit 0; }}
case "$1 $2" in
  "whoami --format") echo '{{"url":"https://harbor.example.test/path","account_uuid":"ACCT-UUID-1","user_uuid":"USER-UUID-1","user_type":"SERVICE_ACCOUNT"}}' ;;
  "vault list") echo '[{{"id":"vaultid00000000000000000001","name":"Harbor","content_version":3}},{{"id":"vaultid00000000000000000002","name":"Dock"}}]' ;;
  "item list") echo '[{{"id":"itemid1","title":"Lantern key","version":2,"vault":{{"id":"vaultid00000000000000000001","name":"Harbor"}},"category":"API_CREDENTIAL","additional_information":"INVENTED-USERNAME","urls":[{{"href":"https://invented.example.test/secret-path"}}],"tags":["invented-tag"],"fields":[{{"id":"credential","value":"INVENTED-VALUE-1234"}}],"notesPlain":"INVENTED-NOTE","last_edited_by":"INVENTED-PERSON"}}]' ;;
  *) echo "[ERROR] unexpected: $*" >&2; exit 1 ;;
esac
"#,
        d = dir.display(),
        mode = mode
    );
    let bin = dir.join("op");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    OpReader::with_bin("invented-token-not-real", bin)
}

pub(crate) fn logged(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("op.argv"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// The words that would read a value, a field, a document, or run something
/// under the vault's session: no argument list of any verb holds one.
const READS: [&str; 10] = [
    "read", "get", "--reveal", "document", "inject", "run", "create", "edit", "delete", "--fields",
];

/// There are three verbs, and the arguments of each are literals but a vault's
/// id, which is letters and digits only.
#[test]
fn the_verbs_are_a_closed_set_of_lists() {
    let id = VaultId::parse("abcd1234efgh5678").unwrap();
    let all = [
        (Verb::Whoami, "whoami --format json"),
        (Verb::Vaults, "vault list --format json"),
        (Verb::Items(None), "item list --format json"),
        (
            Verb::Items(Some(id)),
            "item list --format json --vault abcd1234efgh5678",
        ),
    ];
    for (verb, line) in all {
        let argv = verb.argv();
        assert_eq!(argv.join(" "), line);
        assert!(
            !argv.iter().any(|a| READS.contains(&a.as_str())),
            "{argv:?}"
        );
    }
}

/// A vault id is what `op` gives and nothing else can be one: a flag, a
/// reference, a path, a space, an empty string and a value are all refused.
#[test]
fn a_vault_id_cannot_be_a_flag_a_reference_or_a_path() {
    for hostile in [
        "",
        "--reveal",
        "-h",
        "--vault=x",
        "op://Harbor/Lantern key/credential",
        "../../etc/passwd",
        "id with space",
        "abc",
        "idididid;rm -rf",
        "idididid\n--reveal",
        "idididid=value",
        "caf\u{e9}caf\u{e9}caf\u{e9}",
        &"a".repeat(65),
    ] {
        assert_eq!(VaultId::parse(hostile), None, "{hostile:?}");
    }
    assert!(VaultId::parse("abcd1234efgh5678").is_some());
}

/// The answer is rebuilt from the named keys: an item's username, addresses,
/// tags, fields, notes and editor, and an account's identifiers, are not in
/// what is kept, even though the program listed them.
#[test]
fn what_the_program_adds_does_not_ride_through() {
    let items = parse_items(
        r#"[{"id":"i1","title":"Lantern key","vault":{"id":"v1","name":"Harbor"},"category":"LOGIN",
        "additional_information":"INVENTED-USERNAME","urls":[{"href":"https://x.test/INVENTED-PATH"}],
        "tags":["INVENTED-TAG"],"fields":[{"value":"INVENTED-VALUE"}],"notesPlain":"INVENTED-NOTE",
        "last_edited_by":"INVENTED-PERSON","a_key_added_tomorrow":"INVENTED-NEW"}]"#,
    )
    .unwrap();
    let shown = format!("{items:?}");
    for leaked in [
        "INVENTED-USERNAME",
        "INVENTED-PATH",
        "INVENTED-TAG",
        "INVENTED-VALUE",
        "INVENTED-NOTE",
        "INVENTED-PERSON",
        "INVENTED-NEW",
    ] {
        assert!(!shown.contains(leaked), "{leaked} in {shown}");
    }
    assert_eq!(
        items,
        vec![ItemName {
            id: "i1".into(),
            title: "Lantern key".into(),
            category: "LOGIN".into(),
            vault: "Harbor".into()
        }]
    );
    let account = parse_account(
        r#"{"url":"https://harbor.example.test/x?y=INVENTED-Q","account_uuid":"INVENTED-UUID","user_uuid":"INVENTED-UUID-2","user_type":"SERVICE_ACCOUNT"}"#,
    )
    .unwrap();
    assert_eq!(
        account,
        Account {
            host: "harbor.example.test".into(),
            kind: "a service account".into()
        }
    );
}

/// A title with a line break or a control character is one line, and a long
/// one is cut: it cannot start a line of its own in what the model reads.
#[test]
fn a_name_is_one_short_line() {
    assert_eq!(clean("two\nlines\t and\u{7} more"), "two lines and more");
    let long = clean(&"x".repeat(500));
    assert_eq!(long.chars().count(), MAX_NAME + 1);
    assert!(long.ends_with('…'));
}

/// The reader runs each verb with exactly its arguments, under a time limit
/// and the daemon's token, and what it returns parses.
#[tokio::test]
async fn the_reader_runs_the_verb_and_only_the_verb() {
    let dir = tempfile::tempdir().unwrap();
    let op = stub(dir.path(), "ok");
    let who = op.look(&Verb::Whoami).await.unwrap();
    assert_eq!(parse_account(&who).unwrap().host, "harbor.example.test");
    let vaults = parse_vaults(&op.look(&Verb::Vaults).await.unwrap()).unwrap();
    assert_eq!(vaults.len(), 2);
    let id = VaultId::parse(&vaults[0].id).unwrap();
    let items = parse_items(&op.look(&Verb::Items(Some(id))).await.unwrap()).unwrap();
    assert_eq!(items[0].title, "Lantern key");
    assert_eq!(
        logged(dir.path()),
        [
            "whoami --format json",
            "vault list --format json",
            "item list --format json --vault vaultid00000000000000000001"
        ]
    );
}

/// A failure is a class, in words that name no cause of the program's: its
/// own text, which can hold an item's name, is never returned.
#[tokio::test]
async fn a_failure_is_a_class_and_never_the_programs_words() {
    for (mode, want) in [
        ("fail-denied", Failed::Denied),
        ("fail-net", Failed::Unreachable),
        ("fail-other", Failed::Failed),
        ("big", Failed::TooLarge),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let got = stub(dir.path(), mode).look(&Verb::Vaults).await;
        assert_eq!(got, Err(want), "{mode}");
        let said = want.words();
        for leaked in [
            "INVENTED-WORDS-IN-STDERR",
            "nowhere.example",
            "tcp",
            "ERROR",
        ] {
            assert!(!said.contains(leaked), "{said}");
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let junk = stub(dir.path(), "junk").look(&Verb::Vaults).await.unwrap();
    assert_eq!(parse_vaults(&junk), Err(Failed::Unreadable));
    assert_eq!(parse_items("{}"), Err(Failed::Unreadable));
}

/// An install with no vault session runs nothing and says so.
#[tokio::test]
async fn with_no_vault_nothing_is_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut op = stub(dir.path(), "ok");
    op.absent = Some("no token".into());
    assert!(op.has_no_vault());
    assert_eq!(op.look(&Verb::Whoami).await, Err(Failed::NoVault));
    assert!(logged(dir.path()).is_empty(), "no program was started");
}
