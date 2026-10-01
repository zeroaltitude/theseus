//! The kinds table's validation, the category tree, the write rules, the
//! snapshot's load, and the composition's order. The golden renders and the
//! property tests are in `tests/`.

use super::*;

const OP: Origin = Origin::Operator;
const TRANSPORT: Origin = Origin::Transport;

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn cat(i: &str, name: &str, parent: Option<&str>) -> Category {
    Category {
        id: id(i),
        name: name.into(),
        parent: parent.map(id),
        description: String::new(),
        added_by: "eddie".into(),
    }
}

fn row(name: &str) -> Kind {
    seeds().into_iter().find(|k| k.name == name).unwrap()
}

/// The next version of a row, changed by `f`.
fn changed(mut k: Kind, f: impl FnOnce(&mut Kind)) -> Kind {
    k.version += 1;
    k.added_by = "eddie".into();
    f(&mut k);
    k
}

/// A new interpreted kind, assigned by the operator.
fn new_kind(name: &str, precedence: u32, parent: Option<&str>, rule: Rule) -> Kind {
    Kind {
        name: name.into(),
        basis: Basis::Interpreted,
        assigned_by: vec![Origin::Operator],
        per_session: PerSession::AtMost(2),
        parent: parent.map(str::to_string),
        precedence,
        rule,
        description: String::new(),
        version: 1,
        added_by: "eddie".into(),
    }
}

fn topic(o: &mut Ontology, i: &str, name: &str, parent: Option<&str>) {
    o.put(Record::Category(cat(i, name, parent)), OP).unwrap();
}

fn given(o: &mut Ontology, i: &str, name: &str, parent: Option<&str>) {
    o.put(Record::Category(cat(i, name, parent)), TRANSPORT)
        .unwrap();
}

fn guide(o: &mut Ontology, i: &str, text: &str) {
    let v = o.guidance(&id(i)).map_or(1, |g| g.version + 1);
    o.put(Record::Guidance(Guidance::new(id(i), text, v, "eddie")), OP)
        .unwrap();
}

fn list(session: &str, kind: &str, ids: &[&str]) -> MemberList {
    MemberList {
        session: session.into(),
        kind: kind.into(),
        members: ids
            .iter()
            .map(|i| Membership::operator(id(i), 1_000))
            .collect(),
    }
}

/// guild BigHat › channel general; people Eddie and Sam; topics
/// theseus › rust-harness, theseus › web, and cooking.
fn bighat() -> Ontology {
    let mut o = Ontology::seeded();
    given(&mut o, "guild:100000000000000001", "BigHat", None);
    given(
        &mut o,
        "channel:200000000000000001",
        "general",
        Some("guild:100000000000000001"),
    );
    given(&mut o, "person:300000000000000001", "Eddie", None);
    given(&mut o, "person:300000000000000002", "Sam", None);
    topic(&mut o, "topic:theseus", "theseus", None);
    topic(
        &mut o,
        "topic:rust-harness",
        "rust-harness",
        Some("topic:theseus"),
    );
    topic(&mut o, "topic:web", "web", Some("topic:theseus"));
    topic(&mut o, "topic:cooking", "cooking", None);
    o
}

fn refused(o: &Ontology, r: Record, by: Origin) -> Refusal {
    o.check(&r, by).expect_err("refused")
}

// The kinds table.

#[test]
fn the_seed_rows_check_and_come_in_precedence_order() {
    let o = Ontology::seeded();
    assert_eq!(o.check_all(), Ok(()));
    let names: Vec<&str> = o.kinds().iter().map(|k| k.name.as_str()).collect();
    assert_eq!(names, ["guild", "channel", "person", "topic"]);
    let t = o.kind("topic").unwrap();
    assert_eq!(
        (t.basis, t.per_session, t.parent.as_deref(), t.rule),
        (
            Basis::Interpreted,
            PerSession::AtMost(3),
            Some("topic"),
            Rule::Chain
        )
    );
    assert_eq!(o.kind("person").unwrap().rule, Rule::IntentLine);
    assert!(seeds().iter().all(|k| k.version == 1 && k.added_by == SEED));
}

#[test]
fn a_row_that_names_an_unbuilt_rule_is_refused_and_says_so() {
    let o = Ontology::seeded();
    for (rule, milestone) in [(Rule::Ranked, "M6"), (Rule::RecallOnly, "M6")] {
        let e = refused(
            &o,
            Record::Kind(changed(row("topic"), |k| k.rule = rule)),
            OP,
        );
        assert!(
            matches!(e, Refusal::UnbuiltRule { rule: r, .. } if r == rule),
            "{e:?}"
        );
        let msg = e.to_string();
        assert!(
            msg.contains("not built yet") && msg.contains(milestone),
            "{msg}"
        );
        let e = refused(&o, Record::Kind(new_kind("lesson", 50, None, rule)), OP);
        assert!(matches!(e, Refusal::UnbuiltRule { .. }), "{e:?}");
    }
    // The built ones check.
    for rule in [Rule::Chain, Rule::IntentLine] {
        assert_eq!(
            o.check(&Record::Kind(new_kind("culture", 50, None, rule)), OP),
            Ok(())
        );
    }
}

#[test]
fn a_row_that_names_an_unbuilt_origin_is_refused_and_says_so() {
    let o = Ontology::seeded();
    for (origin, milestone) in [
        (Origin::Jev, "M5"),
        (Origin::Sweep, "M6"),
        (Origin::Dream, "M6"),
    ] {
        let k = changed(row("topic"), |k| k.assigned_by.push(origin));
        let e = refused(&o, Record::Kind(k), OP);
        assert!(
            matches!(e, Refusal::UnbuiltOrigin { origin: got, .. } if got == origin),
            "{e:?}"
        );
        assert!(e.to_string().contains(milestone), "{e}");
    }
    // And nothing writes as one.
    let e = refused(
        &o,
        Record::Kind(new_kind("culture", 50, None, Rule::Chain)),
        Origin::Jev,
    );
    assert!(matches!(e, Refusal::UnbuiltOrigin { .. }), "{e:?}");
}

#[test]
fn a_given_rows_facts_are_the_transports_and_the_rest_is_the_operators() {
    type Change = fn(&mut Kind);
    let o = Ontology::seeded();
    let facts: [(&str, Change); 4] = [
        ("guild", |k| k.basis = Basis::Interpreted),
        ("guild", |k| k.assigned_by = vec![Origin::Operator]),
        ("channel", |k| k.parent = None),
        ("person", |k| k.per_session = PerSession::AtMost(1)),
    ];
    for (name, f) in facts {
        let e = refused(&o, Record::Kind(changed(row(name), f)), OP);
        assert!(matches!(e, Refusal::Given { .. }), "{name}: {e:?}");
        assert!(e.to_string().contains("transport's fact"), "{e}");
    }
    // Its precedence, rule, and description are the operator's.
    let mut o = o;
    o.put(
        Record::Kind(changed(row("guild"), |k| k.precedence = 5)),
        OP,
    )
    .unwrap();
    o.put(
        Record::Kind(changed(row("person"), |k| {
            k.rule = Rule::Chain;
            k.description = "Who is here.".into();
        })),
        OP,
    )
    .unwrap();
    assert_eq!(o.kind("guild").unwrap().version, 2);
}

#[test]
fn only_the_transports_three_kinds_are_given() {
    let o = Ontology::seeded();
    let mut thread = new_kind("thread", 50, None, Rule::Chain);
    thread.basis = Basis::Given;
    thread.assigned_by = vec![Origin::Transport];
    let e = refused(&o, Record::Kind(thread), OP);
    assert!(matches!(e, Refusal::Given { .. }), "{e:?}");
    assert!(
        e.to_string().contains("only guild, channel, and person"),
        "{e}"
    );
    // An interpreted kind takes no membership from the transport.
    let mut culture = new_kind("culture", 50, None, Rule::Chain);
    culture.assigned_by = vec![Origin::Transport, Origin::Operator];
    let e = refused(&o, Record::Kind(culture), OP);
    assert!(matches!(e, Refusal::Given { .. }), "{e:?}");
}

#[test]
fn each_kind_has_its_own_precedence_and_a_parent_kind_comes_first() {
    let o = Ontology::seeded();
    let e = refused(
        &o,
        Record::Kind(new_kind("culture", 40, None, Rule::Chain)),
        OP,
    );
    assert!(matches!(e, Refusal::Precedence { .. }), "{e:?}");
    assert!(
        e.to_string()
            .contains("`culture` and `topic` both have precedence 40"),
        "{e}"
    );
    // A guild after its channels would put the farther guidance later.
    let e = refused(
        &o,
        Record::Kind(changed(row("guild"), |k| k.precedence = 25)),
        OP,
    );
    assert!(matches!(e, Refusal::Precedence { .. }), "{e:?}");
    assert!(
        e.to_string()
            .contains("`channel`'s parent kind `guild` must come first"),
        "{e}"
    );
    let e = refused(
        &o,
        Record::Kind(new_kind("project", 35, Some("topic"), Rule::Chain)),
        OP,
    );
    assert!(matches!(e, Refusal::Precedence { .. }), "{e:?}");
    assert_eq!(
        o.check(
            &Record::Kind(new_kind("project", 45, Some("topic"), Rule::Chain)),
            OP
        ),
        Ok(())
    );
    let e = refused(
        &o,
        Record::Kind(new_kind("project", 45, Some("area"), Rule::Chain)),
        OP,
    );
    assert_eq!(
        e,
        Refusal::Missing {
            what: "parent kind",
            id: "area".into()
        }
    );
}

#[test]
fn a_cycle_in_the_kinds_parents_is_refused() {
    let mut o = Ontology::seeded();
    o.put(Record::Kind(new_kind("area", 50, None, Rule::Chain)), OP)
        .unwrap();
    o.put(
        Record::Kind(new_kind("project", 60, Some("area"), Rule::Chain)),
        OP,
    )
    .unwrap();
    let back = changed(o.kind("area").unwrap().clone(), |k| {
        k.parent = Some("project".into())
    });
    let e = refused(&o, Record::Kind(back), OP);
    assert_eq!(
        e,
        Refusal::Cycle {
            path: "area › project › area".into()
        }
    );
    // A kind that is its own parent nests; it is not a loop.
    assert_eq!(o.kind("topic").unwrap().parent.as_deref(), Some("topic"));
}

#[test]
fn a_change_is_one_version_more_than_the_row_it_supersedes() {
    let o = Ontology::seeded();
    let mut same = row("topic");
    same.description = "Again.".into();
    let e = refused(&o, Record::Kind(same), OP);
    assert_eq!(
        e,
        Refusal::Version {
            what: "kind `topic`".into(),
            want: 2,
            got: 1
        }
    );
    let mut skip = changed(row("topic"), |_| {});
    skip.version = 3;
    assert!(matches!(
        refused(&o, Record::Kind(skip), OP),
        Refusal::Version { want: 2, .. }
    ));
    let mut new = new_kind("culture", 50, None, Rule::Chain);
    new.version = 2;
    assert!(matches!(
        refused(&o, Record::Kind(new), OP),
        Refusal::Version { want: 1, .. }
    ));
}

#[test]
fn a_kind_change_must_leave_every_record_valid() {
    let mut o = bighat();
    guide(&mut o, "topic:cooking", "Metric units.\nGrams, not cups.");
    o.put(
        Record::Members(list(
            "s1",
            "topic",
            &["topic:theseus", "topic:web", "topic:cooking"],
        )),
        OP,
    )
    .unwrap();
    // Topics would stop nesting under nested topics.
    let e = refused(
        &o,
        Record::Kind(changed(row("topic"), |k| k.parent = None)),
        OP,
    );
    assert!(matches!(e, Refusal::WrongKind { .. }), "{e:?}");
    // A session holds three.
    let e = refused(
        &o,
        Record::Kind(changed(row("topic"), |k| {
            k.per_session = PerSession::AtMost(2)
        })),
        OP,
    );
    assert_eq!(
        e,
        Refusal::TooMany {
            kind: "topic".into(),
            max: 2,
            got: 3
        }
    );
    // Cooking's guidance is two lines.
    let e = refused(
        &o,
        Record::Kind(changed(row("topic"), |k| k.rule = Rule::IntentLine)),
        OP,
    );
    assert!(e.to_string().contains("one line per category"), "{e}");
    // Nothing above changed the snapshot.
    assert_eq!(o.kind("topic").unwrap(), &row("topic"));
}

#[test]
fn the_kinds_table_is_the_operators_to_write() {
    let o = Ontology::seeded();
    let e = refused(
        &o,
        Record::Kind(new_kind("culture", 50, None, Rule::Chain)),
        TRANSPORT,
    );
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
}

#[test]
fn a_rows_fields_are_checked() {
    let o = Ontology::seeded();
    let bad: [fn(&mut Kind); 5] = [
        |k| k.name = "Culture".into(),
        |k| k.assigned_by.clear(),
        |k| k.assigned_by.push(Origin::Operator),
        |k| k.per_session = PerSession::AtMost(0),
        |k| k.added_by = " eddie".into(),
    ];
    for f in bad {
        let mut k = new_kind("culture", 50, None, Rule::Chain);
        f(&mut k);
        assert!(o.check(&Record::Kind(k.clone()), OP).is_err(), "{k:?}");
    }
    let mut k = new_kind("culture", 50, None, Rule::Chain);
    k.description = "a\u{7}bell".into();
    assert!(matches!(
        refused(&o, Record::Kind(k), OP),
        Refusal::Invalid { .. }
    ));
}

#[test]
fn per_session_is_written_as_a_number_or_many() {
    let k = row("person");
    let v = serde_json::to_value(&k).unwrap();
    assert_eq!(v["per_session"], "many");
    assert_eq!(
        serde_json::to_value(row("topic")).unwrap()["per_session"],
        3
    );
    let back: Kind = serde_json::from_value(v).unwrap();
    assert_eq!(back, k);
    let mut few = serde_json::to_value(&k).unwrap();
    few["per_session"] = "few".into();
    let e = serde_json::from_value::<Kind>(few).unwrap_err().to_string();
    assert!(e.contains("a number or \"many\""), "{e}");
}

// The category tree.

#[test]
fn a_cycle_in_the_parents_is_refused() {
    let mut o = Ontology::seeded();
    topic(&mut o, "topic:a", "a", None);
    topic(&mut o, "topic:b", "b", Some("topic:a"));
    topic(&mut o, "topic:c", "c", Some("topic:b"));
    let e = refused(
        &o,
        Record::Category(cat("topic:a", "a", Some("topic:c"))),
        OP,
    );
    assert_eq!(
        e,
        Refusal::Cycle {
            path: "topic:a › topic:b › topic:c › topic:a".into()
        }
    );
    let e = refused(
        &o,
        Record::Category(cat("topic:a", "a", Some("topic:a"))),
        OP,
    );
    assert_eq!(
        e,
        Refusal::Cycle {
            path: "topic:a › topic:a".into()
        }
    );
    // Moving a category elsewhere is no loop.
    topic(&mut o, "topic:d", "d", None);
    o.put(Record::Category(cat("topic:b", "b", Some("topic:d"))), OP)
        .unwrap();
    let path: Vec<&str> = o
        .path(&id("topic:c"))
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(path, ["d", "b", "c"]);
    assert_eq!(o.children(Some(&id("topic:a"))), Vec::<&Category>::new());
}

#[test]
fn a_category_nests_only_under_its_kinds_parent_kind() {
    let o = bighat();
    let e = refused(
        &o,
        Record::Category(cat(
            "channel:200000000000000002",
            "random",
            Some("topic:theseus"),
        )),
        TRANSPORT,
    );
    assert!(e.to_string().contains("must be a guild category"), "{e}");
    let e = refused(
        &o,
        Record::Category(cat(
            "guild:100000000000000002",
            "Other",
            Some("guild:100000000000000001"),
        )),
        TRANSPORT,
    );
    assert!(
        e.to_string().contains("`guild` categories do not nest"),
        "{e}"
    );
    let e = refused(
        &o,
        Record::Category(cat("topic:x", "x", Some("topic:nope"))),
        OP,
    );
    assert_eq!(
        e,
        Refusal::Missing {
            what: "category",
            id: "topic:nope".into()
        }
    );
    let e = refused(&o, Record::Category(cat("culture:x", "x", None)), OP);
    assert_eq!(
        e,
        Refusal::Missing {
            what: "kind",
            id: "culture".into()
        }
    );
}

#[test]
fn categories_nest_at_most_eight_deep() {
    let mut o = Ontology::seeded();
    let ids: Vec<String> = (1..=9).map(|n| format!("topic:t{n}")).collect();
    for n in 0..8 {
        let parent = (n > 0).then(|| ids[n - 1].as_str());
        topic(&mut o, &ids[n], &format!("t{}", n + 1), parent);
    }
    let e = refused(&o, Record::Category(cat(&ids[8], "t9", Some(&ids[7]))), OP);
    assert!(
        matches!(
            e,
            Refusal::TooDeep {
                depth: 9,
                max: 8,
                ..
            }
        ),
        "{e:?}"
    );
    // A subtree moved under a deep one counts what hangs below it.
    topic(&mut o, "topic:x", "x", None);
    topic(&mut o, "topic:y", "y", Some("topic:x"));
    let e = refused(&o, Record::Category(cat("topic:x", "x", Some(&ids[6]))), OP);
    assert!(matches!(e, Refusal::TooDeep { depth: 9, .. }), "{e:?}");
    o.put(Record::Category(cat("topic:x", "x", Some(&ids[5]))), OP)
        .unwrap();
}

#[test]
fn an_interpreted_kinds_siblings_need_two_names_and_a_given_kinds_need_not() {
    let mut o = bighat();
    let e = refused(
        &o,
        Record::Category(cat("topic:theseus-2", "Theseus", None)),
        OP,
    );
    assert!(matches!(e, Refusal::Duplicate { .. }), "{e:?}");
    assert!(
        e.to_string().contains("`topic:theseus` is already named"),
        "{e}"
    );
    // The same name under another parent is another category.
    topic(&mut o, "topic:cooking-web", "web", Some("topic:cooking"));
    // Discord allows two channels of one name in a server.
    given(
        &mut o,
        "channel:200000000000000002",
        "general",
        Some("guild:100000000000000001"),
    );
    // Renaming a category keeps it its own sibling.
    o.put(
        Record::Category(cat("topic:web", "Web", Some("topic:theseus"))),
        OP,
    )
    .unwrap();
    let names: Vec<&str> = o
        .children(Some(&id("topic:theseus")))
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["Web", "rust-harness"]);
}

#[test]
fn given_categories_come_from_the_transport_and_topics_from_the_operator() {
    let o = bighat();
    let e = refused(
        &o,
        Record::Category(cat(
            "channel:200000000000000003",
            "ops",
            Some("guild:100000000000000001"),
        )),
        OP,
    );
    assert!(matches!(e, Refusal::Given { .. }), "{e:?}");
    let e = refused(&o, Record::Category(cat("topic:x", "x", None)), TRANSPORT);
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
}

#[test]
fn a_categorys_fields_are_checked() {
    let o = bighat();
    for name in ["", " theseus", "two\nlines", &"x".repeat(NAME_MAX + 1)] {
        let e = refused(&o, Record::Category(cat("topic:x", name, None)), OP);
        assert!(matches!(e, Refusal::Invalid { .. }), "{name:?}: {e:?}");
    }
    assert_eq!(
        o.check(
            &Record::Category(cat("topic:x", &"x".repeat(NAME_MAX), None)),
            OP
        ),
        Ok(())
    );
}

#[test]
fn a_category_id_is_its_kind_and_a_local_part() {
    let t = id("topic:rust-harness");
    assert_eq!((t.kind(), t.local()), ("topic", "rust-harness"));
    assert_eq!(id("person:300000000000000001").kind(), "person");
    for bad in [
        "topic",
        "topic:",
        "Topic:x",
        "topic:a b",
        "topic:a:b",
        ":x",
        "topic:é",
    ] {
        assert!(CategoryId::parse(bad).is_err(), "{bad:?}");
    }
    assert!(CategoryId::parse(&format!("topic:{}", "a".repeat(64))).is_ok());
    assert!(CategoryId::parse(&format!("topic:{}", "a".repeat(65))).is_err());
    let e = serde_json::from_str::<CategoryId>("\"topic\"").unwrap_err();
    assert!(e.to_string().contains("`<kind>:<local>`"), "{e}");
}

#[test]
fn a_minted_id_is_the_names_slug_and_the_next_free_one() {
    let mut o = Ontology::seeded();
    let a = o.mint_id("topic", "Rust Harness!").unwrap();
    assert_eq!(a.as_str(), "topic:rust-harness");
    topic(&mut o, a.as_str(), "Rust Harness!", None);
    let b = o.mint_id("topic", "rust harness").unwrap();
    assert_eq!(b.as_str(), "topic:rust-harness-2");
    assert_eq!(
        o.mint_id("topic", "日本").unwrap().as_str(),
        "topic:category"
    );
    let long = o.mint_id("topic", &"ab ".repeat(40)).unwrap();
    assert!(
        long.local().len() <= 48 && !long.local().ends_with('-'),
        "{long}"
    );
    assert!(o.mint_id("culture", "x").is_err());
}

// Guidance.

#[test]
fn guidance_is_one_version_more_and_its_digest_is_its_texts() {
    let mut o = bighat();
    let g = Guidance::new(
        id("topic:theseus"),
        "  Rust first.\r\nNo unsafe.  ",
        1,
        "eddie",
    );
    assert_eq!(g.text, "Rust first.\nNo unsafe.");
    assert_eq!(g.digest.len(), 16);
    o.put(Record::Guidance(g.clone()), OP).unwrap();
    let e = refused(&o, Record::Guidance(g.clone()), OP);
    assert!(
        matches!(
            e,
            Refusal::Version {
                want: 2,
                got: 1,
                ..
            }
        ),
        "{e:?}"
    );
    let mut forged = Guidance::new(id("topic:theseus"), "Other.", 2, "eddie");
    forged.digest = g.digest;
    assert!(matches!(
        refused(&o, Record::Guidance(forged), OP),
        Refusal::Invalid { .. }
    ));
    let e = refused(
        &o,
        Record::Guidance(Guidance::new(id("topic:nope"), "x", 1, "eddie")),
        OP,
    );
    assert!(
        matches!(
            e,
            Refusal::Missing {
                what: "category",
                ..
            }
        ),
        "{e:?}"
    );
    let e = refused(
        &o,
        Record::Guidance(Guidance::new(id("topic:web"), "x", 1, "eddie")),
        TRANSPORT,
    );
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
}

#[test]
fn an_intent_line_kinds_guidance_is_one_line() {
    let o = bighat();
    let eddie = id("person:300000000000000001");
    let e = refused(
        &o,
        Record::Guidance(Guidance::new(
            eddie.clone(),
            "The owner.\nTerse.",
            1,
            "eddie",
        )),
        OP,
    );
    assert!(e.to_string().contains("one line per category"), "{e}");
    let e = refused(
        &o,
        Record::Guidance(Guidance::new(
            eddie.clone(),
            &"x".repeat(INTENT_LINE_MAX + 1),
            1,
            "eddie",
        )),
        OP,
    );
    assert!(matches!(e, Refusal::Invalid { .. }), "{e:?}");
    assert_eq!(
        o.check(
            &Record::Guidance(Guidance::new(
                eddie,
                &"x".repeat(INTENT_LINE_MAX),
                1,
                "eddie"
            )),
            OP
        ),
        Ok(())
    );
}

#[test]
fn chain_guidance_is_prose_of_at_most_16_kib() {
    let o = bighat();
    let big = "x".repeat(GUIDANCE_MAX + 1);
    let e = refused(
        &o,
        Record::Guidance(Guidance::new(id("topic:theseus"), &big, 1, "eddie")),
        OP,
    );
    assert!(e.to_string().contains("16 KiB"), "{e}");
    let e = refused(
        &o,
        Record::Guidance(Guidance::new(
            id("topic:theseus"),
            "a\u{1b}[31m",
            1,
            "eddie",
        )),
        OP,
    );
    assert!(matches!(e, Refusal::Invalid { .. }), "{e:?}");
    assert_eq!(
        o.check(
            &Record::Guidance(Guidance::new(id("topic:theseus"), "a\n\tb", 1, "eddie")),
            OP
        ),
        Ok(())
    );
}

#[test]
fn empty_guidance_is_no_guidance() {
    let mut o = bighat();
    guide(&mut o, "topic:theseus", "Rust first.");
    guide(&mut o, "topic:theseus", "   ");
    let g = o.guidance(&id("topic:theseus")).unwrap();
    assert!(g.is_empty() && g.version == 2);
    let c = o.compose(&[Membership::operator(id("topic:theseus"), 1)]);
    assert!(c.sections.is_empty() && c.guidance.is_empty(), "{c:?}");
}

// Memberships.

#[test]
fn given_memberships_refuse_writes() {
    let o = bighat();
    for by in [OP, TRANSPORT] {
        let e = refused(
            &o,
            Record::Members(list("s1", "guild", &["guild:100000000000000001"])),
            by,
        );
        assert!(matches!(e, Refusal::Given { .. }), "{e:?}");
        assert!(
            e.to_string().contains("never stored, and cannot be set"),
            "{e}"
        );
    }
}

#[test]
fn a_session_holds_at_most_its_kinds_count() {
    let mut o = bighat();
    topic(&mut o, "topic:four", "four", None);
    let e = refused(
        &o,
        Record::Members(list(
            "s1",
            "topic",
            &["topic:theseus", "topic:web", "topic:cooking", "topic:four"],
        )),
        OP,
    );
    assert_eq!(
        e,
        Refusal::TooMany {
            kind: "topic".into(),
            max: 3,
            got: 4
        }
    );
}

#[test]
fn a_membership_is_of_its_lists_kind_from_an_origin_the_kind_allows() {
    let o = bighat();
    let with = |f: fn(&mut Membership)| {
        let mut l = list("s1", "topic", &["topic:theseus"]);
        f(&mut l.members[0]);
        refused(&o, Record::Members(l), OP)
    };
    assert!(matches!(
        with(|m| m.origin = Origin::Jev),
        Refusal::UnbuiltOrigin { .. }
    ));
    assert!(matches!(
        with(|m| m.origin = Origin::Transport),
        Refusal::Writer { .. }
    ));
    assert!(matches!(
        with(|m| m.confidence = Some(1.5)),
        Refusal::Invalid { .. }
    ));
    assert!(matches!(
        with(|m| m.category = CategoryId::parse("topic:nope").unwrap()),
        Refusal::Missing { .. }
    ));
    assert!(matches!(
        with(|m| m.category = CategoryId::parse("channel:200000000000000001").unwrap()),
        Refusal::WrongKind { .. }
    ));
    let e = refused(
        &o,
        Record::Members(list("s1", "topic", &["topic:web", "topic:web"])),
        OP,
    );
    assert!(matches!(e, Refusal::Duplicate { .. }), "{e:?}");
    let e = refused(&o, Record::Members(list("s:1", "topic", &[])), OP);
    assert!(matches!(e, Refusal::Invalid { .. }), "{e:?}");
}

#[test]
fn a_sessions_memberships_are_its_own_lists_and_an_empty_list_takes_them_away() {
    let mut o = bighat();
    o.put(Record::Kind(new_kind("culture", 50, None, Rule::Chain)), OP)
        .unwrap();
    topic(&mut o, "culture:terse", "terse", None);
    o.put(Record::Members(list("s1", "topic", &["topic:web"])), OP)
        .unwrap();
    o.put(
        Record::Members(list("s1", "culture", &["culture:terse"])),
        OP,
    )
    .unwrap();
    o.put(
        Record::Members(list("s10", "topic", &["topic:cooking"])),
        OP,
    )
    .unwrap();
    let got: Vec<String> = o
        .memberships("s1")
        .iter()
        .map(|m| m.category.to_string())
        .collect();
    assert_eq!(got, ["culture:terse", "topic:web"]);
    o.put(Record::Members(list("s1", "topic", &[])), OP)
        .unwrap();
    assert_eq!(o.memberships("s1").len(), 1);
    assert_eq!(o.memberships("s2"), Vec::<Membership>::new());
}

// Records, keys, and the load.

#[test]
fn every_record_round_trips_through_its_key_and_value() {
    let mut o = bighat();
    guide(&mut o, "topic:theseus", "Rust first.");
    o.put(Record::Members(list("s1", "topic", &["topic:web"])), OP)
        .unwrap();
    let records = o.records();
    assert_eq!(records.len(), 4 + 8 + 1 + 1);
    for r in &records {
        let key = r.key();
        assert!(key.starts_with(keys::PREFIX), "{key}");
        assert_eq!(Record::decode(&key, r.to_value()).as_ref(), Ok(r));
    }
    assert_eq!(keys::members("s1", "topic"), "onto:member:s1:topic");
    assert_eq!(keys::category(&id("topic:web")), "onto:cat:topic:web");
    let topic_row = Record::Kind(row("topic"));
    let e = Record::decode("onto:kind:guild", topic_row.to_value()).unwrap_err();
    assert!(
        e.to_string()
            .contains("holds the record of `onto:kind:topic`"),
        "{e}"
    );
    assert!(Record::decode("fomite:x", topic_row.to_value()).is_err());
    assert!(Record::decode("onto:cat:topic:x", serde_json::json!({"id": 3})).is_err());
    let ledger: Vec<&str> = [
        topic_row,
        Record::Category(cat("topic:x", "x", None)),
        Record::Guidance(Guidance::new(id("topic:x"), "x", 1, "e")),
        Record::Members(list("s", "topic", &[])),
    ]
    .iter()
    .map(Record::ledger_kind)
    .collect();
    assert_eq!(
        ledger,
        [
            "ontology.kind",
            "ontology.category",
            "ontology.guidance",
            "ontology.membership"
        ]
    );
}

#[test]
fn a_load_rebuilds_the_snapshot_from_records_in_any_order() {
    let mut o = bighat();
    o.put(
        Record::Kind(changed(row("topic"), |k| k.precedence = 45)),
        OP,
    )
    .unwrap();
    o.put(
        Record::Kind(new_kind("project", 50, Some("topic"), Rule::Chain)),
        OP,
    )
    .unwrap();
    topic(&mut o, "project:m4", "m4", Some("topic:rust-harness"));
    guide(&mut o, "topic:theseus", "Rust first.");
    guide(&mut o, "person:300000000000000001", "The owner.");
    o.put(
        Record::Members(list("s1", "topic", &["topic:web", "topic:cooking"])),
        OP,
    )
    .unwrap();
    let mut records = o.records();
    records.reverse();
    let (back, dropped) = Ontology::load(records);
    assert_eq!(dropped, vec![]);
    assert_eq!(back, o);
    // An empty store is the seeds.
    assert_eq!(Ontology::load(vec![]).0, Ontology::seeded());
}

#[test]
fn a_load_drops_what_does_not_check_and_says_why() {
    let mut ranked = changed(row("topic"), |k| k.rule = Rule::Ranked);
    ranked.description = "stored by a later build".into();
    let interpreted_guild = changed(row("guild"), |k| k.basis = Basis::Interpreted);
    let mut lists = list("s1", "topic", &["topic:a", "topic:gone"]);
    lists.members.push(Membership::operator(id("topic:a"), 2));
    let records = vec![
        Record::Kind(ranked),
        Record::Kind(interpreted_guild),
        Record::Category(cat("topic:a", "a", None)),
        Record::Category(cat("topic:x", "x", Some("topic:y"))),
        Record::Category(cat("topic:y", "y", Some("topic:x"))),
        Record::Category(cat("topic:orphan", "orphan", Some("topic:never"))),
        Record::Guidance(Guidance::new(id("topic:gone"), "x", 1, "eddie")),
        Record::Members(lists),
        Record::Members(list("s1", "guild", &[])),
    ];
    let (o, dropped) = Ontology::load(records);
    // The seed rows serve in their place.
    assert_eq!(o.kind("topic").unwrap(), &row("topic"));
    assert_eq!(o.kind("guild").unwrap(), &row("guild"));
    let why: Vec<(String, String)> = dropped
        .iter()
        .map(|(key, e)| {
            let kind = format!("{e:?}");
            (
                key.clone(),
                kind[..kind.find(' ').unwrap_or(kind.len())].to_string(),
            )
        })
        .collect();
    let expect = [
        ("onto:kind:guild", "Given"),
        ("onto:kind:topic", "UnbuiltRule"),
        ("onto:cat:topic:orphan", "Missing"),
        ("onto:cat:topic:x", "Cycle"),
        ("onto:cat:topic:y", "Cycle"),
        ("onto:guide:topic:gone", "Missing"),
        ("onto:member:s1:topic (topic:gone)", "Missing"),
        ("onto:member:s1:topic (topic:a)", "Duplicate"),
        ("onto:member:s1:guild", "Given"),
    ];
    let expect: Vec<(String, String)> = expect
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert_eq!(why, expect);
    let cycle = &dropped[3].1;
    assert_eq!(
        cycle.to_string(),
        "the parents loop: topic:x › topic:y › topic:x"
    );
    // What checked serves.
    assert!(o.category(&id("topic:a")).is_some());
    assert_eq!(o.memberships("s1").len(), 1);
    assert_eq!(o.check_all(), Ok(()));
}

#[test]
fn a_load_takes_a_parent_whose_key_sorts_after_its_child() {
    // A load takes each kind by name and each category by id, so these
    // parents come second, and a pass takes each child.
    let records = vec![
        Record::Kind(new_kind("area", 50, Some("zone"), Rule::Chain)),
        Record::Kind(new_kind("zone", 45, None, Rule::Chain)),
        Record::Category(cat("topic:a", "a", Some("topic:b"))),
        Record::Category(cat("topic:b", "b", Some("topic:c"))),
        Record::Category(cat("topic:c", "c", None)),
    ];
    let (o, dropped) = Ontology::load(records);
    assert_eq!(dropped, vec![]);
    assert_eq!(o.kind("area").unwrap().parent.as_deref(), Some("zone"));
    let path: Vec<&str> = o
        .path(&id("topic:a"))
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(path, ["c", "b", "a"]);
}

// The composition's order. The golden renders show the bytes.

fn sections_order(c: &Composition) -> Vec<String> {
    c.sections
        .iter()
        .map(|s| s.lines().next().unwrap().to_string())
        .collect()
}

fn everyone(o: &mut Ontology) -> Vec<Membership> {
    guide(o, "guild:100000000000000001", "Server rules.");
    guide(o, "channel:200000000000000001", "Channel rules.");
    guide(o, "person:300000000000000001", "The owner.");
    guide(o, "topic:theseus", "Rust first.");
    guide(o, "topic:rust-harness", "Tests before code.");
    vec![
        Membership::operator(id("topic:rust-harness"), 3),
        Membership::given(id("person:300000000000000001"), 2),
        Membership::given(id("channel:200000000000000001"), 2),
        Membership::given(id("guild:100000000000000001"), 2),
    ]
}

#[test]
fn the_precedence_order() {
    let mut o = bighat();
    let ms = everyone(&mut o);
    let c = o.compose(&ms);
    assert_eq!(
        sections_order(&c),
        [
            "# Guidance",
            "# Guidance (guild BigHat)",
            "# Guidance (channel BigHat › general)",
            "# Guidance (person)",
            "# Guidance (topic theseus)",
            "# Guidance (topic theseus › rust-harness)",
        ]
    );
    // Topic below the guild: its guidance comes first, and the rest governs.
    o.put(
        Record::Kind(changed(row("topic"), |k| k.precedence = 5)),
        OP,
    )
    .unwrap();
    // A new kind between person and topic's old place.
    o.put(
        Record::Kind(new_kind("culture", 35, None, Rule::IntentLine)),
        OP,
    )
    .unwrap();
    topic(&mut o, "culture:terse", "terse", None);
    guide(&mut o, "culture:terse", "Short answers.");
    let mut ms = ms;
    ms.push(Membership::operator(id("culture:terse"), 4));
    let c = o.compose(&ms);
    assert_eq!(
        sections_order(&c),
        [
            "# Guidance",
            "# Guidance (topic theseus)",
            "# Guidance (topic theseus › rust-harness)",
            "# Guidance (guild BigHat)",
            "# Guidance (channel BigHat › general)",
            "# Guidance (person)",
            "# Guidance (culture)",
        ]
    );
    let kinds: Vec<&str> = c.memberships.iter().map(|m| m.kind.as_str()).collect();
    assert_eq!(kinds, ["topic", "guild", "channel", "person", "culture"]);
}

#[test]
fn chain_admits_the_farthest_first_and_each_category_once() {
    let mut o = bighat();
    guide(&mut o, "topic:theseus", "Rust first.");
    guide(&mut o, "topic:web", "React.");
    guide(&mut o, "topic:rust-harness", "Tests before code.");
    let c = o.compose(&[
        Membership::operator(id("topic:web"), 1),
        Membership::operator(id("topic:rust-harness"), 1),
    ]);
    assert_eq!(
        sections_order(&c),
        [
            "# Guidance",
            "# Guidance (topic theseus)",
            "# Guidance (topic theseus › rust-harness)",
            "# Guidance (topic theseus › web)",
        ]
    );
    let used: Vec<&str> = c.guidance.iter().map(|g| g.category.as_str()).collect();
    assert_eq!(used, ["topic:theseus", "topic:rust-harness", "topic:web"]);
    // The ancestor is in play, but only the memberships are recorded as used.
    assert_eq!(c.memberships.len(), 2);
}

#[test]
fn intent_line_admits_one_line_per_category_and_no_ancestors() {
    let mut o = bighat();
    o.put(
        Record::Kind(new_kind("team", 50, Some("team"), Rule::IntentLine)),
        OP,
    )
    .unwrap();
    topic(&mut o, "team:eng", "eng", None);
    topic(&mut o, "team:infra", "infra", Some("team:eng"));
    guide(&mut o, "team:eng", "Engineering.");
    guide(&mut o, "team:infra", "Runs the machines.");
    guide(&mut o, "person:300000000000000001", "The owner.");
    guide(&mut o, "person:300000000000000002", "A guest.");
    let c = o.compose(&[
        Membership::operator(id("team:infra"), 1),
        Membership::given(id("person:300000000000000002"), 1),
        Membership::given(id("person:300000000000000001"), 1),
    ]);
    assert_eq!(
        c.sections[1..],
        [
            "# Guidance (person)\n\n- Eddie: The owner.\n- Sam: A guest.".to_string(),
            "# Guidance (team)\n\n- eng › infra: Runs the machines.".to_string(),
        ]
    );
}

#[test]
fn a_compose_skips_what_it_cannot_use_and_uses_the_rest() {
    let mut o = bighat();
    topic(&mut o, "topic:four", "four", None);
    guide(&mut o, "topic:cooking", "Metric units.");
    let mut jev = Membership::operator(id("topic:web"), 1);
    jev.origin = Origin::Jev;
    let ms = vec![
        Membership::operator(id("topic:nope"), 1),
        Membership::operator(id("guild:100000000000000001"), 1),
        Membership::given(id("topic:theseus"), 1),
        jev,
        Membership::operator(id("topic:cooking"), 1),
        Membership::operator(id("topic:cooking"), 2),
        Membership::operator(id("topic:four"), 1),
        Membership::operator(id("topic:rust-harness"), 1),
        Membership::operator(id("topic:web"), 1),
    ];
    let c = o.compose(&ms);
    let skipped: Vec<(&str, &str)> = c
        .skipped
        .iter()
        .map(|s| (s.membership.category.as_str(), s.why.as_str()))
        .collect();
    assert_eq!(
        skipped,
        [
            ("topic:nope", "no category `topic:nope`"),
            (
                "guild:100000000000000001",
                "`guild` memberships come from the session's place, not from `operator`"
            ),
            (
                "topic:theseus",
                "the transport gives only guild, channel, and person, not `topic`"
            ),
            ("topic:web", "`jev` may not assign `topic` memberships"),
            ("topic:cooking", "`topic:cooking` is listed more than once"),
            ("topic:web", "a session holds at most 3 `topic` categories"),
        ]
    );
    let used: Vec<&str> = c.memberships.iter().map(|m| m.category.as_str()).collect();
    assert_eq!(used, ["topic:cooking", "topic:four", "topic:rust-harness"]);
    assert_eq!(
        c.memberships[0].as_of_ms, 1,
        "the earlier of the two is kept"
    );
    assert_eq!(
        sections_order(&c),
        ["# Guidance", "# Guidance (topic cooking)"]
    );
}

#[test]
fn memberships_without_guidance_render_nothing_and_are_still_recorded() {
    let o = bighat();
    let c = o.compose(&[
        Membership::given(id("guild:100000000000000001"), 1),
        Membership::operator(id("topic:web"), 1),
    ]);
    assert_eq!(c.render(), "");
    assert!(c.sections.is_empty() && c.guidance.is_empty() && c.skipped.is_empty());
    assert_eq!(c.memberships.len(), 2);
    assert_eq!(o.compose(&[]), Composition::default());
}

#[test]
fn the_guidance_does_not_read_the_memberships_origin_confidence_or_as_of() {
    let mut o = bighat();
    let ms = everyone(&mut o);
    let base = o.compose(&ms);
    let mut other: Vec<Membership> = ms.iter().rev().cloned().collect();
    for (n, m) in other.iter_mut().enumerate() {
        m.as_of_ms = 9_000 + n as u64;
        m.confidence = Some(0.25);
    }
    let c = o.compose(&other);
    assert_eq!((&c.sections, &c.guidance), (&base.sections, &base.guidance));
}
