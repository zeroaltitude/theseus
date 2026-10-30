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
    // A model answer under the confirm bar is graded as the question's
    // `no_match`, not as Jev's pick (theseus-mir9c), and counted apart.
    let shy = judged(
        case("W18"),
        &[
            ("model", "opus", 0.55),
            ("effort", "medium", 0.9),
            ("context", "full", 0.9),
        ],
    );
    let g = grade(&p, case("W18"), Some(&shy));
    assert!(!g.model.agrees && g.model.raw_agrees && g.under_confirm);
    assert_eq!(
        (g.model.raw.as_str(), g.model.got.as_str()),
        ("opus", "sonnet")
    );
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
        "effort" => &g.effort,
        _ => &g.context,
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
        let j = judged(
            c,
            &[
                ("model", pick, conf),
                ("effort", &c.want.effort, 0.9),
                ("context", &c.want.context, 0.9),
            ],
        );
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
    // The grade is the bar's: a case whose model fell, effort and context at
    // 0.9, grades those two as asked.
    let g = ask("W18", "opus", 0.55);
    assert!(g.effort.agrees && g.context.agrees);
}

/// `effort`: under its bar the answer is the question's `no_match`,
/// `default`, which is on no rung.
#[test]
fn an_effort_pick_under_the_bar_is_graded_as_default() {
    let (p, s) = (pack(), set());
    let ask = |id: &str, pick: &str, conf: f64| {
        let c = by_id(&s, id);
        let j = judged(
            c,
            &[
                ("model", &c.want.model, 0.9),
                ("effort", pick, conf),
                ("context", &c.want.context, 0.9),
            ],
        );
        grade(&p, c, Some(&j))
    };
    // L11 wants high. A right `high` at 0.55 is `default` to the daemon: a
    // miss, and not within a rung of `high` either.
    assert_eq!(by_id(&s, "L11").want.effort, "high");
    let g = ask("L11", "high", 0.55);
    assert_eq!(
        graded_q(&g, "effort"),
        ("default".into(), true, "high".into(), false)
    );
    assert!(g.effort.raw_agrees && g.effort.raw_close && !g.effort.close);
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
    // L20 wants `default`: any pick under the bar is a hit; the same pick
    // above it is a miss.
    assert_eq!(by_id(&s, "L20").want.effort, "default");
    let g = ask("L20", "medium", 0.5);
    assert_eq!(
        graded_q(&g, "effort"),
        ("default".into(), true, "medium".into(), true)
    );
    assert!(!g.effort.raw_agrees);
    assert!(!ask("L20", "medium", 0.9).effort.agrees);
}

/// `context`: under its bar the answer is `full`, so a `short` the daemon
/// would not act on is `full`, which the case wants, and a right `short`
/// under the bar is a miss.
#[test]
fn a_context_pick_under_the_bar_is_graded_as_full() {
    let (p, s) = (pack(), set());
    let ask = |id: &str, pick: &str, conf: f64| {
        let c = by_id(&s, id);
        let j = judged(
            c,
            &[
                ("model", &c.want.model, 0.9),
                ("effort", &c.want.effort, 0.9),
                ("context", pick, conf),
            ],
        );
        grade(&p, c, Some(&j))
    };
    // L3 wants full: a `short` at 0.55 compiles `full`: a hit.
    assert_eq!(by_id(&s, "L3").want.context, "full");
    let g = ask("L3", "short", 0.55);
    assert_eq!(
        graded_q(&g, "context"),
        ("full".into(), true, "short".into(), true)
    );
    assert!(!g.context.raw_agrees);
    // A confident `short` stays `short`, and misses.
    let g = ask("L3", "short", 0.8);
    assert_eq!(
        graded_q(&g, "context"),
        ("short".into(), false, "short".into(), false)
    );
    // L2 wants short: a right `short` under the bar is `full`, a miss; at
    // the bar, a hit.
    assert_eq!(by_id(&s, "L2").want.context, "short");
    let g = ask("L2", "short", 0.55);
    assert_eq!(
        graded_q(&g, "context"),
        ("full".into(), true, "short".into(), false)
    );
    assert!(g.context.raw_agrees);
    assert!(ask("L2", "short", 0.60).context.agrees);
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
            "confirm = 0.60\nno_match = \"default\"",
            "confirm = 0.75\nno_match = \"medium\"",
            1,
        )
        .replacen(
            "act = 0.90\nconfirm = 0.60\nno_match = \"full\"",
            "act = 0.97\nconfirm = 0.95\nno_match = \"short\"",
            1,
        );
    let p = Pack::parse(&text).unwrap();
    let s = CaseSet::parse(V10, &p).unwrap();
    let ask = |id: &str, m: (&str, f64), e: (&str, f64), c: (&str, f64)| {
        let case = by_id(&s, id);
        let j = judged(
            case,
            &[
                ("model", m.0, m.1),
                ("effort", e.0, e.1),
                ("context", c.0, c.1),
            ],
        );
        grade(&p, case, Some(&j))
    };
    // W18 wants opus, medium, full. Sonnet at 0.70 is under this model bar
    // (0.80): the `no_match` is opus, a hit. Medium at 0.70 is under 0.75:
    // `medium`, a hit. Full at 0.90 is under 0.95: `short`, a miss.
    let g = ask("W18", ("sonnet", 0.70), ("high", 0.70), ("full", 0.90));
    assert_eq!(
        graded_q(&g, "model"),
        ("opus".into(), true, "sonnet".into(), true)
    );
    assert_eq!(
        graded_q(&g, "effort"),
        ("medium".into(), true, "high".into(), true)
    );
    assert_eq!(
        graded_q(&g, "context"),
        ("short".into(), true, "full".into(), false)
    );
    assert_eq!(
        (g.model.confirm, g.effort.confirm, g.context.confirm),
        (0.80, 0.75, 0.95)
    );
    // The same answers sit above the stock bars (0.60) and are graded as given.
    let (stock, stock_set) = (pack(), set());
    let c = by_id(&stock_set, "W18");
    let j = judged(
        c,
        &[
            ("model", "sonnet", 0.70),
            ("effort", "high", 0.70),
            ("context", "full", 0.90),
        ],
    );
    let g = grade(&stock, c, Some(&j));
    assert!(!g.model.sub_confirm && !g.effort.sub_confirm && !g.context.sub_confirm);
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
            // Every case answered as labelled at 0.9, but L11 (opus, high,
            // full) at 0.5 on all three.
            let conf = if c.id == "L11" { 0.5 } else { 0.9 };
            let j = judged(
                c,
                &[
                    ("model", &c.want.model, conf),
                    ("effort", &c.want.effort, conf),
                    ("context", &c.want.context, conf),
                ],
            );
            grade(&p, c, Some(&j))
        })
        .collect();
    let t = totals(&all);
    // Model and effort fell (opus to sonnet, high to default); context fell to
    // `full`, which L11 wants.
    assert_eq!(
        (t.model_ok, t.effort_exact, t.effort_close, t.context_ok),
        (57, 57, 57, 58)
    );
    assert_eq!(
        (
            t.raw_model_ok,
            t.raw_effort_exact,
            t.raw_effort_close,
            t.raw_context_ok
        ),
        (58, 58, 58, 58)
    );
}
