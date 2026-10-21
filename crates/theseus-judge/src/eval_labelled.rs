//! Labelled sets for packs whose questions have one right answer per case
//! (theseus-98bz.9, theseus-98bz.10): `classify.v4`'s `scope` and `thread`,
//! and `recall.cross.v1`'s `same_subject`. Each case is the pack's own input
//! and the answers it should draw, checked as `eval::check` checks them (an
//! option id, or `high`/`low` for a Noul). `jev-probe --eval <pack>` runs a
//! set live, one call per case. The fixtures' names and texts are made up.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const CLASSIFY_V4: &str = include_str!("../fixtures/eval/classify.v4.json");

/// The packs with a labelled set.
pub const PACKS: [&str; 1] = ["classify.v4"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelledCase {
    pub name: String,
    /// What the case is, and what a miss would mean.
    pub why: String,
    /// The pack's input, as its builder parses it.
    pub input: serde_json::Value,
    /// Question id to an option id, `high`, or `low`.
    pub expect: BTreeMap<String, String>,
}

/// The set of `pack`.
pub fn set(pack: &str) -> Result<Vec<LabelledCase>> {
    let text = match pack {
        "classify.v4" => CLASSIFY_V4,
        other => bail!("no labelled set for {other} (have: {})", PACKS.join(", ")),
    };
    serde_json::from_str(text).with_context(|| format!("fixtures/eval/{pack}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builders::{prepare, Input};
    use crate::pack::by_name;
    use crate::state::NoScrub;

    /// Every case parses as its pack's input, and asks each question it
    /// expects an answer to, with an option the question offers.
    #[test]
    fn each_case_asks_what_it_expects() {
        for pack in PACKS {
            let p = by_name(pack).unwrap();
            for case in set(pack).unwrap() {
                let input = Input::parse(p.builder, &case.input.to_string())
                    .unwrap_or_else(|e| panic!("{}: {e}", case.name));
                let prepared = prepare(&p, &input, &NoScrub).unwrap();
                let asked = p.ask(&prepared.dynamic);
                for (q, want) in &case.expect {
                    let a = asked
                        .iter()
                        .find(|a| &a.def == q)
                        .unwrap_or_else(|| panic!("{}: {q} not asked", case.name));
                    if let crate::client::Question::Choice { options, .. } = &a.question {
                        assert!(
                            options.iter().any(|o| &o.id == want),
                            "{}: {want} is no option of {q}",
                            case.name
                        );
                    }
                }
            }
        }
    }

    /// classify.v4's set holds the four "also this week" probes as `place`,
    /// a question about one stream as `thread`, and a comment that begins with
    /// a voice keeps its stream.
    #[test]
    fn classify_v4s_set_holds_the_threadbench_probes() {
        let cases = set("classify.v4").unwrap();
        let scope = |want: &str| {
            cases
                .iter()
                .filter(|c| c.expect.get("scope").map(String::as_str) == Some(want))
                .count()
        };
        assert_eq!(scope("place"), 4);
        assert!(scope("thread") >= 2);
        assert!(cases.iter().any(|c| {
            c.input["message"]
                .as_str()
                .is_some_and(|m| m.starts_with("Security view:"))
                && c.expect.get("thread").map(String::as_str) == Some("pr-round")
        }));
    }
}
