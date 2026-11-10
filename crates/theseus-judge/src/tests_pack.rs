//! `pack.rs`'s own tests: the loader's rules on every embedded pack, the dynamic
//! options and the record round-trip, and what each pack version keeps of the last.
//!
//! They were an inline `mod tests` at the foot of `pack.rs`, 59 % of that file. They
//! read `pack`'s public surface only, so they sit beside it, as the crate's other
//! `tests_*` modules do, and the shape budget's 2,500 lines are `pack.rs`'s own.

use std::collections::BTreeMap;

use crate::client::{Kind, Question};
use crate::learn::RollbackRule;
use crate::names::hits;
use crate::pack::*;

const GOOD: &str = include_str!("../packs/probe.v1.toml");

fn refused(text: &str) -> Rule {
    match Pack::parse(text) {
        Ok(p) => panic!("{} loaded", p.name()),
        Err(e) => e.rule,
    }
}

/// The good pack with one line replaced.
fn with(from: &str, to: &str) -> String {
    assert!(GOOD.contains(from), "the test pack lacks {from:?}");
    GOOD.replacen(from, to, 1)
}

#[test]
fn every_embedded_pack_loads_and_matches_its_file_name() {
    let packs = embedded().as_ref().expect("embedded packs load");
    assert_eq!(packs.len(), EMBEDDED.len());
    let p = by_name("probe.v1").unwrap();
    assert_eq!(p.jev_model, "jev-1.13.0");
    assert_eq!(p.builder, Builder::Probe);
    assert!(by_name("probe.v9").is_none());
}

/// **The drift test** (theseus-ig13o). Every Choice question of every pack
/// we ship declares a `no_match` that is one of its own options, and every
/// `Asked` built from one carries that default forward, so
/// [`crate::AnswerRecord::ran`] always has something to run under the bar.
/// The loader refuses a Choice without one
/// (`a_choice_without_its_no_match_option_is_refused`), but that only binds
/// a pack as it loads: this binds the shipped set, and the carry.
///
/// The positive count is asserted, because "no Choice lacks a default" is
/// also true of a set with no Choice in it.
#[test]
fn every_choice_question_declares_a_no_match() {
    let packs = embedded().as_ref().expect("embedded packs load");
    let mut choices = 0;
    for p in packs {
        for q in p.questions.iter().filter(|q| q.kind == Kind::Choice) {
            choices += 1;
            let nm = q
                .no_match
                .as_deref()
                .unwrap_or_else(|| panic!("{} {} declares no no_match", p.name(), q.id));
            assert!(
                q.options.iter().any(|o| o.id == nm),
                "{} {}: the no_match {nm:?} is not one of its options",
                p.name(),
                q.id
            );
        }
    }
    assert!(
        choices >= 40,
        "only {choices} Choice questions across {} packs: a bar met by an \
         almost-empty set proves nothing",
        packs.len()
    );
    // And the default is carried onto every question as it is asked, which
    // is how it reaches the answer record.
    let route = by_name("route.v10").expect("route.v10");
    let asked = route.ask(&Dynamic::default());
    let model = asked
        .iter()
        .find(|a| a.def == "model")
        .expect("route.v10 asks `model`");
    assert_eq!(model.no_match.as_deref(), Some("parent"));
}

/// [`crate::AnswerRecord::ran`] is the one place a declared default is
/// read: Jev's own option at or above the question's `confirm` bar, the
/// declared default under it, and nothing for an answer that is no Choice.
#[test]
fn an_answer_runs_jevs_option_above_the_bar_and_the_declared_default_under_it() {
    use crate::band::band;
    use crate::client::Answer;
    use crate::judge::AnswerRecord;
    let route = by_name("route.v10").expect("route.v10");
    let def = route.question("model").expect("`model`");
    let rec = |confidence: f64| {
        let answer = Answer::Choice {
            choice: "opus".into(),
            probabilities: vec![("opus".to_string(), confidence)],
            confidence,
        };
        AnswerRecord {
            question: def.id.clone(),
            def: def.id.clone(),
            about: None,
            band: band(&answer, def.thresholds),
            no_match: def.no_match.clone(),
            answer,
        }
    };
    assert_eq!(def.thresholds.confirm, 0.40);
    assert_eq!(rec(0.95).ran(), Some("opus"), "at the act band, Jev's own");
    assert_eq!(
        rec(0.50).ran(),
        Some("opus"),
        "at the confirm band, Jev's own"
    );
    assert_eq!(
        rec(0.30).ran(),
        Some("parent"),
        "under the bar, the declared default"
    );
    // A question that declares none has nothing to run under its bar.
    let bare = AnswerRecord {
        no_match: None,
        ..rec(0.30)
    };
    assert_eq!(bare.ran(), None);
    // A Noul is not a Choice and runs nothing here.
    let noul = AnswerRecord {
        answer: Answer::Noul { noul: 0.9 },
        ..rec(0.95)
    };
    assert_eq!(noul.ran(), None);
}

#[test]
fn a_choice_without_its_no_match_option_is_refused() {
    assert_eq!(refused(&with("no_match = \"other\"\n", "")), Rule::NoMatch);
    assert_eq!(
        refused(&with("no_match = \"other\"", "no_match = \"elsewhere\"")),
        Rule::NoMatch
    );
}

#[test]
fn a_score_without_its_applies_noul_is_refused() {
    assert_eq!(
        refused(&with("applies = \"severity_applies\"\n", "")),
        Rule::ScoreApplies
    );
    assert_eq!(
        refused(&with(
            "applies = \"severity_applies\"",
            "applies = \"cause\""
        )),
        Rule::ScoreApplies
    );
    assert_eq!(
        refused(&with(
            "applies = \"severity_applies\"",
            "applies = \"nothing\""
        )),
        Rule::ScoreApplies
    );
}

#[test]
fn questions_that_ask_jev_to_compute_are_refused() {
    for bad in [
        "How many tool calls did the assistant make?",
        "On what date was the build last green?",
        "What percentage of the tests failed in this build?",
        "Can you look up the owner of this service?",
        "Calculate whether the cost passed the limit?",
    ] {
        let text = with(
            "instructions = \"Should a person look at this failure today?\"",
            &format!("instructions = \"{bad}\""),
        );
        assert_eq!(refused(&text), Rule::NoComputation, "{bad}");
    }
    assert_eq!(computation_in("How much of the ask is done?"), None);
    assert_eq!(computation_in("Count the files."), Some("count the"));
}

#[test]
fn instructions_must_carry_the_meaning() {
    let q = "instructions = \"Should a person look at this failure today?\"";
    assert_eq!(
        refused(&with(q, "instructions = \"Needs human?\"")),
        Rule::Instructions
    );
    assert_eq!(
        refused(&with(
            q,
            "instructions = \"A person should look at this today.\""
        )),
        Rule::Instructions
    );
    assert_eq!(
        refused(&with(
            q,
            "instructions = \"Is needs_human true for this failure?\""
        )),
        Rule::Instructions
    );
}

#[test]
fn the_model_is_pinned_and_never_latest() {
    assert_eq!(
        refused(&with(
            "jev_model = \"jev-1.13.0\"",
            "jev_model = \"jev-latest\""
        )),
        Rule::PinnedModel
    );
    assert_eq!(
        refused(&with(
            "jev_model = \"jev-1.13.0\"",
            "jev_model = \"jev-1.13\""
        )),
        Rule::PinnedModel
    );
}

#[test]
fn thresholds_caps_and_shapes_are_checked() {
    assert_eq!(refused(&with("act = 0.90", "act = 0.50")), Rule::Thresholds);
    assert_eq!(
        refused(&with("state_cap_tokens = 1000", "state_cap_tokens = 40000")),
        Rule::StateCap
    );
    assert_eq!(refused(&with("sample = 1.0", "sample = 1.5")), Rule::Sample);
    assert_eq!(
        refused(&with("id = \"probe\"", "id = \"Probe\"")),
        Rule::Identity
    );
    assert_eq!(
        refused(&with("action = \"none\"", "action = \"launch\"")),
        Rule::Syntax
    );
    assert_eq!(
        refused(&with("sample = 1.0", "sample = 1.0\nsurprise = 1")),
        Rule::Syntax
    );
    assert_eq!(
        refused(&with(
            "when_false = \"It can wait",
            "#when_false = \"It can wait"
        )),
        Rule::NoulShape
    );
}

#[test]
fn dynamic_options_come_first_and_the_static_ones_follow() {
    let text = with(
        "no_match = \"other\"",
        "no_match = \"other\"\noptions_from = \"tasks\"",
    );
    let p = Pack::parse(&text).unwrap();
    let mut d = Dynamic::default();
    d.sources.insert(
        Source::Tasks,
        vec![
            Item {
                key: "a1b2c3".into(),
                text: "Task a1b2c3: run the tests".into(),
            },
            Item {
                key: "other".into(),
                text: "collides with a static option".into(),
            },
            Item {
                key: "bad key!".into(),
                text: "not a safe id".into(),
            },
            Item {
                key: "a1b2c3".into(),
                text: "a duplicate".into(),
            },
        ],
    );
    let asked = p.ask(&d);
    let cause = asked.iter().find(|a| a.id == "cause").unwrap();
    let Question::Choice { options, .. } = &cause.question else {
        panic!()
    };
    let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "a1b2c3",
            "dependency_change",
            "flaky_infrastructure",
            "code_bug",
            "other"
        ]
    );
}

#[test]
fn a_pack_that_acts_names_rollback_rules_that_can_hold() {
    assert_eq!(
        refused(&with("action = \"none\"", "action = \"notice\"")),
        Rule::Rollback
    );
    let rule = |r: &str| format!("{GOOD}\n[[rollback]]\n{r}\n");
    let acts = |r: &str| rule(r).replacen("action = \"none\"", "action = \"notice\"", 1);
    assert!(Pack::parse(&acts("rule = \"notices_per_day\"\nmax = 30")).is_ok());
    for bad in [
        "rule = \"spend_ratio\"\nratio = 1.0\nmin_tasks = 10",
        "rule = \"spend_ratio\"\nratio = 2.0\nmin_tasks = 0",
        "rule = \"on_path_p95\"\nmax_ms = 0\nmin_samples = 20",
        "rule = \"on_path_p95\"\nmax_ms = 1000\nmin_samples = 0",
        "rule = \"labels_per_day\"\nlabel = \" \"\ncount = 3",
        "rule = \"labels_per_day\"\nlabel = \"noise\"\ncount = 0",
    ] {
        assert_eq!(refused(&rule(bad)), Rule::Rollback, "{bad}");
    }
    assert_eq!(refused(&rule("rule = \"nudge_loops\"")), Rule::Syntax);
    assert_eq!(
        refused(&rule("rule = \"notices_per_day\"\nmax = 30\nwhen = 1")),
        Rule::Syntax
    );
}

/// A pack's shape, one line per item: where it runs, each question's
/// kind, whether it decides, and its options, and the rollback rules.
fn shape(p: &Pack) -> Vec<String> {
    let mut out = vec![format!(
        "{:?} {:?} {:?} {:?}",
        p.point, p.builder, p.baseline, p.action
    )];
    for q in &p.questions {
        let mut line = format!("{} {:?}", q.id, q.kind);
        if q.decides {
            line.push_str(" decides");
            if let Some(bar) = q.decide_above {
                line.push_str(&format!(" above {bar}"));
            }
        }
        if let Some(s) = q.options_from {
            line.push_str(&format!(" from {s:?} +"));
        }
        if !q.options.is_empty() {
            let ids: Vec<&str> = q.options.iter().map(|o| o.id.as_str()).collect();
            line.push_str(&format!(" [{}]", ids.join(" ")));
        }
        if let Some(s) = q.per {
            line.push_str(&format!(" per {s:?} max {}", q.max));
        }
        if let Some(s) = q.only_when {
            line.push_str(&format!(" only_when {s:?}"));
        }
        out.push(line);
    }
    let rules: Vec<&str> = p.rollback.iter().map(RollbackRule::name).collect();
    out.push(format!("rollback [{}]", rules.join(" ")));
    out
}

/// Design §2.4's table and §2.7's rollback table, pack by pack.
#[test]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn the_six_packs_ask_what_the_design_says() {
    let want: &[(&str, &[&str])] = &[
        (
            "loop.v1",
            &[
                "LoopEnd Loop UntilNoToolCalls NudgeTask",
                "announced_unfinished Noul decides",
                "cost_out_of_proportion Noul",
                "same_action_repeating Noul",
                "stopping_point_defined Noul",
                "wants_human_input Noul",
                "work_state Choice decides [complete progressing blocked_needs_human thrashing off_task other]",
                "rollback [nudge_loop spend_ratio operator_stop_within_nudge on_path_p95]",
            ],
        ),
        (
            "security.v1",
            &[
                "Gate Security Posture Notice",
                "beyond_ask Noul",
                "destructive Noul",
                "exfiltrates Noul",
                "kind Choice [read_only local_edit local_exec remote_write publish credentials_or_config other]",
                "risky Noul decides",
                "steered Noul",
                "touches_credentials Noul",
                "rollback [notices_per_day labels_per_day]",
            ],
        ),
        (
            "security.v3",
            &[
                "Gate Security2 Posture None",
                "beyond_ask Noul",
                "destructive Noul",
                "exfiltrates Noul",
                "kind Choice [read_only local_edit local_exec remote_write publish credentials_or_config other]",
                "risky Noul decides",
                "safe Noul",
                "steered Noul decides above 0.75",
                "touches_credentials Noul",
                "rollback []",
            ],
        ),
        (
            "classify.v2",
            &[
                "Inbound Inbound Conversation ThreadRoute",
                "addressed_task Choice from Tasks + [none] only_when Tasks",
                "fragment Noul",
                "kind Choice decides [new_ask follow_up correction control addressed_to_task social other]",
                "mentions_other_conversation Noul",
                "should_promote Noul decides",
                "thread Choice decides from Threads + [new_thread home] only_when Threads",
                "wants_fresh_look Noul",
                "rollback [labels_per_day]",
            ],
        ),
        (
            "classify.v3",
            &[
                "Inbound Inbound Conversation ThreadRoute",
                "addressed_task Choice from Tasks + [none] only_when Tasks",
                "fragment Noul",
                "kind Choice decides [new_ask follow_up correction control addressed_to_task social other]",
                "mentions_other_conversation Noul",
                "offer_task Noul",
                "should_promote Noul decides",
                "thread Choice decides from Threads + [new_thread home] only_when Threads",
                "wants_fresh_look Noul",
                "rollback [labels_per_day]",
            ],
        ),
        (
            "classify.v4",
            &[
                "Inbound Inbound Conversation ThreadRoute",
                "addressed_task Choice from Tasks + [none] only_when Tasks",
                "fragment Noul",
                "kind Choice decides [new_ask follow_up correction control addressed_to_task social other]",
                "mentions_other_conversation Noul",
                "offer_task Noul",
                "scope Choice [thread place] only_when Scope",
                "should_promote Noul decides",
                "thread Choice decides from Threads + [new_thread home] only_when Threads",
                "wants_fresh_look Noul",
                "rollback [labels_per_day]",
            ],
        ),
        (
            "role.v1",
            &[
                "Inbound Inbound CurrentRole RoleHint",
                "role Choice decides from Roles + [other] only_when Roles",
                "rollback [switches_per_exchange labels_per_day]",
            ],
        ),
        (
            "directive.v1",
            &[
                "Inbound Inbound None Capture",
                "kind Choice decides [none order preference fact correction]",
                "scope Choice [session place everywhere unsure]",
                "rollback [labels_per_day]",
            ],
        ),
        (
            "continue.v1",
            &[
                "Compile Continue Append None",
                "decision Choice decides [append recompile_transcript recompile_ring recompile_compaction recompile_fresh other]",
                "stronger_model_for_compaction Noul",
                "rollback []",
            ],
        ),
        (
            "categorize.v1",
            &[
                "ExchangeEnd Categorize NoMembership None",
                "still_member Noul per Memberships max 5",
                "topic Choice decides from Topics + [new_topic none]",
                "rollback []",
            ],
        ),
        (
            "rerank.v1",
            &[
                "Recall Rerank Fused None",
                "helps Noul per Notes max 10",
                "helps_more Noul per MoreNotes max 10",
                "rollback []",
            ],
        ),
        (
            "route.v5",
            &[
                "Inbound Inbound SessionProfile Route",
                "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding review other]",
                "persona Choice from Personas + [prior] only_when Personas",
                "reply_effort Choice decides [low medium high xhigh max unclear]",
                "seal Choice [yes no] only_when ThreadNow",
                "rollback [labels_per_day on_path_p95]",
            ],
        ),
        (
            "route.v6",
            &[
                "Inbound Inbound SessionProfile Route",
                "data Choice [open private unclear]",
                "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding review other]",
                "persona Choice from Personas + [prior] only_when Personas",
                "reply_effort Choice decides [low medium high xhigh max unclear]",
                "seal Choice [yes no] only_when ThreadNow",
                "rollback [labels_per_day on_path_p95]",
            ],
        ),
        (
            "route.v7",
            &[
                "Inbound Inbound SessionProfile Route",
                "data Choice [open private unclear]",
                "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding review other]",
                "persona Choice from Personas + [prior] only_when Personas",
                "personal Choice [none elicit expose]",
                "reply_effort Choice decides [low medium high xhigh max unclear]",
                "seal Choice [yes no] only_when ThreadNow",
                "rollback [labels_per_day on_path_p95]",
            ],
        ),
        (
            "route.v8",
            &[
                "Inbound Inbound SessionProfile Route",
                "data Choice [open private unclear]",
                "mode Choice decides [trivial quick chat sophisticated deep_coding routine_coding review other]",
                "persona Choice from Personas + [prior] only_when Personas",
                "personal Choice [none elicit expose]",
                "reply_effort Choice decides [low medium high xhigh max unclear]",
                "seal Choice [yes no] only_when ThreadNow",
                "rollback [labels_per_day on_path_p95]",
            ],
        ),
        (
            "route.v10",
            &[
                "Inbound Route SessionProfile Route",
                "model Choice decides [haiku sonnet opus fable deepseek_flash parent]",
                "persona Choice from Personas + [prior] only_when Personas",
                "personal Choice [none elicit expose]",
                "seal Choice [yes no] only_when ThreadNow",
                "rollback [labels_per_day on_path_p95 pins_per_day]",
            ],
        ),
        (
            "effort.v1",
            &[
                "Inbound Effort None None",
                "effort Choice decides [low medium high xhigh max]",
                "rollback []",
            ],
        ),
        (
            "work.v1",
            &[
                "Inbound Work None None",
                "work Choice decides [coding research_or_design writing conversation tool_run review]",
                "rollback []",
            ],
        ),
    ];
    for (name, lines) in want {
        let p = by_name(name).unwrap_or_else(|| panic!("{name} is embedded"));
        assert_eq!(shape(&p), *lines, "{name}");
        assert_eq!(p.jev_model, "jev-1.13.0", "{name}");
        assert_eq!(p.sample, 1.0, "{name}: Q16 samples every event");
        // Conservative, but for `directive.v1`, whose bands the owner set:
        // keep at 0.80, ask from 0.50 (2026-10-13, killer OD5).
        let bands = match *name {
            "directive.v1" => (0.80, 0.50),
            _ => (0.90, 0.60),
        };
        for q in &p.questions {
            // route.v10's `model` confirms at 0.40 (theseus-h66v4.7, the owner 2026-11-03: "adjust
            // the instruction prompt, test assumptions and bar until we're safely good").
            let bands = if (*name, q.id.as_str()) == ("route.v10", "model") {
                (0.90, 0.40)
            } else {
                bands
            };
            assert_eq!(
                (q.thresholds.act, q.thresholds.confirm),
                bands,
                "{name}/{}: thresholds start conservative",
                q.id
            );
        }
    }
    // §2.7's numbers, as the rules hold them.
    let rules = |n: &str| by_name(n).unwrap().rollback.clone();
    assert_eq!(
        rules("loop.v1")[1],
        RollbackRule::SpendRatio {
            ratio: 2.0,
            min_tasks: 10
        }
    );
    assert_eq!(
        rules("loop.v1")[3],
        RollbackRule::OnPathP95 {
            max_ms: 1000,
            min_samples: 20
        }
    );
    assert_eq!(
        rules("security.v1"),
        vec![
            RollbackRule::NoticesPerDay { max: 30 },
            RollbackRule::LabelsPerDay {
                label: "noise".into(),
                count: 3
            }
        ]
    );
    assert_eq!(
        rules("route.v5"),
        vec![
            RollbackRule::LabelsPerDay {
                label: "wrong model".into(),
                count: 3
            },
            RollbackRule::OnPathP95 {
                max_ms: 250,
                min_samples: 20
            }
        ]
    );
    assert_eq!(
        rules("route.v6"),
        rules("route.v5"),
        "route.v6 keeps v5's rules"
    );
    assert_eq!(
        rules("route.v7"),
        rules("route.v6"),
        "route.v7 keeps v6's rules"
    );
    assert_eq!(
        rules("route.v8"),
        rules("route.v7"),
        "route.v8 keeps v7's rules"
    );
    assert_eq!(
        rules("classify.v2"),
        vec![RollbackRule::LabelsPerDay {
            label: "wrong thread".into(),
            count: 3
        }]
    );
    assert_eq!(
        rules("classify.v3"),
        rules("classify.v2"),
        "classify.v3 keeps v2's rules"
    );
    assert_eq!(
        rules("classify.v4"),
        rules("classify.v2"),
        "classify.v4 keeps v2's rules"
    );
    assert_eq!(
        rules("directive.v1"),
        vec![RollbackRule::LabelsPerDay {
            label: "wrong capture".into(),
            count: 3
        }]
    );
    assert_eq!(
        rules("role.v1"),
        vec![
            RollbackRule::SwitchesPerExchange { max: 2 },
            RollbackRule::LabelsPerDay {
                label: "wrong role".into(),
                count: 2
            }
        ]
    );
}

/// Each rule, broken in each of the six pack files, refuses that file.
#[test]
fn the_loaders_rules_hold_on_all_the_packs() {
    let six: Vec<&(&str, &str)> = EMBEDDED.iter().filter(|(f, _)| *f != "probe.v1").collect();
    assert_eq!(
        six.len(),
        39,
        "§2.4's six, security.v3, rerank.v1, memory.v1, attribution.v1, route.v5 to v8, route.v10, effort.v1, work.v1, \
         citation.v1, people.v1, people_seen.v1, classify.v2 to v4, compare.v1, heard.v1, directive.v1, \
         onto.v1, same_thing.v1, two_things.v1, same_kind.v1, name_it.v1, recall_cross.v1, families.v1, \
         content.v1 to v4, split.v1, undo.v1 and delay.v1"
    );
    for (file, text) in six {
        let p = Pack::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
        let edit = |from: &str, to: &str| {
            assert!(text.contains(from), "{file} lacks {from:?}");
            text.replacen(from, to, 1)
        };
        let check = |broken: String, rule: Rule, what: &str| {
            assert_eq!(refused(&broken), rule, "{file}: {what}");
        };
        check(
            edit("jev_model = \"jev-1.13.0\"", "jev_model = \"jev-latest\""),
            Rule::PinnedModel,
            "jev-latest",
        );
        check(
            edit(
                &format!("state_cap_tokens = {}", p.state_cap_tokens),
                "state_cap_tokens = 40000",
            ),
            Rule::StateCap,
            "a cap past Jev's limit",
        );
        let act_line = text
            .lines()
            .find(|l| l.starts_with("act = "))
            .unwrap_or_else(|| panic!("{file} has no act line"));
        check(
            edit(act_line, "act = 0.10"),
            Rule::Thresholds,
            "act below confirm",
        );
        for q in &p.questions {
            let line = format!("instructions = \"{}\"", q.instructions);
            check(
                edit(
                    &line,
                    &format!("instructions = \"Calculate this: {}\"", q.instructions),
                ),
                Rule::NoComputation,
                &q.id,
            );
            check(
                edit(&line, "instructions = \"Is it?\""),
                Rule::Instructions,
                &q.id,
            );
            if let Some(nm) = &q.no_match {
                check(
                    edit(&format!("no_match = \"{nm}\"\n"), ""),
                    Rule::NoMatch,
                    &q.id,
                );
            }
        }
        if p.action != Action::None {
            let cut = text
                .find("[[rollback]]")
                .expect("a pack that acts has rules");
            check(text[..cut].to_string(), Rule::Rollback, "no rollback rules");
        }
    }
}

/// `decide_above` rides on a deciding whole Noul, within confirm..=act.
#[test]
fn decide_above_belongs_to_a_deciding_noul_within_its_thresholds() {
    let v3 = include_str!("../packs/security.v3.toml");
    let ok = Pack::parse(v3).unwrap();
    assert_eq!(ok.question("steered").unwrap().decide_above, Some(0.75));
    assert_eq!(ok.question("risky").unwrap().decide_above, None);
    let edit = |from: &str, to: &str| {
        assert!(v3.contains(from), "{from:?}");
        v3.replacen(from, to, 1)
    };
    // Not deciding.
    let s = edit("decides = true\ndecide_above = 0.75", "decide_above = 0.75");
    assert_eq!(refused(&s), Rule::DecideAbove);
    // Below confirm, above act.
    let s = edit("decide_above = 0.75", "decide_above = 0.5");
    assert_eq!(refused(&s), Rule::DecideAbove);
    let s = edit("decide_above = 0.75", "decide_above = 0.95");
    assert_eq!(refused(&s), Rule::DecideAbove);
    // Edges are inside.
    Pack::parse(&edit("decide_above = 0.75", "decide_above = 0.60")).unwrap();
    Pack::parse(&edit("decide_above = 0.75", "decide_above = 0.90")).unwrap();
    // On a Choice.
    let s = edit(
        "no_match = \"other\"",
        "no_match = \"other\"\ndecides = true\ndecide_above = 0.75",
    );
    assert_eq!(refused(&s), Rule::DecideAbove);
}

/// Every pack that exists before `decide_above` loads to the same
/// checked definition: no bar, and a serialization without the field.
#[test]
fn a_pack_without_decide_above_serializes_as_before() {
    for (file, text) in EMBEDDED.iter().filter(|(f, _)| *f != "security.v3") {
        let p = Pack::parse(text).unwrap();
        assert!(
            p.questions.iter().all(|q| q.decide_above.is_none()),
            "{file}"
        );
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("decide_above"), "{file}");
    }
}

/// split.v1 (theseus-z9mze.5, promoted by theseus-h66v4.6) is one question, asked in the inbound
/// batch (the `inbound` state), and now a deciding one: its answer is the fan-out cue. It asks a
/// property of the work, never a recommendation about the machinery: no word of helpers, tasks,
/// cost or fanning out.
#[test]
fn split_v1_is_one_deciding_question_about_the_work() {
    let p = by_name("split.v1").expect("split.v1 is embedded");
    assert_eq!((p.point, p.action), (Point::Inbound, Action::None));
    assert_eq!(p.questions.len(), 1);
    let q = p.question("split").unwrap();
    assert!(
        q.decides && q.kind == Kind::Choice,
        "promoted: it decides the cue"
    );
    assert_eq!(q.no_match.as_deref(), Some("unclear"));
    let ids: Vec<&str> = q.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["splits_by_area", "one_area", "unclear"]);
    let said = format!(
        "{} {}",
        q.instructions,
        q.options
            .iter()
            .filter_map(|o| o.means.as_deref())
            .collect::<String>()
    )
    .to_lowercase();
    for word in ["helper", "task", "cost", "fan out", "fan-out", "budget"] {
        assert!(!said.contains(word), "the question names {word:?}");
    }
    assert_eq!(
        p.builder,
        by_name("classify.v4").unwrap().builder,
        "it batches with classify"
    );
}

/// route.v5 (theseus-c1uz) asks two questions that decide nothing for the
/// pack's promotion: `persona`, whose options are the personas and whose
/// no-match is `prior`, and `seal`, a yes or no.
#[test]
fn route_v5_has_a_persona_and_a_seal_question() {
    let v5 = by_name("route.v5").unwrap();
    let persona = v5.question("persona").unwrap();
    assert!(!persona.decides && persona.kind == Kind::Choice);
    assert_eq!(persona.no_match.as_deref(), Some("prior"));
    assert_eq!(persona.options_from, Some(Source::Personas));
    let seal = v5.question("seal").unwrap();
    assert!(!seal.decides);
    assert_eq!(seal.no_match.as_deref(), Some("no"));
    assert_eq!(seal.only_when, Some(Source::ThreadNow));
}

/// route.v6 (theseus-a1wvq.2) is route.v5 word for word, its rules and its
/// description's opening included, and one question that decides nothing for
/// the pack's promotion: `data`, three options, `unclear` when nothing fits.
#[test]
fn route_v6_is_v5_word_for_word_and_a_data_question() {
    let (v5, v6) = (by_name("route.v5").unwrap(), by_name("route.v6").unwrap());
    for q in ["mode", "reply_effort", "persona", "seal"] {
        assert_eq!(v6.question(q), v5.question(q), "{q} word for word");
    }
    let ids: Vec<&str> = v6.questions.iter().map(|q| q.id.as_str()).collect();
    assert_eq!(ids.len(), 5, "v5's four and data: {ids:?}");
    let data = v6.question("data").unwrap();
    assert!(!data.decides && data.kind == Kind::Choice);
    assert_eq!((data.thresholds.act, data.thresholds.confirm), (0.90, 0.60));
    assert_eq!(data.no_match.as_deref(), Some("unclear"));
    let options: Vec<&str> = data.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(options, ["open", "private", "unclear"]);
    assert_eq!(v6.rollback, v5.rollback, "both rules, word for word");
    assert_eq!(v6.version, 6);
    assert_eq!(
        (v6.point, v6.builder, v6.baseline, v6.action),
        (v5.point, v5.builder, v5.baseline, v5.action)
    );
    assert_eq!(v6.state_cap_tokens, v5.state_cap_tokens);
    let opening = v5.description.trim_end_matches(['.', '"']);
    let opening = opening.replace("and whether it warrants", "whether it warrants");
    assert!(
        v6.description.starts_with(&opening),
        "v6's description keeps v5's: {}",
        v6.description
    );
    assert!(v6.description.contains("outside the owner's trust"));
}

/// route.v7 (theseus-a1wvq) is route.v6 word for word, but for the `data`
/// question's wording: the same id, options, thresholds and no-match, so
/// what reads the answer reads it unchanged; `private` asks for a sign of
/// private matter, and work whose owner the message does not show is
/// `unclear`, which fails closed as `private` does. And one question that
/// decides nothing, `personal` (theseus-gasov): `none`, `elicit` or
/// `expose`, `none` when nothing fits.
#[test]
fn route_v7_is_v6_word_for_word_but_the_data_wording() {
    let (v6, v7) = (by_name("route.v6").unwrap(), by_name("route.v7").unwrap());
    for q in ["mode", "reply_effort", "persona", "seal"] {
        assert_eq!(v7.question(q), v6.question(q), "{q} word for word");
    }
    let options = |q: &QuestionDef| q.options.iter().map(|o| o.id.clone()).collect::<Vec<_>>();
    let ids = |p: &Pack| p.questions.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
    let mut want = ids(&v6);
    want.push("personal".into());
    let mut got = ids(&v7);
    want.sort();
    got.sort();
    assert_eq!(got, want, "v6's five and personal");
    let personal = v7.question("personal").unwrap();
    assert!(!personal.decides && personal.kind == Kind::Choice);
    assert_eq!(personal.no_match.as_deref(), Some("none"));
    assert_eq!(options(personal), ["none", "elicit", "expose"]);
    let (d6, d7) = (v6.question("data").unwrap(), v7.question("data").unwrap());
    assert_ne!(d7.instructions, d6.instructions);
    assert_eq!(
        (&d7.decides, &d7.kind, &d7.thresholds, &d7.no_match),
        (&d6.decides, &d6.kind, &d6.thresholds, &d6.no_match)
    );
    let opt = |q: &QuestionDef, id: &str| {
        q.options
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .means
            .clone()
            .unwrap_or_default()
    };
    assert_eq!(options(d7), options(d6));
    assert!(d7.instructions.contains("not by the place"));
    assert!(opt(d7, "open").contains("self-contained exercise"));
    assert!(opt(d7, "unclear").contains("whose owner it does not show"));
    assert!(!opt(d7, "private").contains("work for a company"));
    assert_eq!(v7.rollback, v6.rollback, "both rules, word for word");
    assert_eq!(v7.version, 7);
    assert_eq!(
        (
            v7.point,
            v7.builder,
            v7.baseline,
            v7.action,
            v7.state_cap_tokens
        ),
        (
            v6.point,
            v6.builder,
            v6.baseline,
            v6.action,
            v6.state_cap_tokens
        )
    );
    let opening = v6.description.trim_end_matches('.');
    assert!(v7.description.starts_with(opening), "{}", v7.description);
}

/// route.v8 (theseus-f9qa8) is route.v7 word for word, but for the `data`
/// question's `open` and `unclear` options: `open` names the public signs
/// and says an unshown owner is no sign of private matter; `unclear` stops
/// counting an unshown owner; `private`, the instructions, the thresholds
/// and the no-match are v7's, so what reads the answer reads it unchanged;
/// and `reply_effort`'s `max` (theseus-ujzuh), which adds the evidence a
/// checkpoint shows to v7's words.
#[test]
fn route_v8_is_v7_word_for_word_but_the_data_options_and_effort_max() {
    let (v7, v8) = (by_name("route.v7").unwrap(), by_name("route.v8").unwrap());
    for q in ["mode", "persona", "seal", "personal"] {
        assert_eq!(v8.question(q), v7.question(q), "{q} word for word");
    }
    let ids = |p: &Pack| p.questions.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&v8), ids(&v7), "the same questions");
    let (d7, d8) = (v7.question("data").unwrap(), v8.question("data").unwrap());
    // A question's frame: all but its options.
    let frame = |q: &QuestionDef| {
        (
            q.instructions.clone(),
            q.decides,
            q.kind,
            q.thresholds,
            q.no_match.clone(),
        )
    };
    assert_eq!(frame(d8), frame(d7));
    let opts = |q: &QuestionDef| q.options.iter().map(|o| o.id.clone()).collect::<Vec<_>>();
    assert_eq!(opts(d8), ["open", "private", "unclear"]);
    assert_eq!(opts(d8), opts(d7));
    let means = |q: &QuestionDef, id: &str| {
        q.options
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .means
            .clone()
            .unwrap_or_default()
    };
    let (e7, e8) = (
        v7.question("reply_effort").unwrap(),
        v8.question("reply_effort").unwrap(),
    );
    assert_eq!(frame(e8), frame(e7));
    assert_eq!(opts(e8), opts(e7));
    for id in opts(e7).iter().filter(|id| *id != "max") {
        assert_eq!(
            means(e8, id),
            means(e7, id),
            "reply_effort {id} word for word"
        );
    }
    assert_eq!(
        means(e8, "max"),
        "The most thought there is: the hardest or most novel problems, or high-stakes work where every step must be checked, or the person asks for the most thorough reply possible, or the work has kept failing after more careful effort was already given, or a check it must pass still fails."
    );
    assert_eq!(
        means(d8, "private"),
        means(d7, "private"),
        "private verbatim"
    );
    let (open7, open8) = (means(d7, "open"), means(d8, "open"));
    assert!(
        open8.starts_with(&open7),
        "open keeps v7's words, then adds"
    );
    assert!(open8.contains("public licence") && open8.contains("toolchain"));
    assert!(open8.contains("not a sign of private matter"));
    let unclear = means(d8, "unclear");
    assert!(unclear.contains("mixes open and private matter"));
    assert!(!unclear.contains("owner it does not show"), "{unclear}");
    assert_eq!(v8.rollback, v7.rollback, "both rules, word for word");
    assert_eq!(v8.version, 8);
    assert_eq!(
        (
            v8.point,
            v8.builder,
            v8.baseline,
            v8.action,
            v8.state_cap_tokens
        ),
        (
            v7.point,
            v7.builder,
            v7.baseline,
            v7.action,
            v7.state_cap_tokens
        )
    );
    assert_eq!(v8.description, v7.description);
}

/// One question of a pack, or a panic naming it.
fn q<'a>(p: &'a Pack, n: &str) -> &'a QuestionDef {
    p.question(n).unwrap_or_else(|| panic!("{n}"))
}

/// Everything about `content.v2` that is not its wording is `content.v1`'s
/// (theseus-78qay): the same id, builder, point, cap, sample, baseline and
/// action, the same four questions deciding at the same bars, and the same
/// rollback rules. `injection` is byte-identical, since its `no` already said
/// that discussing prompt injection is not attempting one.
#[test]
fn content_v2_keeps_v1s_shape_and_its_injection_question() {
    let (v1, v2) = (
        by_name("content.v1").unwrap(),
        by_name("content.v2").unwrap(),
    );
    assert_eq!(v2.version, 2);
    assert_eq!(v2.id, v1.id);
    assert_eq!(
        (
            v2.point,
            v2.builder,
            v2.baseline,
            v2.action,
            v2.state_cap_tokens,
            v2.sample
        ),
        (
            v1.point,
            v1.builder,
            v1.baseline,
            v1.action,
            v1.state_cap_tokens,
            v1.sample
        )
    );
    let ids = |p: &Pack| p.questions.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&v2), ids(&v1));
    for n in ["confidential", "injection", "personal", "secret"] {
        let (a, b) = (q(&v2, n), q(&v1, n));
        assert_eq!(
            (
                &a.decides,
                &a.kind,
                &a.thresholds,
                &a.no_match,
                &a.only_when
            ),
            (
                &b.decides,
                &b.kind,
                &b.thresholds,
                &b.no_match,
                &b.only_when
            ),
            "{n}"
        );
        assert!(a.decides, "{n}");
    }
    assert_eq!(q(&v2, "injection"), q(&v1, "injection"));
    assert_eq!(v2.rollback, v1.rollback, "both rules, word for word");
    assert_ne!(v2.description, v1.description, "it reveals, not shows");
}

/// `content.v2`'s one idea, added and nothing taken away (theseus-78qay, the
/// owner 2026-10-27: a flag describes what an item REVEALS, not what it is
/// ABOUT). Each of the three revealing questions carries the rule in its
/// `instructions` and names the three near-misses in its `no`.
#[test]
fn content_v2_adds_the_reveal_rule_and_changes_nothing_else() {
    let (v1, v2) = (
        by_name("content.v1").unwrap(),
        by_name("content.v2").unwrap(),
    );
    let means = |d: &QuestionDef, id: &str| {
        d.options
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .means
            .clone()
            .unwrap_or_default()
    };
    // Each revealing question carries the reveal rule in the question itself,
    // where the text being judged cannot argue with it. It does NOT ask whether
    // the party is real: a first draft did, and on the live set it cost recall
    // 1.000 -> 0.623, because one item rarely shows whose anything is and Jev
    // answered "cannot tell" as "no". The rule that holds is reveal-versus-about,
    // and the near-misses are named in `no`, where they are observable.
    for n in ["secret", "confidential", "personal"] {
        let (a, b) = (q(&v2, n), q(&v1, n));
        assert_ne!(a.instructions, b.instructions, "{n}: v2 rewords it");
        assert!(
            a.instructions.contains("not what it is about"),
            "{n}: {}",
            a.instructions
        );
        assert!(
            !a.instructions.contains("real"),
            "{n}: `real` cost recall; the rule is reveal-versus-about: {}",
            a.instructions
        );
        // `yes` keeps v1's words and then adds; `no` grows, never shrinks.
        assert!(
            means(a, "yes").starts_with(&means(b, "yes")),
            "{n}: v2's yes drops words v1's had"
        );
        assert!(means(a, "no").len() > means(b, "no").len(), "{n}");
    }
    // And each `no` names the three near-misses v1 flagged on.
    for (n, words) in [
        (
            "secret",
            &[
                "about credentials",
                "exercise's own answer",
                "working output",
                // Scoped to the exercise, never to the tree: a password in an
                // ordinary project's configuration is a credential. A review of
                // 2026-10-27 found the unscoped version, and the set now holds
                // `sec_14` and `sec_15` against it.
                "whatever directory it sits in",
            ][..],
        ),
        (
            "confidential",
            &[
                "topic of confidentiality",
                "self-contained technical exercise",
                "invented organisation names",
                // A real organisation's own test suite is a test suite; its
                // fixtures are not exempt for that reason (`conf_14`).
                "actual customers",
            ][..],
        ),
        (
            "personal",
            &[
                "topic of privacy",
                "Placeholder identities",
                "benchmark task",
            ][..],
        ),
    ] {
        let no = means(q(&v2, n), "no");
        for w in words {
            assert!(no.contains(w), "{n}'s no lacks {w:?}: {no}");
        }
    }
}

/// `content.v3` (theseus-gyjyo.11) keeps everything of `content.v2` but its
/// questions' wording: the same id, builder, point, cap, bars, rollback, and
/// `injection` byte for byte, with the reveal rule still in each revealing
/// question's `instructions` and `yes` still starting with v2's words. What
/// v3 adds goes in each `no`, as the explicit cues for the five new fields,
/// and it adds them, never replaces: v2's three near-misses and `sec_14`'s
/// ordinary-project clause stay.
#[test]
fn content_v3_keeps_v2s_shape_and_adds_the_field_cues_to_each_no() {
    let (v2, v3) = (
        by_name("content.v2").unwrap(),
        by_name("content.v3").unwrap(),
    );
    assert_eq!(v3.version, 3);
    assert_eq!(v3.id, v2.id);
    assert_eq!(
        (
            v3.point,
            v3.builder,
            v3.baseline,
            v3.action,
            v3.state_cap_tokens,
            v3.sample
        ),
        (
            v2.point,
            v2.builder,
            v2.baseline,
            v2.action,
            v2.state_cap_tokens,
            v2.sample
        )
    );
    assert_eq!(v3.rollback, v2.rollback);
    assert_eq!(v3.jev_model, v2.jev_model);
    assert_eq!(
        q(&v3, "injection"),
        q(&v2, "injection"),
        "injection is v2's, byte for byte"
    );
    let means = |d: &QuestionDef, id: &str| {
        d.options
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .means
            .clone()
            .unwrap_or_default()
    };
    for n in ["secret", "confidential", "personal"] {
        let (a, b) = (q(&v3, n), q(&v2, n));
        assert_eq!(
            a.instructions, b.instructions,
            "{n}: the reveal rule stays as it was"
        );
        assert!(a.instructions.contains("not what it is about"), "{n}");
        assert_eq!(
            (&a.decides, &a.thresholds, &a.no_match, &a.only_when),
            (&b.decides, &b.thresholds, &b.no_match, &b.only_when),
            "{n}"
        );
        assert!(
            means(a, "yes").starts_with(&means(b, "yes")),
            "{n}: v3's yes drops v2's words"
        );
        let (no3, no2) = (means(a, "no"), means(b, "no"));
        assert!(no3.starts_with(&no2), "{n}: v3's no drops v2's words");
        // The cue for the fields this question reads, in words the state's keys
        // appear in, so the pack and the builder cannot drift apart.
        assert!(
            no3.contains("`public`"),
            "{n}'s no does not say what `public` means: {no3}"
        );
        assert!(
            no3.contains("`context`"),
            "{n}'s no does not read `context`: {no3}"
        );
    }
    // The clause that keeps `sec_14` and `sec_15` flagging is untouched, and v3
    // says outright that public does not mean clean.
    assert!(means(q(&v3, "secret"), "no").contains("whatever directory it sits in"));
    for n in ["secret", "confidential"] {
        let no = means(q(&v3, n), "no");
        assert!(no.contains("still"), "{n}: public is not clean: {no}");
    }
    assert!(means(q(&v3, "confidential"), "no").contains("`author`"));
    assert!(means(q(&v3, "confidential"), "no").contains("`project`"));
    assert_ne!(v3.description, v2.description);
}

/// No pack's text names an organisation: the matcher runs over every pack
/// against the template's `[privacy] sensitive_names` line, the owner's
/// company and repository names (the owner, 2026-10-17: "just use the
/// config", not a hashed name), and the mechanism is proved on a planted
/// text. The daemon also checks every embedded pack against the operator's
/// own list at start (`names::packs_naming`, theseus-oejh3).
#[test]
fn the_pack_text_names_no_organisation() {
    let template = include_str!("../../theseus-core/config/theseus.example.toml");
    let line = template
        .lines()
        .find_map(|l| l.strip_prefix("# sensitive_names = "))
        .expect("the template's [privacy] sensitive_names line");
    let names: Vec<String> = toml::from_str::<toml::Table>(&format!("n = {line}")).unwrap()["n"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["examplecorp", "examplecorpinc", "example-repo"]);
    // The repository is public: the template names none of the old
    // line's four names, which are held only as digests.
    assert!(
        !crate::names::names_an_old_name(template, &mut Default::default()),
        "the template names an old name"
    );
    // The directory, not `EMBEDDED`: a case file beside a pack is no pack, and it is where two
    // names got through (theseus-ev3ir). That is the LOCATION: neither of those two names is one this
    // test's name sets match (they are the template's three organisations and `names::OLD`'s digests).
    // The name scrub the gate runs over a branch's added lines (`scripts/scrub-added.sh`) is what holds
    // the configured and vault names; this test holds the public-safety floor over the whole directory.
    let packs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packs");
    for (name, text) in swept_texts(&packs) {
        assert_eq!(hits(&text, &names), Vec::<String>::new(), "{name}");
    }
    let planted = format!("{}\nExampleCorp's private plans", EMBEDDED[0].1);
    assert_eq!(hits(&planted, &names), ["examplecorp"]);
}

/// The text of every file under the packs directory, subdirectories and any extension
/// included: a pack, its cases, anything put beside them or below them (theseus-ev3ir). Named
/// by the path relative to `dir`.
fn swept_texts(dir: &std::path::Path) -> Vec<(String, String)> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
                out.push((name, text));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// A `.cases.toml` is a case file, not a pack, so it is not in
/// `EMBEDDED`: two sensitive names reached public refs in one, and the
/// pack-text test never read it (theseus-ev3ir). The sweep reads the
/// directory, and a fake name planted in a new file there is found, in a
/// subdirectory and in a file that is no `.toml` too.
#[test]
fn a_cases_file_beside_the_packs_is_swept() {
    let dir = std::env::temp_dir().join(format!("theseus-ev3ir-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("cases")).unwrap();
    let planted = "[[case]]\nlabel = \"ExampleCorp's private plans\"\n";
    for file in ["zz.cases.toml", "cases/nested.cases.toml", "notes.md"] {
        std::fs::write(dir.join(file), planted).unwrap();
    }
    std::fs::write(dir.join("clean.toml"), "label = \"nothing\"\n").unwrap();
    let names = vec!["examplecorp".to_string()];
    let texts = swept_texts(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    let found: Vec<&str> = texts
        .iter()
        .filter(|(_, text)| !hits(text, &names).is_empty())
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        found,
        ["cases/nested.cases.toml", "notes.md", "zz.cases.toml"],
        "the planted name was not found everywhere it was put"
    );
}

/// The real packs directory: the sweep reads every embedded pack (byte for byte), and at
/// least one case file, so it is not an empty or a pack-only sweep; and nothing in it names
/// the template's organisations or one of the old names held as digests.
#[test]
fn the_packs_directory_names_no_organisation() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packs");
    let texts = swept_texts(&dir);
    for (name, text) in EMBEDDED {
        let swept = texts.iter().find(|(n, _)| n == &format!("{name}.toml"));
        assert_eq!(
            swept.map(|(_, t)| t.as_str()),
            Some(*text),
            "the sweep does not hold the embedded pack {name}"
        );
    }
    assert!(
        texts.iter().any(|(n, _)| n.ends_with(".cases.toml")),
        "the sweep reads no case file"
    );
    let names = [
        "examplecorp",
        "examplecorpinc",
        "example-repo",
        "example-utils",
    ]
    .map(String::from);
    let mut clean = Default::default();
    for (name, text) in &texts {
        assert_eq!(hits(text, &names), Vec::<String>::new(), "{name}");
        assert!(
            !crate::names::names_an_old_name(text, &mut clean),
            "{name} names an old name"
        );
    }
}

/// A judgment's record of its dynamic sources (theseus-gyjyo.1): every
/// source the pack's questions draw on, an empty one included, and only
/// those; read back for a pack that needs the same, and refused, naming
/// them, for one that needs another.
#[test]
fn recorded_sources_round_trip_and_name_what_is_missing() {
    let pack = by_name("content.v2").unwrap();
    assert_eq!(
        Dynamic::needed(&pack).into_iter().collect::<Vec<_>>(),
        [Source::Untrusted]
    );
    let held = Dynamic {
        sources: BTreeMap::from([
            (
                Source::Untrusted,
                vec![Item {
                    key: "untrusted".into(),
                    text: "not the operator's own words".into(),
                }],
            ),
            (Source::Tasks, vec![]),
        ]),
        assets: vec![],
    };
    let rec = held.recorded(&pack);
    assert_eq!(
        rec,
        serde_json::json!({"untrusted": [{"key": "untrusted",
            "text": "not the operator's own words"}]})
    );
    let back = Dynamic::from_record(&rec, &pack).unwrap();
    assert_eq!(back.items(Source::Untrusted), held.items(Source::Untrusted));
    // Nothing found is recorded as an empty list, which is a source held.
    let none = Dynamic::default().recorded(&pack);
    assert_eq!(none, serde_json::json!({"untrusted": []}));
    assert!(Dynamic::from_record(&none, &pack).is_ok());
    // A record of nothing, or of other sources, holds none of what it needs.
    for rec in [
        serde_json::json!({}),
        serde_json::json!(null),
        serde_json::json!({"tasks": []}),
    ] {
        assert_eq!(
            Dynamic::from_record(&rec, &pack).unwrap_err(),
            [Source::Untrusted]
        );
    }
}
