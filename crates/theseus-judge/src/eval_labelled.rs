//! Labelled sets for packs whose questions have one right answer per case
//! (theseus-98bz.9, theseus-98bz.10): `classify.v4`'s `scope` and `thread`,
//! `recall_cross.v1`'s `same_subject`, `families.v1`'s `relevant`
//! (theseus-xlri5), and `content.v1`'s four flags (theseus-9ymz0). Each case is the pack's own input
//! and the answers it should draw, checked as `eval::check` checks them (an
//! option id, or `high`/`low` for a Noul). `jev-probe --eval <pack>` runs a
//! set live, one call per case. The fixtures' names and texts are made up.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const CLASSIFY_V4: &str = include_str!("../fixtures/eval/classify.v4.json");
const RECALL_CROSS_V1: &str = include_str!("../fixtures/eval/recall_cross.v1.json");
const FAMILIES_V1: &str = include_str!("../fixtures/eval/families.v1.json");
const CONTENT_V1: &str = include_str!("../fixtures/eval/content.v1.json");

/// The packs with a labelled set.
pub const PACKS: [&str; 4] = ["classify.v4", "recall_cross.v1", "families.v1", "content.v1"];

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
        "recall_cross.v1" => RECALL_CROSS_V1,
        "families.v1" => FAMILIES_V1,
        "content.v1" => CONTENT_V1,
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
                        .find(|a| &a.def == q || &a.id == q)
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

    /// `content.v1`'s set is design §10's: ten clear cases for each of the four
    /// flags, twenty clean cases chosen adversarially, and five near-miss
    /// pairs a flag, each pair a flagged and a clean member.
    #[test]
    fn content_v1s_set_has_the_counts_the_design_names() {
        let cases = set("content.v1").unwrap();
        let count = |prefix: &str| cases.iter().filter(|c| c.name.starts_with(prefix)).count();
        for clear in ["inj_", "sec_", "conf_", "pers_"] {
            assert_eq!(count(clear), 10, "{clear}");
        }
        assert_eq!(count("clean_"), 20);
        for flag in ["injection", "secret", "confidential", "personal"] {
            assert_eq!(count(&format!("pair_{flag}_")), 10, "{flag}: five pairs");
            for n in 1..=5 {
                for member in ["flagged", "clean"] {
                    let name = format!("pair_{flag}_{n}_{member}");
                    assert!(cases.iter().any(|c| c.name == name), "{name}");
                }
            }
        }
        assert_eq!(cases.len(), 100);
        let names: std::collections::BTreeSet<&str> =
            cases.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names.len(), cases.len(), "names are unique");
    }

    /// A flagged case expects its one flag `yes`, and nothing else; a clean case
    /// expects every question it is asked `no`. The operator's own words are
    /// never expected to answer `injection`, which is not asked of them.
    #[test]
    fn content_v1s_expectations_match_the_case_names() {
        for case in set("content.v1").unwrap() {
            let flag = |f: &str| case.expect.get(f).map(String::as_str);
            let operator = case.input["kind"] == "operator_message";
            if operator {
                assert_eq!(flag("injection"), None, "{}: never asked", case.name);
            }
            let flagged = ["inj_", "sec_", "conf_", "pers_"]
                .iter()
                .any(|p| case.name.starts_with(p))
                || case.name.ends_with("_flagged");
            if flagged {
                assert_eq!(case.expect.len(), 1, "{}", case.name);
                assert!(case.expect.values().all(|v| v == "yes"), "{}", case.name);
            } else {
                assert_eq!(case.expect.len(), if operator { 3 } else { 4 }, "{}", case.name);
                assert!(case.expect.values().all(|v| v == "no"), "{}", case.name);
            }
        }
    }

    /// The set is invented through and through: no vendor's token prefix that
    /// a secret scanner would take for an issued credential, and no address
    /// but the reserved `.invalid` and `example` ones.
    #[test]
    fn content_v1s_set_holds_no_real_credential_shape() {
        let text = CONTENT_V1;
        for prefix in ["ghp_", "gho_", "github_pat_", "xoxb-", "xoxp-", "sk_live_", "whsec_", "sk-ant-", "AIza"] {
            assert!(!text.contains(prefix), "{prefix}");
        }
        for (i, _) in text.match_indices('@') {
            let after: String = text[i + 1..]
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\\' && *c != ',' && *c != ')' && *c != '\'')
                .collect();
            assert!(
                after.contains(".invalid") || after.starts_with("example."),
                "an address that could be real: {after}"
            );
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
