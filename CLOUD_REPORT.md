# CLOUD_REPORT: p3a-kimi (theseus-rgbtz)

Branch `cloud/20261024-p3a-kimi`, from `ac298666`. One work commit: `6470b334`.

## 1. For the maintainer

- Store format: 51, untouched. No store change.
- `BUILTIN_VERSION`: `2026-10-25.1` (was `2026-10-18.1`). The pin in `catalog.rs` (`c.version`) is updated. That is the one expected pinned-value edit.
- No ceiling raised. `catalog.rs` is 2,136 lines (cap 2,500). The new tests are in `catalog/tests_moonshot.rs` and `config/tests_moonshot.rs`.
- No OWNER CALL, no STOP.
- Two edits to existing tests, both additive and both needed by the new row. Expect the keel guard to read them as changes to existing tests (it printed nothing with `--base ac298666`):
  - `catalog::tests::builtin_rows_are_sane_and_priced` asserted that every non-Anthropic row has a 1h cache write equal to its 5m write. Kimi is priced 5m at 2.00 and 1h at 4.00 (2x input), so I added a `moonshot` branch asserting that. I did not under-price Kimi's 1h write to fit the old rule, because an under-price would under-reserve against the money gate.
  - `broker::the_templates_harness_only_keys` and its line: `moonshot_api_key` joins the providers' keys list, as `deepseek_api_key` is. It comes from the new commented secrets line in the template.
- **The gate script fails here in `door`/`keel`, not on this change.** `origin/main` is at `2d1d8d6f`, ahead of the base `ac298666`. The gate judges against `origin/main`, so the door and keel phases flag other people's later commits (persona_cmd tests, keel-guard.py, refused acks). The base-pinned guard run below is clean. I ran the gate's substantive phases by hand (section 7).

## 2. The data rule

Describe-only: this row routes nothing to Kimi. It is in no route pack, `cn_draft`, `cn_profiles` or routing default, and `data_gate.rs` is unchanged.

- **How Kimi reaches `Cn`**
  - By host: `moonshot.ai`, `moonshot.cn`, `kimi.ai` and `kimi.com`, and any subdomain, are in `CN_HOSTS`.
  - By name: `origin_of` reads any model id containing `kimi` or `moonshot` as `Cn` on any host, whatever a provider vouches. The catalog rows are also `origin = Cn`.
- **Tests that private work cannot reach it**
  - `data_gate::tests::kimi_is_gated_exactly_as_glm_is`. It runs a GLM case and five Kimi cases: both Kimi models on the Moonshot host, a Claude id asked of the Moonshot host, and `kimi-k3.1-pro` and `acme/kimi-k4` behind a host that vouches `us`. Each is refused with no pass, refused with a sensitive name even with a pass, sent nothing, and sent open work only with a pass.
  - `config::tests_moonshot::the_moonshot_host_and_rows_cannot_be_loosened_to_us`.

## 3. Catalog entries

Both rows are built by `kimi()`:

| field | value | source |
|---|---|---|
| provider | `moonshot` | brief |
| context / max output | 1,048,576 / 131,072 | brief |
| input / output | 2.00 / 10.00; highspeed 4.00 / 20.00 | brief |
| cache read | 0.20 on both | brief |
| cache write 5m | = input (2.00; highspeed 4.00) | brief |
| cache write 1h | 2 x input (4.00; highspeed 8.00) | brief gives 4.00 for the pro model |
| thinking | `Implicit` | the mode for "always thinks, effort is the only knob"; no `thinking` field is sent |
| effort | on | `three_levels` already maps medium/xhigh to high, so the wire carries low/high/max |
| vision | on | brief |
| pdf, tool_search, refusal_fallbacks | off | brief |
| origin | `Cn` | brief |
| `bytes_per_token` | `TokenRates::CLAUDE` | unmeasured; the safe over-estimate, as for DeepSeek |

The catalog has both write fields, so the 5m and 1h prices both map exactly. **Assumption:** the brief gives no highspeed cache-write prices; I used the same 1x and 2x pattern, so 4.00 and 8.00. The live check should confirm that.

## 4. Config

The template has a commented `[providers.moonshot]` block beside DeepSeek's. It holds `api_base = "https://api.moonshot.ai/anthropic"`, `api_key_secret = "moonshot_api_key"`, `kind = "anthropic_messages"` and `origin = "cn"`, with DeepSeek's comment. It also has two commented profiles, `kimi` (effort high) and `kimimax` (effort max), both on `kimi-k3.1-pro`, and a commented `moonshot_api_key` secrets line.

The later local step has to:
1. Add the key item to the vault.
2. Uncomment the provider, the secret line and the profiles.
3. Add Kimi to the owner's config.
4. Update `crates/theseus-core/fixtures/owner_shape*.toml` and any resolved-profiles snapshot in the same step. I touched none of them.

## 5. Forced tools and refusals

- **Forced tools.** `Catalog::may_force` is false for any `Implicit` thinker, so every one-tool call to a Kimi model gets `tool_choice: auto` with a strict, closed tool. No path forces a named tool on Kimi. This is tested in `no_call_forces_a_named_tool_on_a_kimi_model`.
  - Open item for the live check: the tool is sent with `strict: true` and `additionalProperties: false`. Moonshot may not accept `strict`.
- **Refusals.** `stop_reason: refusal` flows through the generic parser to the existing refusal handling (`turn::ProviderRefused` and the fallback step). The Kimi row has no `refusal_fallback_model`, so the same-profile fallback does not apply to it. For a cn draft, the existing rule that Claude redoes the draft is unchanged. I did not trace it end to end beyond confirming the stop reason is parsed.
- **Parser.** No parser change.
  - Tested: input, output, cache read and creation, and the `ephemeral_1h_input_tokens` split price correctly.
  - Ignored, as they are for every endpoint: `service_tier`, `inference_geo`, `output_tokens_details`. Thinking tokens are billed as output only if `output_tokens` already includes them, which is the assumption in the pricing test.
  - Assumed: the vendor's `cache_creation.ephemeral_*` keys have the standard Anthropic names (`ephemeral_5m_input_tokens`, `ephemeral_1h_input_tokens`). The brief wrote them without the `_input_tokens` suffix. If Moonshot uses the short names, 1h writes would be priced as 5m. The live check must show the raw usage.
  - Tested: the thinking `signature_delta` is kept in the stream (`a_kimi_stream_keeps_its_usage_thinking_signature_and_refusal`). Replay across tool turns is not tested here.

## 6. Tests and plants

New tests: 4 in `catalog/tests_moonshot.rs`, 2 in `config/tests_moonshot.rs`, 1 in `data_gate`, 1 in `provider`. Also changed: the `moonshot` arm in the origin-listing test, and the `c.version` pin.

Plants. Each was restored from a backup, `touch`ed, and checked with `git status`:

| plant | result |
|---|---|
| P1: drop `moonshot.ai` from `CN_HOSTS` | 3 red: `moonshot_hosts_and_kimi_names_are_cn_whatever_is_vouched`, `the_moonshot_host_and_rows_cannot_be_loosened_to_us`, `kimi_is_gated_exactly_as_glm_is` |
| P2: `origin_of` no longer reads kimi/moonshot names | 1 red: the hosts-and-names test. The gate test stayed green until I added the uncatalogued `acme/kimi-k4` case; with that case the name plant (`|| false`) turns it red too |
| P3: remove the host-forces-cn validation rule | 1 red: the loosening test |

All were green after restore. The tests were not run at the base: the rows do not exist there, so they could not have passed.

## 7. Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` fails at `door`/`keel` for the reason in section 1, so it never reached the suite. By hand:

- `cargo fmt --all -- --check`: clean.
- `scripts/shape.sh`: ok.
- `cargo clippy -p theseus-core --all-targets -- -D warnings`: clean.
- `cargo nextest run -p theseus-core -p theseus -p theseusd`, full run, 5,576 tests: 5,540 passed, 36 failed, all in theseusd. Of those:
  - 15 are the known L1 sandbox and `mcp_l1` tests (root VM).
  - The rest failed only because my partial build had not built `theseus-index`, `theseus-sim` or `theseus-lsp-fake`. After building index and sim, I reran those binaries (tender, mcp, acceptance_journey, extend, stop_checkpoint, store_close, why and lsp): 24 passed, 1 failed, 1 skipped. The failure is `lsp::a_server_starts_at_the_first_call_and_none_outlives_the_daemons_kill`, which asserts `theseus-lsp-fake` exists and I did not build it.
  - All of theseus-core and theseus passed, apart from one failure of mine, fixed before the commit. The first run flagged `config::tests::example_template_uncommented_still_parses`, which runs `broker::the_templates_harness_only_keys` and found the new `moonshot_api_key`. The harness-only list is updated and the test passes.
  - Not run: the rest of `machine_checks` (protocol types, benches), the cockpit steps and `cargo deny`. No protocol types changed.
- The VM filled its disk twice during the builds. I cleared `target/debug/incremental` and large stale test binaries only.

## 8. Keel

`python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` printed nothing and exited 0, run on the committed range.

## 9. DM golden

`git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` printed nothing and exited 0.

## 10. Join notes: files touched

- `crates/theseus-core/config/theseus.example.toml`: `[providers]` and profiles section, plus one commented secrets line.
- `crates/theseus-core/src/catalog.rs`: `CN_HOSTS`, `origin_of`, `TokenRates::of`, `kimi()` and `kimi_rows()`, `BUILTIN_VERSION`, and the edited tests above.
- New `crates/theseus-core/src/catalog/tests_moonshot.rs`.
- `crates/theseus-core/src/config.rs`: one `mod tests_moonshot;` line.
- New `crates/theseus-core/src/config/tests_moonshot.rs`.
- `crates/theseus-core/src/data_gate.rs`: tests only, no rules.
- `crates/theseus-core/src/provider.rs`: one test.
- `crates/theseus-core/src/broker.rs`: test pin only.

Expect conflicts on `BUILTIN_VERSION`, the `CN_HOSTS` and `origin_of` lines, and the broker pin if another lane touches them.

## 11. Live checks for the later local step (needs the owner's Moonshot key)

1. **Raw usage.** One short call; compare the usage JSON with the parser. Confirm the `cache_creation` key names, that thinking tokens are inside `output_tokens`, and the highspeed cache-write prices.
2. **Tool names and schema limits.** Run the real tool set. Names must match `^[a-zA-Z_][a-zA-Z0-9-_]{0,127}$`, and the tool names with dots need checking. Check `strict: true` and `additionalProperties: false` on the one-tool calls, and a `tool_choice: auto` one-tool call.
3. **Thinking signatures.** Replay a thinking block with its signature across a tool turn.
4. **Images.** Base64 image blocks work. Confirm no URL image source or document block is ever sent (`pdf = false` already covers PDFs).
5. **Refusal.** Provoke a `refusal` stop and confirm a cn draft is redone by Claude.
6. **Cache hits.** Two identical-prefix calls should show cache reads. Check that effort changes break the prefix cache.
7. **Sampling params.** Check whether `temperature` or `top_p` give a 400.
8. Then, before any routing share, the cn-draft A/B (Kimi vs GLM-5.3 vs DeepSeek flash): cost, solve rate and review rework.

## 12. Follow-ups

- Per-family `bytes_per_token` for Kimi is unmeasured (`CLAUDE` rates are used).
- `kimi-k3` and the K2 models are deliberately skipped.
- The routing, Jev and owner_shape changes wait for the A/B.
- Docs to update at review: `docs/status.md` and the spec, which should list the Kimi rows and the new `BUILTIN_VERSION`.
- theseus-core's `AGENTS.md` mentions "GLM, DeepSeek" for implicit thinkers; Kimi should be added there.
