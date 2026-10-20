//! The machine origins' writes (Jev, the sweep, the dream): what each may
//! write, that the operator wins on conflict, and the undo of a write.

use super::*;

const OP: Origin = Origin::Operator;
const JEV: Origin = Origin::Jev;

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn cat(i: &str, by: &str) -> Category {
    Category::new(id(i), i.split_once(':').unwrap().1, by)
}

fn entry(i: &str, origin: Origin) -> Membership {
    Membership {
        category: id(i),
        origin,
        confidence: (origin.is_machine()).then_some(0.8),
        as_of_ms: 1_000,
    }
}

fn list(session: &str, kind: &str, members: Vec<Membership>) -> MemberList {
    MemberList {
        session: session.into(),
        kind: kind.into(),
        members,
    }
}

/// Topics `web` and `ops`, minted by the operator, and a culture, `manners`.
fn base() -> Ontology {
    let mut o = Ontology::seeded();
    for i in ["topic:web", "topic:ops", "culture:manners"] {
        o.put(Record::Category(cat(i, "ada")), OP).unwrap();
    }
    o
}

fn refused(o: &Ontology, r: Record, by: Origin) -> Refusal {
    o.check(&r, by).expect_err("refused")
}

#[test]
fn jev_files_a_topic_list_and_mints_its_category() {
    let mut o = base();
    let minted = cat("topic:billing", "jev:j-0001");
    let l = list("s1", "topic", vec![entry("topic:billing", JEV)]);
    o.put(Record::Category(minted), JEV).unwrap();
    o.put(Record::Members(l.clone()), JEV).unwrap();
    assert_eq!(o.member_list("s1", "topic"), Some(&l));
    // And a person is filed by Jev, whose row names it.
    o.put(Record::Category(cat("person:ada-l", "jev:j-0001")), JEV)
        .unwrap();
    o.put(
        Record::Members(list("s1", "person", vec![entry("person:ada-l", JEV)])),
        JEV,
    )
    .unwrap();
}

#[test]
fn a_kind_that_does_not_name_jev_refuses_its_lists_and_categories() {
    let o = base();
    let e = refused(
        &o,
        Record::Members(list("s1", "culture", vec![entry("culture:manners", JEV)])),
        JEV,
    );
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
    let e = refused(&o, Record::Category(cat("culture:tone", "jev:j-1")), JEV);
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
    // The dream's kind does take the dream's.
    let c = cat("culture:tone", "dream:d-1");
    assert_eq!(o.check(&Record::Category(c), Origin::Dream), Ok(()));
}

#[test]
fn a_machine_category_names_its_writer_in_its_added_by() {
    let o = base();
    for bad in ["sweep:x", "jev", "jev:", "ada", "Jev:x"] {
        let e = refused(&o, Record::Category(cat("topic:new", bad)), JEV);
        assert!(matches!(e, Refusal::Writer { .. }), "{bad}: {e:?}");
    }
    assert_eq!(
        o.check(&Record::Category(cat("topic:new", "jev:x")), JEV),
        Ok(())
    );
    let long = format!("jev:{}", "x".repeat(ADDED_BY_MAX));
    assert!(o
        .check(&Record::Category(cat("topic:new", &long)), JEV)
        .is_err());
}

#[test]
fn a_machine_origin_does_not_supersede_or_retire_the_operators_category() {
    let o = base();
    let e = refused(&o, Record::Category(cat("topic:web", "jev:j-1")), JEV);
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
    let gone = cat("topic:web", "jev:j-1").retired(5);
    assert!(matches!(
        refused(&o, Record::Category(gone), JEV),
        Refusal::Writer { .. }
    ));
}

#[test]
fn the_operator_wins_on_conflict() {
    let mut o = base();
    let mine = entry("topic:web", OP);
    let theirs = entry("topic:ops", JEV);
    o.put(
        Record::Members(list("s1", "topic", vec![mine.clone(), theirs.clone()])),
        OP,
    )
    .unwrap();

    // A list that drops the operator's entry is refused, naming it.
    let drops = list("s1", "topic", vec![theirs]);
    let e = refused(&o, Record::Members(drops), JEV);
    assert!(
        matches!(&e, Refusal::Writer { why } if why.contains("topic:web")),
        "{e:?}"
    );
    // So is one that changes it.
    let changed = Membership {
        as_of_ms: 2_000,
        ..mine.clone()
    };
    let e = refused(
        &o,
        Record::Members(list("s1", "topic", vec![changed])),
        Origin::Sweep,
    );
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
    // One that keeps it and adds one is admitted, and drops its own.
    o.put(Record::Category(cat("topic:new", "jev:j-2")), JEV)
        .unwrap();
    let keeps = list("s1", "topic", vec![mine.clone(), entry("topic:new", JEV)]);
    o.put(Record::Members(keeps), JEV).unwrap();
    // It may not add an entry as the operator.
    let forged = list(
        "s1",
        "topic",
        vec![mine, entry("topic:new", JEV), entry("topic:ops", OP)],
    );
    assert!(matches!(
        refused(&o, Record::Members(forged), JEV),
        Refusal::Writer { .. }
    ));
    // The operator replaces anything, a machine entry included.
    let replaced = list("s1", "topic", vec![entry("topic:ops", OP)]);
    assert_eq!(o.check(&Record::Members(replaced), OP), Ok(()));
}

#[test]
fn the_imports_entries_are_kept_too() {
    let mut o = base();
    let imported = entry("topic:web", Origin::Import);
    o.put(
        Record::Members(list("s1", "topic", vec![imported])),
        Origin::Import,
    )
    .unwrap();
    let e = refused(
        &o,
        Record::Members(list("s1", "topic", vec![])),
        Origin::Dream,
    );
    assert!(matches!(e, Refusal::Writer { .. }), "{e:?}");
}

/// What a machine write replaced, as the core records it.
fn written(o: &Ontology, wrote: &[Record]) -> Vec<Option<Record>> {
    o.replaced(wrote)
}

fn apply_all(o: &mut Ontology, records: &[Record], by: Origin) {
    for r in records {
        o.put(r.clone(), by).unwrap();
    }
}

#[test]
fn an_undo_restores_the_earlier_list_and_retires_what_the_write_minted() {
    let mut o = base();
    let earlier = Record::Members(list("s1", "topic", vec![entry("topic:ops", JEV)]));
    apply_all(&mut o, &[earlier], JEV);

    let wrote = [
        Record::Category(cat("topic:billing", "jev:j-0002")),
        Record::Members(list("s1", "topic", vec![entry("topic:billing", JEV)])),
    ];
    let before = written(&o, &wrote);
    assert_eq!(before[0], None);
    let bytes = |r: &Record| serde_json::to_vec(&r.to_value()).unwrap();
    let was = bytes(before[1].as_ref().unwrap());
    apply_all(&mut o, &wrote, JEV);

    let undo = o.undo(&wrote, &before, 9_000).unwrap();
    assert_eq!(undo.len(), 2);
    let list_back = undo
        .iter()
        .find(|r| matches!(r, Record::Members(_)))
        .unwrap();
    assert_eq!(bytes(list_back), was, "the earlier list, byte for byte");
    let cat_back = undo.iter().find_map(|r| match r {
        Record::Category(c) => Some(c),
        _ => None,
    });
    assert_eq!(cat_back.unwrap().retired_ms, Some(9_000));
    // The inverse applies as the operator's write.
    apply_all(&mut o, &undo, OP);
    assert_eq!(
        o.member_list("s1", "topic"),
        Some(&list("s1", "topic", vec![entry("topic:ops", JEV)]))
    );
    assert_eq!(o.category(&id("topic:billing")), None);
}

#[test]
fn an_undo_of_a_mint_another_session_now_uses_is_refused() {
    let mut o = base();
    let wrote = [
        Record::Category(cat("topic:billing", "jev:j-0002")),
        Record::Members(list("s1", "topic", vec![entry("topic:billing", JEV)])),
    ];
    let before = written(&o, &wrote);
    apply_all(&mut o, &wrote, JEV);
    // Another session files into it afterwards.
    o.put(
        Record::Members(list("s2", "topic", vec![entry("topic:billing", JEV)])),
        JEV,
    )
    .unwrap();
    let e = o.undo(&wrote, &before, 9_000).unwrap_err();
    assert!(matches!(e, Refusal::InUse { .. }), "{e:?}");
}

#[test]
fn an_undo_that_does_not_pair_its_records_is_refused() {
    let o = base();
    assert!(o
        .undo(&[Record::Category(cat("topic:x", "jev:j"))], &[], 1)
        .is_err());
}

#[test]
fn an_undo_takes_back_a_kind_the_sweep_added() {
    let mut o = base();
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
    let wrote = [Record::Kind(k)];
    let before = written(&o, &wrote);
    apply_all(&mut o, &wrote, Origin::Sweep);
    let undo = o.undo(&wrote, &before, 9_000).unwrap();
    apply_all(&mut o, &undo, OP);
    assert_eq!(o.kind("workflow").unwrap().retired_ms, Some(9_000));
}
