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
//! still a person's job. Nor is the world more than literal source text: a file
//! that turns the judge on through a shared constructor (`rpc/tests_thread_route.rs`
//! builds through `rig_on(.., Some(jev), ..)`, sets `total_secs = 5` 17 times and
//! holds no `judge.enabled =` line) is outside the walk, though the LEFT table
//! below lists it.
//!
//! **The before/after classification, measured over the merge of `origin/main`
//! `12889bcf` (B7, theseus-4vsrz), not inferred.** `has_a_budget` used to read
//! `total_secs` per line under a comment skip but `judge_config` over the
//! whole text, so a file whose only mention of `judge_config` sat in a comment
//! counted as budgeted. Both arms now run per line under the one comment skip,
//! which classifies strictly more files as unbudgeted and so can only find
//! work, never hide it. Over the 1,529 source files under this crate's `src/`:
//!
//! | | before | after |
//! |---|---|---|
//! | files enabling the judge | 22 | 22 |
//! | of those, unbudgeted | 5 | 5 |
//! | moved into unbudgeted | — | none |
//! | moved out of unbudgeted | — | none |
//!
//! No file moved class: the five unbudgeted files are exactly [`EXEMPT`]'s
//! five, before and after, with no stale row. The asymmetry the sweep closed
//! was therefore latent and not live on this tree — the hole was real and
//! nothing had fallen into it yet, which is why a strengthening reds nothing.
//! `least: 5` is met exactly, so dropping an `EXEMPT` row without a file
//! leaving the class fails, as it should.
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

//!
//! The six raises the last sweep made (theseus-4vsrz, `63ca7587c`), at the
//! lines they stood on its branch (`git grep -n total_secs` finds them now):
//!
//! | site | old -> new | the guard it stays under |
//! |---|---|---|
//! | `own_bench::tests::rig` (`own_bench/tests.rs`) | 2 -> 30 | none on the verdict path |
//! | `own_bench::whole::judged_world` (`own_bench/whole/tests.rs`) | 2 -> 30 | none |
//! | `tests_security::core` | 2 -> 15 | 20 s, twice |
//! | `tests_notices::on_store` | 2 -> 15 | 20 s helper |
//! | `tests_learn_loop::rig` | 5 -> 30 | none |
//! | `tests_gateway` (theseus-discord) | 2 -> 15 | 20 s helper |
//!
//! Budgets the sweep left alone, read at base 85697868 (the line is the
//! `git grep -n` hit; "verdict" is whether the test needs one):
//!
//! | site | budget | why it stays |
//! |---|---|---|
//! | `tests_inbound.rs:435` | 5 s | verdicts; `FakeMode::Slow(10 s)` at `:419` must reach class `timeout`, so the budget has to be short of it |
//! | `tests_offer.rs:223` | 5 s | verdict; `LATE_BATCH` is 2.5 s (`:265`) and is the test |
//! | `tests_stop_nudge.rs:60` | 3 s | verdicts, but the turn waits only `judge::stop::WAIT` (1 s), so the budget does not bind |
//! | `tests_stop_nudge.rs:423` | 6 s | same: `Slow(3 s)` past the 1 s wait; a latent product-wait class, reported and not decided |
//! | `google/tests_label.rs:562` | 10 s | verdict; below the in-body 20 s guard at `:585` |
//! | `rpc/tests_thread_route.rs` | 17 sites at 5 s | the tests that need a verdict set `route_wait_ms = 3000` (`:584`, `:698`, `:752`, and five more), so the product's own wait binds |
//! | `tests_rerank.rs:86` | 5 s | verdict; no short guard over it |
//! | `tests_route.rs:306`, `:602`, `:965` | 30 s | already raised; `:602` holds `Held` on purpose and `:306` is `held()` with a 200 ms wait |
//! | `theseus-discord/src/tests_gateway/content.rs:248` | 2 s | `FakeMode::Down` on purpose at `:234`: nothing is awaited |
//!
//! The daemon tests set a JSON budget and are not touched here:
//! `crates/theseusd/tests/judge.rs:65`, `capture.rs:69`, `stop_nudge.rs:62`
//! (5 s each) and `bench_profile.rs:135` (600).
//!
//! These rows were checked by reading each site's line and its neighbours,
//! not by running a starve against each; the stop nudge's two sites are the
//! ones most worth a later look.

//! **The effective budget is not necessarily `total_secs`** (theseus-4vsrz).
//! The judge client caps live/awaited HTTP time at
//! `min(config.total, deadline.saturating_sub(queued))`. Raising the first
//! number above the second buys nothing. The source census below pins every
//! non-total dispatch expression, with a reason; adding a fourth or removing
//! one without updating its row fails. This is a syntax census, not data-flow
//! analysis: the six `total`/`total_secs` bindings were read back to `svc.cfg`.
//! A verdict's record and a verdict observed by a turn have different bounds:
//!
//! | bound | value / knob | observable it bounds |
//! |---|---|---|
//! | `[threads] route_wait_ms` | 400 ms default; configurable | the thread choice and cross/families verdict the turn can use (`rpc/thread_flow.rs`, `turn/cross_step.rs`, `turn/digest_step.rs`) |
//! | `judge::stop::WAIT` | 1,000 ms; constant, no knob | whether `loop.v1`'s nudge reaches the stopping turn |
//! | `rerank::DEADLINE` | 600 ms; `set_rerank_deadline` in tests | the rerank call and its waiting turn |
//! | cross/families call deadline | 5 s; literal, no knob | the call and its eventual record, even after the turn stopped waiting |
//!
//! A future budget sweep must identify the observable and its shortest bound
//! before raising a rig: the file-level census alone cannot establish it.

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
        !l.starts_with("//")
            && ((l.contains("total_secs") && l.contains('=')) || l.contains("judge_config"))
    })
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
    assert!(!has_a_budget(
        "// unlike judge_config, this rig sets its own budget"
    ));
    assert!(!has_a_budget("/// see judge_config for the shared rig"));
    assert!(has_a_budget(
        "  let c = judge_config(dir, j); // shared rig"
    ));
    assert!(!enables_the_judge("let x = 1;"));
}

/// Exceptions keyed by source and exact expression, never by a drifting line
/// number or just a file (a second short dispatch in that file must fail too).
const SHORT_DISPATCH: &[(&str, &str)] = &[
    (
        "rerank.rs:live(deadline)",
        "600 ms on-path rerank budget (about 350 ms expected); tests can use set_rerank_deadline",
    ),
    (
        "cross.rs:live(Duration::from_secs(5))",
        "five-second detached recall-call window; the turn separately waits route_wait_ms",
    ),
    (
        "families.rs:live(Duration::from_secs(5))",
        "five-second detached recall-call window; the turn separately waits route_wait_ms",
    ),
];

/// Rust tokens keep comments and quoted examples out; groups handle multiline
/// calls and nested Duration constructors without a line-based regular expression.
fn dispatch_arguments(stream: proc_macro2::TokenStream, out: &mut Vec<(String, String)>) {
    use proc_macro2::{Delimiter, TokenTree};
    let tokens: Vec<_> = stream.into_iter().collect();
    for call in tokens.windows(5) {
        if let [TokenTree::Ident(ty), TokenTree::Punct(a), TokenTree::Punct(b), TokenTree::Ident(mode), TokenTree::Group(arg)] =
            call
        {
            let mode = mode.to_string();
            if ty == "Urgency"
                && a.as_char() == ':'
                && b.as_char() == ':'
                && matches!(mode.as_str(), "live" | "awaited")
                && arg.delimiter() == Delimiter::Parenthesis
            {
                let argument: String = arg
                    .stream()
                    .to_string()
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                out.push((mode, argument.trim_end_matches(',').to_owned()));
            }
        }
    }
    for token in tokens {
        if let TokenTree::Group(group) = token {
            dispatch_arguments(group.stream(), out);
        }
    }
}

fn uses_total(argument: &str) -> bool {
    matches!(
        argument,
        "total" | "Duration::from_secs(total)" | "Duration::from_secs(total_secs)"
    )
}

#[test]
fn every_short_judge_dispatch_has_a_reason_and_no_reason_is_stale() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/judge");
    let mut files = Vec::new();
    walk(&root, &mut files, &root);
    let mut found = BTreeMap::new();
    let mut calls = 0;
    for (path, text) in files {
        if Path::new(&path).components().any(|c| {
            let name = c.as_os_str().to_string_lossy();
            name == "tests" || name == "tests.rs" || name.starts_with("tests_")
        }) {
            continue;
        }
        let mut arguments = Vec::new();
        dispatch_arguments(
            text.parse().expect("judge source is Rust tokens"),
            &mut arguments,
        );
        calls += arguments.len();
        for (mode, argument) in arguments {
            if uses_total(&argument) {
                continue;
            }
            let site = format!("{path}:{mode}({argument})");
            assert!(
                found
                    .insert(site.clone(), "non-total dispatch budget".to_owned())
                    .is_none(),
                "duplicate short dispatch {site}: one allowlist row cannot excuse two calls"
            );
        }
    }
    assert!(
        calls >= 9,
        "found only {calls} live/awaited calls: check the source walk"
    );
    crate::complete::check(
        &crate::complete::MustBeComplete {
            item: "judge dispatch whose deadline is not the configured total",
            list: "tests_judge_budget::SHORT_DISPATCH",
            world: "non-test Rust files under theseus-core/src/judge",
            add: "use the config-derived total, or document the shorter bound and its reason in SHORT_DISPATCH; raising total_secs alone cannot extend this call",
            drop: "the dispatch was removed or now uses total: remove its stale SHORT_DISPATCH row",
            least: 0,
            stale_fails: true,
        },
        SHORT_DISPATCH.iter().map(|(site, why)| (site.to_string(), why.to_string())),
        found,
    );
}

#[test]
fn dispatch_scan_sees_literals_constants_and_multiline_calls_not_prose() {
    let mut got = Vec::new();
    dispatch_arguments(
        r#"
        // Urgency::live(Duration::from_secs(99))
        const DOC: &str = "Urgency::awaited(Duration::from_millis(99))";
        fn send() {
            Urgency :: live (
                Duration::from_secs(5),
            );
            Urgency::awaited(Duration::from_millis(50));
            Urgency::live(DEADLINE);
            Urgency::live(total);
            Urgency::live(Duration::from_secs(total));
            Urgency::live(Duration::from_secs(total_secs));
        }
        "#
        .parse()
        .unwrap(),
        &mut got,
    );
    assert_eq!(got.len(), 6);
    assert_eq!(got[0], ("live".into(), "Duration::from_secs(5)".into()));
    assert_eq!(
        got[1],
        ("awaited".into(), "Duration::from_millis(50)".into())
    );
    assert_eq!(got[2], ("live".into(), "DEADLINE".into()));
    assert_eq!(got.iter().filter(|(_, arg)| !uses_total(arg)).count(), 3);
}
