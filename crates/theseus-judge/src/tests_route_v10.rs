//! `route.v10` and its cases (theseus-h66v4, row 1): the pack is embedded and
//! lints (no computation words, `confirm <= act`, every `no_match` among its
//! options, its rollback rules real), holds the design's text but for the
//! owner's two edits of 2026-10-29 22:46, and carries v8's (so v9's)
//! `persona`, `seal` and `personal` word for word; the 58 replay cases parse,
//! hold the design's counts, and are graded as the design's bar says.

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
        ["context", "effort", "model"],
        "the loader keeps questions by id"
    );
    let model = p.question("model").unwrap();
    let ids: Vec<&str> = model.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["haiku", "sonnet", "opus", "fable", "deepseek_flash"]);
    assert_eq!(model.no_match.as_deref(), Some("sonnet"));
    assert_eq!(
        p.question("effort").unwrap().no_match.as_deref(),
        Some("default")
    );
    assert_eq!(
        p.question("context").unwrap().no_match.as_deref(),
        Some("full")
    );
    // The three act at 0.90 and confirm at 0.60: v8's and v9's bars.
    for q in ["model", "effort", "context"] {
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
    // 58 replay cases: L1 to L20 and W1 to W38 (W39 and W40 are daemon tests).
    let ids: Vec<&str> = s.cases.iter().map(|c| c.id.as_str()).collect();
    let want: Vec<String> = (1..=20)
        .map(|n| format!("L{n}"))
        .chain((1..=38).map(|n| format!("W{n}")))
        .collect();
    assert_eq!(ids, want.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(s.cases.len() as u32 + PINNED, 60);
    // The design's table of counts, less the two pins (haiku W40, fable W39).
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
        (18, 21, 11, 4, 4)
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

#[test]
fn a_case_asks_the_three_deciding_questions_and_personal_and_its_state_is_under_the_cap() {
    let p = pack();
    for c in &set().cases {
        let prepared = prepared(&p, c);
        let asked: Vec<String> = ask(&p, c, &json!({"purpose": "replay"}))
            .asked
            .iter()
            .map(|a| a.id.clone())
            .collect();
        for q in ["model", "effort", "context", "personal"] {
            assert!(asked.contains(&q.to_string()), "{}: {q} in {asked:?}", c.id);
        }
        for q in ["persona", "seal"] {
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
            "last_reply_model",
        ] {
            assert!(v.get(field).is_some(), "{}: no {field} in {v}", c.id);
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
             [[case]]\nid = \"X1\"\nmessage = \"hi\"\n{extra}\nmodel = \"{model}\"\n\
             effort = \"low\"\ncontext = \"short\"\n"
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
    // An unknown key, and a duplicate id.
    assert!(CaseSet::parse(&one("colour = \"red\"", "haiku"), &p).is_err());
    let twice = format!("{}{}", one("", "haiku"), "[[case]]\nid = \"X1\"\nmessage = \"x\"\nmodel = \"haiku\"\neffort = \"low\"\ncontext = \"short\"\n");
    let e = CaseSet::parse(&twice, &p).unwrap_err();
    assert!(e.contains("twice"), "{e}");
    // Written for another pack.
    let e = CaseSet::parse(&one("", "haiku").replace("route.v10", "route.v8"), &p).unwrap_err();
    assert!(e.contains("route.v8"), "{e}");
}

/// A judgment that answered `model`, `effort` and `context` as given, each
/// at its confidence.
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
fn a_judgment_is_graded_on_model_effort_and_context_apart() {
    let (p, s) = (pack(), set());
    let case = |id: &str| s.cases.iter().find(|c| c.id == id).unwrap();
    // W18 wants opus, medium, full.
    let right = judged(
        case("W18"),
        &[
            ("model", "opus", 0.95),
            ("effort", "medium", 0.9),
            ("context", "full", 0.9),
        ],
    );
    let g = grade(&p, case("W18"), Some(&right));
    assert!(
        g.answered && g.model.agrees && g.effort.agrees && g.context.agrees && !g.under_confirm
    );
    assert!(g.must);
    // A wrong model is a miss with Jev's top option and its confidence; one rung of effort is close, two is not.
    let off = judged(
        case("W18"),
        &[
            ("model", "sonnet", 0.71),
            ("effort", "high", 0.9),
            ("context", "short", 0.8),
        ],
    );
    let g = grade(&p, case("W18"), Some(&off));
    assert_eq!(
        (g.model.agrees, g.model.got.as_str(), g.model.confidence),
        (false, "sonnet", 0.71)
    );
    assert_eq!((g.effort.agrees, g.effort.close), (false, true));
    assert!(!g.context.agrees && !g.context.close);
    let far = judged(
        case("W18"),
        &[
            ("model", "opus", 0.9),
            ("effort", "max", 0.9),
            ("context", "full", 0.9),
        ],
    );
    assert!(!grade(&p, case("W18"), Some(&far)).effort.close);
    // `default` is on no rung: it agrees with `default` only (L20 wants it).
    let l20 = case("L20");
    assert_eq!(l20.want.effort, "default");
    let near = judged(
        l20,
        &[
            ("model", "sonnet", 0.9),
            ("effort", "medium", 0.9),
            ("context", "full", 0.9),
        ],
    );
    assert!(!grade(&p, l20, Some(&near)).effort.close);
    let same = judged(
        l20,
        &[
            ("model", "sonnet", 0.9),
            ("effort", "default", 0.9),
            ("context", "full", 0.9),
        ],
    );
    assert!(grade(&p, l20, Some(&same)).effort.agrees);
    // A model answer under the confirm bar is counted apart from its agreement.
    let shy = judged(
        case("W18"),
        &[
            ("model", "opus", 0.55),
            ("effort", "medium", 0.9),
            ("context", "full", 0.9),
        ],
    );
    let g = grade(&p, case("W18"), Some(&shy));
    assert!(g.model.agrees && g.under_confirm);
    // No judgment, no answer: every question a miss.
    let g = grade(&p, case("W18"), None);
    assert!(!g.answered && !g.model.agrees && !g.effort.close && !g.context.agrees);
}

fn graded(model: bool, effort_close: bool, context: bool, must: bool, under: bool) -> Graded {
    let c = |agrees, close| crate::route_cases::Check {
        want: "w".into(),
        got: "g".into(),
        confidence: 0.9,
        agrees,
        close,
    };
    Graded {
        id: "X".into(),
        must,
        answered: true,
        model: c(model, model),
        effort: c(effort_close, effort_close),
        context: c(context, context),
        under_confirm: under,
    }
}

#[test]
fn the_bar_counts_the_two_pin_tests_as_met_and_needs_every_must_case() {
    // 58 cases, all right: met. The bar's 54 is 52 here, with the two pins.
    let mut all: Vec<Graded> = (0..58)
        .map(|i| graded(true, true, true, i < 13, false))
        .collect();
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
    // A file that is not the 58 has no bar to meet.
    assert_eq!(totals(&all[..10]).bar, None);
}
