//! People beyond the transport (theseus-wy7y): the person kind's stored
//! side, handles, and merges.

use super::*;

const OP: Origin = Origin::Operator;
const IMPORT: Origin = Origin::Import;
const TRANSPORT: Origin = Origin::Transport;

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn person(i: &str, name: &str, handles: &[&str], by: &str) -> Category {
    Category {
        handles: handles.iter().map(|h| h.to_string()).collect(),
        ..Category::new(id(i), name, by)
    }
}

fn people(session: &str, ids: &[&str], origin: Origin) -> MemberList {
    MemberList {
        session: session.into(),
        kind: "person".into(),
        members: ids
            .iter()
            .map(|i| Membership {
                category: id(i),
                origin,
                confidence: None,
                as_of_ms: 1_000,
            })
            .collect(),
    }
}

/// A DM's person from the transport, and two people the import made.
fn harbour() -> Ontology {
    let mut o = Ontology::seeded();
    o.put(
        Record::Category(person(
            "person:400000000000000001",
            "@quill",
            &[],
            "transport",
        )),
        TRANSPORT,
    )
    .unwrap();
    o.put(
        Record::Category(person(
            "person:marlo-quill",
            "Marlo Quill",
            &["slack:U0TIDE01", "name:Marlo Quill"],
            "import tide-2026",
        )),
        IMPORT,
    )
    .unwrap();
    o.put(
        Record::Category(person(
            "person:pell",
            "Pell",
            &["name:Pell"],
            "import tide-2026",
        )),
        IMPORT,
    )
    .unwrap();
    o
}

#[test]
fn the_person_kind_stays_given_and_gains_a_stored_side() {
    let k = seeds().into_iter().find(|k| k.name == "person").unwrap();
    assert!(k.is_given());
    assert_eq!(
        k.assigned_by,
        [Origin::Transport, Origin::Operator, Origin::Import]
    );
    assert!(k.stores());
    for name in ["guild", "channel"] {
        let k = seeds().into_iter().find(|k| k.name == name).unwrap();
        assert!(!k.stores(), "{name} keeps no stored side");
    }
    assert_eq!(k.check_row(), Ok(()));
}

#[test]
fn the_operator_and_the_import_declare_people_and_assign_them() {
    let mut o = harbour();
    o.put(
        Record::Category(person(
            "person:tamsin-reef",
            "Tamsin Reef",
            &[],
            "the operator",
        )),
        OP,
    )
    .unwrap();
    o.put(
        Record::Members(people(
            "ses_tide",
            &["person:marlo-quill", "person:pell"],
            IMPORT,
        )),
        IMPORT,
    )
    .unwrap();
    o.put(
        Record::Members(people("ses_reef", &["person:tamsin-reef"], OP)),
        OP,
    )
    .unwrap();
    assert_eq!(o.memberships("ses_tide").len(), 2);
    assert_eq!(o.member_counts()[&id("person:pell")], 1);
}

#[test]
fn a_stored_person_list_never_holds_the_transport_and_guilds_keep_none() {
    let o = harbour();
    let e = o
        .check(
            &Record::Members(people("ses_tide", &["person:pell"], TRANSPORT)),
            OP,
        )
        .unwrap_err();
    assert!(matches!(e, Refusal::Given { .. }), "{e}");
    let e = o
        .check(
            &Record::Members(people("ses_tide", &["person:pell"], IMPORT)),
            TRANSPORT,
        )
        .unwrap_err();
    assert!(matches!(e, Refusal::Writer { .. }), "{e}");
    let e = o
        .check(
            &Record::Category(Category::new(id("channel:9"), "deck", "the operator")),
            OP,
        )
        .unwrap_err();
    assert!(matches!(e, Refusal::Given { .. }), "{e}");
    let guild = MemberList {
        session: "ses_tide".into(),
        kind: "guild".into(),
        members: vec![],
    };
    let e = o.check(&Record::Members(guild), OP).unwrap_err();
    assert!(matches!(e, Refusal::Given { .. }), "{e}");
}

#[test]
fn handles_are_checked_and_only_a_person_carries_them() {
    assert_eq!(
        handle(" Email:Marlo@Harbour.Example ").unwrap(),
        "email:marlo@harbour.example"
    );
    assert_eq!(handle("slack:U0TIDE01").unwrap(), "slack:U0TIDE01");
    for bad in ["phone:5", "slack:", "nothing", "name:a\nb"] {
        assert!(handle(bad).is_err(), "{bad}");
    }
    let o = harbour();
    let mut t = Category::new(id("topic:tides"), "tides", "the operator");
    t.handles = vec!["name:tides".into()];
    assert!(o.check(&Record::Category(t), OP).is_err());
    let twice = person(
        "person:kit",
        "Kit",
        &["name:Kit", "name:Kit"],
        "the operator",
    );
    assert!(matches!(
        o.check(&Record::Category(twice), OP).unwrap_err(),
        Refusal::Duplicate { .. }
    ));
}

#[test]
fn two_people_never_share_a_handle_but_a_name() {
    let o = harbour();
    // An exact handle: refused, and the refusal says to merge.
    let e = o
        .check(
            &Record::Category(person(
                "person:mq",
                "MQ",
                &["slack:U0TIDE01"],
                "the operator",
            )),
            OP,
        )
        .unwrap_err();
    assert!(e.to_string().contains("merge"), "{e}");
    // The transport's DM person holds its discord id without writing it.
    let e = o
        .check(
            &Record::Category(person(
                "person:quill-again",
                "Quill",
                &["discord:400000000000000001"],
                "the operator",
            )),
            OP,
        )
        .unwrap_err();
    assert!(e.to_string().contains("person:400000000000000001"), "{e}");
    // A display name alone never makes two people one.
    o.check(
        &Record::Category(person(
            "person:pell-2",
            "Pell",
            &["name:Pell"],
            "the operator",
        )),
        OP,
    )
    .unwrap();
    assert_eq!(
        o.person_by_handle("discord:400000000000000001").unwrap().id,
        id("person:400000000000000001")
    );
    assert!(o.person_by_handle("name:Pell").is_none());
}

#[test]
fn a_merge_keeps_one_id_and_moves_memberships_handles_and_guidance() {
    let mut o = harbour();
    o.put(
        Record::Members(people(
            "ses_tide",
            &["person:marlo-quill", "person:pell"],
            IMPORT,
        )),
        IMPORT,
    )
    .unwrap();
    o.put(
        Record::Members(people("ses_both", &["person:marlo-quill"], IMPORT)),
        IMPORT,
    )
    .unwrap();
    o.put(
        Record::Guidance(Guidance::new(
            id("person:marlo-quill"),
            "Runs the tide survey; send soundings to the shared sheet.",
            1,
            "the operator",
        )),
        OP,
    )
    .unwrap();
    let dm = id("person:400000000000000001");
    let old_guidance = o.guidance(&id("person:marlo-quill")).unwrap().clone();
    let m = o.merge(&id("person:marlo-quill"), &dm).unwrap();
    assert_eq!(m.sessions, ["ses_both", "ses_tide"]);
    assert!(m.guidance.is_some());
    for r in m.records.clone() {
        o.put(r, OP).unwrap();
    }
    assert!(o.category(&id("person:marlo-quill")).is_none());
    let kept = o.category(&dm).unwrap();
    assert!(kept.handles.contains(&"slack:U0TIDE01".to_string()));
    assert!(kept.handles.contains(&"name:Marlo Quill".to_string()));
    assert_eq!(o.memberships("ses_tide")[0].category, dm);
    assert!(o.guidance(&dm).unwrap().text.starts_with("Runs the tide"));
    assert!(o.guidance(&id("person:marlo-quill")).is_none());

    // The load agrees: the merged record is no category, and its old
    // guidance is left unread, not warned of.
    let mut gone = m.absorbed.clone();
    gone.merged_into = Some(dm.clone());
    gone.handles.clear();
    let (again, dropped) = Ontology::load(
        o.records()
            .into_iter()
            .chain([Record::Category(gone), Record::Guidance(old_guidance)]),
    );
    assert!(dropped.is_empty(), "{dropped:?}");
    assert!(again.category(&id("person:marlo-quill")).is_none());

    // Undone from what it moved.
    for r in o
        .unmerge(
            &m.absorbed,
            &dm,
            &m.survivor_handles_before,
            &m.lists_before,
        )
        .unwrap()
    {
        o.put(r, OP).unwrap();
    }
    assert_eq!(
        o.category(&id("person:marlo-quill")).unwrap().handles,
        ["slack:U0TIDE01", "name:Marlo Quill"]
    );
    assert!(o.category(&dm).unwrap().handles.is_empty());
    assert_eq!(
        o.memberships("ses_both")[0].category,
        id("person:marlo-quill")
    );
}

#[test]
fn a_merge_refuses_what_it_cannot_do() {
    let mut o = harbour();
    let dm = id("person:400000000000000001");
    // The transport's person is read by its id: it survives, never goes.
    let e = o.merge(&dm, &id("person:pell")).unwrap_err();
    assert!(matches!(e, Refusal::Given { .. }), "{e}");
    let e = o.merge(&id("person:pell"), &id("person:pell")).unwrap_err();
    assert!(matches!(e, Refusal::Duplicate { .. }), "{e}");
    assert!(o.merge(&id("person:nobody"), &dm).is_err());
    for (i, text) in [
        ("person:pell", "Keeps the logbook."),
        ("person:marlo-quill", "Runs the survey."),
    ] {
        o.put(
            Record::Guidance(Guidance::new(id(i), text, 1, "the operator")),
            OP,
        )
        .unwrap();
    }
    let e = o
        .merge(&id("person:pell"), &id("person:marlo-quill"))
        .unwrap_err();
    assert!(matches!(e, Refusal::InUse { .. }), "{e}");
    // A merged record that a list still names is refused.
    o.put(
        Record::Members(people("ses_tide", &["person:pell"], IMPORT)),
        IMPORT,
    )
    .unwrap();
    let mut gone = o.category(&id("person:pell")).unwrap().clone();
    gone.merged_into = Some(dm);
    assert!(matches!(
        o.check(&Record::Category(gone), OP).unwrap_err(),
        Refusal::InUse { .. }
    ));
}
