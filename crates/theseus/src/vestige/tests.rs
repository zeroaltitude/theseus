//! A Vestige dump read as imported notes (theseus-71ib): the shapes of a
//! dump, a note's episode (its id the digest of its text, so once), and the
//! directory form. Invented notes only.

use serde_json::Value;

use super::*;

const T0: i64 = 1_790_000_000_000;

#[test]
fn a_dump_may_be_an_array_or_an_object_and_a_note_a_string_or_an_object() {
    let array = r#"["the skiff key is on the hook", {"content": "tide log is in the boathouse",
        "created_at": "2026-09-01T08:00:00Z", "tags": ["boats", "logs"]},
        {"text": "second form", "timestamp": 1790000000, "tag": "x"},
        {"memory": "third form", "date": 1790000000123},
        {"nothing": "here"}, 7, "   "]"#;
    let got = parse(array, T0).unwrap();
    assert_eq!(got.len(), 4);
    assert_eq!(
        (got[0].text.as_str(), got[0].at_ms),
        ("the skiff key is on the hook", T0)
    );
    assert_eq!(got[1].at_ms, 1_788_249_600_000);
    assert_eq!(got[1].tags, ["boats", "logs"]);
    assert_eq!(
        (got[2].at_ms, got[2].tags.clone()),
        (1_790_000_000_000, vec!["x".to_string()])
    );
    assert_eq!(
        got[3].at_ms, 1_790_000_000_123,
        "milliseconds past 1e11 are milliseconds"
    );
    for key in ["memories", "items", "nodes", "entries", "data"] {
        let obj = format!(r#"{{"version": 2, "{key}": ["one"]}}"#);
        assert_eq!(parse(&obj, T0).unwrap().len(), 1, "{key}");
    }
    for bad in ["not json", "42", r#"{"version": 2}"#] {
        assert!(parse(bad, T0).is_err(), "{bad}");
    }
}

#[test]
fn a_note_is_an_episode_whose_id_is_its_texts_digest() {
    let a = Memory {
        text: "the tide log is in the boathouse".into(),
        at_ms: T0,
        tags: vec!["logs".into()],
    };
    let b = Memory {
        at_ms: T0 + 5000,
        tags: vec![],
        ..a.clone()
    };
    let (ia, la) = episode(&a, "vestige.json", 0);
    let (ib, lb) = episode(&b, "vestige-2.json", 9);
    assert_eq!(
        ia, ib,
        "the same text is the same note, whatever its time and tags"
    );
    assert_ne!(la, lb);
    let v: Value = serde_json::from_str(&la).unwrap();
    assert_eq!(v["source"], "vestige");
    assert_eq!(v["import_tag"], "vestige");
    assert_eq!(v["place"]["kind"], "file");
    assert_eq!(v["place"]["name"], "vestige.json");
    assert_eq!(v["messages"][0]["author"], "vestige");
    assert_eq!(v["messages"][0]["integrity"], "agent");
    assert_eq!(
        v["messages"][0]["text"],
        "the tide log is in the boathouse\n[tags: logs]"
    );
    assert_eq!(v["messages"][0]["unit"], "vestige.json#0");
    assert!(v["episode_id"].as_str().unwrap().starts_with("ep_"));
    assert_eq!(v["episode_id"].as_str().unwrap().len(), 3 + 64);
    assert_eq!(v["hash"].as_str().unwrap().len(), 64);
    let again = episode(&a, "vestige.json", 0);
    assert_eq!(again.1, la, "the same note, the same bytes");
}

#[test]
fn a_directory_gives_its_vestige_dumps_and_a_file_itself() {
    let d = tempfile::tempdir().unwrap();
    for name in [
        "vestige.json",
        "Vestige-2026.JSON",
        "notes.json",
        "vestige.txt",
    ] {
        std::fs::write(d.path().join(name), "[]").unwrap();
    }
    std::fs::create_dir(d.path().join("vestige-sub.json")).unwrap();
    let names: Vec<String> = dumps_at(d.path())
        .unwrap()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["Vestige-2026.JSON", "vestige.json"].map(str::to_string)
    );
    assert_eq!(dumps_at(&d.path().join("notes.json")).unwrap().len(), 1);
    assert!(dumps_at(&d.path().join("missing")).is_err());
}
