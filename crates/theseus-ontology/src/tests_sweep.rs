//! The sweep's plan and the pieces it stands on (theseus-63tfk): a general
//! category merge, a machine's merge and its undo, decay, the dream's first
//! guidance and its undo.

use super::*;

const OP: Origin = Origin::Operator;
const JEV: Origin = Origin::Jev;
const SWEEP: Origin = Origin::Sweep;
const DAY: u64 = 24 * 3_600_000;
const NOW: u64 = 100 * DAY;

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn cat(i: &str, name: &str, by: &str) -> Category {
    Category::new(id(i), name, by)
}

fn entry(i: &str, origin: Origin, as_of_ms: u64) -> Membership {
    Membership {
        category: id(i),
        origin,
        confidence: origin.is_machine().then_some(0.8),
        as_of_ms,
    }
}

fn list(session: &str, kind: &str, members: Vec<Membership>) -> Record {
    Record::Members(MemberList {
        session: session.into(),
        kind: kind.into(),
        members,
    })
}

fn put_all(o: &mut Ontology, records: &[Record], by: Origin) {
    for r in records {
        o.put(r.clone(), by).unwrap();
    }
}

/// Two topics Jev minted under two spellings of one name, each held by a
/// session of its own.
fn twins() -> Ontology {
    let mut o = Ontology::seeded();
    put_all(
        &mut o,
        &[
            Record::Category(cat("topic:rust-harness", "Rust Harness", "jev:j-1")),
            Record::Category(cat("topic:rust-harness-2", "rust-harness", "jev:j-2")),
            list("s1", "topic", vec![entry("topic:rust-harness", JEV, NOW)]),
            list("s2", "topic", vec![entry("topic:rust-harness-2", JEV, NOW)]),
        ],
        JEV,
    );
    o
}

fn merges(plan: &[Step]) -> Vec<(&CategoryId, &CategoryId)> {
    plan.iter()
        .filter_map(|s| match s {
            Step::Merge {
                absorbed, survivor, ..
            } => Some((absorbed, survivor)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_planted_duplicate_pair_merges_and_the_undo_restores_it() {
    let mut o = twins();
    let was = o.clone();
    let plan = o.sweep_plan(NOW);
    assert_eq!(
        merges(&plan),
        vec![(&id("topic:rust-harness-2"), &id("topic:rust-harness"))],
        "the later id folds into the earlier"
    );
    let Step::Merge {
        sessions, records, ..
    } = &plan[0]
    else {
        panic!("{plan:?}")
    };
    assert_eq!(sessions, &["s2".to_string()]);
    let before = o.replaced(records);
    put_all(&mut o, records, SWEEP);
    assert_eq!(o.category(&id("topic:rust-harness-2")), None);
    let moved = o.member_list("s2", "topic").unwrap();
    assert_eq!(moved.members.len(), 1);
    assert_eq!(moved.members[0].category, id("topic:rust-harness"));
    assert_eq!(
        moved.members[0].origin, SWEEP,
        "a moved entry is the sweep's"
    );
    assert_eq!(moved.members[0].confidence, Some(0.8));
    assert_eq!(moved.members[0].as_of_ms, NOW);
    // Nothing more to do once it is merged.
    assert!(o.sweep_plan(NOW).is_empty());

    let undo = o.undo(records, &before, NOW + 1).unwrap();
    put_all(&mut o, &undo, OP);
    assert_eq!(
        o.category(&id("topic:rust-harness-2")),
        was.category(&id("topic:rust-harness-2"))
    );
    assert_eq!(o.member_list("s2", "topic"), was.member_list("s2", "topic"));
    assert_eq!(o.member_list("s1", "topic"), was.member_list("s1", "topic"));
}

#[test]
fn an_undo_of_a_merge_is_refused_once_the_survivor_list_changed() {
    let mut o = twins();
    let plan = o.sweep_plan(NOW);
    let records = plan[0].records().to_vec();
    let before = o.replaced(&records);
    put_all(&mut o, &records, SWEEP);
    put_all(
        &mut o,
        &[list(
            "s2",
            "topic",
            vec![entry("topic:rust-harness", OP, NOW)],
        )],
        OP,
    );
    let e = o.undo(&records, &before, NOW + 1).unwrap_err();
    assert!(
        format!("{e:?}").contains("changed since the write"),
        "{e:?}"
    );
}

#[test]
fn the_operators_category_is_the_survivor_and_two_of_the_operators_are_left() {
    let mut o = Ontology::seeded();
    put_all(
        &mut o,
        &[
            Record::Category(cat("topic:billing", "Billing Ops", "jev:j-1")),
            Record::Category(cat("topic:invoices", "billing-ops", "ada")),
            list("s1", "topic", vec![entry("topic:billing", JEV, NOW)]),
        ],
        OP,
    );
    let plan = o.sweep_plan(NOW);
    assert_eq!(
        merges(&plan),
        vec![(&id("topic:billing"), &id("topic:invoices"))]
    );
    // Two the operator wrote are never merged by the sweep.
    let mut both = Ontology::seeded();
    put_all(
        &mut both,
        &[
            Record::Category(cat("topic:a", "Same Name", "ada")),
            // The same name under another parent is not a twin.
            Record::Category(Category {
                parent: Some(id("topic:a")),
                ..cat("topic:b", "same-name", "ada")
            }),
            Record::Category(cat("topic:c", "same_name", "ada")),
        ],
        OP,
    );
    assert!(both.sweep_plan(NOW).is_empty());
}

#[test]
fn a_category_with_guidance_children_or_an_operators_membership_is_not_merged() {
    let guide = |c: &str| Record::Guidance(Guidance::new(id(c), "Keep it short.", 1, "ada"));
    let base = || {
        let mut o = twins();
        put_all(
            &mut o,
            &[Record::Category(cat(
                "topic:rust-harness-3",
                "rust_harness",
                "jev:j-3",
            ))],
            JEV,
        );
        o
    };
    // The later two both fold into the first: three twins, two merges.
    assert_eq!(base().sweep_plan(NOW).len(), 2);

    let mut with_guidance = base();
    put_all(&mut with_guidance, &[guide("topic:rust-harness-2")], OP);
    assert_eq!(
        merges(&with_guidance.sweep_plan(NOW)),
        vec![(&id("topic:rust-harness-3"), &id("topic:rust-harness"))]
    );

    let mut with_child = base();
    put_all(
        &mut with_child,
        &[Record::Category(Category {
            parent: Some(id("topic:rust-harness-2")),
            ..cat("topic:sub", "Sub", "jev:j-4")
        })],
        JEV,
    );
    assert_eq!(
        merges(&with_child.sweep_plan(NOW)),
        vec![(&id("topic:rust-harness-3"), &id("topic:rust-harness"))]
    );

    let mut held = base();
    put_all(
        &mut held,
        &[list(
            "s9",
            "topic",
            vec![entry("topic:rust-harness-3", OP, NOW)],
        )],
        OP,
    );
    assert_eq!(
        merges(&held.sweep_plan(NOW)),
        vec![(&id("topic:rust-harness-2"), &id("topic:rust-harness"))]
    );
    // The general merge says why, for the absorbed one.
    let e = held
        .merge_category(
            &id("topic:rust-harness-3"),
            &id("topic:rust-harness"),
            SWEEP,
        )
        .unwrap_err();
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
}

#[test]
fn the_sweep_merges_nothing_an_operator_wrote_and_only_the_sweep_merges() {
    let o = twins();
    let mut taken = o.category(&id("topic:rust-harness-2")).unwrap().clone();
    taken.merged_into = Some(id("topic:rust-harness"));
    // Another machine origin may not.
    for by in [JEV, Origin::Dream] {
        let e = o.check(&Record::Category(taken.clone()), by).unwrap_err();
        assert!(matches!(e, Refusal::Writer { .. }), "{by}: {e:?}");
    }
    // The sweep, with the lists moved first, may.
    let m = o
        .merge_category(
            &id("topic:rust-harness-2"),
            &id("topic:rust-harness"),
            SWEEP,
        )
        .unwrap();
    let mut next = o.clone();
    put_all(&mut next, &m.records, SWEEP);
    // But not what the operator wrote.
    let mut ops = Ontology::seeded();
    put_all(
        &mut ops,
        &[
            Record::Category(cat("topic:x", "x", "ada")),
            Record::Category(cat("topic:y", "y", "jev:j-1")),
        ],
        OP,
    );
    let mut x = ops.category(&id("topic:x")).unwrap().clone();
    x.merged_into = Some(id("topic:y"));
    let e = ops.check(&Record::Category(x), SWEEP).unwrap_err();
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
    // Nor retire what a machine wrote.
    let y = ops.category(&id("topic:y")).unwrap().retired(5);
    let e = ops.check(&Record::Category(y), SWEEP).unwrap_err();
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
    // And a merge that changes more than merged_into is refused.
    let mut sneaky = taken;
    sneaky.name = "renamed".into();
    let e = o.check(&Record::Category(sneaky), SWEEP).unwrap_err();
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
}

#[test]
fn the_operator_merges_a_category_into_another_of_its_kind_only() {
    let mut o = Ontology::seeded();
    put_all(
        &mut o,
        &[
            Record::Category(cat("topic:a", "a", "ada")),
            Record::Category(cat("topic:b", "b", "ada")),
            Record::Category(cat("resource:r", "r", "ada")),
            list(
                "s1",
                "topic",
                vec![entry("topic:a", OP, 5), entry("topic:b", JEV, 6)],
            ),
        ],
        OP,
    );
    let m = o
        .merge_category(&id("topic:a"), &id("topic:b"), OP)
        .unwrap();
    put_all(&mut o, &m.records, OP);
    let l = o.member_list("s1", "topic").unwrap();
    assert_eq!(l.members.len(), 1, "the list held both: one entry now");
    assert_eq!(l.members[0].category, id("topic:b"));
    assert!(matches!(
        o.merge_category(&id("topic:b"), &id("resource:r"), OP),
        Err(Refusal::WrongKind { .. })
    ));
    assert!(matches!(
        o.merge_category(&id("topic:b"), &id("topic:b"), OP),
        Err(Refusal::Duplicate { .. })
    ));
    // A person merges with its handles, in person.rs.
    let mut p = Ontology::seeded();
    put_all(
        &mut p,
        &[
            Record::Category(cat("person:ada", "Ada", "ada")),
            Record::Category(cat("person:ada-2", "Ada", "ada")),
        ],
        OP,
    );
    assert!(matches!(
        p.merge_category(&id("person:ada-2"), &id("person:ada"), OP),
        Err(Refusal::WrongKind { .. })
    ));
}

#[test]
fn decay_drops_an_old_machine_membership_and_keeps_an_owners() {
    let mut o = Ontology::seeded();
    let old = NOW - 31 * DAY;
    let fresh = NOW - 29 * DAY;
    put_all(
        &mut o,
        &[
            Record::Category(cat("topic:web", "web", "ada")),
            Record::Category(cat("topic:ops", "ops", "ada")),
            Record::Category(cat("topic:sec", "sec", "ada")),
            Record::Category(cat("resource:repo", "repo", "ada")),
            list(
                "s1",
                "topic",
                vec![
                    entry("topic:web", OP, old),
                    entry("topic:ops", JEV, old),
                    entry("topic:sec", JEV, fresh),
                ],
            ),
            // A list of only stale machine entries empties.
            list("s2", "resource", vec![entry("resource:repo", JEV, old)]),
        ],
        OP,
    );
    let plan = o.sweep_plan(NOW);
    assert_eq!(plan.len(), 1);
    let Step::Decay { dropped, records } = &plan[0] else {
        panic!("{plan:?}")
    };
    assert_eq!(
        dropped,
        &vec![
            ("s1".to_string(), id("topic:ops")),
            ("s2".to_string(), id("resource:repo"))
        ]
    );
    let before = o.replaced(records);
    put_all(&mut o, records, SWEEP);
    let kept: Vec<_> = o
        .member_list("s1", "topic")
        .unwrap()
        .members
        .iter()
        .map(|m| (m.category.as_str().to_string(), m.origin))
        .collect();
    assert_eq!(
        kept,
        vec![
            ("topic:web".to_string(), OP),
            ("topic:sec".to_string(), JEV)
        ],
        "the owner's old one stays, the recent machine one stays"
    );
    assert!(o.member_list("s2", "resource").unwrap().members.is_empty());
    assert!(o.sweep_plan(NOW).is_empty());
    // And the undo puts both lists back as they were.
    let was = lists_of(&before);
    let undo = o.undo(records, &before, NOW + 1).unwrap();
    put_all(&mut o, &undo, OP);
    for (s, k) in [("s1", "topic"), ("s2", "resource")] {
        assert_eq!(o.member_list(s, k), was.get(&(s, k)).copied());
    }
}

/// The lists in `before`, by session and kind.
fn lists_of(before: &[Option<Record>]) -> std::collections::BTreeMap<(&str, &str), &MemberList> {
    before
        .iter()
        .flatten()
        .filter_map(|r| match r {
            Record::Members(l) => Some(((l.session.as_str(), l.kind.as_str()), l)),
            _ => None,
        })
        .collect()
}

#[test]
fn decay_leaves_a_list_the_sweep_may_not_write() {
    // `culture` is assigned by the operator and the dream: the sweep may not
    // write its lists, so an old dream entry stays for the operator to see.
    let mut o = Ontology::seeded();
    put_all(
        &mut o,
        &[
            Record::Category(cat("culture:manners", "manners", "ada")),
            list(
                "s1",
                "culture",
                vec![entry("culture:manners", Origin::Dream, 1)],
            ),
        ],
        OP,
    );
    assert!(o.sweep_plan(NOW).is_empty());
}

fn dream_guidance(c: &str, text: &str, version: u32) -> Record {
    Record::Guidance(Guidance::new(id(c), text, version, "dream:d-1"))
}

#[test]
fn the_dream_writes_a_categorys_first_guidance_and_the_undo_takes_it_back() {
    let mut o = Ontology::seeded();
    put_all(
        &mut o,
        &[Record::Category(cat("topic:web", "web", "ada"))],
        OP,
    );
    let g = dream_guidance("topic:web", &"x".repeat(400), 1);
    o.check(&g, Origin::Dream).unwrap();
    let wrote = [g];
    let before = o.replaced(&wrote);
    assert_eq!(before, vec![None]);
    put_all(&mut o, &wrote, Origin::Dream);
    assert_eq!(o.guidance(&id("topic:web")).unwrap().text.len(), 400);
    // A second one is refused: the dream writes only where none exists.
    let again = dream_guidance("topic:web", "other", 2);
    assert!(matches!(
        o.check(&again, Origin::Dream),
        Err(Refusal::Writer { .. })
    ));

    let undo = o.undo(&wrote, &before, NOW).unwrap();
    assert_eq!(undo.len(), 1);
    put_all(&mut o, &undo, OP);
    let now = o.guidance(&id("topic:web")).unwrap();
    assert!(now.is_empty());
    assert_eq!(now.version, 2, "an empty guidance, one more version");
    // With none left, the dream may write one again.
    o.check(&dream_guidance("topic:web", "again", 3), Origin::Dream)
        .unwrap();
}

#[test]
fn the_dreams_guidance_is_short_its_own_and_never_another_origins() {
    let mut o = Ontology::seeded();
    put_all(
        &mut o,
        &[Record::Category(cat("topic:web", "web", "ada"))],
        OP,
    );
    let long = dream_guidance("topic:web", &"x".repeat(401), 1);
    assert!(matches!(
        o.check(&long, Origin::Dream),
        Err(Refusal::Invalid { .. })
    ));
    let ok = dream_guidance("topic:web", "short", 1);
    for by in [JEV, SWEEP] {
        assert!(
            matches!(o.check(&ok, by), Err(Refusal::Writer { .. })),
            "{by}"
        );
    }
    let forged = Record::Guidance(Guidance::new(id("topic:web"), "short", 1, "ada"));
    assert!(matches!(
        o.check(&forged, Origin::Dream),
        Err(Refusal::Writer { .. })
    ));
    let empty = dream_guidance("topic:web", "", 1);
    assert!(matches!(
        o.check(&empty, Origin::Dream),
        Err(Refusal::Writer { .. })
    ));
    // The operator's guidance is never replaced by the dream's.
    put_all(
        &mut o,
        &[Record::Guidance(Guidance::new(
            id("topic:web"),
            "Mine.",
            1,
            "ada",
        ))],
        OP,
    );
    assert!(matches!(
        o.check(&dream_guidance("topic:web", "short", 2), Origin::Dream),
        Err(Refusal::Writer { .. })
    ));
}

#[test]
fn an_undo_of_a_kind_the_sweep_took_away_puts_it_back_one_version_on() {
    let mut o = Ontology::seeded();
    let k = Kind {
        name: "workflow".into(),
        basis: Basis::Interpreted,
        assigned_by: vec![Origin::Sweep, Origin::Jev],
        per_session: PerSession::AtMost(2),
        parent: None,
        precedence: 55,
        rule: Rule::Chain,
        description: String::new(),
        version: 1,
        added_by: "sweep:s-1".into(),
        retired_ms: None,
        examples: Vec::new(),
        mined_from: Vec::new(),
    };
    put_all(&mut o, &[Record::Kind(k.clone())], SWEEP);
    let gone = Kind {
        version: 2,
        retired_ms: Some(7),
        ..k
    };
    let wrote = [Record::Kind(gone)];
    let before = o.replaced(&wrote);
    put_all(&mut o, &wrote, SWEEP);
    assert!(o.kind("workflow").unwrap().retired_ms.is_some());
    let undo = o.undo(&wrote, &before, 9).unwrap();
    put_all(&mut o, &undo, OP);
    let back = o.kind("workflow").unwrap();
    assert_eq!((back.version, back.retired_ms), (3, None));
}
