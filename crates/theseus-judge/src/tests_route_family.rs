//! The route family's case sets (theseus-h66v4.6): `work.v1`'s and `split.v1`'s own, and the model
//! file's `effort` labels graded by `effort.v1`. Each set parses against the pack it is for, is
//! held to the counts its header states, and agrees with `route.v10.cases.toml` about every
//! message they share, so the sets cannot drift apart unseen.

use std::collections::BTreeMap;

use crate::pack::by_name;
use crate::route_cases::{bar_need, graded_questions, CaseSet, SPLIT, V10, WORK};

fn parse(text: &str, pack: &str) -> CaseSet {
    let p = by_name(pack).unwrap_or_else(|| panic!("{pack} is embedded"));
    CaseSet::parse(text, &p).unwrap_or_else(|e| panic!("{pack}: {e}"))
}

fn counts(set: &CaseSet, q: &str) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for c in &set.cases {
        *out.entry(c.want.of(q).to_string()).or_default() += 1;
    }
    out
}

#[test]
fn each_pack_grades_the_questions_it_decides() {
    let q = |p: &str| graded_questions(&by_name(p).unwrap());
    assert_eq!(q("route.v10"), ["model"]);
    assert_eq!(q("effort.v1"), ["effort"]);
    assert_eq!(q("work.v1"), ["work"]);
    assert_eq!(q("split.v1"), ["split"]);
    assert!(
        q("route.v8").is_empty(),
        "route.v8 decides `mode`, which no file labels"
    );
}

#[test]
fn the_model_file_grades_the_effort_too_and_no_other_pack_may_read_it() {
    let effort = parse(V10, "effort.v1");
    let model = parse(V10, "route.v10");
    assert_eq!(effort.cases.len(), 61);
    assert_eq!(
        effort.cases, model.cases,
        "one file, two packs, the same cases"
    );
    // Never graded by a pack the file is not for.
    let work = by_name("work.v1").unwrap();
    assert!(CaseSet::parse(V10, &work)
        .unwrap_err()
        .contains("written for route.v10"));
    // The effort bar over 61 within one rung needs 49: the ratio, not a number kept.
    assert_eq!(bar_need(effort.cases.len() as u32), 49);
}

#[test]
fn the_work_set_is_sixty_five_cases_over_six_kinds_and_agrees_with_the_model_set() {
    let work = parse(WORK, "work.v1");
    assert_eq!(work.cases.len(), 65);
    let by = counts(&work, "work");
    let n = |k: &str| by[k];
    assert_eq!(
        (
            n("coding"),
            n("research_or_design"),
            n("writing"),
            n("conversation"),
            n("tool_run"),
            n("review")
        ),
        (12, 11, 9, 16, 9, 8)
    );
    // Every kind is tested by enough cases that a bar over the whole is not one kind's.
    assert!(by.values().all(|&c| c >= 8));
    // 80% of 65, rounded up.
    assert_eq!(bar_need(work.cases.len() as u32), 52);
    // A message the two sets share is the same message, in the same place, in both.
    let model = parse(V10, "route.v10");
    let mut shared = 0;
    for c in &work.cases {
        if let Some(m) = model.cases.iter().find(|m| m.id == c.id) {
            shared += 1;
            assert_eq!(c.message, m.message, "{}", c.id);
            assert_eq!(c.state.place, m.state.place, "{}", c.id);
            assert_eq!(c.state.place_kind, m.state.place_kind, "{}", c.id);
        } else {
            assert!(c.id.starts_with('K'), "{} is new, so it is a K-case", c.id);
        }
    }
    assert!(shared >= 35, "{shared} messages are the model set's own");
    // No two cases ask the same message.
    let mut msgs: Vec<&str> = work.cases.iter().map(|c| c.message.as_str()).collect();
    msgs.sort_unstable();
    msgs.dedup();
    assert_eq!(msgs.len(), work.cases.len());
}

#[test]
fn the_work_state_carries_the_live_tasks_of_a_case() {
    let work = parse(WORK, "work.v1");
    let pack = by_name("work.v1").unwrap();
    let c = work.cases.iter().find(|c| c.id == "K27").unwrap();
    assert_eq!(c.state.live_tasks.len(), 1);
    let v = crate::route_cases::prepared(&pack, c).state.value();
    assert_eq!(v["live_tasks"][0]["id"], "t-gate");
    let mut keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["live_tasks", "message", "place_kind"]);
}

#[test]
fn the_split_set_is_sixty_two_cases_and_its_core_is_the_deleted_cues_forty() {
    let split = parse(SPLIT, "split.v1");
    assert_eq!(split.cases.len(), 62);
    let by = counts(&split, "split");
    assert_eq!(
        (by["splits_by_area"], by["one_area"], by["unclear"]),
        (25, 29, 8)
    );
    // S1 to S40 are the cue's own labelled asks: twenty that fired, twenty that did not.
    let core: Vec<_> = split
        .cases
        .iter()
        .filter(|c| c.id.starts_with('S') && c.id[1..].parse::<u32>().is_ok_and(|n| n <= 40))
        .collect();
    assert_eq!(core.len(), 40);
    assert_eq!(
        core.iter()
            .filter(|c| c.want.split == "splits_by_area")
            .count(),
        20
    );
    assert_eq!(
        core.iter().filter(|c| c.want.split == "one_area").count(),
        20
    );
    assert_eq!(bar_need(split.cases.len() as u32), 50);
    // The ones taken from the model set are byte for byte its own.
    let model = parse(V10, "route.v10");
    for id in ["L12", "W14", "W8", "W9", "W7", "W26", "W28", "L9"] {
        let (a, b) = (
            split.cases.iter().find(|c| c.id == id).unwrap(),
            model.cases.iter().find(|c| c.id == id).unwrap(),
        );
        assert_eq!(a.message, b.message, "{id}");
    }
}

#[test]
fn a_case_without_the_label_its_pack_grades_is_refused() {
    let p = by_name("work.v1").unwrap();
    let file = "pack = \"work.v1\"\n[defaults]\nplace = \"dm\"\nrestricted_models = \"allowed\"\n\
                price_period = \"off_peak\"\nest_tokens = 1\nlast_reply_model = \"m\"\n\
                switch_cost = \"cheaper_to_switch\"\n[[case]]\nid = \"X1\"\nmessage = \"hi\"\n";
    let e = CaseSet::parse(file, &p).unwrap_err();
    assert!(e.contains("no `work` label"), "{e}");
    let e = CaseSet::parse(&format!("{file}work = \"poetry\"\n"), &p).unwrap_err();
    assert!(e.contains("poetry") && e.contains("does not offer"), "{e}");
    assert!(CaseSet::parse(&format!("{file}work = \"writing\"\n"), &p).is_ok());
}
