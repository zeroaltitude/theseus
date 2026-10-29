//! The labelled set for `heard.v1` (theseus-s3dwj): invented transcripts of a
//! voice call, each with what it was, so a build can be scored against live
//! Jev (`jev-probe --eval heard.v1`: the answers and the accuracy) and the
//! builder and checks are held with the fake. A case is an `echo` (Theseus's
//! own voice back), a `fragment` (a clipped or garbled word or two, an echo or
//! noise), an `answer` (speech to answer, however short) or `noise`; its
//! `expect` is the option Jev should choose. The fixture's names and
//! sentences are made up.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::builders::HeardInput;
use crate::client::Answer;
use crate::eval::Check;
use crate::judge::AnswerRecord;

const SET: &str = include_str!("../fixtures/eval/heard.v1.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Echo,
    Fragment,
    Answer,
    Noise,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeardCase {
    pub name: String,
    pub category: Kind,
    /// What the case plants, and what a miss would mean.
    pub why: String,
    pub input: HeardInput,
    /// `addressed`, `echo` or `other`.
    pub expect: String,
}

pub fn set() -> Result<Vec<HeardCase>> {
    serde_json::from_str(SET).context("fixtures/eval/heard.v1.json")
}

/// The case's expectation against the answers of its judgment; an
/// unanswered question is a miss.
pub fn check(case: &HeardCase, answers: &[AnswerRecord]) -> Check {
    let (got, met) = match answers.iter().find(|a| a.def == "heard").map(|a| &a.answer) {
        Some(Answer::Choice {
            choice, confidence, ..
        }) => (format!("{choice} {confidence:.2}"), *choice == case.expect),
        Some(_) => ("(wrong kind of answer)".into(), false),
        None => ("(no answer)".into(), false),
    };
    Check {
        question: "heard".into(),
        wanted: case.expect.clone(),
        got,
        met,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::breaker::BreakerConfig;
    use crate::builders::{prepare, Input};
    use crate::client::{ClientConfig, JevClient, StaticKey, Urgency};
    use crate::eval::pack_of;
    use crate::fake::{FakeJev, Scripted};
    use crate::judge::{Ask, DecisionPoint, JevJudge, Judge, Mode, Outcome};
    use crate::price;
    use crate::state::NoScrub;

    #[test]
    fn the_set_has_every_kind_and_unique_names_and_legal_expectations() {
        let cases = set().unwrap();
        assert!(cases.len() >= 20, "{} cases", cases.len());
        for kind in [Kind::Echo, Kind::Fragment, Kind::Answer, Kind::Noise] {
            assert!(cases.iter().any(|c| c.category == kind), "{kind:?}");
        }
        let mut names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), cases.len(), "case names are unique");
        let pack = pack_of("heard.v1").unwrap();
        let options: Vec<&str> = pack
            .question("heard")
            .unwrap()
            .options
            .iter()
            .map(|o| o.id.as_str())
            .collect();
        for c in &cases {
            assert!(options.contains(&c.expect.as_str()), "{}", c.name);
            assert!(!c.input.said.is_empty(), "{}", c.name);
        }
        // Each answer a real one, each echo and noise something else: the set
        // is not one-sided.
        for want in ["addressed", "echo", "other"] {
            assert!(
                cases.iter().filter(|c| c.expect == want).count() >= 5,
                "{want}"
            );
        }
    }

    #[test]
    fn every_case_builds_under_the_cap_and_cuts_nothing() {
        let pack = pack_of("heard.v1").unwrap();
        for case in set().unwrap() {
            let p = prepare(&pack, &Input::Heard(case.input.clone()), &NoScrub).unwrap();
            assert!(p.state.tokens <= pack.state_cap_tokens, "{}", case.name);
            assert!(
                p.state.truncated.is_empty(),
                "{}: {:?}",
                case.name,
                p.state.truncated
            );
            assert!(
                p.state.json.contains(&case.input.transcript),
                "{}",
                case.name
            );
        }
    }

    /// The whole set over the wire against the fake, as `jev-probe --eval
    /// heard.v1` runs it against Jev: scripted to agree, every expectation is
    /// met; scripted to answer `other` for all, exactly the cases that do not
    /// expect it are missed, so the checker is not vacuous.
    #[tokio::test]
    async fn the_set_runs_over_the_wire_and_the_checks_read_the_answers() {
        let fake = FakeJev::start().unwrap();
        let client = JevClient::new(
            ClientConfig {
                api_base: fake.base(),
                connect: Duration::from_secs(2),
                total: Duration::from_secs(5),
                max_in_flight: 4,
                reserved: None,
            },
            Arc::new(StaticKey::new(
                "apik-test-0123456789-abcdefghijklmnop".into(),
            )),
        )
        .unwrap();
        let judge = JevJudge::new(client, price::builtin(), BreakerConfig::default());
        let pack = pack_of("heard.v1").unwrap();
        let cases = set().unwrap();
        for agree in [true, false] {
            let mut right = 0;
            for case in &cases {
                let option = if agree { case.expect.as_str() } else { "other" };
                fake.script(
                    "heard.v1/heard",
                    Scripted::Choice {
                        option: option.into(),
                        confidence: 0.9,
                    },
                );
                let p = prepare(&pack, &Input::Heard(case.input.clone()), &NoScrub).unwrap();
                let ask = Ask::new(pack.clone(), &p, Mode::Shadow, json!({"case": case.name}));
                let out = judge
                    .judge(DecisionPoint {
                        asks: vec![ask],
                        urgency: Urgency::Shadow,
                    })
                    .await;
                assert_eq!(out[0].outcome, Outcome::Answered, "{}", case.name);
                let c = check(case, &out[0].answers);
                assert_eq!(
                    c.met,
                    agree || case.expect == "other",
                    "{}: {c:?}",
                    case.name
                );
                right += usize::from(c.met);
            }
            let others = cases.iter().filter(|c| c.expect == "other").count();
            assert_eq!(right, if agree { cases.len() } else { others });
        }
        assert_eq!(fake.seen().len(), cases.len() * 2);
        let state = fake.seen()[0].body["state"].to_string();
        assert!(
            state.contains("\"said\"") && state.contains("seus here"),
            "{state}"
        );
    }

    #[test]
    fn an_unanswered_question_is_a_miss() {
        let case = set().unwrap().remove(0);
        let c = check(&case, &[]);
        assert!(!c.met && c.got == "(no answer)");
    }
}
