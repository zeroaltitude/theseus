//! `route.v12` (issue 46): `route.v11` without the two questions nothing read (`seal`, `personal`), the
//! latency rule nothing fed (`on_path_p95`), and the two clauses of the `model` question that spoke of a
//! state's "not allowed" models, which the daemon no longer writes. Everything else is v11's, field for
//! field; v11 is retired, its file kept as it was.

use crate::pack::{by_name, Pack};
use crate::route_cases::{CaseSet, V10};

/// The `model` instructions' one change: the clause about models the state lists as not allowed.
const INSTRUCTIONS: (&str, &str) = (
    "choose another model only for a reason its description gives, and never a model the state lists as not allowed. Quality",
    "choose another model only for a reason its description gives. Quality",
);

/// The `deepseek_flash` option's one change: the same rule's clause at the end of its "never" list.
const FLASH: (&str, &str) = (
    "Never for a program from scratch, terminal step-by-step work, design, review, or when the state says China-origin models are not allowed.",
    "Never for a program from scratch, terminal step-by-step work, design or review.",
);

/// The description, which said the pack asks about sealing and personal details.
const DESCRIPTION: &str =
    "ROUTE: which model answers the person's new message; which specialist should answer.";

fn swap(text: &str, (old, new): (&str, &str)) -> String {
    assert_eq!(text.matches(old).count(), 1, "{old:?} once in {text:?}");
    text.replace(old, new)
}

/// v12 is v11 parsed, with exactly these taken out or changed: the `seal` and `personal` questions, the
/// `on_path_p95` rule, the two clauses ([`INSTRUCTIONS`], [`FLASH`]), the description and the version.
/// Every other field of every question, option, bar and rule is equal.
#[test]
fn route_v12_is_route_v11_without_seal_personal_and_the_unfed_latency_rule() {
    let v11 = crate::pack::retired("route.v11").expect("route.v11 is retired, its file kept");
    let v12 = by_name("route.v12").expect("route.v12 is embedded");
    assert_eq!((v11.version, v12.version), (11, 12));
    let ids = |p: &Pack| p.questions.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&v11), ["model", "persona", "personal", "seal"]);
    assert_eq!(ids(&v12), ["model", "persona"]);
    let mut planted = (*v11).clone();
    planted.version = 12;
    planted.sha256 = v12.sha256.clone();
    planted.description = DESCRIPTION.to_string();
    planted
        .questions
        .retain(|q| q.id != "seal" && q.id != "personal");
    planted
        .rollback
        .retain(|r| !matches!(r, crate::learn::RollbackRule::OnPathP95 { .. }));
    assert_eq!(planted.rollback.len(), 2, "labels_per_day and pins_per_day");
    for q in planted.questions.iter_mut().filter(|q| q.id == "model") {
        q.instructions = swap(&q.instructions, INSTRUCTIONS);
        for o in q.options.iter_mut().filter(|o| o.id == "deepseek_flash") {
            o.means = Some(swap(o.means.as_deref().unwrap(), FLASH));
        }
    }
    assert_eq!(planted, *v12);
    // No word of the provider rule is left in what the pack asks.
    let model = v12.question("model").unwrap();
    let words = std::iter::once(model.instructions.clone())
        .chain(model.options.iter().filter_map(|o| o.means.clone()));
    for w in words {
        assert!(!w.contains("not allowed"), "{w}");
    }
}

/// v11's file is under `packs/retired/` as it was: its digest is the one its stored judgments hold.
#[test]
fn route_v11_moved_unchanged_and_is_retired_with_its_own_rules() {
    let v11 = crate::pack::retired("route.v11").unwrap();
    assert_eq!(
        v11.sha256,
        "b6a713c642331156caefd8563c1dfbe37ce31623e5b769baf356111e0cd68678"
    );
    assert!(by_name("route.v11").is_none(), "no longer embedded");
    assert!(crate::pack::RETIRED.contains(&"route.v11"));
    assert_eq!(
        crate::pack::retired_rollback("route.v11").unwrap(),
        v11.rollback
    );
    assert!(crate::pack::readable_ids().contains(&"route".to_string()));
}

/// A thread's own session with two personas asks v11 four questions; v12 asks its two, `model` and `persona`,
/// and nothing about sealing or personal details.
#[test]
fn the_acting_route_pack_asks_no_seal_and_no_personal_in_a_threads_session() {
    use crate::builders::{prepare, InboundInput, Input, PersonaInput, ThreadNow, TurnInput};
    let mut i: InboundInput =
        serde_json::from_str(include_str!("../fixtures/inputs/inbound.json")).unwrap();
    i.personas = ["birch", "alder"]
        .iter()
        .map(|id| PersonaInput {
            id: (*id).into(),
            name: (*id).into(),
            card: "A specialist.".into(),
        })
        .collect();
    i.thread_now = Some(ThreadNow {
        title: "pr round".into(),
        goal: "Land the fix.".into(),
        turns: vec![TurnInput {
            said: "first words".into(),
            answered_by: None,
        }],
    });
    let asked = |p: &Pack| {
        let prepared = prepare(p, &Input::Inbound(i.clone()), &crate::state::NoScrub).unwrap();
        let mut ids: Vec<String> = p.ask(&prepared.dynamic).into_iter().map(|a| a.id).collect();
        ids.sort();
        ids
    };
    let v11 = crate::pack::retired("route.v11").unwrap();
    assert_eq!(asked(&v11), ["model", "persona", "personal", "seal"]);
    let v12 = by_name("route.v12").expect("route.v12 is embedded");
    assert_eq!(asked(&v12), ["model", "persona"]);
}

/// The labelled cases (route.v10's file) are read for v12, the same cases; v11's text still reads them, so
/// `--pack-file packs/retired/route.v11.toml` still replays them.
#[test]
fn v10s_cases_are_read_for_v12_and_for_v11() {
    let a = CaseSet::parse(V10, &crate::pack::retired("route.v11").unwrap()).unwrap();
    let b = CaseSet::parse(V10, &by_name("route.v12").expect("route.v12")).unwrap();
    assert!(a.cases.len() >= 62);
    assert_eq!(
        a.cases.iter().map(|c| (&c.id, &c.want)).collect::<Vec<_>>(),
        b.cases.iter().map(|c| (&c.id, &c.want)).collect::<Vec<_>>()
    );
}
