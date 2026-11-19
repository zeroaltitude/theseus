//! What `look.theseus` is asked for (issue 26). What it says is played through
//! the whole core in `config_change/tests_look.rs`.

use serde_json::json;

use super::*;

/// No part named is every part, in the order they are given; a part named once
/// is given once; an unknown one is refused and names the choices.
#[test]
fn the_parts_are_all_by_default_and_checked_when_named() {
    assert_eq!(parts_of(&json!({})).unwrap(), PARTS.to_vec());
    assert_eq!(parts_of(&json!({"what": []})).unwrap(), PARTS.to_vec());
    assert_eq!(
        parts_of(&json!({"what": ["build", "profiles", "build"]})).unwrap(),
        ["profiles", "build"]
    );
    let e = parts_of(&json!({"what": ["keys"]})).unwrap_err();
    assert!(e.contains("profiles, providers, health, build"), "{e}");
    assert!(
        parts_of(&json!({"key": "x"})).is_err(),
        "no other key is taken"
    );
}

/// The ontology is a part that may be named and is not part of "everything".
#[test]
fn the_ontology_is_askable_and_not_in_the_default() {
    assert!(!PARTS.contains(&"ontology"));
    assert_eq!(
        parts_of(&json!({"what": ["ontology", "build"]})).unwrap(),
        ["build", "ontology"]
    );
    assert_eq!(
        parts_of(&json!({"what": ["ontology"]})).unwrap(),
        ["ontology"]
    );
    assert_eq!(parts_of(&json!({})).unwrap(), PARTS.to_vec());
}
