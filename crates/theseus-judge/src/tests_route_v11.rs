//! `route.v11`: `route.v10` with the `fable` option's description saying Fable 5.2 and its prices (the owner,
//! 2026-11-14, "1 A; 2 A"), and nothing else. Both are retired now (issue 46), their files kept as they were,
//! and v10's cases are read for each.

use crate::pack::Pack;
use crate::route_cases::{CaseSet, V10};

fn means_of(p: &Pack) -> String {
    p.question("model")
        .expect("route packs ask `model`")
        .options
        .iter()
        .find(|o| o.id == "fable")
        .expect("`model` has a fable option")
        .means
        .clone()
        .expect("an option says what it means")
}

/// v11 is v10 but for the version and one option's words: every question, option, bar, rollback rule and
/// description of v10, equal, once that option's description is v11's.
#[test]
fn v11_is_v10_but_for_the_fable_options_words() {
    let v10 = crate::pack::retired("route.v10").expect("route.v10 is retired, its file kept");
    let v11 = crate::pack::retired("route.v11").expect("route.v11 is retired, its file kept");
    assert_eq!((v10.version, v11.version), (10, 11));
    let mut planted = (*v10).clone();
    planted.version = 11;
    // The file's own hash is the one field that is its text, not what it says.
    planted.sha256 = v11.sha256.clone();
    for q in planted.questions.iter_mut().filter(|q| q.id == "model") {
        for o in q.options.iter_mut().filter(|o| o.id == "fable") {
            o.means = Some(means_of(&v11));
        }
    }
    assert_eq!(planted, *v11);
    assert_ne!(means_of(&v10), means_of(&v11));
}

/// The two descriptions: v10's is untouched (it still describes the model it did, under the prices it
/// had), and v11's names Fable 5.2 at the catalog's prices ($8 in, $40 out, $0.20 a cache read) and
/// nothing of 5.1.
#[test]
fn v10_still_says_fable_5_1_and_v11_says_5_2_at_its_prices() {
    let old = means_of(&crate::pack::retired("route.v10").unwrap());
    assert!(
        old.starts_with("Fable 5.1 (Anthropic, US), the most capable: $10 in, $50 out, $0.25 a cache read per million."),
        "{old}"
    );
    let new = means_of(&crate::pack::retired("route.v11").unwrap());
    assert!(
        new.starts_with("Fable 5.2 (Anthropic, US), the most capable: $8 in, $40 out, $0.20 a cache read per million."),
        "{new}"
    );
    assert!(
        !new.contains("5.1") && !new.contains("$10") && !new.contains("$50"),
        "{new}"
    );
}

/// The labelled cases are written for v10 and read for v11 as they are for v10: the same file, the same
/// cases, no label touched. Another route pack's file is still refused for v11.
#[test]
fn v10s_cases_are_read_for_v11_as_they_are_for_v10() {
    let a = CaseSet::parse(V10, &crate::pack::retired("route.v10").unwrap()).unwrap();
    let b = CaseSet::parse(V10, &crate::pack::retired("route.v11").unwrap()).unwrap();
    assert_eq!(a.cases.len(), b.cases.len());
    assert!(a.cases.len() >= 62);
    assert_eq!(
        a.cases.iter().map(|c| (&c.id, &c.want)).collect::<Vec<_>>(),
        b.cases.iter().map(|c| (&c.id, &c.want)).collect::<Vec<_>>()
    );
    let v8 = V10.replace("pack = \"route.v12\"", "pack = \"route.v8\"");
    let e = CaseSet::parse(&v8, &crate::pack::retired("route.v11").unwrap()).unwrap_err();
    assert!(e.contains("written for route.v8, not route.v11"), "{e}");
}
