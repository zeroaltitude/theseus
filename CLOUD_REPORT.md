# CLOUD_REPORT: tail6-onto (theseus-e3pxa)

Branch `cloud/20261021-tail6-onto`. Commits: `17818ca7` (core), `dbdebe1f` (cockpit). Commits are unsigned (no key on this VM). No line ceiling was passed. Store format is unchanged (49); no stored record gained a field.

## Findings and changes

- **F1.** `renamed.rs` `titles_of` now reads the store inside `theseus_store::blocking`, as `held_by` does. There is no test rig that fails a store read on the async worker, so this rests on the code read.
- **F2.** `mint.rs` `kind_pairs` now asks a pair when its sessions add to either name's, as long as the open (unlisted) names reach 3 sessions between them. Singleton chains (`a~b`, `b~c`) are asked, and two names that are each already a cluster are asked before both mint. The bound is the existing `KIND_PAIRS_MAX` (20 a run, largest union first) in `renamed.rs`, with the 30-day not-again key. One cost to know: with many one-session names the pair count grows with the square of the names, so the 20-a-run cap, not the rule, bounds the spend.
- **F3.** A kind-name pick is kept in the day mark (`judged.rs` `Mark.named`, a serde-default map in the existing META record `onto.sweep.judged`). It lives as long as its `kind-name <key>` ask is not asked again (pruned with it at 30 days) and is used when the cluster mints later. `kind_names` also paces its asks with `pressure::quiet`, as the other asks do.
- **F4.** The template's `[ontology] sweep_usd_per_day` comment had lost its separator before "a category of 12 sessions": a comma is restored. The comment still reads "..., and names the `other` answers...". I had no copy of the review, so this is my reading of the spot. The `config/ontology.rs` module doc now names all four packs.
- **Visibility.** A sweep-minted kind Jev named ends its description with `mint::NAMED_NOTE` ("Jev picked its name."). The morning note's kind line gains ", named by Jev". The cockpit's minted panel shows a "named by Jev" pill (`mintedOf` reads the kind row's `undo.wrote.description`) and lists renames ("renamed topic:x: old → new, when"; `renamedOf`, from `-rename` writes). The detection is a fixed sentence written by our own code, not guessed text. No protocol change.
- **Docs.** Core `AGENTS.md`, the ontology section: the pair rule, the kept pick and the note.

## Proof

- `cargo nextest run -p theseus-core onto`: 89 passed.
- Planted reverts, each restored and `touch`ed, `git status` clean after:
  - Union-of-3 rule restored in `kind_pairs`: `a_chain_of_singletons_clusters` failed (`tests_onto_mint.rs:149`).
  - Clusters skipped in `kind_pairs`: `two_clusters_are_asked_before_both_mint` failed (`:180`).
  - Kept pick not stored: `a_kind_name_answer_waits_for_its_mint` failed (`tests_onto_names.rs:253`).
  - Digest suffix removed: `a_kind_jev_named_says_so_in_its_line` failed (`tests_onto_digest.rs:304`).
  - Cockpit pill removed: `a kind Jev named is marked, and a rename shows both names and when` failed. The cockpit has no component runner, so that test checks the pure `mintedOf` and `renamedOf` and reads the panel source for the pill and the rename line (the way the Ontology view test already does).
- `a_kind_name_answer_waits_for_its_mint` covers two paths. A cluster whose kind cannot mint (`2026 …` is no kind name) keeps its pick in the mark and counts as refused. A later night with a kept pick and a spent ask mints under the kept name and asks only the pair. I did not exercise "cluster mints a later night" end to end from the first night, because the cluster key and names would change.
- Cockpit: `npm test` 523 pass; `tsc -b` clean; lint shows only the existing warnings.
- Load runs: none; no race was involved.
- Keel guard (`scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, exit 0.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, clippy-probe, cockpit, test build and reader rule passed. The suite phase failed only on the known L1 cases: theseus-sandbox `contract` tests and theseusd `sandbox` tests (root daemon with no job cgroup, theseus-pv6i). No other test failed. I did not run the phases after the suite (protocol types and the benches); this change touches no protocol type and the benches are skipped. `cargo deny` was not run on its own. This VM had little disk, so I built with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`.

## Live check for the maintainer

On a scratch daemon with a copy of the store: run `theseus ontology` sweep night with 3 `other` sessions under 3 one-session names and confirm `same_kind` is asked for each pair; then check the morning note shows "named by Jev" and the cockpit's Ontology → minted panel shows the pill and a rename line.

## For docs/status.md

"The ontology sweep reads titles off the worker, asks `same_kind` for singleton chains and for two clusters, keeps a paid kind-name pick until the kind mints, and shows Jev-named kinds and renames in the morning note and the cockpit (theseus-e3pxa)."

## Follow-ups

- The "named by Jev" mark is a sentence in a kind's description; a stored field would be sturdier but needs a store format bump.
- A kept pick is keyed by the cluster's smallest folded name, so it is lost if the cluster's names change before it mints.
