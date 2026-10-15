# CLOUD_REPORT: token-budget (theseus-to0xw)

Branch `cloud/20261013-token-budget`. Built on a3812226 (store format 39; no bump, no stored record changed).

## What I found (today's values, before any trim)

First request as sent, default config (deferred tools at their constant, search tool at 264), debug build:

| place | before | after | budget |
|---|---|---|---|
| cli | 7,343 | 7,343 | 8,000 |
| task | 6,599 | 6,599 | 8,000 |
| dm | **15,642** | 14,584 | 15,000 |
| channel | 7,201 | 6,998 | 7,900 (today + 10 %, to the hundred) |
| headless | 7,860 | 7,860 | 8,600 (same) |

(The scratch daemon reads cli 7,340 and the bench's daemon 7,345: the path in the session block differs.)

- cli and task were already under 8,000. The `bench prompt` baseline (16,006) was stale.
- **A bug in the census**: `--place dm` built the DM's system but chose the loaded families as the CLI's, because a census session has no outbox target. dm read 7,340, equal to cli. Counted as the DM, it is 15,642, over 15,000.
- The byte line was 41.5 KB with the default tools, 58.6 KB with the bench config's.
- Accuracy against the provider: I could not check. No key here. The constants (93 deferred, 264 search) are theseus-0nnk's measurement. Maintainer: `theseus census --place dm --exact` for each place and compare `exact.total` with `total_tokens`.

## Steps and commits

1. a55802a7 `census: count dm, channel and headless as their own kind...` Fixes the bug above (`TurnRunner::census_deferral`, `deferral_in` in turn/defer_step.rs). New `channel` (shared, public tools alone) and `headless` place names in `PLACES`, CLI help, generated TS. New `Core::first_request(place)` in `first_request.rs` is the census's own total plus the tools the request loads. It runs in the bench and census only. `tests_census`'s refusal test now compares against `PLACES.join(", ")` (a rewrite, not a weakening).
2. 61643df6 `tools: a DM defers the channels family by default`. dm 15,642 to 14,786 (-855: channel_post and channel_read). No tool removed. A call to an unsearched deferred tool gets the existing miss words, and the ledger's `load_share` (10 %) puts it back. The Discord glide test (about approval, and scripts a model calling channel_post in a DM at once) loads the family in its fixture.
3. daa157cc `task_create's description says once what its schema says`. 14,786 to 14,640.
4. f1b15d01 `wake_at's description leaves its formats to the schema`. 14,640 to 14,584.
5. 46f32de6 `core: budgets ... and an allowlist`. `first_request::{CLI_TASK_TOKENS 8000, DM_TOKENS 15000, CHANNEL_TOKENS 7900, HEADLESS_TOKENS 8600, budget()}`, and `ALWAYS_LOADED`: every tool a place loads, with the place kinds and a one-line reason. Test `a_loaded_tool_is_named_in_the_allowlist_with_its_reason` also fails a stale entry. Test `every_kind_of_place_is_within_its_budget`.
6. 2918f50b `sim: the turn bench judges the first request in tokens as sent, and the byte line is gone`. `FIRST_REQUEST_KB` and its check removed. Verdicts `first_request_tokens_<place>` for cli, task, dm, channel, headless, from `context.census` on the default-config daemon. Bytes remain as information. Unit tests for at-budget passes and one-over fails, and for the byte line being gone. `history.rs`'s near-limit exception and its test now name a token row. sim AGENTS.md updated.
7. 009b9a8d `sim: bench prompt counts every kind of place`. Baseline moved (cli 7,345, task 6,601, dm 14,586, channel 6,998, headless 7,862, harness 7,335); it only warns.

I did not trim cli or task (under budget). The descriptions I shortened lose no rule; I compared them line by line.

Evidence for the trims: the cloud has no ledger, and `theseus census`'s use column reads 0 calls everywhere here. So I applied the owner's rule (deferred by default) to the family a DM least needs (it answers in itself), and shortened the two descriptions the census shows longest per what they say. **The ledger on the owner's machine should confirm or reverse the channels deferral**: `theseus census --place dm` shows channel_post and channel_read calls.

## Proof

- `cargo nextest run -p theseus-core first_request`: 9 pass (measure equals census for all six names; each kind loads its own families; budgets; allowlist).
- Planted reverts, each restored and `touch`ed, `git status` clean after:
  - census_deferral ignores the asked kind: `each_kind_of_place_is_counted_with_its_own_families` fails (the DM no longer loads task_create).
  - cli also loads the terminals family: `a_loaded_tool_is_named_in_the_allowlist...` fails (term_close is named) and `every_kind_of_place_is_within_its_budget` fails.
  - dm loads channels again: both fail (15,440 against 15,000).
  - the census counts a deferred tool at its full definition: `every_kind_of_place_is_within_its_budget` fails (cli 17,446) and `tests_census::with_tools_deferred_the_census_is_the_request_a_turn_sends` fails.
  - Not planted: `tools_defer::tools_bytes` (the estimate's own, not the measure's); reverting it fails `tests_defer::each_place_kinds_first_request_sends_less_than_the_whole_catalog`.
- Live: `target/debug/theseus-sim bench turn --check --runs 3 --theseusd target/debug/theseusd`: every row ok, including `first_request_tokens_cli: 7345 tokens at the p95, budget 8000`, dm 14586/15000, task 6601/8000, channel 6998/7900, headless 7862/8600.
- Whole suite, by hand (below), before each of commits 1, 2, 3-4 together, 6: only the known failures.

## Live check for the maintainer

```
theseus-sim bench turn --check --runs 5 --theseusd target/release-thin/theseusd | grep first_request_tokens   # all ok
theseus census --place dm --exact          # compare exact.total with the request line; the channels family is now (deferred)
theseus census --place channel; theseus census --place headless    # new place names
theseus-sim bench prompt                   # no warnings against the new baseline
```

On a real DM place the system blocks are larger than on a scratch daemon (context files, categories, standing orders), so the 15,000 is judged on the scratch daemon, as the other bench rows are. The DM's headroom there is 414 tokens.

## Gate

`scripts/gate.sh` fails in its first phase here, for reasons that are not this branch's: the keel guard reports `crates/theseus-sim/src/lifecycle.rs:155: budget-raised "seed"`, and refuses two acks already on main (238ecead06 and 1c2beb2747: "signature does not verify (%G? is E)", since this VM has not the owner's key). `python3 scripts/keel-guard.py` output (minus the "same test" move lines) is exactly that one finding; none comes from my commits.

So I ran the later phases by hand (never a modified copy of gate.sh): `cargo fmt --all -- --check`, `scripts/shape.sh`, `cargo clippy --workspace --all-targets -q -- -D warnings`, the cockpit's lint, test and build, `cargo nextest run --workspace --no-fail-fast`, `cargo deny --offline check`, and the generated-TS test. All green except:

- 33 failures in the known L1 group (theseus-sandbox contract tests and theseusd's sandbox tests: root without a job cgroup, theseus-pv6i).
- Seen once in the first run and passing alone (3 runs): `theseus-core tests_standing::the_records_rebuilt_after_a_restart_equal_the_ones_kept_in_memory` (two records created in the same millisecond come back in the other order; the order is not mine) and `theseus herdr a_change_reaches_herdr_within_ten_ms_at_the_99th_percentile` (a timing test). Neither is on the known list: name them.
- No benches (THESEUS_GATE_NO_BENCH) except the live turn bench above.

I ran with `CARGO_INCREMENTAL=0` and no debuginfo: this VM's disk allowance (about 40 GB) filled twice.

## Keel findings expected

- The commit that removes the byte line (2918f50b) carries no `Keel:` trailer. The brief asked for one, but this VM's tool policy refused the commit that included it, and I can't sign anyway. The guard did not flag counted.rs in my range (the 42 KB raise and my removal net out), but if the maintainer's run does: rule `budget-raised`, path `crates/theseus-sim/src/perf/counted.rs`, why: the owner's decision of 2026-10-14 16:36 ("I accept your recommendations"), the byte line replaced by token budgets, cli/task 8,000, dm 15,000. The owner's ack goes on the merge.
- No other test, budget or ceiling is loosened. The one assertion I rewrote (`tests_census` place list) is as strong.

## Left, uncertain, and for the owner

- The DM's headroom is thin (414 tokens on the scratch daemon). The next tool in the DM's tasks family will cost it a trade. `ALWAYS_LOADED` makes that a written decision.
- The budgets for channel and headless are set from the pre-trim values; the channel now reads 6,998.
- Docs to change at review (not touched here): the spec's Part III item for this row and `docs/status.md` (the byte budget is gone; token budgets per place), docs/technical-overview.md if it quotes `FIRST_REQUEST_KB`, and the census's place list (`cli, dm, channel, task, headless, harness`).
