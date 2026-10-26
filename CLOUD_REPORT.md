# CLOUD REPORT: theseus-01n2l and theseus-fhwo7 (branch cloud/20261026-p3p-bench-providers)

Base 78fcfb6d. Python and TOML under bench/harbor only. No Rust, store format (51) or kernel schema touched. No model call, no key, no AWS.

## Half 1: theseus-01n2l (commit f6089036)

**Found.** `reservation_usd` raised `ValueError: no list price for glm-5.3: add it to efficiency.LIST_PRICES`, and `list_price` returned `None` for any dataset with one unpriced model. Cited sites matched: LIST_PRICES efficiency.py:76, reservation_usd theseus_bench.py:90 (raise at :97), list_price efficiency.py:861, `_priced` :129.

**Rows encoded** (read from `crates/theseus-core/src/catalog.rs` at base; $/Mtok input / cache_read / cache_write / output):

| model | values | catalog.rs source |
|---|---|---|
| glm-5.3 | 1.40 / 0.26 / 1.40 / 4.40 | :976 `glm(1_000_000,128_000,1.40,4.40,0.26,1.40)`; `fn glm` (window,max_out,input,output,cache_read,cache_write) |
| kimi-k3.1-pro | 2.00 / 0.20 / 2.00 / 10.00 | :819 `kimi(2.00,10.00,..)`; `fn kimi` cache_read literal 0.20, cache_write = input |
| deepseek-v4-pro | 1.32 / 0.044 / 1.32 / 3.96 | :847 `deepseek(1.32,3.96,0.044,..)`; cache_write = input |
| claude-sonnet-5 | 2.0 / 0.20 / 2.50 / 10.0 | :938 `claude(m,128_000,2.0,10.0,0.20,2.50,..)`; signature order (input, output, cache_read, cache_write) confirmed |
| claude-opus-5 | 5.0 / 0.50 / 6.25 / 25.0 | :910 |

`claude-opus-5` (5/25) is distinct from the existing `claude-opus-5-5` (4/20); a test asserts they differ.

**DeepSeek: PEAK rates encoded.** The catalog's own source string says "peak rates (off-peak is half)". Peak matches the catalog (one authority) and is the conservative direction for a reservation. Consequence for the owner: an off-peak trial is costed at up to 2x its bill. Stated in the comment above LIST_PRICES. OWNER CALL if a time-of-day rate is wanted.

Kimi's 1-hour cache write (2x input) is not representable in the table; only the 5-minute write is encoded (noted in the comment).

**Tests** (class `ListPrices` in test_bench.py): reservation per new model (bare and prefixed), mixed zai/glm-5.3 + claude-sonnet-5 dataset priced equals hand sum, every row has the four classes positive. Red on base: `ValueError: no list price for glm-5.3 ...` and `KeyError: 'claude-sonnet-5'`. The structural test passes on base (the dict had no malformed row), as expected.

## Half 2: theseus-fhwo7 (commit b7263e99)

**Found.** Refusal at theseus_agent.py:189-191 as cited; MODEL_CONNECTION :126-128; env key :222. Daemon schema (theseus.example.toml:307) is `[providers.<name>] api_base, api_key_secret, kind = "anthropic_messages"`, which expresses everything needed: no OWNER CALL on schema.

**Changed.** theseus_bench.py: `PROVIDERS` (zai, moonshot, deepseek, the catalog's spellings), `split_model` (unknown provider raises, naming anthropic, zai, moonshot, deepseek), `key_env`, and `settings(provider=...)` which sets `[model]`/`[profiles.bench]` provider, `[model] api_key_secret`, a `[providers.<name>]` block, and `[secrets] <name>_api_key = "env:<VAR>"`. theseus_agent.py: the guard replaced by `split_model`; the exec env carries the provider's variable (value from the runner's own environment, empty if unset); ANTHROPIC_BASE_URL is not applied to other providers; docstring updated. Routed arm with another provider raises.

**Env var names a human must set (names only):** `ZAI_API_KEY` (provider zai, models glm-*), `MOONSHOT_API_KEY` (moonshot, kimi-*), `DEEPSEEK_API_KEY` (deepseek). Use `-m zai/glm-5.3`, `-m moonshot/kimi-k3.1-pro`, `-m deepseek/deepseek-v4-pro`.

**Anthropic byte-identity.** Profile for `claude-sonnet-5-5` (spend 3.75, ceiling 7.5) hashed on base and after the change: both sha256 189d8e65a1675e7bcc5c205fa497b61bca4ab8952b601bc2cddc5c67ca40376b (3862 bytes). The regression test pins that hash for `anthropic/claude-sonnet-5-5`, bare, and None. Red-first exception: on base it errors with AttributeError (no `split_model`) because it uses the new seam; its equality was proven separately by hashing on base as above.

**Tests** (class `Providers`, six tests, fake provider `fakeprov` with placeholder var `FAKEPROV_PLACEHOLDER_VAR` patched into PROVIDERS): fake provider lands in profile; unknown raises naming supported; catalog spellings and var-name shape; Anthropic unchanged; routed refuses; no key material (sentinel values set in the environment never appear in a rendered profile; secrets are only `env:NAME`). On base all six error with AttributeError (not the ValueError at :191, since the seam did not exist).

**Uncertain / for the live check.** (1) The profile still holds `[secrets] anthropic_api_key = "env:ANTHROPIC_API_KEY"`; with that variable unset in a CN-arm container, the daemon may report it in health/`check` (secrets.rs `local_value` returns an error for unset env). I did not verify whether it blocks startup. The live check should start a scratch daemon with this profile and only the CN variable set. (2) Harbor 0.23.0 `Theseus(model_name="zai/glm-5.3")` constructs fine without ANTHROPIC_API_KEY set. (3) No `ModelConnectionSpec` change was made; `access.api_key` is used for Anthropic only.

## Suites
- Python 3.11 stdlib: `python3 -m unittest discover -s bench/harbor` base 205 tests OK (skipped 45); after half 1: 208 OK; after half 2: 214 OK (skipped 45).
- Harbor 0.23.0 venv (python3.12): 214 tests, 2 errors: `Routed.test_the_key_reaches_only_the_routed_arm` and `..._clears_an_opus_or_fable_reserve` (`'Env' object has no attribute 'environment_dir'`). Identical 2 errors on the base (205 tests), so pre-existing, not mine.

## Keel
The gate's keel phase: `keel: ok (78fcfb6d..working tree, 4 files changed, 0 findings acked)`. A bare `python3 scripts/keel-guard.py` against a fetched origin/main printed findings in unrelated crates (signature checks fail here); none concern bench/.

## Docs to change (maintainer)
docs/benchmarks.md / bench/README.md: mention `-m <provider>/<model>` and the three variable names. Not edited.

## Gate
`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run on both commits together (HEAD b7263e99 tree; the report commit adds only CLOUD_REPORT.md).
- Two earlier attempts died in `test build` with `No space left on device` (the session's disk allowance, ~39 GB, is smaller than `df` suggests). I removed `target/debug` and reran; the third run passed door, keel (0 findings), keel tests, fmt, shape, recall tests, features, clippy, clippy-probe, cockpit, test build, reader rule.
- Suite: 8625 run, 8591 passed, 34 failed, 56 skipped. All 34 are the known root/L1 failures: theseus-sandbox `contract` clauses and theseusd `sandbox` tests (theseus-pv6i). No other failure, no timing flake.
- Because the suite failed, I ran the later phases by hand: protocol types (cockpit/src/protocol.gen unchanged: ok); turn bench `--check --runs 5 --burst 0` ok (first_request budgets ok); `cargo deny fetch` then `cargo deny --offline check`: advisories ok, bans ok, licenses ok, sources ok. Lifecycle, jobs and size phases were not run (NO_BENCH; no release build). Counted green per the task.
