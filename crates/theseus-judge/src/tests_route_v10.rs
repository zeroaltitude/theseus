//! `route.v10` and its cases (theseus-h66v4, row 1): the pack is embedded and
//! lints (no computation words, `confirm <= act`, every `no_match` among its
//! options, its rollback rules real), holds the design's text but for the
//! owner's two edits of 2026-10-29 22:46, and carries v8's (so v9's)
//! `persona`, `seal` and `personal` word for word; the owner's 22:50 answers
//! (theseus-h66v4.1): no `context` question, the cost sentence in `model`'s
//! instructions, `effort` with no `default` and `no_match = "high"`; the 61
//! replay cases parse, hold the counts, and are graded on `model` and `effort`.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;

use crate::band::{band, Thresholds};
use crate::client::Answer;
use crate::judge::{AnswerRecord, Judgment, StateRecord};
use crate::pack::{by_name, Pack};
use crate::route_cases::{
    ask, grade, model_counts, prepared, totals, CaseSet, Graded, PINNED, V10,
};

const T: Thresholds = Thresholds {
    act: 0.9,
    confirm: 0.6,
};

fn pack() -> Arc<Pack> {
    by_name("route.v10").expect("route.v10 is embedded")
}

fn set() -> CaseSet {
    CaseSet::parse(V10, &pack()).unwrap_or_else(|e| panic!("{e}"))
}

fn means(pack: &Pack, q: &str, option: &str) -> String {
    pack.question(q)
        .unwrap()
        .options
        .iter()
        .find(|o| o.id == option)
        .unwrap_or_else(|| panic!("{q} has no option {option}"))
        .means
        .clone()
        .expect("an option says what it means")
}

#[test]
fn route_v10_is_embedded_not_wired_and_has_the_designs_questions() {
    let p = pack();
    assert_eq!((p.id.as_str(), p.version), ("route", 10));
    assert_eq!(p.jev_model, "jev-1.13.0");
    let decides: Vec<&str> = p
        .questions
        .iter()
        .filter(|q| q.decides)
        .map(|q| q.id.as_str())
        .collect();
    assert_eq!(
        decides,
        ["effort", "model"],
        "the loader keeps questions by id"
    );
    let model = p.question("model").unwrap();
    let ids: Vec<&str> = model.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["haiku", "sonnet", "opus", "fable", "deepseek_flash"]);
    assert_eq!(model.no_match.as_deref(), Some("sonnet"));
    assert_eq!(
        p.question("effort").unwrap().no_match.as_deref(),
        Some("high")
    );
    // The two act at 0.90 and confirm at 0.60: v8's and v9's bars.
    for q in ["model", "effort"] {
        let t = p.question(q).unwrap().thresholds;
        assert_eq!((t.act, t.confirm), (0.90, 0.60), "{q}");
    }
    // Not the acting pack, and the three rollback rules the design names.
    assert_ne!(
        by_name("route.v8").unwrap().name(),
        p.name(),
        "route.v8 stays what ROUTE_PACK names"
    );
    let rules: Vec<String> = p
        .rollback
        .iter()
        .map(|r| {
            serde_json::to_value(r).unwrap()["rule"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(rules, ["labels_per_day", "on_path_p95", "pins_per_day"]);
}

/// theseus-h66v4.1, the owner's 2026-10-29 22:50 answers: the `context`
/// question is deleted, `model` carries the cost sentence verbatim, `effort`
/// has no `default` and opens with its usual-effort sentence.
#[test]
fn the_owners_22_50_answers_are_in_the_pack() {
    let p = pack();
    assert!(
        p.question("context").is_none(),
        "the context question is gone"
    );
    let model = p.question("model").unwrap().instructions.clone();
    assert!(
        model.contains(
            "Quality decides first. When more than one model fits the work, the cheaper one at \
             this message's cost wins; when the state says `switch_cost = cheaper_to_stay`, a \
             cheaper model is not cheaper for this message."
        ),
        "{model}"
    );
    let effort = p.question("effort").unwrap();
    assert!(
        effort.instructions.starts_with(
            "High is the usual effort; choose less only for a reason the message gives."
        ),
        "{}",
        effort.instructions
    );
    let ids: Vec<&str> = effort.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["low", "medium", "high", "xhigh", "max"], "no default");
    assert!(
        !p.description.to_lowercase().contains("context"),
        "{}",
        p.description
    );
}

#[test]
fn the_owners_two_edits_are_in_the_text_and_nothing_else_moved() {
    let p = pack();
    // (a) the Fable block ends at "ordinary programming": no cyber or biology clause.
    let fable = means(&p, "model", "fable");
    assert!(
        fable.ends_with("Never for ordinary programming."),
        "{fable}"
    );
    for word in ["cyber", "biology", "classifier"] {
        assert!(!fable.to_lowercase().contains(word), "{word} in {fable}");
    }
    // (b) the Haiku block names tool-call turns, and keeps the rest of its sentence.
    let haiku = means(&p, "model", "haiku");
    assert!(
        haiku.contains(
            "a summary of what was just said, a tool-call turn — a command or tool to run and \
             report — or a small edit whose cause is known. Not for a long context"
        ),
        "{haiku}"
    );
    // The Sonnet, Opus and Flash blocks are the design's word for word.
    assert!(means(&p, "model", "sonnet")
        .starts_with("Sonnet 5.5 (Anthropic, US). The usual choice: $2 in, $10 out"));
    assert!(means(&p, "model", "sonnet").ends_with("Choose it when unsure."));
    assert!(means(&p, "model", "opus").ends_with("Sonnet solves nearly as much for far less."));
    assert!(means(&p, "model", "deepseek_flash")
        .contains("half of each when the state says price_period is off_peak"));
    assert!(means(&p, "model", "deepseek_flash")
        .ends_with("or when the state says China-origin models are not allowed."));
}

#[test]
fn persona_seal_and_personal_are_the_earlier_packs_word_for_word() {
    let (v10, v8) = (pack(), by_name("route.v8").unwrap());
    for q in ["persona", "seal", "personal"] {
        assert_eq!(
            serde_json::to_value(v10.question(q).unwrap()).unwrap(),
            serde_json::to_value(v8.question(q).unwrap()).unwrap(),
            "{q}"
        );
    }
    // The retired questions are gone.
    for q in ["mode", "reply_effort", "data"] {
        assert!(v10.question(q).is_none(), "{q}");
    }
}

#[test]
fn the_cases_hold_the_designs_counts_and_ids() {
    let s = set();
    assert_eq!(s.pack, "route.v10");
    // 61 replay cases: L1 to L20, W1 to W38 (W39 and W40 are daemon tests) and
    // the three switch-cost cases W41 to W43.
    let ids: Vec<&str> = s.cases.iter().map(|c| c.id.as_str()).collect();
    let want: Vec<String> = (1..=20)
        .map(|n| format!("L{n}"))
        .chain((1..=38).map(|n| format!("W{n}")))
        .chain((41..=43).map(|n| format!("W{n}")))
        .collect();
    assert_eq!(ids, want.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(s.cases.len() as u32 + PINNED, 63);
    // The design's table of counts, less the two pins (haiku W40, fable W39), with
    // W1 relabelled to sonnet and W41 (sonnet), W42 (opus) and W43 (haiku) added.
    let counts: BTreeMap<String, u32> = model_counts(&s);
    let n = |m: &str| counts[m];
    assert_eq!(
        (
            n("haiku"),
            n("sonnet"),
            n("opus"),
            n("fable"),
            n("deepseek_flash")
        ),
        (18, 23, 12, 4, 4)
    );
    // The `must` cases: fourteen in the design, W40 being a daemon test.
    let must: Vec<&str> = s
        .cases
        .iter()
        .filter(|c| c.must)
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(
        must,
        ["W1", "W8", "W9", "W11", "W14", "W15", "W16", "W17", "W18", "W22", "W26", "W30", "W31"]
    );
}

#[test]
fn the_cases_carry_the_states_the_design_gives_them() {
    let s = set();
    let case = |id: &str| s.cases.iter().find(|c| c.id == id).unwrap();
    // Defaults: dm, allowed, off_peak, 40K, sonnet, no task.
    let w2 = &case("W2").state;
    assert_eq!(
        (
            w2.place.as_str(),
            w2.restricted_models.as_str(),
            w2.price_period.as_str(),
            w2.est_tokens
        ),
        ("dm", "allowed", "off_peak", 40_000)
    );
    assert_eq!(w2.last_reply_model, "claude-sonnet-5-5");
    assert!(w2.task_models.is_empty());
    assert_eq!(w2.place_kind, "discord_dm");
    assert_eq!(case("W1").state.est_tokens, 200_000);
    assert_eq!(case("W13").state.price_period, "peak");
    assert_eq!(case("W14").state.restricted_models, "not_allowed");
    assert_eq!(case("W15").state.restricted_models, "not_allowed");
    assert_eq!(
        case("W14").message,
        case("W11").message,
        "W14 is W11 with the label"
    );
    assert_eq!(case("L1").state.place_kind, "discord_channel");
    assert_eq!(case("W30").state.task_models, ["deepseek-flash"]);
    assert_eq!(case("W32").state.task_models, ["claude-sonnet-5-5"]);
    for id in ["W34", "W35", "W36", "W37", "W38"] {
        assert_eq!(case(id).state.place_kind, "job", "{id}");
    }
    assert_eq!(case("W35").state.restricted_models, "not_allowed");
}

/// theseus-h66v4.1: the three switch-cost cases the owner named (22:50), and
/// the states that make each a fair test: a long Sonnet session where
/// switching costs more, a long Haiku session where it does too, and a fresh
/// session with nothing warm.
#[test]
fn the_three_switch_cost_cases_carry_the_states_and_labels_the_owner_named() {
    let s = set();
    let case = |id: &str| s.cases.iter().find(|c| c.id == id).unwrap();
    // A "thanks" on a 200K Sonnet session stays on Sonnet (W1 is relabelled to say so).
    for id in ["W1", "W41"] {
        let c = case(id);
        assert_eq!(c.state.est_tokens, 200_000, "{id}");
        assert_eq!(c.state.current_model, "claude-sonnet-5-5", "{id}");
        assert_eq!(c.state.switch_cost, "cheaper_to_stay", "{id}");
        assert_eq!(c.want.model, "sonnet", "{id}");
    }
    // A hard design question on a long Haiku session moves to Opus despite cheaper_to_stay.
    let c = case("W42");
    assert_eq!(c.state.current_model, "claude-haiku-5-5");
    assert_eq!(c.state.switch_cost, "cheaper_to_stay");
    assert_eq!(c.want.model, "opus");
    // A fresh session's quick fact goes to Haiku; nothing is warm.
    let c = case("W43");
    assert_eq!(c.state.current_model, "");
    assert_eq!(c.state.last_reply_model, "");
    assert_eq!(c.state.switch_cost, "about_the_same");
    assert_eq!(c.want.model, "haiku");
    // The state a case builds says the same, and a fresh session names no warm model.
    let st = |id: &str| crate::route_cases::state(case(id), 3000, &crate::state::NoScrub).value();
    assert_eq!(st("W42")["current_model"], "claude-haiku-5-5");
    assert_eq!(st("W42")["switch_cost"], "cheaper_to_stay");
    assert!(st("W43").get("current_model").is_none());
    assert_eq!(st("W43")["switch_cost"], "about_the_same");
    // No case asks Jev about a context any more, and L20 (once `default`) wants high.
    assert_eq!(case("L20").want.effort, "high");
}

#[test]
fn a_case_asks_the_two_deciding_questions_and_personal_and_its_state_is_under_the_cap() {
    let p = pack();
    for c in &set().cases {
        let prepared = prepared(&p, c);
        let asked: Vec<String> = ask(&p, c, &json!({"purpose": "replay"}))
            .asked
            .iter()
            .map(|a| a.id.clone())
            .collect();
        for q in ["model", "effort", "personal"] {
            assert!(asked.contains(&q.to_string()), "{}: {q} in {asked:?}", c.id);
        }
        for q in ["persona", "seal", "context"] {
            assert!(!asked.contains(&q.to_string()), "{}: {q}", c.id);
        }
        assert!(prepared.state.tokens <= p.state_cap_tokens, "{}", c.id);
        let v = prepared.state.value();
        for field in [
            "message",
            "place_kind",
            "price_period",
            "restricted_models",
            "est_tokens",
            "switch_cost",
        ] {
            assert!(v.get(field).is_some(), "{}: no {field} in {v}", c.id);
        }
        // A model that is none (a fresh session, a job) is left out, as the daemon leaves it.
        for (field, model) in [
            ("last_reply_model", &c.state.last_reply_model),
            ("current_model", &c.state.current_model),
        ] {
            assert_eq!(
                v.get(field).is_some(),
                !model.is_empty(),
                "{}: {field}",
                c.id
            );
        }
        assert_eq!(
            v.get("task_models").is_some(),
            !c.state.task_models.is_empty(),
            "{}",
            c.id
        );
        assert_eq!(v["message"], json!(c.message), "{}", c.id);
    }
}

#[test]
fn a_case_file_that_breaks_a_rule_is_refused() {
    let p = pack();
    let one = |extra: &str, model: &str| {
        format!(
            "pack = \"route.v10\"\n[defaults]\nplace = \"dm\"\nrestricted_models = \"allowed\"\n\
             price_period = \"off_peak\"\nest_tokens = 40000\nlast_reply_model = \"m\"\n\
             switch_cost = \"cheaper_to_switch\"\n\
             [[case]]\nid = \"X1\"\nmessage = \"hi\"\n{extra}\nmodel = \"{model}\"\n\
             effort = \"low\"\n"
        )
    };
    assert!(CaseSet::parse(&one("", "haiku"), &p).is_ok());
    // A label the pack does not offer.
    let e = CaseSet::parse(&one("", "glm"), &p).unwrap_err();
    assert!(e.contains("glm") && e.contains("does not offer"), "{e}");
    // A state word outside its set.
    let e = CaseSet::parse(&one("price_period = \"off-peak\"", "haiku"), &p).unwrap_err();
    assert!(e.contains("price_period"), "{e}");
    let e = CaseSet::parse(&one("restricted_models = \"no\"", "haiku"), &p).unwrap_err();
    assert!(e.contains("restricted_models"), "{e}");
    let e = CaseSet::parse(&one("switch_cost = \"dearer\"", "haiku"), &p).unwrap_err();
    assert!(e.contains("switch_cost"), "{e}");
    // The deleted question's label is an unknown key now.
    assert!(CaseSet::parse(&one("context = \"short\"", "haiku"), &p).is_err());
    // An unknown key, and a duplicate id.
    assert!(CaseSet::parse(&one("colour = \"red\"", "haiku"), &p).is_err());
    let twice = format!(
        "{}{}",
        one("", "haiku"),
        "[[case]]\nid = \"X1\"\nmessage = \"x\"\nmodel = \"haiku\"\neffort = \"low\"\n"
    );
    let e = CaseSet::parse(&twice, &p).unwrap_err();
    assert!(e.contains("twice"), "{e}");
    // Written for another pack.
    let e = CaseSet::parse(&one("", "haiku").replace("route.v10", "route.v8"), &p).unwrap_err();
    assert!(e.contains("route.v8"), "{e}");
}

/// A judgment that answered `model` and `effort` as given, each at its
/// confidence.
fn judged(case: &crate::route_cases::Case, answers: &[(&str, &str, f64)]) -> Judgment {
    let p = pack();
    let state = StateRecord::from(prepared(&p, case).state.as_ref());
    let records: Vec<AnswerRecord> = answers
        .iter()
        .map(|(q, top, c)| {
            let answer = Answer::Choice {
                choice: (*top).into(),
                probabilities: vec![((*top).to_string(), *c)],
                confidence: *c,
            };
            AnswerRecord {
                question: (*q).into(),
                def: (*q).into(),
                about: None,
                band: band(&answer, T),
                answer,
            }
        })
        .collect();
    serde_json::from_value(json!({
        "id": "jdg_x", "pack": "route.v10", "version": 10, "pack_sha256": "00", "point": "inbound",
        "mode": "shadow", "model": "jev-1.13.0", "answered_by": "jev-1.13.0",
        "model_drift": false, "state": state, "questions": records.len(), "answers": records,
        "call": null, "timing": {"queued_ms": 0, "http_ms": 1, "total_ms": 1},
        "usage": null, "cost_micros": 1, "reserve_micros": 1,
        "outcome": {"outcome": "answered"}, "circuit": null, "rate_limit": {}, "context": {},
    }))
    .unwrap()
}

#[test]
fn a_judgment_is_graded_on_model_and_effort_apart() {
    let (p, s) = (pack(), set());
    let case = |id: &str| s.cases.iter().find(|c| c.id == id).unwrap();
    // W18 wants opus, medium.
    let right = judged(
        case("W18"),
        &[("model", "opus", 0.95), ("effort", "medium", 0.9)],
    );
    let g = grade(&p, case("W18"), Some(&right));
    assert!(g.answered && g.model.agrees && g.effort.agrees && !g.under_confirm);
    assert!(g.must);
    // A wrong model is a miss with Jev's top option and its confidence; one rung of effort is close, two is not.
    let off = judged(
        case("W18"),
        &[("model", "sonnet", 0.71), ("effort", "high", 0.9)],
    );
    let g = grade(&p, case("W18"), Some(&off));
    assert_eq!(
        (g.model.agrees, g.model.got.as_str(), g.model.confidence),
        (false, "sonnet", 0.71)
    );
    assert_eq!((g.effort.agrees, g.effort.close), (false, true));
    let far = judged(
        case("W18"),
        &[("model", "opus", 0.9), ("effort", "max", 0.9)],
    );
    assert!(!grade(&p, case("W18"), Some(&far)).effort.close);
    // A model answer under the confirm bar is graded as the question's
    // `no_match`, not as Jev's pick (theseus-mir9c), and counted apart.
    let shy = judged(
        case("W18"),
        &[("model", "opus", 0.55), ("effort", "medium", 0.9)],
    );
    let g = grade(&p, case("W18"), Some(&shy));
    assert!(!g.model.agrees && g.model.raw_agrees && g.under_confirm);
    assert_eq!(
        (g.model.raw.as_str(), g.model.got.as_str()),
        ("opus", "sonnet")
    );
    // No judgment, no answer: every question a miss.
    let g = grade(&p, case("W18"), None);
    assert!(!g.answered && !g.model.agrees && !g.effort.close);
}

fn graded(model: bool, effort_close: bool, must: bool, under: bool) -> Graded {
    let c = |agrees, close| crate::route_cases::Check {
        want: "w".into(),
        got: "g".into(),
        confidence: 0.9,
        agrees,
        close,
        raw: "g".into(),
        confirm: 0.6,
        sub_confirm: false,
        raw_agrees: agrees,
        raw_close: close,
    };
    Graded {
        id: "X".into(),
        must,
        answered: true,
        model: c(model, model),
        effort: c(effort_close, effort_close),
        under_confirm: under,
    }
}

#[test]
fn the_bar_counts_the_two_pin_tests_as_met_and_needs_every_must_case() {
    // 58 cases, all right: met. The bar's 54 is 52 here, with the two pins.
    let mut all: Vec<Graded> = (0..58).map(|i| graded(true, true, i < 13, false)).collect();
    assert_eq!(totals(&all).bar, Some(true));
    assert_eq!(totals(&all).must_total, 13);
    // 52 of 58 on model is 54 of 60: met; 51 is not.
    for (n, met) in [(52usize, true), (51, false)] {
        let mut g = all.clone();
        for x in g.iter_mut().skip(13).take(58 - 13 - (n - 13)) {
            x.model.agrees = false;
        }
        let t = totals(&g);
        assert_eq!(t.model_ok as usize, n);
        assert_eq!(t.bar, Some(met), "{n}");
    }
    // One `must` case wrong, or under the confirm bar, fails the bar whatever the rest.
    all[0].model.agrees = false;
    assert_eq!(totals(&all).bar, Some(false));
    all[0].model.agrees = true;
    all[1].under_confirm = true;
    let t = totals(&all);
    assert_eq!((t.must_under_confirm, t.bar), (1, Some(false)));
    // A file that is not the 58 has no bar to meet; nor does the 61 it is now.
    assert_eq!(totals(&all[..10]).bar, None);
    let sixty_one: Vec<Graded> = (0..61).map(|i| graded(true, true, i < 13, false)).collect();
    assert_eq!(totals(&sixty_one).bar, None);
}

/// Finding B (theseus-h66v4 row 3): every case's state has exactly the fields
/// the daemon's state has (`ROUTE_STATE_KEYS`, which `tests_route_state` holds
/// the builder to), and a task's cases add `task_models` and nothing else.
#[test]
fn every_cases_state_is_the_daemons_shape() {
    let pack = pack();
    let set = CaseSet::parse(crate::route_cases::V10, &pack).unwrap();
    assert!(!set.cases.is_empty());
    for c in &set.cases {
        let v = crate::route_cases::state(c, pack.state_cap_tokens, &crate::state::NoScrub).value();
        let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        let mut want: Vec<String> = crate::builders::ROUTE_STATE_KEYS
            .iter()
            .map(|s| s.to_string())
            .collect();
        if !c.state.task_models.is_empty() {
            want.push("task_models".into());
        }
        // A fresh session or a job has no model that answered: neither key.
        want.retain(|k| match k.as_str() {
            "last_reply_model" => !c.state.last_reply_model.is_empty(),
            "current_model" => !c.state.current_model.is_empty(),
            _ => true,
        });
        want.sort();
        assert_eq!(keys, want, "{}", c.id);
        assert_eq!(v["est_tokens"], c.state.est_tokens, "{}", c.id);
    }
}

/// The case by id.
fn by_id<'a>(s: &'a CaseSet, id: &str) -> &'a crate::route_cases::Case {
    s.cases.iter().find(|c| c.id == id).unwrap()
}

/// What `grade` makes of one question: its graded answer, whether it fell
/// to the `no_match`, and the raw pick.
fn graded_q(g: &Graded, q: &str) -> (String, bool, String, bool) {
    let c = match q {
        "model" => &g.model,
        _ => &g.effort,
    };
    (c.got.clone(), c.sub_confirm, c.raw.clone(), c.agrees)
}

/// theseus-mir9c: the daemon applies each question's `confirm` bar and, under
/// it, takes the question's `no_match` (`inbound::verdict`'s `sure`,
/// `Verdict::short`). `--cases` graded the raw pick; it now grades what
/// routing would decide. `model`: a correct pick under the bar is the
/// `no_match` (`sonnet`), a wrong one under the bar is right when the label
/// is the `no_match`; at or above the bar nothing changes.
#[test]
fn a_model_pick_under_the_bar_is_graded_as_the_no_match() {
    let (p, s) = (pack(), set());
    let ask = |id: &str, pick: &str, conf: f64| {
        let c = by_id(&s, id);
        let j = judged(c, &[("model", pick, conf), ("effort", &c.want.effort, 0.9)]);
        grade(&p, c, Some(&j))
    };
    // W18 wants opus. Opus at 0.55 is a right pick the daemon would not act
    // on: it runs `sonnet`, a miss.
    assert_eq!(by_id(&s, "W18").want.model, "opus");
    let g = ask("W18", "opus", 0.55);
    assert_eq!(
        graded_q(&g, "model"),
        ("sonnet".into(), true, "opus".into(), false)
    );
    assert!(g.model.raw_agrees && g.under_confirm);
    // At the bar (0.60) the band is confirm, which routing acts on; below it
    // by a hair, it is not.
    let g = ask("W18", "opus", 0.60);
    assert_eq!(
        graded_q(&g, "model"),
        ("opus".into(), false, "opus".into(), true)
    );
    assert!(!g.under_confirm);
    assert!(!ask("W18", "opus", 0.59).model.agrees);
    // Above the bar, a wrong pick stays wrong.
    let g = ask("W18", "sonnet", 0.95);
    assert_eq!(
        graded_q(&g, "model"),
        ("sonnet".into(), false, "sonnet".into(), false)
    );
    // L3 wants sonnet: a pick of haiku under the bar runs `sonnet`, a hit that
    // the raw grade called a miss; sonnet itself under the bar is still a hit.
    assert_eq!(by_id(&s, "L3").want.model, "sonnet");
    let g = ask("L3", "haiku", 0.55);
    assert_eq!(
        graded_q(&g, "model"),
        ("sonnet".into(), true, "haiku".into(), true)
    );
    assert!(!g.model.raw_agrees);
    assert!(ask("L3", "sonnet", 0.55).model.agrees);
    // The grade is the bar's: a case whose model fell, effort at 0.9, grades
    // the effort as asked.
    let g = ask("W18", "opus", 0.55);
    assert!(g.effort.agrees);
}

/// `effort`: under its bar the answer is the question's `no_match`, `high`
/// (the usual effort; the `default` option is gone, theseus-h66v4.1).
#[test]
fn an_effort_pick_under_the_bar_is_graded_as_high() {
    let (p, s) = (pack(), set());
    let ask = |id: &str, pick: &str, conf: f64| {
        let c = by_id(&s, id);
        let j = judged(c, &[("model", &c.want.model, 0.9), ("effort", pick, conf)]);
        grade(&p, c, Some(&j))
    };
    // L11 wants high. A `medium` at 0.55 is `high` to the daemon: a hit that
    // the raw grade called one rung off.
    assert_eq!(by_id(&s, "L11").want.effort, "high");
    let g = ask("L11", "medium", 0.55);
    assert_eq!(
        graded_q(&g, "effort"),
        ("high".into(), true, "medium".into(), true)
    );
    assert!(!g.effort.raw_agrees && g.effort.raw_close);
    // At the bar it stands; one rung off above the bar is close, not exact.
    let g = ask("L11", "high", 0.60);
    assert_eq!(
        graded_q(&g, "effort"),
        ("high".into(), false, "high".into(), true)
    );
    let g = ask("L11", "medium", 0.9);
    assert_eq!(
        (g.effort.agrees, g.effort.close, g.effort.sub_confirm),
        (false, true, false)
    );
    // L3 wants medium: a right `medium` under the bar is `high` to the daemon,
    // a miss within one rung; the same pick above the bar is a hit.
    assert_eq!(by_id(&s, "L3").want.effort, "medium");
    let g = ask("L3", "medium", 0.5);
    assert_eq!(
        (g.effort.got.as_str(), g.effort.agrees, g.effort.close),
        ("high", false, true)
    );
    assert!(g.effort.raw_agrees);
    assert!(ask("L3", "medium", 0.9).effort.agrees);
}

/// The bar and the `no_match` come from the pack replayed, never from this
/// code: a pack with other bars and other `no_match`s grades by its own.
#[test]
fn the_bar_and_the_no_match_are_the_replayed_packs_own() {
    let text = include_str!("../packs/route.v10.toml")
        .replacen(
            "confirm = 0.60\nno_match = \"sonnet\"",
            "confirm = 0.80\nno_match = \"opus\"",
            1,
        )
        .replacen(
            "confirm = 0.60\nno_match = \"high\"",
            "confirm = 0.75\nno_match = \"medium\"",
            1,
        );
    let p = Pack::parse(&text).unwrap();
    let s = CaseSet::parse(V10, &p).unwrap();
    let ask = |id: &str, m: (&str, f64), e: (&str, f64)| {
        let case = by_id(&s, id);
        let j = judged(case, &[("model", m.0, m.1), ("effort", e.0, e.1)]);
        grade(&p, case, Some(&j))
    };
    // W18 wants opus, medium. Sonnet at 0.70 is under this model bar
    // (0.80): the `no_match` is opus, a hit. High at 0.70 is under 0.75:
    // `medium`, a hit.
    let g = ask("W18", ("sonnet", 0.70), ("high", 0.70));
    assert_eq!(
        graded_q(&g, "model"),
        ("opus".into(), true, "sonnet".into(), true)
    );
    assert_eq!(
        graded_q(&g, "effort"),
        ("medium".into(), true, "high".into(), true)
    );
    assert_eq!((g.model.confirm, g.effort.confirm), (0.80, 0.75));
    // The same answers sit above the stock bars (0.60) and are graded as given.
    let (stock, stock_set) = (pack(), set());
    let c = by_id(&stock_set, "W18");
    let j = judged(c, &[("model", "sonnet", 0.70), ("effort", "high", 0.70)]);
    let g = grade(&stock, c, Some(&j));
    assert!(!g.model.sub_confirm && !g.effort.sub_confirm);
}

/// The totals count the graded answer and, beside it, the raw pick: the
/// change the fix makes, on the same judgments.
#[test]
fn the_totals_count_the_graded_answer_and_the_raw_pick_apart() {
    let (p, s) = (pack(), set());
    let all: Vec<Graded> = s
        .cases
        .iter()
        .map(|c| {
            // Every case answered as labelled at 0.9, but L11 (opus, high) at
            // 0.5 on both.
            let conf = if c.id == "L11" { 0.5 } else { 0.9 };
            let j = judged(
                c,
                &[
                    ("model", &c.want.model, conf),
                    ("effort", &c.want.effort, conf),
                ],
            );
            grade(&p, c, Some(&j))
        })
        .collect();
    let t = totals(&all);
    let n = s.cases.len() as u32;
    // Model fell (opus to sonnet); effort fell to `high`, which L11 wants.
    assert_eq!((t.model_ok, t.effort_exact, t.effort_close), (n - 1, n, n));
    assert_eq!(
        (t.raw_model_ok, t.raw_effort_exact, t.raw_effort_close),
        (n, n, n)
    );
}
