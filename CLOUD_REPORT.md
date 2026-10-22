# CLOUD_REPORT: tail4-onto (theseus-63tfk.1)

Branch `cloud/20261021-tail4-onto`, base `8260dbab`. Commits: `2bece9b` (ontology: the rename step), `1fc4343` (the three
asks, packs, tests, docs). Commits are unsigned (no key on this VM). Store format stays 49: no stored field was added.

## What changed

The sweep now does three things it did not:

1. **Rename** a stale machine category by a Choice among mined names (`name_it.v1`). The old name goes into the
   description (`was: <old>`, ahead of any description); act, tell (morning note), undo (the record as it was, byte for
   byte, so name and description both come back).
2. **`same_kind`**: before `mint::plan` clusters, pairs of mined `other` names that no session shares are asked a
   `same_kind.v1` Noul; a yes at the act band joins their sessions into one cluster.
3. **The kind's name**: a cluster that will mint and has two names or more is asked a Choice among its names
   (`name_it.v1`, no current name). Jev's pick at the act band names the kind; `none`, a pick below the band, a single
   name, or no answer falls back to the most shared name (today's rule). `Swept.named_kinds` and the sweep's info log
   say how many kinds were named by Jev; the rest took the rule.

Files: `theseus-ontology/src/renamed.rs` (`rename_candidates`, `plan_rename`, `Step::Rename`);
`theseus-judge` (`builders/sweep_names.rs`, packs `same_kind.v1.toml` and `name_it.v1.toml`, `Builder::{SameKind,NameIt}`,
`Input::{SameKind,NameIt}`, replay/rebuild arms, fixtures and goldens); `theseus-core/src/judge/onto/renamed.rs` (the
asks), `mint.rs` (`Hints`, clusters across names, `plan_with`), `judged.rs` (`Run` and helpers made `pub(super)`,
`judged_run`, the rename write), `sweep.rs` (`onto_sweep_night`), `digest.rs` (the rename line); `WIRED` lists both new
packs `Live`, like `same_thing.v1`.

`onto_sweep(now)` is unchanged in meaning (the plan with no model, mint with default hints), so every existing sweep,
mint and judged test passes untouched. The nightly tender calls `onto_sweep_night` instead: hints (same_kind pairs, kind
names), then the plan with them, then pairs, splits, renames.

## The rename's evidence rule

A category qualifies when a machine wrote it, it has no guidance and nothing below it, and `stale_names` says so from the
titles of its newest 50 titled private sessions: at least **5** titled sessions; the category's folded name is in **none**
of the folded titles; one mined name (paths, hosts, capitalised terms, via `theseus_judge::mine`) is in at least **3**
titles and at least **half** of them. The options are the mined names in two titles or more, most shared first, at most
ten; `none` keeps the name. At most 3 renames asked a run. The name must also fold differently from the old one and pass
the ontology's own checks (a sibling's name is refused) or nothing is written.
Note: mined names are titles/capitalised terms, so the "name appears in a title" test (a substring on folded text) is
what keeps a category named in plain words from being called stale; I chose that over "no mined name folds to it".

## Packs: two new, and why

| Pack | Question | Bands |
|---|---|---|
| `same_kind.v1` | `same_kind` (Noul): are the first group's and second group's sessions about one kind of thing? | act 0.85, confirm 0.60 |
| `name_it.v1` | `name` (Choice among `n1..n10` mined names, `none`): which name best names the thing, keeping the current name unless one fits clearly better | act 0.85, confirm 0.60 |

`name_it` serves both the rename (state shows `current`; `none` keeps it) and the kind's name (no current; `none`
leaves it to the rule): one Choice, one state builder, one pack. `same_kind` is its own pack since its state is two
groups and its answer a Noul. Both are separate from `same_thing.v1` and `two_things.v1` because their state shapes
differ (those show categories, these show groups of `other` answers or a category to rename); a question added to an
existing pack would have changed that pack's state and versions. Both are off every turn's path.

## Money and not-again rules (all five asks)

One mark (META `onto.sweep.judged`), one day cap (`[ontology] sweep_usd_per_day`, default unchanged, 0 turns all five
off and the plan still runs): every ask goes through `judge_sweep` with `Run::left()`, is reserved on the judge's day
budget, and is booked with `Run::paid` (mark written after each paid ask). Keys: `pair a b`, `split c` (existing),
`rename <category>`, `kind <folded a> <folded b>`, `kind-name <cluster key>`; each is not asked again for 30 days
whatever the answer. Caps a run: pairs 20, splits 3, renames 3, same_kind pairs 20 (`KIND_PAIRS_MAX`), kind names up to
3 (`mint::PER_RUN` clusters). The hints and the later asks are two `Run`s that each read the mark fresh, so one day's
money is shared. `Run::going()` (the stop) is checked before each ask in all of them.
Design choices to hear: a same_kind pair is a pair of *names* (not sessions), asked only when their sessions together
make a cluster neither makes alone and not both already make one; a cluster with a single name is not asked (nothing to
choose). A name Jev picked that makes no kind falls back to the most shared name before the cluster is counted refused.

## Tests and planted reverts

New file `tests_onto_names.rs` (named tests live there, not in `tests_onto_sweep.rs`, which has no Jev rig; the rig is
shared with `tests_onto_judged.rs`, whose helpers became `pub(crate)`), `tests_renamed.rs` in theseus-ontology, and one
in `tests_onto_digest.rs`. Each plant: break, run the named test, restore with `git checkout` and `touch`, `git status`
clean.

| Test | Plant | Failed as |
|---|---|---|
| `a_stale_category_is_renamed_by_jevs_pick_and_undone` | `onto_judged_writes` does not push the rename step | the test failed (renamed 0) |
| `a_rename_below_the_act_band_writes_nothing` | `named()` also takes option `n1` below the band | `left: (1, 1) right: (0, 0)` |
| `two_other_names_jev_calls_one_kind_cluster_and_mint` | `plan_with` clusters with no joins | `left: (0, 0) right: (1, 0)` (no kind) |
| `a_minted_kind_is_named_by_jevs_choice` | `picked = None` (always the most shared name) | `left: (1, 0) right: (1, 1)` |
| `the_sweeps_new_asks_stop_at_the_days_money` | the shared `ask()` passes `Micros::MAX` instead of `left()` | `left: 3 right: 0` (calls begun with the day spent) |
| `with_no_pick_the_most_shared_name_names_it` | (guards the fallback; two cases: `none`, and `n2` at 0.7) | not separately planted: the `named()` plant above also breaks it |

I did not plant "skip `going()`": `going()` is the stop check, not the cap; the cap test plants `left()` in the one
function all three new asks share. Also: `a_name_in_the_titles_or_too_few_sessions_is_not_stale` (pure),
ontology `a_rename_keeps_the_id_and_the_old_name_in_the_description_and_undoes` (the undo is the old record byte for
byte) and `only_a_loose_machine_category_is_renamed_and_never_to_itself`, and `a_rename_is_a_line_with_both_names`
(digest).

Runs: `cargo nextest run -p theseus-ontology -p theseus-judge` 307 passed; `cargo nextest run -p theseus-core onto` 83
passed (before the digest test; 84 with it); the whole workspace in the gate below. `cargo nextest run -p theseusd judge`
ran inside the gate's suite (passed). No run under load: the task asks for none for this row.

## Pack-count changes (a listing, not a weakened test)

Embedded packs 27 to 29: `pack.rs`'s loader test (`six.len()` 26 to 28 and its message), the golden request and state
for each new pack (`UPDATE_GOLDEN=1`), the `builders` golden list, and the health pack lines in `tests_inbound.rs`
(`INBOUND_ALONE` 21 to 23), `tests_continue.rs`, `tests_judge.rs` (two lists) and theseusd's `tests/judge.rs` (each gains
`same_kind.v1: live` and `name_it.v1: live`).

## Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run on the tree before it was split in two commits)

door, keel (0 findings), fmt, shape, features, clippy (also `clippy --workspace --all-targets -- -D warnings` clean),
clippy-probe, cockpit, test build and reader rule passed. Suite: **7829 run, 7795 passed, 34 failed, all of them the
known L1 root-daemon set** (theseus-pv6i): theseus-sandbox `bench spawn_100` and 19 `contract` tests, and 14 theseusd
`sandbox` tests (`a_cancel_of_an_l1_job...`, `a_deadline_ends_a_job...`, `a_granted_program_in_l1...`,
`a_job_with_no_list...`, `a_listed_host_is_reached...`, `a_probe_script_in_l1...`, `an_aws_granted_l1_job...`,
`an_l1_jobs_own_pkill_f...`, `health_reports_the_last_real_l1_launch`, `l1_argv_routes_a_call_to_l1`,
`the_jobs_bench_l1_row`, `unlisted_and_private_hosts...`, `a_running_l1_jobs_command...`, `a_host_beyond_the_list...`;
34 = 20 + 14). No other failure; the output golden passed under `TZ=America/Phoenix`.
The phases after the suite: `protocol_types` clean (no protocol change), `cargo deny check bans licenses sources` ok (run by hand;
the advisories phase was not run). Benches skipped (`THESEUS_GATE_NO_BENCH=1`).

Keel guard against the stated base: `python3 scripts/keel-guard.py --base 8260dbab --head HEAD --quiet` printed nothing
(exit 0). Against a freshly fetched `origin/main` it lists erosions of other lanes' tests: `origin/main` has moved past
`8260dbab` and those are not this branch's.

## Live check for the maintainer

On a scratch daemon with a copy of the store and a real Jev key, a config with `[ontology] sweep_usd_per_day = 1.0`:
`theseusd ... ` then run the nightly (restart the scratch daemon: one sweep runs after a start) and look in the ledger:
`theseus ledger ...` for `ontology.category` rows scoped `ontology.write:sweep-*-rename`, `theseus ontology undo
sweep-<ms>-<n>-rename` (should restore the name and description), and the morning note lines (`renamed topic "x" to
"y" · undo: ...`). Judgments for `same_kind` and `name_it` appear in `theseus judge list`.

## Uncertain, and follow-ups

- The mined-name options for a rename come from titles only; a category whose sessions have no titles is never asked.
- `Swept` is not a ledger row; "named by Jev vs. the rule" is in the info log and the struct, not the morning note.
- Cockpit's Ontology view is unchanged (a rename shows as a name change).
- Disk: this VM's writable allowance (about 40 GB) filled during the build; I cleared `incremental` and stale test
  binaries in `target/` and ran with `CARGO_INCREMENTAL=0`.
- Doc for the integrator, `docs/status.md`: "The nightly ontology sweep can also rename a stale machine category (Jev picks
  among names mined from its sessions' titles; the old name is kept in the description; undoable), join `other` answers
  under different names into one kind (`same_kind.v1`), and let Jev name a minted kind (`name_it.v1`); all at the act band
  and under the sweep's day cap (theseus-63tfk.1)."
