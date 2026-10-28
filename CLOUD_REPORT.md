# CLOUD_REPORT: theseus-xkzsn (DeepSeek reservation)

Base worked from: `ee725a2d` (branch head `67bc8538`, the task commit). No rebase, no merge.

## Step 0: citations
All confirmed at `ee725a2d`, none wrong: `theseus_bench.py:45-53` comment, `:54` budget, `:55` RESERVE_OUTPUT_TOKENS,
`:56`, `:57`, `:125-134` reservation_usd (`raw` at `:133`), `:137`/`:143`; `efficiency.py:84-96` LIST_PRICES (values as
stated); `catalog.rs:750-756` deepseek (`max_output_tokens: 384_000` at `:755`); `catalog.rs:789` kimi, whose
`max_output_tokens` is **131_072** (`:793`), as the row says. GLM 5.3 is 128_000 (`:979`). The Claude rows are 128_000.
GPT rows in LIST_PRICES are not in the catalog.

## Option 2 is pinned shut: agreed
`test_bench.py:193` asserts `assertNotIn("max_output_tokens", text)` on the written profile, and
`bench/theseus-bench.toml:16-17` says the cap is the model's own, "as Claude Code's is: no `max_output_tokens` line
(theseus-7gir.19)". Option 2 would reverse that. I implemented option 1.

## Arithmetic (recomputed; input side 128,000 tokens, rounded up to $0.25)
| model | output cap | reservation before | limit before | reservation after | limit after |
|---|---|---|---|---|---|
| deepseek-v4-pro | 384,000 (flat 128,000 before) | 0.676 -> 0.75 | 2.75 | 1.690 -> 1.75 | 3.75 |
| kimi-k3.1-pro | 131,072 | 1.536 -> 1.75 | 3.75 | 1.567 -> 1.75 | 3.75 (no change after rounding) |
| glm-5.3 | 128,000 | 0.742 -> 0.75 | 2.75 | same | 2.75 (no mismatch) |

DeepSeek's output reservation alone is 384,000 x 3.96 / 1e6 = $1.52. With the flat constant a trial that had spent
$1.06 (2.75 - 1.69) was refused by the reservation alone. The task's "about $1.00 / $1.78" is the same shape: I
count input at 128,000 tokens ($0.17), giving $1.69. Kimi's mismatch is real in raw dollars (+$0.03) but absorbed by the
quarter-dollar rounding, so Kimi's limit does not move; its test row passes both before and after.

## Change (bench/harbor only)
`efficiency.MAX_OUTPUT_TOKENS` (beside `LIST_PRICES`, 128,000 default via `DEFAULT_MAX_OUTPUT_TOKENS`), and
`reservation_usd` reads it, falling back to `RESERVE_OUTPUT_TOKENS`. `RESERVE_INPUT_TOKENS` and `RESERVE_STEP_USD`
are untouched. Public shapes of `reservation_usd`/`spend_limit_usd` are unchanged. Callers checked:
`theseus_agent.py:201-204`, `bench/self/selfdriver.py:78-82,127`, and the tests in `test_bench.py` (`:349-361`,
`:508-525`, `:1075-1080`; the last mirrored the flat formula and now reads the cap).

## Red first
Before the change (test written first, run on the unchanged code):
`AssertionError: 2.75 not greater than or equal to 3.6795999999999998 : deepseek-v4-pro: limit 2.75 < 1.99 + reservation 1.6896`
After: `Ran 218 tests ... OK (skipped=45)` for bench/harbor; the new test passes (DeepSeek limit 3.75).
Planted revert (`theseus_bench.py` restored to base, new tests kept): the same failure again, `limit 2.75 < 1.99 + reservation 1.6896`, then restored.

## Rust/Python divergence
`test_the_output_caps_match_the_rust_catalog` reads `catalog.rs` by regex (the `deepseek`/`kimi` helper literals, the
`glm`/`claude` constructor rows) and compares them with `MAX_OUTPUT_TOKENS`; it skips if the file is absent.
`test_every_priced_model_has_an_output_cap_row_and_no_stray_ones` catches a model added to LIST_PRICES without a cap.
It is a text match, so a reshaped catalog fails it loudly (a failure, not silence); a better check would be a
Rust-side test that emits the table. The bench tests are not in the gate (theseus-nk6pb), so this runs only when
someone runs them.

## Commands run
- `python3 -m unittest discover -s bench/harbor`: Ran 218, OK (skipped=45)
- `python3 -m unittest discover -s bench/report`: Ran 90, OK
- `python3 -m unittest discover -s bench/self`: Ran 59, OK (skipped=2)
- `bench/async` was started and had not finished when I wrote this; it does not call `reservation_usd`.
No Rust touched, so no Rust gate or fmt run. No spend, no bench, no AWS.

## keel-guard.py
`python3 scripts/keel-guard.py` (against a fetched origin/main, newer than my base) printed only noise from other
people's merged work (crates/theseus-core tests, theseus-discord courier, cockpit bench test, etc.), none of it a file
I touched; I changed only three files under bench/harbor and this report.

## Integrator notes
For main: the one code commit (`bench/harbor/efficiency.py`, `theseus_bench.py`, `test_bench.py`).
Not for main: `CLOUD_TASK.md` and `CLOUD_REPORT.md`. Published datasets under docs/benchmarks are not affected:
no published number changes (only DeepSeek's default limit rises 2.75 -> 3.75). bench/README.md's limit table
still describes the flat 128k; the sentence is true for the Claude arms and I left it.

CLOUD REPORT COMPLETE
