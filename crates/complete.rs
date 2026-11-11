// One shape for a list that must be complete (theseus-3i6d1).
//
// **The pattern this exists to kill.** A hand-maintained list that is a
// guard — the kinds an operator post may carry, the system calls a sandbox
// denies, the catalog members a credential rule holds — is kept by hand and
// checked by nothing. Three of them were found in one night, in three
// crates, by three agents: `outbox::OPERATOR_KINDS`, the seccomp filter's
// `denied()`, and `MARKED` in theseus-core's secret-shape rule. In every
// one the same thing was true: a test read the list and judged the world by
// it, so deleting an entry deleted the test's own question with it. The
// reviewer deleted `perf_event_open` from `denied()` and the whole sandbox
// suite stayed 40/40 green.
//
// **A third pattern, beside this one and "a rule that lives only in a
// comment": a test that documents the defect as the design** (theseus-4vsrz,
// B7; found by the theseus-a2kh6 builder). `tests_tier_work.rs` said, in
// the one admission test, that "the pick has to be a switch" and that a pick
// staying on the session's model "has no second settle, and its tools keep
// the ordinary ceiling whatever the route row says": the precise description
// of the bug a2kh6 was sent to fix, written as settled behaviour. A stale
// comment misleads and a list with no completeness test fails silently, but a
// test that encodes the defect *defends* it: whoever fixes the bug sees that
// test fail, reads a confident sentence saying the behaviour is intended, and
// backs out. **The tell** is a test or its comment explaining why the narrower
// behaviour is correct with no decision, row or owner quote beside it ("has
// to be", "whatever X says", "never re-"). A real constraint has a source; an
// accidental one only has prose. A test that pins a known gap on purpose must
// say so, name what closes it and say which edit makes it fail, as
// `theseus-tui`'s `a_routine_runs_question_says_waiting_while_the_run_deadline_drops_it`
// does.
//
// **The other pattern, "a rule that lives only in a comment".** The tell: a
// comment or doc sentence states a rule ("never X", "always Y before Z") that
// no test and no code executes, so the sentence can go false and nothing
// fails. The fix is one of two: write the test that holds the rule, or delete
// the sentence. A comment may explain a rule a test holds; it may not be the
// only place the rule is kept.
//
// **Corrected, historical, and the history now found.** The
// `tests_tier_work.rs` citation above is no longer a live instance, and the
// two commits it lived between are these: the sentences were written at
// `b447da3e` (2026-11-02, theseus-h66v4.9, "tier tests restored through
// work.v1") and removed at `a714497d3` (2026-11-07, theseus-a2kh6, "turn: a
// deep turn keeps the deep tool ceiling on its own model"), so the pattern's
// first known instance stood for five days and was retired by the fix it was
// defending. **Two earlier passes over this paragraph said the commit could
// not be named**, and the second one's diagnosis was right: a `git log -S`
// over those sentences returns nothing on a SHALLOW clone, which the cloud's
// is and which is where both passes were authored. Its conclusion — that the
// commit is outside the history rather than in it — was wrong. On the full
// clone (`git rev-parse --is-shallow-repository` false, 6,077 commits on
// `main`) the search finds both with no `--all`, and both are ancestors of
// `main`. **A cloud session's "`git log -S` found nothing" is evidence of
// nothing; re-run it on a full clone before writing "not found" into a tree.**
//
// **The repair is the shape to copy**, because `a714497d3` did not only
// delete the sentence. It rewrote the module's own header to name all three
// cases admission must follow and to say which of them the tier had been
// missing, **with the row id beside it**: "for a switch, for a pick that
// stays on the session's own model, and for a verdict that lands after the
// turn's tools have settled, which is the one the tier used to miss
// (theseus-a2kh6)". A sentence that cites its row cannot become this pattern,
// and that is the cheapest guard there is.
//
// **Swept, and no live instance** (theseus-4vsrz, 2026-11-10;
// `scripts/test-code-map.py --sweep`). **1,772 test-code regions in 1,765
// files, measured on `7c4b3e15`** — a figure every merge of main moves, so it
// names its head, and `--sweep` prints today's; it was 1,770 in 1,763 one
// departure earlier. The regions are taken from the declarations the compiler
// reads and not from any file's own text or `tests_`-shaped name, and were
// searched for the tells above.
// 101 comment lines carried one with nothing to cite, and every one, read by
// hand, was false. **Why the tells are weaker here than they look is the part
// worth keeping:** "whatever X says" is this codebase's idiom for
// *strengthening* an invariant — "the result is Y whatever X says" — which is
// this pattern's exact opposite, and it was 36 of the 101. The two hits that
// came closest turned out to be the prescribed correct shape, each naming its
// gap, what closes it and which edit makes it fail: `theseus-tui`'s
// `tests_questions.rs:105`-`:117` and `theseus-tools`' `tests_stale.rs:147`-
// `:158`. So the sweep is a reading aid; it is not a gate, and a hit is a
// question rather than a finding.
//
// **The shape.** Whatever *enumerates* the thing being listed — the call
// sites in a crate's source, the requirement a filter is built against, the
// members of a catalog — is walked by the test, and the list is judged
// against that walk, never the other way round. An item the walk found that
// the list lacks fails. The walk's own size is asserted with a figure, so an
// enumeration that silently returns nothing cannot pass: a "zero missing"
// bar is met by an empty set, and that is the very bug class this file is
// here to close.
//
// **The failure message is the point** (copied from
// `the_rules_reach_is_what_the_design_doc_says`, which is the one working
// instance anybody had built: `crates/theseus-core/src/aws/
// tests_secret_shapes.rs`). A completeness test is only worth its run time
// if its failure tells the next person *where to look*. So [`check`] never
// fails with a bare count or a bare `assert!(missing.is_empty())`: it names
// **both documents** — the list, and whatever enumerated the world — says
// which items each one claims, and says what to do about it. A test that
// fails with "a number changed" teaches nothing; one that says "this walk
// found X, that list does not hold it, add it there" fixes itself.
//
// **Why a file and not a crate.** theseus-core, theseus-sandbox and the
// aws tests are three crates, and the rule is that no step adds a
// dependency. A test-only crate would be a dependency; a module inside a
// shipped crate would put test scaffolding in the daemon. So this is one
// file, included where it is needed:
//
// ```ignore
// #[cfg(test)]
// mod complete {
//     include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../complete.rs"));
// }
// ```
//
// `CARGO_MANIFEST_DIR` is the including crate's own directory, so the path
// is the same however deep in `src/` the including file sits. The repository
// already reaches across a crate boundary for a test helper this way
// (`crates/theseusd/tests/jail_stop.rs` takes theseus-sandbox's
// `tests/common/mod.rs`).
//
// Standard library only, and no `dev-dependencies` of its own.

use std::collections::BTreeMap;

/// What a list that must be complete is, in words its failure can use.
///
/// Every field is a sentence fragment the report prints, so fill them in as
/// if writing the message the next person will read at 2 a.m.
pub struct MustBeComplete<'a> {
    /// What one entry is, singular: "a denied system call", "an operator
    /// post's kind", "a credential-shaped member".
    pub item: &'a str,
    /// Where the hand-kept list lives, precisely enough to open it:
    /// `theseus_sandbox::seccomp::denied()`.
    pub list: &'a str,
    /// What enumerated the world, precisely enough to open it: "the operator
    /// call sites under crates/theseus-core/src".
    pub world: &'a str,
    /// What to do about an item the world holds and the list does not.
    pub add: &'a str,
    /// What to do about an item the list holds and the world does not. Read
    /// only when [`Self::stale_fails`] is set.
    pub drop: &'a str,
    /// The least number of items the walk must have found. A walk that found
    /// fewer read the wrong tree, or read nothing, and the test fails before
    /// it compares anything: this is the bar an empty enumeration trips.
    pub least: usize,
    /// Whether an entry on the list that the walk did not reach is a
    /// finding. True where the list and the walk must be the same set; false
    /// where the list is one of several reasons an item may be held, and a
    /// staleness test of its own guards the other direction.
    pub stale_fails: bool,
}

/// Judge `listed` against `found`, and panic with the whole report when they
/// disagree.
///
/// `found` is the walk: each item, and where it was found, which the failure
/// quotes. `listed` is the hand-kept list: each item, and (where it helps)
/// what the list says about it; an empty string is fine.
///
/// Returns how many items the walk found, so a caller can print or further
/// assert it.
///
/// # Panics
///
/// When the walk found fewer than [`MustBeComplete::least`] items, when the
/// walk found an item the list lacks, or (with
/// [`MustBeComplete::stale_fails`]) when the list holds one the walk did not
/// reach.
pub fn check(
    spec: &MustBeComplete<'_>,
    listed: impl IntoIterator<Item = (String, String)>,
    found: impl IntoIterator<Item = (String, String)>,
) -> usize {
    let listed: BTreeMap<String, String> = listed.into_iter().collect();
    let found: BTreeMap<String, String> = found.into_iter().collect();

    assert!(
        found.len() >= spec.least,
        "the walk of {} found only {} of {}: it expected at least {}, so it \
         read the wrong thing or read nothing at all, and a completeness test \
         that walks nothing passes on an empty set. Fix the walk before \
         trusting {}.",
        spec.world,
        found.len(),
        spec.item,
        spec.least,
        spec.list,
    );

    let mut report = Vec::new();
    let missing: Vec<&String> = found.keys().filter(|k| !listed.contains_key(*k)).collect();
    if !missing.is_empty() {
        report.push(format!(
            "{} of {} that {} found and {} does not hold:",
            missing.len(),
            spec.item,
            spec.world,
            spec.list,
        ));
        for k in missing {
            let site = &found[k];
            if site.is_empty() {
                report.push(format!("  {k}"));
            } else {
                report.push(format!("  {k}  ({site})"));
            }
        }
        report.push(format!("  -> {}", spec.add));
    }
    if spec.stale_fails {
        let stale: Vec<&String> = listed.keys().filter(|k| !found.contains_key(*k)).collect();
        if !stale.is_empty() {
            report.push(format!(
                "{} of {} that {} holds and {} did not reach:",
                stale.len(),
                spec.item,
                spec.list,
                spec.world,
            ));
            for k in stale {
                let says = &listed[k];
                if says.is_empty() {
                    report.push(format!("  {k}"));
                } else {
                    report.push(format!("  {k}  ({says})"));
                }
            }
            report.push(format!("  -> {}", spec.drop));
        }
    }
    assert!(
        report.is_empty(),
        "{} and {} disagree about {} ({} walked, {} listed).\n{}",
        spec.world,
        spec.list,
        spec.item,
        found.len(),
        listed.len(),
        report.join("\n"),
    );
    found.len()
}

/// The same judgement, for a list and a walk of bare names.
pub fn check_names<'a>(
    spec: &MustBeComplete<'_>,
    listed: impl IntoIterator<Item = &'a str>,
    found: impl IntoIterator<Item = &'a str>,
) -> usize {
    check(
        spec,
        listed.into_iter().map(|s| (s.to_string(), String::new())),
        found.into_iter().map(|s| (s.to_string(), String::new())),
    )
}

#[cfg(test)]
mod complete_tests {
    use super::*;

    fn spec() -> MustBeComplete<'static> {
        MustBeComplete {
            item: "a widget",
            list: "the WIDGETS table",
            world: "the walk of the widget factory",
            add: "add it to WIDGETS",
            drop: "delete its WIDGETS row",
            least: 2,
            stale_fails: true,
        }
    }

    #[test]
    fn a_walk_that_found_nothing_fails_before_it_compares() {
        let e = std::panic::catch_unwind(|| check_names(&spec(), ["a", "b"], []))
            .expect_err("an empty walk must fail");
        let said = e.downcast_ref::<String>().expect("a message").clone();
        assert!(said.contains("read nothing at all"), "{said}");
        assert!(said.contains("at least 2"), "{said}");
    }

    #[test]
    fn an_item_the_walk_found_and_the_list_lacks_fails_naming_both() {
        let e = std::panic::catch_unwind(|| check_names(&spec(), ["a"], ["a", "b", "c"]))
            .expect_err("a missing item must fail");
        let said = e.downcast_ref::<String>().expect("a message").clone();
        assert!(said.contains("the WIDGETS table"), "{said}");
        assert!(said.contains("the walk of the widget factory"), "{said}");
        assert!(said.contains("  b"), "{said}");
        assert!(said.contains("  c"), "{said}");
        assert!(said.contains("add it to WIDGETS"), "{said}");
    }

    #[test]
    fn a_stale_row_fails_only_where_the_list_and_the_walk_are_one_set() {
        let e = std::panic::catch_unwind(|| check_names(&spec(), ["a", "b", "z"], ["a", "b"]))
            .expect_err("a stale row must fail");
        let said = e.downcast_ref::<String>().expect("a message").clone();
        assert!(said.contains("delete its WIDGETS row"), "{said}");
        assert!(said.contains("  z"), "{said}");
        let loose = MustBeComplete {
            stale_fails: false,
            ..spec()
        };
        assert_eq!(check_names(&loose, ["a", "b", "z"], ["a", "b"]), 2);
    }

    #[test]
    fn a_list_the_walk_agrees_with_passes_and_says_how_many_it_walked() {
        assert_eq!(check_names(&spec(), ["a", "b"], ["a", "b"]), 2);
    }

    #[test]
    fn the_failure_quotes_where_each_item_was_found() {
        let found = [
            ("a".to_string(), "src/one.rs:12".to_string()),
            ("b".to_string(), "src/two.rs:4".to_string()),
        ];
        let e = std::panic::catch_unwind(move || check(&spec(), [], found))
            .expect_err("two missing items must fail");
        let said = e.downcast_ref::<String>().expect("a message").clone();
        assert!(said.contains("src/two.rs:4"), "{said}");
    }
}
