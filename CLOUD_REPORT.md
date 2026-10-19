# CLOUD_REPORT: gw-core (theseus-batvg.1)

Branch `cloud/20261019-gw-core`, base `bbc65530`. Commits: `a0e7ea4f` (the work), `fcee8c6e` (rustfmt of one test file), and the commit that adds this report with the help golden and `docs/cli.md` line.

## What I found
- The brief says `scrub/shapes.rs` already knows `ya29.` and `1//`. It did not (only `AIza`). I added both shapes with corpus rows (`scrub/shapes.rs`, `scrub/corpus.rs`).
- `theseus_tools::ToolOutput` has no marker field and `ProviderRequest` is built by many literals (one in `compiler.rs`, at its ceiling). So the private mark rides in `ToolOutput.meta["private"]`, is persisted as a META key `google.private.<session>` in the result's own frame (`toolrun/complete.rs`), and the provider floor reads a task-local set around the provider call (`google/private.rs`, `data_gate.rs`, `turn.rs`, `turn/compaction.rs`). No stored record gained a field, so **I did not bump the store format** (45 stays); new ledger kinds and META keys need none (precedent: initiative, retract).
- The decision reason for an authority plan prints "approve (layer 1: ...)" because `Policy` words every `Plan.authority` that way. Cosmetic; a later row may reword it.

## What I built (all in `a0e7ea4f`)
1. `crates/theseus-google` (new): OAuth client, single-flight refresh (5 min margin), PKCE S256, loopback listener, paste parser, REST client, retry/backoff (`Retry::NonRepeatable` -> `outcome_unknown`), error map (`invalid_grant`, `SERVICE_DISABLED`/`accessNotConfigured`, Advanced Protection, `admin_policy_enforced`), scope map with per-tool scopes, client-secret parser (two labelled lines either order, or the console JSON), method table (about 85 methods), classifier, and the fake (`fake.rs`, feature `fake`). `[google]` config, `validate_google`, template section and marker comments.
2. Consent and minting (`google/auth.rs`, `secrets/write.rs`): account checked on the `id_token` before anything is stored; `op://` minted with `op item create/edit` JSON on stdin and read back by sha256; `file:` atomic 0600; `env:` refused; a refused write holds the token in memory and says so.
3. Daemon and CLI: `google.check` phase (`Core::google_after_serving`, called from `warm_after_serving`), one notice and one `google.auth_lapsed` row per lapse, `google.revoke`/`status`, CLI-only RPCs (`rpc/google.rs`, routed by prefix), `theseus google auth [--manual]|status|revoke` (`google_cmd.rs`, paste read from the terminal), health field `google`.
4. Tool pieces: `google::finish` (private + External with the owner-authored rule), `google.called` and auth facts, daily send counter (persisted in META, counts `google.call` sends), families `mail`/`calendar`/`docs`, `google::tools` with marker comments, `google.call`, crown-jewel names (`HarnessOnly.google`; a `[broker]` grant naming a Google secret is refused at load).
5. `docs/privacy.md` rewritten (what is read, where the token lives, revoke, the private rule). It states honestly that the turn and its compaction carry the mark but keep-warm and the other background calls do not yet.

Also touched, small: `docs/cli.md` and `tests/golden/help_all.txt` (the new `google` command), `config/tests.rs` (tool count 59 -> 60, `google::NAMES` in the tool lists), `broker.rs` template test line, protocol `ts.rs` list (appended to the existing last line), `cockpit/src/protocol.gen/`.

## Proof
- `cargo nextest run -p theseus-google`: 25 passed. `-p theseus-core -E 'test(/google/) | test(/config::tests/) | test(/broker/)'`: 109 passed. Core tests added: `google/tests*.rs`, `config/tests_google.rs`, `google/external.rs`.
- Planted reverts (each failed the named test; file restored, `touch`ed, `git status` checked):
  - skip account check -> `a_refresh_token_for_another_account_is_refused_and_not_stored`
  - token as op argument -> `the_refresh_token_never_reaches_op_argv`
  - skip read-back -> `a_minted_token_is_read_back_before_success`
  - env accepted -> `env_ref_refresh_token_is_refused`
  - store error returned -> `a_refused_op_write_holds_the_token_in_memory_and_says_so`
  - state check removed -> `pkce_state_mismatch_is_refused` (crate and core)
  - require `client_id` first -> `the_client_secret_parses_from_two_labelled_lines_in_either_order`
  - notice on every lapse -> `invalid_grant_sets_needs_auth_and_one_notice`
  - no single-flight lock -> crate `refresh_is_single_flight` (the core copy still passes: the crate test is the guard)
  - drafts.send as Write -> `a_send_through_google_call_asks_at_every_posture_and_no_grant_loosens_it`, `google_call_classifies_delete_permissions_and_acl_as_authority` (crate and plan)
  - private mark dropped -> `a_google_result_makes_the_session_private_and_the_floor_refuses_a_restricted_model`, `a_google_read_runs_marks_its_session_private_and_holds_it_as_outside_text`
  - cap not counted -> `the_daily_send_cap_counts_google_call_sends`
  - disabled API mapping off -> `a_disabled_api_names_the_api_to_enable` (3 copies)
  - `missing()` empty -> `partial_grant_turns_off_only_the_tools_missing_a_scope` (2)
  - deny_unknown_fields off -> `config_rejects_unknown_google_keys`
  - `from_config` ignoring empty -> `no_google_section_registers_nothing_and_reads_nothing`
  - broker check off -> `google_secrets_never_reach_l1`
  Not planted: `owner_authored_unshared_content_is_not_external`, `a_token_shaped_value_never_reaches_a_fact_or_a_log`, `a_pasted_redirect_url_finishes_auth`, `a_disabled_api...` beyond the above.
- Keel guard against my base: `python3 scripts/keel-guard.py --base bbc65530 --head HEAD --quiet` printed nothing, exit 0.

## The gate
`scripts/gate.sh` itself **failed in its keel phase**, not on my code: its door compared against the newer `origin/main` (`d8e09f6a`) and reported four erosions in files I did not touch (`rsi/join/install.rs`, `rsi/pr.rs`, `scripts/keel-guard.py`, `scripts/test_keel_guard.py`). I was told not to merge a newer main, so I ran the later phases by hand (TZ=America/Phoenix): fmt ok; shape ok; workspace clippy `-D warnings` ok; cockpit lint/test/build ok; `cargo deny --offline check bans licenses sources` ok; protocol types regenerated and committed; whole workspace suite: 6908 run, 6870 passed. Failures:
- 31 known L1 (theseus-sandbox contract and `spawn_100`, theseusd `sandbox`): root VM, theseus-pv6i.
- `theseus::help_docs` and `help_all_is_the_golden`: mine (the new command); fixed in the report commit, both pass.
- `tests_horizon` (2) and a theseus-discord `react` test timed out under load; each passes alone.
The speed benches were not run (`THESEUS_GATE_NO_BENCH`, per the brief).

## Live check for the maintainer
1. Config: `[google] account = "<owner address>"`, `[secrets]` `google_client` (note with `client_id:` and `client_secret:` lines) and `google_refresh_token` (an `op://` field in the writable vault, or `file:`); restart the scratch daemon.
2. `theseus --socket <s> google status`: `needs_auth`, "no refresh token yet".
3. `theseus --socket <s> google auth` (from a shell, not a job): open the URL on the desktop; expect the done page, or paste the address. Then `google status`: `ok`, granted scopes, tools on. A different Google account must print "different account ... nothing was stored".
4. **Check `op item create/edit` with stdin JSON against the real `op`** (I could only prove it against a fake): if the real CLI wants `--template /dev/stdin` or a different JSON shape, `secrets/write.rs::put` is the one place to change.
5. `theseus --socket <s> health` shows `google: <account> · ok · N tools on`; ask the model to list unread mail through `google_call` (`gmail.users.messages.list`, params `{"q":"is:unread"}`); then try a `drafts.send`: it must wait for approval at any posture.
6. `theseus google revoke`: revoked at Google, token cleared, tools off.

## Left, uncertain, and for the owner
- Unknown method ids are refused; the long tail uses `"VERB /path"`. Classification is by verb and path, so the path form cannot dodge authority.
- Keep-warm, the memory pass and other background calls do not carry the private mark; a task a private session starts does not inherit it. Both need a follow-up (data-routing lane).
- The lapse notice uses the existing `spend_ceiling` outbox kind (text only), since `theseus-discord` is off limits; gw-surfaces should give it its own kind.
- An unminted refresh token shows as a failed `[secrets]` entry in health until the first `theseus google auth`.
- `google.call` approval cards show the verb, path and a body `id`, not the body; gw-mail owns the send card.
- `docs/status.md` and the spec need a Part III item for this row; I did not touch them.
