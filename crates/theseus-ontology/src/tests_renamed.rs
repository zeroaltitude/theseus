//! The sweep's rename (theseus-63tfk.1): the write, what it keeps, and its undo.

use super::*;

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn cat(i: &str, name: &str, by: &str) -> Record {
    Record::Category(Category::new(id(i), name, by))
}

fn ontology() -> Ontology {
    let mut o = Ontology::seeded();
    for r in [
        cat("topic:old-name", "old name", "jev:j-1"),
        cat("topic:other", "other thing", "jev:j-2"),
        cat("topic:mine", "operator topic", "ada"),
    ] {
        o.put(r, Origin::Operator).unwrap();
    }
    o
}

#[test]
fn a_rename_keeps_the_id_and_the_old_name_in_the_description_and_undoes() {
    let mut o = ontology();
    let before = o.replaced(&[Record::Category(
        o.category(&id("topic:old-name")).unwrap().clone(),
    )]);
    let step = o
        .plan_rename(&id("topic:old-name"), "Lantern Ledger", "sweep-1-0-rename")
        .unwrap();
    let c = o.category(&id("topic:old-name")).unwrap();
    assert_eq!(c.name, "Lantern Ledger");
    assert_eq!(c.description, "was: old name");
    assert_eq!(c.added_by, "sweep:sweep-1-0-rename");
    assert_eq!(step.word(), "rename");
    // The undo is the record as it was, byte for byte.
    let mut written = ontology();
    for r in step.records() {
        written.put(r.clone(), Origin::Sweep).unwrap();
    }
    let inverse = written.undo(step.records(), &before, 9).unwrap();
    assert_eq!(inverse, vec![before[0].clone().unwrap()]);
}

#[test]
fn only_a_loose_machine_category_is_renamed_and_never_to_itself() {
    let mut o = ontology();
    assert!(o.rename_candidates().contains(&id("topic:old-name")));
    assert!(!o.rename_candidates().contains(&id("topic:mine")));
    assert!(o.plan_rename(&id("topic:mine"), "Else", "w").is_none());
    assert!(o
        .plan_rename(&id("topic:old-name"), "Old-Name", "w")
        .is_none());
    assert!(o.plan_rename(&id("topic:old-name"), "  ", "w").is_none());
    // A sibling's name is not taken.
    assert!(o
        .plan_rename(&id("topic:old-name"), "other thing", "w")
        .is_none());
}
