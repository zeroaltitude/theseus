//! What `look.vault` is asked for (issue 26). What it says, and what it runs,
//! is played through the whole core in `config_change/tests_look.rs`.

use serde_json::json;

use super::*;

/// No part named is every part; the cap is bounded both ways; a blank filter
/// or vault is none.
#[test]
fn a_call_is_checked_and_its_cap_is_bounded() {
    let a = ask_of(&json!({})).unwrap();
    assert_eq!(a.parts, PARTS.to_vec());
    assert_eq!(a.limit, DEFAULT_LIMIT);
    assert_eq!(ask_of(&json!({"limit": 0})).unwrap().limit, 1);
    assert_eq!(ask_of(&json!({"limit": 5000})).unwrap().limit, MAX_LIMIT);
    let a = ask_of(&json!({"vault": "  ", "filter": ""})).unwrap();
    assert!(a.vault.is_none() && a.filter.is_none());
    assert!(ask_of(&json!({"what": ["value"]})).is_err());
    assert!(
        ask_of(&json!({"field": "x"})).is_err(),
        "no other key is taken"
    );
}

/// A reference gives its vault and its item and nothing after them: the field
/// is never kept.
#[test]
fn a_reference_gives_its_vault_and_item_and_never_its_field() {
    assert_eq!(
        vault_and_item("op://Harbor/Lantern key/credential"),
        Some(("Harbor".into(), "Lantern key".into()))
    );
    assert_eq!(
        vault_and_item("op://Harbor/Lantern key/section/field#label"),
        Some(("Harbor".into(), "Lantern key".into()))
    );
    assert_eq!(vault_and_item("op://Harbor"), None);
    assert_eq!(vault_and_item("env:X"), None);
}

/// A vault is found by its listed id, else its name in any case; a string that
/// is neither finds nothing, so it is never passed on.
#[test]
fn a_vault_is_found_by_listed_id_or_name_and_nothing_else() {
    let vs = vec![
        Vault {
            id: "vaultid00000000000000000001".into(),
            name: "Harbor".into(),
        },
        Vault {
            id: "vaultid00000000000000000002".into(),
            name: "Dock".into(),
        },
    ];
    assert_eq!(
        vault_named(&vs, "vaultid00000000000000000002")
            .unwrap()
            .name,
        "Dock"
    );
    assert_eq!(vault_named(&vs, "harbor").unwrap().name, "Harbor");
    for hostile in ["--reveal", "op://Harbor/x/y", "Harbor ", "Har", "*", ""] {
        assert!(vault_named(&vs, hostile).is_none(), "{hostile:?}");
    }
}

/// The list says how many matched, how many it shows, and the cap, and when
/// it stopped short it says how to see the rest.
#[test]
fn the_list_says_its_cap_and_how_many_it_left_out() {
    let item = |n: usize| ItemName {
        id: format!("i{n}"),
        title: format!("Lantern {n:03}"),
        category: "LOGIN".into(),
        vault: "Harbor".into(),
    };
    let all: Vec<ItemName> = (0..7).map(item).collect();
    let ask = ask_of(&json!({"limit": 3, "filter": "LANTERN"})).unwrap();
    let said = items_words(&ask, &Ok((all.clone(), Some("Harbor".into()))));
    assert!(
        said.contains("Items in Harbor whose title has \"LANTERN\": 3 of 7 shown (cap 3;"),
        "{said}"
    );
    assert!(said.contains("narrow it"), "{said}");
    assert_eq!(said.lines().filter(|l| l.starts_with("- ")).count(), 3);
    let ask = ask_of(&json!({"filter": "zzz"})).unwrap();
    let said = items_words(&ask, &Ok((all, None)));
    assert!(said.contains("0 of 0 shown"), "{said}");
    let said = items_words(&ask, &Err("the vault did not answer in time".into()));
    assert!(
        said.contains("not available: the vault did not answer in time"),
        "{said}"
    );
}
