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
const CONTENT_V4: &str = include_str!("../fixtures/eval/content.v4.json");
const UNDO_V1: &str = include_str!("../fixtures/eval/undo.v1.json");

/// The packs with a labelled set.
pub const PACKS: [&str; 8] = [
    "classify.v4",
    "recall_cross.v1",
    "families.v1",
    "content.v1",
    "content.v2",
    "content.v3",
    "content.v4",
    "undo.v1",
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
        "content.v4" => CONTENT_V4,
        "undo.v1" => UNDO_V1,
        other => bail!("no labelled set for {other} (have: {})", PACKS.join(", ")),
    };
    serde_json::from_str(text).with_context(|| format!("fixtures/eval/{pack}.json"))
}

/// The bar a labelled set is measured against: 80% of its own cases, rounded up
/// (the owner, 2026-10-30 16:01, "the bars should all be set to about 80% of the
/// total cases"). A ratio with an explicit rounding rule, never an integer
/// written down: a fixed integer is how the release smoke came to gate on a
/// retired count (theseus-h66v4.4).
pub fn bar_need(of: usize) -> usize {
    (of * 4).div_ceil(5)
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
    }

    /// The new cases state the fields (the public ones state `public`, and the true
    /// positives sit in public trees), and the owner's own example is there as a
    /// clean case with the text the daemon judged.
    #[test]
    fn content_v3s_new_cases_state_the_fields_and_hold_the_owners_example() {
        let (v2, v3) = (set("content.v2").unwrap(), set("content.v3").unwrap());
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
            .find(|c| c.name == "fp_06_the_owners_own_example_as_judged")
            .expect("the owner's own example");
        assert!(own.expect.values().all(|v| v == "no"));
        assert_eq!(own.input["kind"], "tool_result");
        assert_eq!(own.input["source"], "fs.grep");
        // Its path lines, as the daemon's grep printed them.
        assert!(own.input["text"]
            .as_str()
            .unwrap()
            .contains("CLAUDE.md:13:"));
        for c in v3.iter().filter(|c| c.name.starts_with("fp_")) {
            assert!(c.expect.values().all(|v| v == "no"), "{}", c.name);
        }
        let names: std::collections::BTreeSet<&str> = v3.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names.len(), v3.len(), "names are unique");
    }

    /// `content.v4`'s set is v3's, case for case and in order, **verbatim**, plus
    /// the eleven pairs its wording exists for (theseus-3i6d1): the bar moved from
    /// "is it personal" to "is this specifically sensitive information that would
    /// not be appropriate to be publicly shared", and the only way to measure that
    /// is a clean member and a flagged one that differ by one clause. A favourite
    /// animal against a prescription; an amateur orchestra against a recovery
    /// group; a dog's name against a seizure disorder. **Every clean member is
    /// matched by a flagged one**, so a classifier that simply stopped answering
    /// `personal` would fail eleven cases here rather than pass twenty-two: that
    /// pairing is the point, not a convention.
    ///
    /// The eleventh pair came after the other ten and from a different source: the
    /// first ten are the owner's own words, and `pair_personal_16` is the owner's
    /// ledger. The item that actually raised the card carried no conversational
    /// line at all, so none of the ten was the shape that failed, and the set
    /// needed a pair whose only person-shaped content is a path under a home
    /// directory. `content_v4s_host_output_pair_is_the_shape_the_ledger_measured`
    /// holds what that pair has to keep being.
    #[test]
    fn content_v4s_set_keeps_every_v3_case_verbatim_and_adds_the_sensitivity_pairs() {
        let (v3, v4) = (set("content.v3").unwrap(), set("content.v4").unwrap());
        for (a, b) in v3.iter().zip(v4.iter()) {
            assert_eq!(a.name, b.name, "v4 reorders or drops v3's cases");
            assert_eq!(a.why, b.why, "{}: v3's why changed", a.name);
            assert_eq!(a.input, b.input, "{}: v3's input changed", a.name);
            assert_eq!(a.expect, b.expect, "{}: v3's expectation changed", a.name);
        }
        assert_eq!(v4.len(), v3.len() + 22);
        // The eleven new pairs, each with both members, and the new cases are only
        // those pairs.
        let new: Vec<&LabelledCase> = v4.iter().skip(v3.len()).collect();
        for n in 6..=16 {
            for member in ["flagged", "clean"] {
                let name = format!("pair_personal_{n}_{member}");
                assert!(new.iter().any(|c| c.name == name), "{name}");
            }
        }
        assert_eq!(
            new.len(),
            22,
            "the new cases are the eleven pairs and nothing else"
        );
        // The halves balance: as many that must fire as must not. A set whose
        // clean members outnumbered its flagged ones would reward an always-off
        // classifier.
        let fired = new
            .iter()
            .filter(|c| c.expect.get("personal").map(String::as_str) == Some("yes"))
            .count();
        let quiet = new
            .iter()
            .filter(|c| c.expect.get("personal").map(String::as_str) == Some("no"))
            .count();
        assert_eq!((fired, quiet), (11, 11), "as many must fire as must not");
        // Each new case states the fields the daemon sends, as v3's do.
        for c in &new {
            for field in ["author", "place", "project", "context"] {
                assert!(c.input.get(field).is_some(), "{}: no {field}", c.name);
            }
        }
        // The innocuous shapes the owner named are all there, in the clean half.
        let clean: String = new
            .iter()
            .filter(|c| c.name.ends_with("_clean"))
            .map(|c| c.input["text"].as_str().unwrap_or_default().to_lowercase())
            .collect::<Vec<_>>()
            .join("\n");
        for word in [
            "praying mantis",
            "clarinet",
            "dog called",
            "birthday",
            "hiking",
        ] {
            assert!(clean.contains(word), "no clean case holds {word:?}");
        }
        // The owner's own planted line, as he typed it, is a clean case: it is the
        // item this version exists for.
        let planted = new
            .iter()
            .find(|c| c.name == "pair_personal_6_clean")
            .expect("the planted line");
        assert_eq!(planted.input["kind"], "operator_message");
        assert!(planted.input["text"]
            .as_str()
            .unwrap()
            .contains("buy a praying mantis (my favorite animal)"));
        assert!(planted.expect.values().all(|v| v == "no"));
        let names: std::collections::BTreeSet<&str> = v4.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names.len(), v4.len(), "names are unique");
    }

    /// `pair_personal_16` is the one pair in the set that comes from a
    /// measurement rather than from a sentence, and this test holds the three
    /// things that make it that pair.
    ///
    /// **Why it exists.** The row the line was opened about is `kind:
    /// tool_result`, `source: proc.run`, and its result is 414 characters of
    /// clock, mount-table, inode, cache-size and pressure output. The sentence the
    /// owner had planted in the conversation **is not in it**. So the classifier
    /// did not over-read a person's chatter echoed into a program's output -- which
    /// is what `pair_personal_15` supposes and what every other pair here is
    /// about -- it over-read **an account name inside a filesystem path**, which
    /// the old question's `yes` list swallowed as an identity attribute and a
    /// location. Nothing else in the set is that shape: strip the conversational
    /// line out of `pair_personal_15` and no case remains.
    ///
    /// **What it must keep being**, because each clause is a way the pair could
    /// quietly stop measuring anything:
    ///
    /// 1. **One differing line.** If an edit changed any line but the `du` target,
    ///    the pair would be measuring two things and attributing the result to
    ///    one.
    /// 2. **No conversational line in either member.** A `cat` of someone's notes
    ///    put back into the clean member would make it `pair_personal_15` again and
    ///    the measured shape would be gone from the set.
    /// 3. **The input reaches the wording that changed.** The pair is worth nothing
    ///    if the question it draws is not the narrowed one, so it is prepared and
    ///    asked here, against v4 and against v3, and the two questions must differ.
    ///    This is the only assertion in the crate that ties a case to the wording
    ///    it was written to measure; `tests_content_v4` holds the wording itself,
    ///    which is a different claim.
    ///
    /// **What this test does NOT do, stated so nobody reads it as more than it
    /// is.** It does not judge. Whether v4 actually answers the clean member `no`
    /// where v3 answered `yes` is one live call per case against the probe, and no
    /// deterministic test can stand in for it. This test proves the instrument is
    /// pointed at the right thing; the reading is still owed.
    #[test]
    fn content_v4s_host_output_pair_is_the_shape_the_ledger_measured() {
        let v4 = set("content.v4").unwrap();
        let case = |n: &str| {
            v4.iter()
                .find(|c| c.name == n)
                .unwrap_or_else(|| panic!("{n}"))
        };
        let (flagged, clean) = (
            case("pair_personal_16_flagged"),
            case("pair_personal_16_clean"),
        );
        let text = |c: &LabelledCase| c.input["text"].as_str().unwrap().to_owned();

        // Both members are a program's output, not anybody's words.
        for c in [flagged, clean] {
            assert_eq!(c.input["kind"], "tool_result", "{}", c.name);
            let source = c.input["source"].as_str().unwrap();
            assert!(source.starts_with("proc.run"), "{}: {source}", c.name);
            assert!(
                source.contains("df"),
                "{}: not a disk report: {source}",
                c.name
            );
        }

        // 1. The members differ in exactly one line, and it is the `du` target.
        let (f, c) = (text(flagged), text(clean));
        let (fl, cl): (Vec<&str>, Vec<&str>) = (f.lines().collect(), c.lines().collect());
        assert_eq!(fl.len(), cl.len(), "the members are not the same report");
        let differing: Vec<usize> = (0..fl.len()).filter(|&i| fl[i] != cl[i]).collect();
        assert_eq!(
            differing.len(),
            1,
            "the members differ in {} lines, not one: {:?}",
            differing.len(),
            differing
                .iter()
                .map(|&i| (fl[i], cl[i]))
                .collect::<Vec<_>>()
        );
        let d = differing[0];
        for line in [fl[d], cl[d]] {
            assert!(
                line.contains("/Users/owner/"),
                "the one differing line is not the du target: {line:?}"
            );
        }
        // And the difference is a real one: the flagged target names a health
        // matter and the clean one does not. Two innocuous targets would pass
        // every assertion above and measure nothing.
        assert!(
            fl[d].contains("neurology"),
            "the flagged du target names nothing sensitive: {:?}",
            fl[d]
        );
        assert!(
            !cl[d].contains("neurology"),
            "the clean du target is the flagged one: {:?}",
            cl[d]
        );

        // 2. Neither member carries a conversational line: that is the whole
        // difference between this pair and the ten that came from the owner's
        // words, and it is what the ledger measured.
        for (name, body) in [(&flagged.name, &f), (&clean.name, &c)] {
            for cue in ["cat ", "reminders", "praying mantis", "cinema"] {
                assert!(
                    !body.contains(cue),
                    "{name}: a conversational line is back in the measured pair ({cue:?})"
                );
            }
        }
        // Its only person-shaped content is a path under the stand-in's home.
        assert!(c.contains("/Users/owner/.cache"), "{c}");

        // 3. The input reaches the question v4 changed, and v3 asks it something
        // else. Asked through `prepare`/`ask`, as the daemon asks it.
        let asked_personal = |pack: &str| -> String {
            let p = by_name(pack).unwrap();
            let input = Input::parse(p.builder, &clean.input.to_string())
                .unwrap_or_else(|e| panic!("{pack}: {e}"));
            let prepared = prepare(&p, &input, &NoScrub).unwrap();
            let a = p
                .ask(&prepared.dynamic)
                .into_iter()
                .find(|a| a.def == "personal" || a.id == "personal")
                .unwrap_or_else(|| panic!("{pack}: personal is not asked of a tool result"));
            match a.question {
                crate::client::Question::Choice { instructions, .. } => instructions,
                other => panic!("{pack}: personal is not a choice: {other:?}"),
            }
        };
        let (v4q, v3q) = (asked_personal("content.v4"), asked_personal("content.v3"));
        assert!(
            v4q.contains(
                "specifically sensitive information about a person that would not be \
                 appropriate to be publicly shared"
            ),
            "the measured pair is not asked v4's question: {v4q}"
        );
        assert!(
            v3q.contains("does it reveal a person's private information"),
            "v3 no longer asks the question this pair exists to move away from: {v3q}"
        );
        assert_ne!(v4q, v3q, "v4 and v3 ask this pair the same question");
    }

    /// v4's new cases carry no real home directory either, for the same reason
    /// v3's do not. The two needles are built from pieces rather than written out:
    /// the name scrub's `private paths` family matches the text of a home
    /// directory wherever it stands, and these are the detector's needles, not paths.
    #[test]
    fn content_v4s_new_cases_carry_no_real_home_directory() {
        let (home, stand_in) = (concat!("/ho", "me/"), concat!("/ho", "me/owner"));
        let v4 = set("content.v4").unwrap();
        for c in v4.iter().skip(set("content.v3").unwrap().len()) {
            let text = c.input.to_string();
            for (i, _) in text.match_indices(home) {
                assert!(
                    text[i..].starts_with(stand_in),
                    "{}: a home directory that could be real: {}",
                    c.name,
                    &text[i..(i + 24).min(text.len())]
                );
            }
        }
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
                    text[i..].starts_with("/home/owner"),
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
            .chain(set("content.v3").unwrap())
            .chain(set("content.v4").unwrap());
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
        for text in [CONTENT_V1, CONTENT_V2, CONTENT_V3, CONTENT_V4] {
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

    /// `undo.v1`'s set (theseus-p5kln) is the instrument the reversibility
    /// classifier is measured on, so its shape is held here rather than left to
    /// whoever edits the fixture next.
    ///
    /// **Nine families**, named by each case's own prefix, because
    /// reversibility is a property of a call and families are the only honest
    /// way to show the property holds across the tool surface rather than on
    /// one tool: `p` programs, `f` files, `a` the cloud API, `l` the L1
    /// sandbox, `w` the web, `t` terminals, `m` messages and publishing, `s`
    /// self-build and tasks, `b` batches.
    ///
    /// **Hard pairs.** Nine cases are one half of a pair whose other half has
    /// the same tool and the same arguments and the opposite verdict, differing
    /// only in a fact code computed: `f2`/`f3` (a clean against a dirty tree),
    /// `p14`/`p15` (committed against untracked), `a1`/`a2` and `a8`/`a9` (a
    /// store that versions against one that does not), `a3`/`a4` (one boolean
    /// in the request), `m3`/`m4` (a service that keeps revisions against one
    /// that does not), `p1`/`l1` (the same command inside and outside the
    /// sandbox), `t1`/`t2` (what is waiting on the screen), and `b1`/`b2` (one
    /// step of a batch). They are the whole argument for computing recovery
    /// facts instead of asking Jev to guess, and a set without them would
    /// measure nothing but vocabulary.
    ///
    /// **No case turns on where a file lives.** Bar 2 of the tools feature is
    /// that no path and no location is special; a set keyed on paths would
    /// smuggle the approve lists back in as test data.
    ///
    /// **The `must` set** is the cases where a miss means the feature does not
    /// do what it was built for: the owner's own example, the ones where a
    /// wrong `reversible` lets a real loss through, and — just as many — the
    /// ones where a wrong hold would make the classifier an interruption
    /// machine, because "asks less every week" is half the promise and the
    /// approve lists died of the other half.
    #[test]
    fn undo_v1s_set_is_the_instrument_the_design_describes() {
        let cases = set("undo.v1").unwrap();
        let names: std::collections::BTreeSet<&str> =
            cases.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names.len(), cases.len(), "names are unique");
        assert_eq!(cases.len(), 54);
        assert_eq!(bar_need(cases.len()), 44, "the bar on the whole set");

        // Every family is represented, so the measurement is of a property and
        // not of one tool's vocabulary.
        let count = |p: &str| cases.iter().filter(|c| c.name.starts_with(p)).count();
        for (family, n) in [
            ("p", 20),
            ("f", 8),
            ("a", 9),
            ("l", 2),
            ("w", 3),
            ("t", 3),
            ("m", 4),
            ("s", 3),
            ("b", 2),
        ] {
            assert_eq!(count(family), n, "family {family}");
        }

        let musts: Vec<&LabelledCase> =
            cases.iter().filter(|c| c.name.ends_with("_must")).collect();
        assert_eq!(musts.len(), 15);
        assert_eq!(bar_need(musts.len()), 12, "the bar on the must set");

        // The must set is balanced: a wrong hold is a failure as much as a
        // wrong pass, so neither direction may own the set.
        let must_reversible = musts
            .iter()
            .filter(|c| c.expect.get("reversible").map(String::as_str) == Some("reversible"))
            .count();
        assert!(
            (6..=9).contains(&must_reversible),
            "{must_reversible} of 15 musts expect `reversible`: the set has tilted"
        );

        // All three verdicts are reachable and none is a token presence: a set
        // where `irreversible_final` appeared once would not measure it.
        let verdict = |v: &str| {
            cases
                .iter()
                .filter(|c| c.expect.get("reversible").map(String::as_str) == Some(v))
                .count()
        };
        assert!(verdict("reversible") >= 20);
        assert!(verdict("irreversible_reshapeable") >= 12);
        assert!(verdict("irreversible_final") >= 6);
        assert_eq!(
            verdict("reversible")
                + verdict("irreversible_reshapeable")
                + verdict("irreversible_final"),
            cases.len(),
            "every case labels the verdict"
        );

        // A case that is not held must carry no reshape, and one that is held
        // must carry one: inventing a shape for a call that needs none is its
        // own failure, and a hold with no shape is an obstacle, not a guard.
        for c in &cases {
            let held =
                c.expect.get("reversible").map(String::as_str) == Some("irreversible_reshapeable");
            let shape = c.expect.get("shape").map(String::as_str).unwrap_or("");
            assert_eq!(
                held,
                shape != "none",
                "{}: a shape of {shape:?} beside a verdict that is {}held",
                c.name,
                if held { "" } else { "not " }
            );
        }

        // Every case says what it is and what a miss would mean.
        for c in &cases {
            assert!(c.why.len() > 60, "{}: its `why` says too little", c.name);
        }
    }

    /// The fail-open rule, asserted rather than assumed (theseus-p5kln).
    ///
    /// When Jev's answer is under the question's confirm bar the call proceeds:
    /// `undo.v1`'s `no_match` is `reversible`, deliberately, because the
    /// classifier is a net addition to a gate that still has every guard it had
    /// in 1.0.4 — so a wrong `reversible` leaves the call to the floor, the
    /// place rule, the ceiling, `notify_argv` and the posture, while a wrong
    /// hold is the approve lists' failure mode in a new costume.
    ///
    /// Asserted here because `theseus-h66v4.6.1` found that an under-bar answer
    /// can fall back to the model's default rather than the pack's `no_match`.
    /// For this pack the two happen to coincide, which is luck and not design:
    /// if that fallback changes, this is where the pack notices.
    #[test]
    fn undo_v1_fails_open_under_the_bar_and_acts_nowhere_yet() {
        let p = by_name("undo.v1").expect("undo.v1 is embedded");
        let verdict = p.question("reversible").expect("it asks `reversible`");
        assert_eq!(verdict.no_match.as_deref(), Some("reversible"));
        assert!(verdict.decides, "the verdict decides");
        let shape = p.question("shape").expect("it asks `shape`");
        assert_eq!(shape.no_match.as_deref(), Some("none"));

        // The three verdicts are the owner's three labels and nothing else.
        let ids: Vec<&str> = verdict.options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "reversible",
                "irreversible_reshapeable",
                "irreversible_final"
            ]
        );

        // It acts nowhere yet: going live is the owner's separate decision, as
        // route.v10 landed before `ROUTE_PACK` named it.
        assert_eq!(p.action, crate::pack::Action::None);
        assert!(
            !p.rollback.is_empty(),
            "its rollback rules are declared before it is wired, so the numbers can be argued with"
        );
    }
}
