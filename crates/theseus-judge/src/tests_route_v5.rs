//! `route.v5`'s `persona` and `seal` questions (theseus-c1uz): the options
//! come from the personas the place may speak as, the question rides the one
//! `inbound` request, and a state with no personas and no thread is byte for
//! byte the one `route.v4` read.

use crate::builders::{prepare, InboundInput, Input, PersonaInput, ThreadNow, TurnInput};
use crate::client::Question;
use crate::pack::{by_name, Source};
use crate::state::NoScrub;

fn input() -> InboundInput {
    serde_json::from_str(include_str!("../fixtures/inputs/inbound.json")).unwrap()
}

fn persona(id: &str, name: &str, card: &str) -> PersonaInput {
    PersonaInput {
        id: id.into(),
        name: name.into(),
        card: card.into(),
    }
}

fn two() -> Vec<PersonaInput> {
    vec![
        persona(
            "birch",
            "Birch",
            "Security engineer: CVEs, sandbox escapes, threat models and disclosure.",
        ),
        persona(
            "alder",
            "Alder",
            "Release manager: changelogs, version bumps, packaging and CI.",
        ),
    ]
}

fn thread() -> ThreadNow {
    ThreadNow {
        title: "pr round".into(),
        goal: "Land the sandbox fix.\nSecond line.".into(),
        turns: (1..=5)
            .map(|n| TurnInput {
                said: format!("turn {n} words"),
                answered_by: (n % 2 == 0).then(|| "birch".to_string()),
            })
            .collect(),
    }
}

fn ids(q: &Question) -> Vec<String> {
    let Question::Choice { options, .. } = q else {
        panic!("a choice")
    };
    options.iter().map(|o| o.id.clone()).collect()
}

/// The persona question has one option per persona, each reading the
/// persona's card, then `prior`.
#[test]
fn the_persona_question_offers_each_persona_beside_prior() {
    let p = by_name("route.v5").unwrap();
    let mut i = input();
    i.personas = two();
    i.speaker = Some("alder".into());
    let prepared = prepare(&p, &Input::Inbound(i), &NoScrub).unwrap();
    let asked = p.ask(&prepared.dynamic);
    let q = asked.iter().find(|a| a.id == "persona").expect("asked");
    assert_eq!(ids(&q.question), ["birch", "alder", "prior"]);
    let Question::Choice { options, .. } = &q.question else {
        panic!()
    };
    let first = options[0].means.as_deref().unwrap();
    assert!(first.starts_with("Birch: Security engineer"), "{first}");
    assert!(first.contains("sandbox escapes"), "{first}");
    assert_eq!(prepared.dynamic.items(Source::Personas).len(), 2);
    let state = prepared.state.value();
    assert_eq!(state["speaker"], "alder");
    assert_eq!(state["personas"].as_array().unwrap().len(), 2);
    assert!(state["personas"][0]["card"]
        .as_str()
        .unwrap()
        .contains("CVEs"));
    // Not asked in a thread: no seal.
    assert!(asked.iter().all(|a| a.id != "seal"));
    // The route pack's other two questions ride the same asks.
    assert!(asked.iter().any(|a| a.id == "mode"));
    assert!(asked.iter().any(|a| a.id == "reply_effort"));
}

/// One persona, or none with a card, leaves nothing to choose: the question
/// is not asked and the state is the one without personas.
#[test]
fn fewer_than_two_personas_ask_nothing() {
    let p = by_name("route.v5").unwrap();
    let base = prepare(&p, &Input::Inbound(input()), &NoScrub).unwrap();
    for personas in [
        vec![],
        vec![persona("birch", "Birch", "Security engineer.")],
        vec![
            persona("birch", "Birch", "Security engineer."),
            persona("alder", "Alder", "  "),
        ],
    ] {
        let mut i = input();
        i.personas = personas;
        i.speaker = Some("birch".into());
        let got = prepare(&p, &Input::Inbound(i), &NoScrub).unwrap();
        assert_eq!(got.state.json, base.state.json);
        assert!(p.ask(&got.dynamic).iter().all(|a| a.id != "persona"));
    }
}

/// A thread's title, goal and last three turns, each with who answered, are
/// in the state, and the `seal` question is asked.
#[test]
fn a_thread_adds_its_title_goal_and_last_three_turns() {
    let p = by_name("route.v5").unwrap();
    let mut i = input();
    i.thread_now = Some(thread());
    let prepared = prepare(&p, &Input::Inbound(i), &NoScrub).unwrap();
    let v = prepared.state.value();
    assert_eq!(v["thread_title"], "pr round");
    assert_eq!(v["thread_goal"], "Land the sandbox fix.");
    let all = v["thread_recent"].as_array().unwrap();
    assert_eq!(
        all[0]["cut"], "2 earlier left out",
        "the turns before are counted"
    );
    let recent = &all[1..];
    let said: Vec<&str> = recent.iter().map(|t| t["said"].as_str().unwrap()).collect();
    assert_eq!(said, ["turn 3 words", "turn 4 words", "turn 5 words"]);
    assert_eq!(recent[1]["answered_by"], "birch");
    assert!(recent[0].get("answered_by").is_none());
    let asked = p.ask(&prepared.dynamic);
    let seal = asked.iter().find(|a| a.id == "seal").expect("asked");
    assert_eq!(ids(&seal.question), ["yes", "no"]);
    assert!(asked.iter().all(|a| a.id != "persona"), "no personas");
    assert!(prepared.state.tokens <= p.state_cap_tokens);
}

/// A turn's words are cut to about 120 tokens, and the whole state stays
/// within its cap with twelve personas and a long thread.
#[test]
fn the_state_stays_within_its_cap_and_carries_no_persona_file() {
    let p = by_name("route.v5").unwrap();
    let mut i = input();
    i.personas = (0..20)
        .map(|n| {
            persona(
                &format!("p{n}"),
                &format!("P{n}"),
                &"expert in things. ".repeat(40),
            )
        })
        .collect();
    let mut t = thread();
    t.turns[4].said = "long words ".repeat(400);
    i.thread_now = Some(t);
    let prepared = prepare(&p, &Input::Inbound(i), &NoScrub).unwrap();
    assert!(prepared.state.tokens <= p.state_cap_tokens);
    let q = p
        .ask(&prepared.dynamic)
        .into_iter()
        .find(|a| a.id == "persona")
        .unwrap();
    assert_eq!(ids(&q.question).len(), crate::builders::PERSONA_OPTIONS + 1);
    let all = prepared.state.value()["thread_recent"].clone();
    let last = all.as_array().unwrap().last().unwrap()["said"].clone();
    assert!(last.as_str().unwrap().chars().count() <= 481, "{last}");
}

/// An inbound state stored before the personas and the thread (none of either)
/// still reads, and writes back without them.
#[test]
fn a_stored_state_reads_with_no_personas_or_thread() {
    let v5 = by_name("route.v5").unwrap();
    let text = include_str!("../fixtures/inputs/inbound.json");
    assert!(!text.contains("personas") && !text.contains("thread_now"));
    let i: InboundInput = serde_json::from_str(text).unwrap();
    let prepared = prepare(&v5, &Input::Inbound(i), &NoScrub).unwrap();
    assert!(prepared.state.value().get("personas").is_none());
    let back = serde_json::to_string(&serde_json::from_str::<InboundInput>(text).unwrap()).unwrap();
    assert!(!back.contains("personas") && !back.contains("speaker"));
}
