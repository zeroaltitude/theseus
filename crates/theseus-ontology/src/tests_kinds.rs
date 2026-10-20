//! The kinds of the design: the table of seeds, a kind's retirement, and its
//! optional fields.

use super::*;

const OP: Origin = Origin::Operator;

fn row(name: &str) -> Kind {
    seeds().into_iter().find(|k| k.name == name).unwrap()
}

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

/// A kind the sweep added.
fn mined(name: &str, precedence: u32) -> Kind {
    Kind {
        name: name.into(),
        basis: Basis::Interpreted,
        assigned_by: vec![Origin::Operator, Origin::Sweep, Origin::Jev],
        per_session: PerSession::AtMost(2),
        parent: None,
        precedence,
        rule: Rule::Chain,
        description: String::new(),
        version: 1,
        added_by: "sweep:s-1".into(),
        retired_ms: None,
        examples: Vec::new(),
        mined_from: Vec::new(),
    }
}

fn retired(k: &Kind, by: &str) -> Kind {
    Kind {
        version: k.version + 1,
        added_by: by.into(),
        retired_ms: Some(5_000),
        ..k.clone()
    }
}

fn with_category(o: &mut Ontology, kind: &str, local: &str) {
    let c = Category::new(id(&format!("{kind}:{local}")), local, "ada");
    o.put(Record::Category(c), OP).unwrap();
}

#[test]
fn the_new_seeds_pass_the_table_and_the_given_five_are_named() {
    let o = Ontology::seeded();
    assert_eq!(o.check_all(), Ok(()));
    assert_eq!(GIVEN, ["guild", "channel", "thread", "project", "person"]);
    let given: Vec<&str> = o
        .kinds()
        .iter()
        .filter(|k| k.is_given())
        .map(|k| k.name.as_str())
        .collect();
    assert_eq!(given, GIVEN);
    let precedence: Vec<u32> = o.kinds().iter().map(|k| k.precedence).collect();
    assert_eq!(precedence, [10, 20, 25, 28, 30, 40, 50, 60, 70, 80, 90]);
    // A given kind the transport assigns, and thread and project store nothing.
    assert!(!row("thread").stores() && !row("project").stores());
    assert!(row("person").assigned_by.contains(&Origin::Jev));
}

#[test]
fn a_row_without_the_new_fields_serializes_as_it_did() {
    let today = r#"{"name":"guild","basis":"given","assigned_by":["transport"],"per_session":1,"precedence":10,"rule":"chain","description":"The server a session's place is in, from the transport.","version":1,"added_by":"seed"}"#;
    let k = row("guild");
    assert_eq!(serde_json::to_string(&k).unwrap(), today);
    let back: Kind = serde_json::from_str(today).unwrap();
    assert_eq!(back, k);
}

#[test]
fn examples_are_at_most_five_names_and_mined_from_is_a_closed_set() {
    let o = Ontology::seeded();
    let mut k = mined("workflow", 55);
    k.examples = (1..=5).map(|n| format!("example {n}")).collect();
    k.mined_from = vec![MinedFrom::Hosts, MinedFrom::Capitalised];
    assert_eq!(o.check(&Record::Kind(k.clone()), Origin::Sweep), Ok(()));
    let json = serde_json::to_value(&k).unwrap();
    assert_eq!(
        json["mined_from"],
        serde_json::json!(["hosts", "capitalised"])
    );
    k.examples.push("example 6".into());
    let e = o
        .check(&Record::Kind(k.clone()), Origin::Sweep)
        .unwrap_err();
    assert!(matches!(e, Refusal::Invalid { .. }), "{e:?}");
    k.examples = vec!["two\nlines".into()];
    assert!(o.check(&Record::Kind(k), Origin::Sweep).is_err());
    let mut json = serde_json::to_value(mined("workflow", 55)).unwrap();
    json["mined_from"] = serde_json::json!(["rumours"]);
    assert!(serde_json::from_value::<Kind>(json).is_err());
}

#[test]
fn a_kind_is_retired_once_nothing_uses_it_and_then_takes_nothing_new() {
    let mut o = Ontology::seeded();
    let k = mined("workflow", 55);
    o.put(Record::Kind(k.clone()), Origin::Sweep).unwrap();
    with_category(&mut o, "workflow", "deploys");
    let gone = retired(&k, "ada");
    let e = o.check(&Record::Kind(gone.clone()), OP).unwrap_err();
    assert!(matches!(e, Refusal::InUse { .. }), "{e:?}");

    let c = o.category(&id("workflow:deploys")).unwrap().retired(9);
    o.put(Record::Category(c), OP).unwrap();
    o.put(Record::Kind(gone), OP).unwrap();
    assert_eq!(o.kind("workflow").unwrap().retired_ms, Some(5_000));

    let new = Category::new(id("workflow:builds"), "builds", "ada");
    let e = o.check(&Record::Category(new), OP).unwrap_err();
    assert!(e.to_string().contains("retired"), "{e}");
    let list = MemberList {
        session: "s1".into(),
        kind: "workflow".into(),
        members: vec![Membership::operator(id("workflow:deploys"), 1)],
    };
    assert!(o.check(&Record::Members(list), OP).is_err());
}

#[test]
fn a_stored_list_keeps_its_kind_in_use() {
    let mut o = Ontology::seeded();
    let k = mined("workflow", 55);
    o.put(Record::Kind(k.clone()), Origin::Sweep).unwrap();
    with_category(&mut o, "workflow", "deploys");
    let list = MemberList {
        session: "s1".into(),
        kind: "workflow".into(),
        members: vec![Membership::operator(id("workflow:deploys"), 1)],
    };
    o.put(Record::Members(list), OP).unwrap();
    // The category is in use too, so the refusal names the category's count.
    let e = o.check(&Record::Kind(retired(&k, "ada")), OP).unwrap_err();
    assert!(matches!(e, Refusal::InUse { .. }), "{e:?}");
}

#[test]
fn a_given_or_seeded_kind_is_never_retired() {
    let o = Ontology::seeded();
    for name in ["guild", "thread", "person"] {
        let e = o
            .check(&Record::Kind(retired(&row(name), "ada")), OP)
            .unwrap_err();
        assert!(matches!(e, Refusal::Given { .. }), "{name}: {e:?}");
    }
    for name in ["topic", "culture", "event"] {
        let e = o
            .check(&Record::Kind(retired(&row(name), "ada")), OP)
            .unwrap_err();
        assert!(matches!(e, Refusal::Writer { .. }), "{name}: {e:?}");
    }
}

#[test]
fn the_sweep_takes_away_only_a_kind_it_added() {
    let mut o = Ontology::seeded();
    let k = mined("workflow", 55);
    o.put(Record::Kind(k.clone()), Origin::Sweep).unwrap();
    assert_eq!(
        o.check(&Record::Kind(retired(&k, "sweep:s-2")), Origin::Sweep),
        Ok(())
    );
    // An operator's kind is not the sweep's to take away.
    let mut op = mined("ledger", 56);
    op.added_by = "ada".into();
    o.put(Record::Kind(op.clone()), OP).unwrap();
    let e = o
        .check(&Record::Kind(retired(&op, "sweep:s-2")), Origin::Sweep)
        .unwrap_err();
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
}

#[test]
fn a_retired_kind_frees_its_precedence() {
    let mut o = Ontology::seeded();
    let k = mined("workflow", 55);
    o.put(Record::Kind(k.clone()), Origin::Sweep).unwrap();
    let clash = mined("pipeline", 55);
    let e = o
        .check(&Record::Kind(clash.clone()), Origin::Sweep)
        .unwrap_err();
    assert!(matches!(e, Refusal::Precedence { .. }), "{e:?}");
    o.put(Record::Kind(retired(&k, "ada")), OP).unwrap();
    assert_eq!(o.check(&Record::Kind(clash), Origin::Sweep), Ok(()));
}

#[test]
fn the_sweep_adds_a_new_interpreted_kind_and_changes_nothing_else() {
    let o = Ontology::seeded();
    assert_eq!(
        o.check(&Record::Kind(mined("workflow", 55)), Origin::Sweep),
        Ok(())
    );
    // Not an existing row.
    let topic = Kind {
        version: 2,
        added_by: "sweep:s-1".into(),
        description: "changed".into(),
        ..row("topic")
    };
    let e = o.check(&Record::Kind(topic), Origin::Sweep).unwrap_err();
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
    // Not a given kind.
    let given = Kind {
        basis: Basis::Given,
        assigned_by: vec![Origin::Transport, Origin::Sweep, Origin::Jev],
        ..mined("workflow", 55)
    };
    assert!(o.check(&Record::Kind(given), Origin::Sweep).is_err());
    // Its row names the sweep and Jev.
    let narrow = Kind {
        assigned_by: vec![Origin::Sweep],
        ..mined("workflow", 55)
    };
    assert!(o.check(&Record::Kind(narrow), Origin::Sweep).is_err());
    // And is the sweep's: its added_by says so.
    let mine = Kind {
        added_by: "ada".into(),
        ..mined("workflow", 55)
    };
    assert!(o.check(&Record::Kind(mine), Origin::Sweep).is_err());
    // Jev and the dream never write a kind.
    for by in [Origin::Jev, Origin::Dream] {
        let e = o
            .check(&Record::Kind(mined("workflow", 55)), by)
            .unwrap_err();
        assert!(matches!(e, Refusal::Writer { .. }), "{by}: {e:?}");
    }
}

/// The records of a store from before this row, as it wrote them: the four
/// seed rows, a guild channel and a DM's category, an operator's topic list
/// and a person list from the import. By hand, in the layout of the build
/// before onto-core.
const OLD_STORE: &[(&str, &str)] = &[
    ("onto:kind:guild", r#"{"name":"guild","basis":"given","assigned_by":["transport"],"per_session":1,"precedence":10,"rule":"chain","description":"The server a session's place is in, from the transport.","version":1,"added_by":"seed"}"#),
    ("onto:kind:channel", r#"{"name":"channel","basis":"given","assigned_by":["transport"],"per_session":1,"parent":"guild","precedence":20,"rule":"chain","description":"The place a session lives in, from the transport. A channel's parent is its guild.","version":1,"added_by":"seed"}"#),
    ("onto:kind:person", r#"{"name":"person","basis":"given","assigned_by":["transport","operator","import"],"per_session":"many","precedence":30,"rule":"intent_line","description":"The people a session involves.","version":1,"added_by":"seed"}"#),
    ("onto:kind:topic", r#"{"name":"topic","basis":"interpreted","assigned_by":["operator","import"],"per_session":3,"parent":"topic","precedence":40,"rule":"chain","description":"What a session is about.","version":1,"added_by":"seed"}"#),
    ("onto:cat:channel:200000000000000001", r#"{"id":"channel:200000000000000001","name":"lab","added_by":"transport"}"#),
    ("onto:cat:person:300000000000000001", r#"{"id":"person:300000000000000001","name":"@wren","added_by":"transport"}"#),
    ("onto:cat:person:wren-ash","{\"id\":\"person:wren-ash\",\"name\":\"Wren Ash\",\"description\":\"\",\"added_by\":\"import tide-2026\",\"handles\":[\"name:Wren Ash\"]}"),
    ("onto:cat:topic:tides", r#"{"id":"topic:tides","name":"tides","added_by":"ada"}"#),
    ("onto:member:s1:topic", r#"{"session":"s1","kind":"topic","members":[{"category":"topic:tides","origin":"operator","as_of_ms":1000}]}"#),
    ("onto:member:s1:person", r#"{"session":"s1","kind":"person","members":[{"category":"person:wren-ash","origin":"import","as_of_ms":1000}]}"#),
];

#[test]
fn a_store_from_before_loads_whole_and_serves_the_same_memberships() {
    let records: Vec<Record> = OLD_STORE
        .iter()
        .map(|(key, json)| Record::decode(key, serde_json::from_str(json).unwrap()).unwrap())
        .collect();
    let (o, dropped) = Ontology::load(records);
    assert_eq!(dropped, vec![]);
    let got: Vec<String> = o
        .memberships("s1")
        .iter()
        .map(|m| format!("{}:{}", m.category, m.origin))
        .collect();
    assert_eq!(got, ["person:wren-ash:import", "topic:tides:operator"]);
    // The stored rows stand, as the seeds the build carried: the old
    // topic row still names the operator and the import alone.
    assert_eq!(
        o.kind("topic").unwrap().assigned_by,
        [Origin::Operator, Origin::Import]
    );
    assert_eq!(
        o.category(&id("channel:200000000000000001"))
            .unwrap()
            .parent,
        None
    );
    // And the new rows stand beside them.
    assert!(o.kind("culture").is_some() && o.kind("thread").is_some());
}
