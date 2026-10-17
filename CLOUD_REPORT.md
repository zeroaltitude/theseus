# CLOUD_REPORT: cn-catalog (theseus-a1wvq.1)

Branch `cloud/20261017-cn-catalog`. One step, one commit: `1c3549b3`.

## What I found
- `fn glm` said `ThinkingMode::None`, `effort: false`, and flash/flashx `cache_write` 0.0. GLM always thinks, takes three effort levels, and bills a write as input.
- Sonnet 5.5's cache read was $0.20, now $0.10. This moves `the_break_even_of_a_switch` (Sonnet to GLM-5.3 break-even 11 to 52.5 turns) and the cost figures in `tests_m3` and the catalog tests.
- A catalog `us` row proves nothing for a host: DeepSeek's endpoint serves any `claude*` id with a DeepSeek model.

## What I changed (1c3549b3)
- `catalog.rs`: `ModelOrigin`, `CatalogEntry.origin`, `CatalogRow.origin`, `origin_of`, `is_cn_endpoint`, `ThinkingMode::Implicit`, `fn deepseek` and `deepseek_rows`, GLM rows fixed, Sonnet 5.5 at 0.10, `TokenRates::of("deepseek-")` (Claude's rates, unmeasured), `BUILTIN_VERSION = "2026-10-17.1"`.
- `config.rs`: `ProviderConfig.origin`, `Config::profile_origin`, and two validate refusals (a loosened built-in cn row; a provider origin other than cn on a China-origin host).
- `compiler/effort.rs`: an Implicit row that takes effort sends low, high or max (medium and xhigh as high). The rule reads the catalog's thinking mode, not a provider name. `compiler.rs` needed no change: every `ThinkingMode::` use sends only for Always and Adaptive.
- The template has a commented `[providers.deepseek]`, profiles `dsflash`, `dsflashmax`, `dspro`, and a commented secret. They are commented because `validate` requires a provider's secret, and the secret line stays commented (no key exists). The un-comment test covers them. `broker.rs`'s harness-only assertion and `fixtures/owner_shape.toml` moved with it.
- Tests changed because behaviour moved (nothing loosened): `glm_5_3_and_5_2_are_text_only...` (deepseek-v4-pro is also text only), `late_results_render_as_text...` (no effort asked, none sent), `tests_route_effort::a_model_without_effort_ignores_it` (it now turns GLM-5.3's `effort` off by a config row), `the_break_even_of_a_switch` (range 52 to 53), the micro-dollar and Sonnet cost tests.
- No store-format change, no protocol change, no new dependency.

## How I proved it
- `cargo nextest run -p theseus-core`: 3024 of 3024 passed after fixes. The first run had 6 failures, all from the changes above.
- Planted revert 1: dropped the `is_cn_endpoint` rule in `origin_of`. `a_china_endpoint_or_family_is_cn_whatever_the_model_id_says` failed at the deepseek.com case (`Us` instead of `Cn`). Restored and touched.
- Planted revert 2: made medium, high and xhigh pass through unmapped in `three_levels`. `glm_rows_think_implicitly_and_take_three_effort_levels` failed (`"medium"`). Restored and touched.
- New tests: `every_builtin_row_names_its_origin_and_glm_and_deepseek_are_cn`, `a_config_model_without_origin_is_unknown_and_restricted`, `a_china_endpoint_or_family_is_cn_whatever_the_model_id_says`, `deepseek_rows_are_priced_as_research_says`, `glm_cache_writes_are_priced_at_input`, `sonnet_5_5_cache_reads_are_ten_cents`, `a_config_table_cannot_loosen_a_builtin_cn_origin`, `a_provider_cannot_vouch_for_a_china_endpoint`, `a_profiles_origin_reads_its_provider_its_model_and_the_catalog`, `glm_rows_think_implicitly_and_take_three_effort_levels`, `implicit_thinking_sends_no_thinking_field`, `sse_keep_alive_comments_are_skipped`.
- Not run under load (the task asked for none).
- `cargo fmt`, `scripts/shape.sh` and `clippy --workspace --all-targets -D warnings` are clean. Clippy's 100-line limit made me split `deepseek_rows` out of `builtin`.
- Workspace suite (6248 tests, `TZ=America/Phoenix`), failures:
  - The known L1 set: theseus-sandbox contract and `spawn_100`, theseusd sandbox tests.
  - `theseus-core tests_horizon::a_met_watch_ends_done...` failed once in the parallel run and once alone beside a busy machine, then passed 3 of 3 alone. It is timing-flaky, and it passes at base alone.
  - `theseus-discord tests_task_threads::a_task_in_a_guild_channel_gets_a_thread...` times out at 120 s. It times out identically with my changes stashed, so it fails at base. Its three siblings timed out only under full-suite load and passed alone.
  - `theseusd::wakes a_repeating_wake_at_the_floor...` was SIGTERMed by the run's end in the full run, and passed alone (63 s).
- Not run: `scripts/gate.sh` in full. The gate stops at its door and keel phases on three erosions that come from the base history, not from this commit: `lifecycle.rs` "seed", `keel-guard.py` and `test_keel_guard.py`. Their signed acks fail verification here (`%G? is E`). My first gate run also flagged a removed assertion in `haiku_5_5_is_priced...`. I restored it, and the second run no longer lists it. `cargo deny` was not run. The `cargo deny fetch` result was lost with the setup log, which I deleted when the disk filled.
- The cockpit was not built.

## Live check for the maintainer
- Probe GLM (the z.ai key exists): `theseus ask -P glm "say hi"` on a scratch daemon of this build. The request now carries `output_config.effort` (low, high or max) and no `thinking`. Check the served model, the usage object, and that the output-token counts drop with effort (research §7.5.2).
- `theseus catalog` should show `deepseek-flash` and `deepseek-v4-pro` at 0.3/1.2 and 1.32/3.96, and thinking `implicit` for the GLM and DeepSeek rows.
- The DeepSeek probe waits for the key: usage fields, SSE names, and thinking-block round-trip with tools (research §1.2 and §1.3).

## Left or uncertain
- DeepSeek's `bytes_per_token` is unmeasured. It uses Claude's rates, which over-estimate. A probe is owed.
- Doc changes for the maintainer to make: spec Part III item and version line, `docs/status.md`, and `crates/theseus-core/AGENTS.md` (the catalog's origin and `Implicit`).
- Design choices to hear about: `output_config` mapping keys on `thinking == Implicit` plus `effort`, not a new catalog field. A CatalogRow cannot loosen a built-in cn row but may tighten one.
