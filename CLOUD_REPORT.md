# CLOUD_REPORT: tail7-onto (theseus-3327o)

Branch `cloud/20261021-tail7-onto`, base `ae420a49`. Code commit `b255cd79`. Commits are **unsigned**: this VM has no signing key (`git log --format=%G?` says `N`).

## Where the kept yeses live
`Mark.same: BTreeSet<(String, String)>` in `judge/onto/judged.rs`, beside `named`, `#[serde(default)]`. The prune in `judged_run` drops a pair once its `kind <a> <b>` ask key is older than 30 days. `renamed.rs::kind_hints` adds the night's act-band yeses to the mark, and `Hints::same` is the whole kept set. A kept pair whose name is no longer open is ignored: `clusters` skips unknown names. **Store format:** `Mark` is a META JSON record, so a serde-default field needs no bump (still 49).

## The pair rule and its bound
`mint::kind_pairs(o, names, kept)` groups the open names by the kept yeses. A pair across two groups is asked when:
- it adds a session to the larger of the two groups (the old rule, now by group), and
- the two groups plus the sessions of every name no kept yes has linked reach `CLUSTER` (3).

A kept group extends through its own pairs, so another kept group's sessions do not count for it. With no kept yeses this is the old global rule: three singletons still ask `a~b`, `b~c`. Two isolated singletons ask nothing, and two groups that are each a cluster are still asked.
The bound is unchanged: `KIND_PAIRS_MAX` = 20 a run, largest union first, 30-day key. Union is now the groups' union.

## Kept pick across a key change
Done, about 10 lines in `renamed.rs::kind_names`. If `kind-name <new key>` is not asked and a kept `named` entry's key is one of the cluster's folded names and its pick is among the cluster's names, the pick and its ask time are copied to the new key. Nothing is paid and the "named by Jev" mark stays the description sentence. The old entry stays until its own 30 days pass.

## Tests and planted reverts (each restored, `touch`ed, `git status` clean)
| Test | Plant | Failure |
|---|---|---|
| `a_chain_asked_over_two_nights_clusters` (7 singletons, 21 pairs, the cap leaves `Foxtrot~Golf` for night 2; night 1 says yes to `Echo~Foxtrot` only) | `Hints::same` = tonight's yeses only | `tests_onto_names.rs:305` kinds left 0, right 1 |
| `two_isolated_singletons_ask_nothing` | the reach guard always true | `tests_onto_mint.rs:224` |
| `a_pair_that_cannot_reach_a_cluster_is_not_asked` | the same plant | `tests_onto_mint.rs:247` |
| the same test | the global reach rule (all open names count) | `tests_onto_mint.rs:247` |
| `a_kept_yes_is_pruned_with_its_ask` | the prune line removed | `tests_onto_names.rs:334`, left all three pairs, right `ccc~ddd` |
| `a_kept_pick_follows_its_cluster_to_a_new_key` | the carry disabled | fails (the kind's name is asked, the kind is not Jev-named) |
| cockpit `a category write that is no rename...` (C2) | dropped the `-rename` check | fails |
| cockpit source-grep in the `named by Jev` test (the table hidden when nothing minted) | table always shown | fails |

The existing `a_chain_of_singletons_clusters` and `two_clusters_are_asked_before_both_mint` pass with the new `kind_pairs` argument. The other planted tests also ran green unplanted.

## Cockpit
- `OntologyGrowth.tsx`: `MintedPanel` shows the minted table only when `minted.length > 0`. The title count is unchanged.
- `test/ontology.test.ts`: the C2 test, and a source-grep (the existing pattern) for the hidden table.
- `npm test`: 524 pass before the plants, `npx tsc -b` clean.

## Nits
- The `Kind` literal in `mint.rs` is back to the old layout, with `let note` computed first.
- The `tests_onto_names.rs` doc now says the later night asks only the pair, no kind name.

## Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 scripts/gate.sh`)
door, keel, fmt, shape, features, clippy, clippy-probe, cockpit, test build and the reader rule all pass. The suite ran 7964 tests: 7930 pass, 34 fail. All 34 are the known L1 ones (`theseus-sandbox::contract` and `theseusd::sandbox`, the root VM). The phases after the suite (protocol types, lifecycle and the other benches, turn, size) were not run separately: benches are skipped by `NO_BENCH`, and the protocol-types test is inside the suite and passed. `theseus-core onto`: 94 of 94 pass.
Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, exit 0. The gate's own keel phase also printed a rename note for a routing test that comes from the base, not from this branch.
The VM ran out of disk twice (a 39 GB allowance). I removed `target/` (inside the repo) and rebuilt with `CARGO_INCREMENTAL=0`.

## Live check for the maintainer
On a scratch daemon with the sweep's model part on, plant 7 or more one-session `other` names (more than 20 pairs) and run two nights. Night 1 should ask 20 pairs and keep any yeses in the `onto.sweep.judged` META record's `same`. Night 2 should ask the rest, and a chain through a kept yes should mint one kind.

## `docs/status.md` sentence
The ontology sweep keeps its `same_kind` yeses for 30 days, so a chain asked over two nights still mints. It asks a pair only when its groups could reach 3 sessions, and a kept kind-name pick follows its cluster when the cluster's key changes.

## Follow-ups and uncertainty
- A kept pick whose picked name left the cluster is not carried. That is deliberate.
- Many names in one kept group can yield several pairs between two groups, and all are asked, up to the cap.
- Ordering by group union is a small change from name union. It only differs once yeses are kept.
