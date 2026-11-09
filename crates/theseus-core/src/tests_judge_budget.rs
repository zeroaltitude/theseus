//! A census of the test files that turn the judge on (theseus-4vsrz, 1.0.5 B7).
//!
//! A test that asserts a Jev verdict while its rig leaves `judge.total_secs`
//! short loses the verdict on a loaded host: the call is abandoned, booked as a
//! timeout, and the test fails about a missing verdict. The first sweeps found
//! such files by hand, twice. This holds the rule so a third is caught when it
//! is written.
//!
//! **The rule judges a file, not each rig in it.** Every file under
//! `crates/theseus-core/src` whose source enables the judge (`judge.enabled =
//! true` or an `api_base` assignment) must set a budget (a `total_secs`
//! assignment), build through `tests_judge::judge_config` (which gives 10 s),
//! or be on [`EXEMPT`] with the reason nothing in it waits on a verdict. A new
//! rig in a file that already has a budget is not seen; reviewing that is
//! still a person's job.
//!
//! The 13 `api_base`-without-budget sites audited at base 85697868 (paths as
//! they are in the tree), and why each is exempt or not a problem:
//!
//! | site | why |
//! |---|---|
//! | `config/data_floor.rs:130,134` | Config validation, no rig, nothing judged |
//! | `learning/tests_report_undo.rs:48,92` | points at `127.0.0.1:9`: nothing judged |
//! | `tests_ladder.rs:40` | `127.0.0.1:9` only when no Jev; built through `judge_config` |
//! | `tests_ladder_route_v10.rs:25`, `tests_ladder_unread.rs:181` | `127.0.0.1:9`; built through `judge_config` |
//! | `tests_learning.rs:41`, `tests_prove.rs:56` | `127.0.0.1:9`: nothing judged |
//! | `tests_route.rs:569` | `127.0.0.1:9` on purpose: a turn does not wait on an unreachable Jev |
//! | `tests_judge.rs:453` | a rig built with `None`; asserts no connection |
//! | `own_bench/tests_sampler.rs:128` | the cap is checked before the call: asserts no call |
//! | `tests_inbound.rs:342` | asserts verdicts, but `life_with` builds its own config at the build's 5 s, not `judge_config`'s |
//!
//! The shared `judge_config` budget was 1 s and is 10 s (item 1 of the row
//! flipped it: the whole `--lib` suite of 5149 tests passed before and after,
//! with no pin needed).

use std::collections::BTreeMap;
use std::path::Path;

/// Files that enable the judge with no budget and no `judge_config`, and why
/// that is safe. Keyed by the path under `src/`.
const EXEMPT: &[(&str, &str)] = &[
    (
        "config/data_floor.rs",
        "validation tests of the config: no rig, nothing is judged",
    ),
    (
        "learning/tests_report_undo.rs",
        "points the judge at 127.0.0.1:9: nothing is judged",
    ),
    (
        "own_bench/tests_sampler.rs",
        "asserts the cap is checked before any call: no judgment is awaited",
    ),
    (
        "tests_learning.rs",
        "points the judge at 127.0.0.1:9; the tests write the judgments themselves",
    ),
    (
        "tests_prove.rs",
        "points the judge at 127.0.0.1:9: nothing is judged",
    ),
];

/// This file, which names the strings it searches for.
const OWN: &str = "tests_judge_budget.rs";

fn enables_the_judge(text: &str) -> bool {
    text.contains("judge.enabled = true") || text.contains("judge.api_base =")
}

fn has_a_budget(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim_start();
        !l.starts_with("//") && l.contains("total_secs") && l.contains('=')
    }) || text.contains("judge_config")
}

fn walk(dir: &Path, out: &mut Vec<(String, String)>, root: &Path) {
    for e in std::fs::read_dir(dir).expect("read the crate's source") {
        let p = e.expect("a source entry").path();
        if p.is_dir() {
            walk(&p, out, root);
        } else if p.extension().is_some_and(|x| x == "rs") {
            let rel = p.strip_prefix(root).unwrap().display().to_string();
            out.push((
                rel,
                std::fs::read_to_string(&p).expect("read a source file"),
            ));
        }
    }
}

#[test]
fn every_file_that_enables_the_judge_sets_a_budget_or_says_why_not() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&root, &mut files, &root);
    assert!(
        files.len() > 100,
        "only {} source files found: the walk read the wrong tree",
        files.len()
    );
    let enabling: BTreeMap<&str, &str> = files
        .iter()
        .filter(|(p, t)| p != OWN && enables_the_judge(t))
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    // 22 files enabled the judge when this was written.
    assert!(
        enabling.len() >= 20,
        "the walk found {} files enabling the judge: it matched almost nothing",
        enabling.len()
    );
    let unbudgeted: Vec<(String, String)> = enabling
        .iter()
        .filter(|(_, t)| !has_a_budget(t))
        .map(|(p, _)| ((*p).to_string(), String::new()))
        .collect();
    let spec = crate::complete::MustBeComplete {
        item: "a file that enables the judge with no total_secs and no judge_config",
        list: "tests_judge_budget::EXEMPT",
        world: "the walk of every file under crates/theseus-core/src that enables the judge",
        add: "set `c.judge.total_secs` in the rig (past any run's work, below \
              until_judged's 20 s guard) or build through tests_judge::judge_config; \
              only if nothing in the file waits on a verdict, add it to EXEMPT with the reason",
        drop: "the file now sets a budget or no longer enables the judge: delete its EXEMPT row",
        least: 5,
        stale_fails: true,
    };
    crate::complete::check(
        &spec,
        EXEMPT
            .iter()
            .map(|(p, why)| ((*p).to_string(), (*why).to_string())),
        unbudgeted,
    );
}

/// The rule itself, on text: a file with a budget or `judge_config` passes,
/// one with neither is found, and a commented budget counts for nothing.
#[test]
fn the_rule_reads_what_a_file_sets() {
    let bare = "c.judge.enabled = true;\nc.judge.api_base = base;";
    assert!(enables_the_judge(bare) && !has_a_budget(bare));
    assert!(has_a_budget("c.judge.total_secs = 10;"));
    assert!(has_a_budget("let c = tests_judge::judge_config(dir, j);"));
    assert!(!has_a_budget("// c.judge.total_secs = 10;"));
    assert!(!enables_the_judge("let x = 1;"));
}
