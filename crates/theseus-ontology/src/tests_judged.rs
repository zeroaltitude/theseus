//! What the sweep may ask the model about (theseus-nzzz4): the pairs that
//! look alike, the categories large enough to split, and the split's records.

use super::judged::SPLIT_AT;
use super::*;

const JEV: Origin = Origin::Jev;
const OP: Origin = Origin::Operator;

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn cat(i: &str, name: &str, by: &str) -> Record {
    Record::Category(Category::new(id(i), name, by))
}

fn held(session: &str, i: &str, origin: Origin) -> Record {
    Record::Members(MemberList {
        session: session.into(),
        kind: "topic".into(),
        members: vec![Membership {
            category: id(i),
            origin,
            confidence: origin.is_machine().then_some(0.9),
            as_of_ms: 1_000,
        }],
    })
}

fn put_all(o: &mut Ontology, records: &[Record], by: Origin) {
    for r in records {
        o.put(r.clone(), by).unwrap();
    }
}

#[test]
fn a_pair_is_two_machine_categories_that_share_a_word_or_hold_a_name() {
    let mut o = Ontology::seeded();
    put_all(
        &mut o,
        &[
            cat("topic:release-process", "release process", "jev:j-1"),
            cat("topic:release-checklist", "release checklist", "jev:j-2"),
            cat("topic:voice-echo", "voice echo", "jev:j-3"),
            cat("topic:echo-cancel", "echo cancel", "jev:j-4"),
            cat("topic:ledger", "ledger", "jev:j-5"),
            cat("topic:ledger-import", "ledger import", "jev:j-6"),
            cat("topic:garden", "garden notes", "jev:j-7"),
            // Folded to one name: the deterministic plan's, not a question.
            cat("topic:rust-harness", "Rust Harness", "jev:j-8"),
            cat("topic:rust-harness-2", "rust-harness", "jev:j-9"),
        ],
        JEV,
    );
    let pairs = o.merge_pairs();
    let has = |a: &str, b: &str| pairs.contains(&(id(a), id(b)));
    assert!(has("topic:release-checklist", "topic:release-process"));
    assert!(
        has("topic:echo-cancel", "topic:voice-echo"),
        "a shared word"
    );
    assert!(
        has("topic:ledger", "topic:ledger-import"),
        "one name holds the other"
    );
    assert!(!pairs
        .iter()
        .any(|(a, b)| a.as_str().contains("garden") || b.as_str().contains("garden")));
    assert!(
        !has("topic:rust-harness", "topic:rust-harness-2"),
        "folded names are the plan's"
    );
}

#[test]
fn a_category_with_guidance_or_children_or_the_operators_is_not_paired() {
    let mut o = Ontology::seeded();
    o.put(cat("topic:release-notes", "release notes", "operator"), OP)
        .unwrap();
    put_all(
        &mut o,
        &[
            cat("topic:release-process", "release process", "jev:j-1"),
            cat("topic:release-plan", "release plan", "jev:j-2"),
        ],
        JEV,
    );
    assert_eq!(
        o.merge_pairs(),
        vec![(id("topic:release-plan"), id("topic:release-process"))],
        "the operator's `release notes` is left out"
    );
    o.put(
        Record::Guidance(Guidance::new(
            id("topic:release-plan"),
            "Plans are short.",
            1,
            "operator:x",
        )),
        OP,
    )
    .unwrap();
    assert!(o.merge_pairs().is_empty(), "guidance holds a category");
}

#[test]
fn a_pair_merges_by_the_one_survivor_rule() {
    let mut o = Ontology::seeded();
    put_all(
        &mut o,
        &[
            cat("topic:release-process", "release process", "jev:j-1"),
            cat("topic:release-checklist", "release checklist", "jev:j-2"),
            held("s1", "topic:release-process", JEV),
            held("s2", "topic:release-checklist", JEV),
            held("s3", "topic:release-checklist", JEV),
        ],
        JEV,
    );
    let step = o
        .plan_pair_merge(&id("topic:release-process"), &id("topic:release-checklist"))
        .expect("a merge");
    let Step::Merge {
        absorbed,
        survivor,
        sessions,
        ..
    } = &step
    else {
        panic!("{step:?}")
    };
    assert_eq!(
        (absorbed, survivor),
        (&id("topic:release-process"), &id("topic:release-checklist"))
    );
    assert_eq!(sessions, &["s1".to_string()]);
    assert!(o.category(&id("topic:release-process")).is_none());
}

fn large(o: &mut Ontology, by: &str, n: u64) {
    let mut records = vec![cat("topic:lantern", "lantern", by)];
    for i in 0..n {
        records.push(held(&format!("s{i:02}"), "topic:lantern", JEV));
    }
    if by.starts_with("jev") {
        put_all(o, &records, JEV);
    } else {
        put_all(o, &records, OP);
    }
}

#[test]
fn a_machine_category_of_twelve_sessions_is_a_split_candidate() {
    let mut o = Ontology::seeded();
    large(&mut o, "jev:j-1", SPLIT_AT - 1);
    assert!(o.split_candidates().is_empty(), "eleven are not enough");
    large(&mut o, "jev:j-1", SPLIT_AT);
    assert_eq!(o.split_candidates(), vec![id("topic:lantern")]);
    // The operator's is never one, and only a kind that nests has any.
    let mut o = Ontology::seeded();
    large(&mut o, "operator", SPLIT_AT);
    assert!(o.split_candidates().is_empty());
}

#[test]
fn a_split_moves_the_machines_memberships_and_leaves_the_operators() {
    let mut o = Ontology::seeded();
    large(&mut o, "jev:j-1", SPLIT_AT);
    // One session's membership is the operator's own.
    put_all(
        &mut o,
        &[held("s00", "topic:lantern", Origin::Operator)],
        OP,
    );
    let first: Vec<String> = ["s00", "s01", "s02"].map(String::from).to_vec();
    let second: Vec<String> = ["s03", "s04"].map(String::from).to_vec();
    let step = o
        .plan_split(
            &id("topic:lantern"),
            ["Ledger", "Beacon"],
            [&first, &second],
            "sweep-1-0-split",
        )
        .expect("a split");
    let Step::Split {
        children, sessions, ..
    } = &step
    else {
        panic!("{step:?}")
    };
    assert_eq!(sessions.len(), 4, "the operator's s00 stays: {sessions:?}");
    let [one, two] = children;
    assert_eq!(o.category(one).unwrap().parent, Some(id("topic:lantern")));
    assert_eq!(o.category(one).unwrap().added_by, "sweep:sweep-1-0-split");
    assert_eq!(
        o.member_list("s01", "topic").unwrap().members[0].category,
        *one
    );
    assert_eq!(
        o.member_list("s03", "topic").unwrap().members[0].category,
        *two
    );
    assert_eq!(
        o.member_list("s00", "topic").unwrap().members[0].category,
        id("topic:lantern")
    );
    assert_eq!(
        o.member_list("s09", "topic").unwrap().members[0].category,
        id("topic:lantern"),
        "a session not named stays in the parent"
    );
    // A child with no session to move into it is not made.
    let mut o2 = Ontology::seeded();
    large(&mut o2, "jev:j-1", SPLIT_AT);
    assert!(o2
        .plan_split(
            &id("topic:lantern"),
            ["Ledger", "Beacon"],
            [&first, &[]],
            "sweep-1-0-split"
        )
        .is_none());
}
