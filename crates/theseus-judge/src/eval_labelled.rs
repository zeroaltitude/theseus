//! Labelled sets for packs whose questions have one right answer per case
//! (theseus-98bz.9, theseus-98bz.10): `classify.v4`'s `scope` and `thread`,
//! `recall_cross.v1`'s `same_subject`, `families.v1`'s `relevant`
//! (theseus-xlri5), and `content.v1`'s four flags (theseus-9ymz0). Each case is the pack's own input
//! and the answers it should draw, checked as `eval::check` checks them (an
//! option id, or `high`/`low` for a Noul). `jev-probe --eval <pack>` runs a
//! set live, one call per case. The fixtures' names and texts are made up.
//!
//! A pack's versions keep their own sets, and an older one's stays frozen as the
//! record of what it was measured on. To compare two versions, run both over the
//! newer set: `jev-probe --eval content.v1 --eval-set content.v2` (theseus-78qay), and
//! `jev-probe --score-at-bar` scores the runs.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const CLASSIFY_V4: &str = include_str!("../fixtures/eval/classify.v4.json");
const RECALL_CROSS_V1: &str = include_str!("../fixtures/eval/recall_cross.v1.json");
const FAMILIES_V1: &str = include_str!("../fixtures/eval/families.v1.json");
const CONTENT_V1: &str = include_str!("../fixtures/eval/content.v1.json");
const CONTENT_V2: &str = include_str!("../fixtures/eval/content.v2.json");
const CONTENT_V3: &str = include_str!("../fixtures/eval/content.v3.json");

/// The packs with a labelled set.
pub const PACKS: [&str; 6] = [
    "classify.v4",
    "recall_cross.v1",
    "families.v1",
    "content.v1",
    "content.v2",
    "content.v3",
];

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
        "content.v2" => CONTENT_V2,
        "content.v3" => CONTENT_V3,
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

    /// `content.v2`'s set is v1's, case for case and in order, plus the cases its
    /// wording exists for (theseus-78qay): fourteen `tb_` shapes that v1 flagged on
    /// terminal-bench 2.0 and v2 must call clean, and eleven true positives that must
    /// still flag -- a credential out of a real environment, two in an ordinary
    /// project's configuration with no exercise framing, a real partner's
    /// unannounced work, an unfixed vulnerability, signed deal terms, a real person's
    /// health and pay. **A superset, never a replacement**: dropping one of v1's
    /// cases to make v2 look better is the one way this change could lie, so the
    /// test holds the prefix, in order, not just the count.
    #[test]
    fn content_v2s_set_keeps_every_v1_case_and_adds_the_new_shapes() {
        let (v1, v2) = (set("content.v1").unwrap(), set("content.v2").unwrap());
        for (a, b) in v1.iter().zip(v2.iter()) {
            assert_eq!(a.name, b.name, "v2 reorders or drops v1's cases");
            assert_eq!(a.input, b.input, "{}: v1's input changed", a.name);
            assert_eq!(a.expect, b.expect, "{}: v1's expectation changed", a.name);
        }
        let count = |prefix: &str| v2.iter().filter(|c| c.name.starts_with(prefix)).count();
        assert_eq!(count("tb_"), 14);
        assert_eq!(v2.len(), v1.len() + 25);
        // Every `tb_` case is clean: that is the whole point of them.
        for c in v2.iter().filter(|c| c.name.starts_with("tb_")) {
            assert!(c.expect.values().all(|v| v == "no"), "{}", c.name);
        }
        // The new true positives, each flagging the one thing it is for.
        for (name, flag) in [
            ("sec_11_aws_credentials_file", "secret"),
            ("sec_12_dotenv_of_a_live_service", "secret"),
            ("sec_13_operator_pastes_a_token", "secret"),
            ("conf_11_partner_programme_memo", "confidential"),
            ("conf_12_unfixed_vulnerability", "confidential"),
            ("conf_13_signed_deal_terms", "confidential"),
            ("pers_11_colleague_medical_leave", "personal"),
            ("pers_12_payroll_row", "personal"),
            // The two the review of 2026-10-27 asked for: a plaintext password in an
            // ordinary project's configuration, with no production cue and no exercise
            // framing. `secret`'s exercise clause must not reach either of them, and
            // the scrubber's five finders do not match a bare `password =`, so this
            // question is their only reader.
            ("sec_14_project_config_password", "secret"),
            ("sec_15_local_settings_ini", "secret"),
            // `confidential`'s `no` now exempts a test suite, and a real company's own
            // test suite is a test suite: this fixture holds its actual customers.
            ("conf_14_real_customers_in_a_test_fixture", "confidential"),
        ] {
            let c = v2.iter().find(|c| c.name == name).expect(name);
            assert_eq!(
                c.expect.get(flag).map(String::as_str),
                Some("yes"),
                "{name}"
            );
        }
        let names: std::collections::BTreeSet<&str> = v2.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names.len(), v2.len(), "names are unique");
    }

    /// `content.v3`'s set is v2's, case for case and in order, **verbatim**, plus
    /// what its fields exist for (theseus-gyjyo.11): the false positives the owner
    /// named as synthetic equivalents (`fp_`: his own public side project's README
    /// and `CLAUDE.md` searched, GPL source and a fixture of placeholder people
    /// inside a public tree, a listing that names `secret.txt`, and the item he
    /// called miscategorised, with its text as the daemon judged it); ten clean
    /// items from trees the operator lists as public (`pub_`); and ten true
    /// positives inside public trees (`pubtp_`), the ones that matter more: a
    /// pack that learns "public means clean" is worse than v2. Every new case
    /// carries the new fields, so the pack is measured on what the daemon sends.
    #[test]
    fn content_v3s_set_keeps_every_v2_case_verbatim_and_adds_what_its_fields_are_for() {
        let (v2, v3) = (set("content.v2").unwrap(), set("content.v3").unwrap());
        for (a, b) in v2.iter().zip(v3.iter()) {
            assert_eq!(a.name, b.name, "v3 reorders or drops v2's cases");
            assert_eq!(a.why, b.why, "{}: v2's why changed", a.name);
            assert_eq!(a.input, b.input, "{}: v2's input changed", a.name);
            assert_eq!(a.expect, b.expect, "{}: v2's expectation changed", a.name);
        }
        let count = |prefix: &str| v3.iter().filter(|c| c.name.starts_with(prefix)).count();
        assert_eq!(count("tb_"), 14, "the terminal-bench shapes stay");
        assert_eq!(count("fp_"), 6);
        assert_eq!(count("pub_"), 10);
        assert_eq!(count("pubtp_"), 10);
        assert_eq!(v3.len(), v2.len() + 26);
        // The new cases state the fields; the public ones state `public`, and the
        // true positives sit in public trees.
        for c in v3.iter().skip(v2.len()) {
            for field in ["author", "place", "project", "context"] {
                assert!(c.input.get(field).is_some(), "{}: no {field}", c.name);
            }
        }
        for c in v3.iter().filter(|c| c.name.starts_with("pubtp_")) {
            assert_eq!(c.input["public"], true, "{}", c.name);
            assert_eq!(c.expect.len(), 1, "{}: one flag", c.name);
        }
        for c in v3.iter().filter(|c| c.name.starts_with("pub_")) {
            assert_eq!(c.input["public"], true, "{}", c.name);
            assert!(c.expect.values().all(|v| v == "no"), "{}", c.name);
        }
        // The owner's own example: the item the daemon judged confidential at 0.70
        // under content.v1, as a clean case.
        let own = v3
            .iter()
            .find(|c| c.name == "fp_06_stratajam_claude_md_grep_as_judged")
            .expect("the owner's own example");
        assert!(own.expect.values().all(|v| v == "no"));
        assert_eq!(own.input["kind"], "tool_result");
        assert_eq!(own.input["source"], "fs.grep");
        // Its path lines, as the daemon's grep printed them.
        assert!(own.input["text"].as_str().unwrap().contains("CLAUDE.md:13:"));
        for c in v3.iter().filter(|c| c.name.starts_with("fp_")) {
            assert!(c.expect.values().all(|v| v == "no"), "{}", c.name);
        }
        let names: std::collections::BTreeSet<&str> = v3.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names.len(), v3.len(), "names are unique");
    }

    /// No new case carries the operator's own home directory: a path in one is
    /// under a stand-in's (`/home/owner`), so the set can be published.
    /// (`tests_public_names` holds the names themselves.)
    #[test]
    fn content_v3s_new_cases_carry_no_real_home_directory() {
        let v3 = set("content.v3").unwrap();
        for c in v3.iter().skip(set("content.v2").unwrap().len()) {
            let text = c.input.to_string();
            for (i, _) in text.match_indices("/home/") {
                assert!(
                    text[i..].starts_with("/home/owner/"),
                    "{}: a home directory that could be real: {}",
                    c.name,
                    &text[i..(i + 24).min(text.len())]
                );
            }
        }
    }

    /// A flagged case expects its one flag `yes`, and nothing else; a clean case
    /// expects every question it is asked `no`. The operator's own words are
    /// never expected to answer `injection`, which is not asked of them.
    #[test]
    fn content_v1s_expectations_match_the_case_names() {
        let both = set("content.v1")
            .unwrap()
            .into_iter()
            .chain(set("content.v2").unwrap())
            .chain(set("content.v3").unwrap());
        for case in both {
            let flag = |f: &str| case.expect.get(f).map(String::as_str);
            // The exempt kinds come from the one list, never restated here:
            // three places stated this rule and disagreed until the review of
            // 2026-10-26, and this test was one of them.
            let kind = case.input["kind"].as_str().unwrap_or_default();
            let asked = crate::builders::asks_injection(kind);
            if !asked {
                assert_eq!(flag("injection"), None, "{}: never asked", case.name);
            }
            let flagged = ["inj_", "sec_", "conf_", "pers_", "pubtp_"]
                .iter()
                .any(|p| case.name.starts_with(p))
                || case.name.ends_with("_flagged");
            if flagged {
                assert_eq!(case.expect.len(), 1, "{}", case.name);
                assert!(case.expect.values().all(|v| v == "yes"), "{}", case.name);
            } else {
                assert_eq!(case.expect.len(), 3 + usize::from(asked), "{}", case.name);
                assert!(case.expect.values().all(|v| v == "no"), "{}", case.name);
            }
        }
    }

    /// The set is invented through and through: no vendor's token prefix that
    /// a secret scanner would take for an issued credential, and no address
    /// but the reserved `.invalid` and `example` ones.
    #[test]
    fn content_v1s_set_holds_no_real_credential_shape() {
        for text in [CONTENT_V1, CONTENT_V2, CONTENT_V3] {
            for prefix in [
                "ghp_",
                "gho_",
                "github_pat_",
                "xoxb-",
                "xoxp-",
                "sk_live_",
                "whsec_",
                "sk-ant-",
                "AIza",
            ] {
                assert!(!text.contains(prefix), "{prefix}");
            }
            for (i, _) in text.match_indices('@') {
                let after: String = text[i + 1..]
                    .chars()
                    .take_while(|c| {
                        !c.is_whitespace()
                            && *c != '"'
                            && *c != '\\'
                            && *c != ','
                            && *c != ')'
                            && *c != '\''
                    })
                    .collect();
                assert!(
                    after.contains(".invalid") || after.starts_with("example."),
                    "an address that could be real: {after}"
                );
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
