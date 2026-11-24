//! A view's pieces on the wire: a fold names its rung, in its JSON and in the plain lines; no other piece
//! carries the key, so a reader that does not know it reads every other piece as before.

use super::*;

fn piece(kind: &str, level: Option<u8>) -> ViewPiece {
    ViewPiece {
        handle: format!("x:{kind}"),
        kind: kind.into(),
        first: "trn_a".into(),
        last: "trn_b".into(),
        band: "ladder".into(),
        form: "whole".into(),
        tokens: 812,
        reasons: vec!["ladder".into()],
        label: "testimony".into(),
        level,
        ..ViewPiece::default()
    }
}

fn drop(kind: &str, level: Option<u8>) -> ViewDrop {
    ViewDrop {
        handle: format!("x:{kind}-cut"),
        kind: kind.into(),
        slot: "here".into(),
        band: "ladder".into(),
        reasons: vec!["ladder".into()],
        tokens: 70,
        why: "over_budget".into(),
        level,
    }
}

#[test]
fn a_fold_piece_names_its_level_and_no_other_does() {
    let fold = serde_json::to_value(piece("folds", Some(2))).unwrap();
    assert_eq!(fold["level"], 2, "{fold}");
    let other = serde_json::to_value(piece("exchanges", None)).unwrap();
    assert!(other.get("level").is_none(), "{other}");
    let cut = serde_json::to_value(drop("folds", Some(3))).unwrap();
    assert_eq!(cut["level"], 3, "{cut}");
    assert!(serde_json::to_value(drop("exchanges", None))
        .unwrap()
        .get("level")
        .is_none());
    // A piece written before the key reads as no level.
    let old: ViewPiece = serde_json::from_value(serde_json::json!({"kind": "folds"})).unwrap();
    assert_eq!(old.level, None);

    let v = ViewShown {
        slots: vec![ViewSlot {
            slot: "here".into(),
            pieces: vec![piece("folds", Some(1)), piece("exchanges", None)],
            tokens: 1_624,
        }],
        dropped: vec![drop("folds", Some(3)), drop("exchanges", None)],
        ..ViewShown::default()
    };
    let said = lines(&v);
    for line in [
        "    x:folds folds@1 trn_a..trn_b [ladder] whole 812 tokens, testimony: ladder\n",
        "    x:exchanges exchanges trn_a..trn_b [ladder] whole 812 tokens, testimony: ladder\n",
        "    x:folds-cut folds@3 here [ladder] 70 tokens, over_budget: ladder\n",
        "    x:exchanges-cut exchanges here [ladder] 70 tokens, over_budget: ladder\n",
    ] {
        assert!(said.contains(line), "{line}\n{said}");
    }
}
