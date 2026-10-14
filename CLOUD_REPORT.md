# CLOUD_REPORT: ext-graduate (theseus-pw1q.13)

Branch `cloud/20261013-ext-graduate`, step commit 86b5694.

## What I found
- A call of a loaded extension already writes `action.succeeded` / `action.failed` with producer `async:mcp:ext-<name>/<tool>` and `duration_ms`. An error result, a timeout and a server crash all settle `failed`, so use is countable with no new row or counter. A restart's `action.outcome_unknown` carries the reconciler's producer, so the action's tool names it.
- self-backlog has joined, so graduation is one more cause in its night, ranked and started like any item.
- A failed extension call also files the ordinary `action.failed:async` cause; the test for "one failure" asserts on graduate items only.

## What I changed (86b5694)
- `extend/usage.rs`: `Core::extension_usage(name, now, days)` and `extension_calls` (ledger pages by kind, 300k-row cap, a truncated read is a floor). `extend.list {usage:true}` (new `ExtendListParams`, `ExtendUsage`, `ExtendLoadedInfo.usage`; cached 60 s), `theseus extend list`, the cockpit card ("used 34 times in 14 days, 0 failures, p50 41 ms").
- `rsi/backlog/graduate.rs`: the rule (`[self] graduate_min_calls` 20, `graduate_window_days` 14; calls counted since the current digest was acked; zero failures; whole read; once per digest via the cause key `graduate.ext:<name>:<digest12>`); the brief (frozen path, schemas, declared tests, what done is, no widening of the grant); `self.graduate.proposed` row in the filing frame (`LedgerKind::SelfGraduateProposed`, `SELF_ROWS`).
- Keel: network extensions use key `graduate.ext.net:` (class security, `waiting_human`, never starts).
- `extension_revoke_graduated(name, digest, merge)`: `extend.revoked` with `reason: "graduated"`, by `self`; asks `rsi::gate`, only for a filed port at that digest. The owner's revoke shares `revoke_loaded` and writes no `reason`. Nothing calls it yet: self-join (pw1q.9) is its reader.
- Config keys with defaults and template lines. No store format bump. TS regenerated.

## Proof
- New tests: `rpc/tests_self_graduate.rs` (10), `extend/tests_load/usage.rs` (real calls through a loaded stand-in extension: 3 calls, 1 `isError` failure read back), keel and unit tests, a cockpit test. 91 selected core tests pass.
- Planted reverts (each caught): failed counted as success (3 tests); `>=` to `>` (2); one failure allowed (2); no per-digest dedupe (1); window ignoring the load time (2); truncated read still fires (1); network not keel (1); revoke without a filed port (1); revoke ignoring the gate (1). Not caught: dropping the row-time filter in `extension_calls` (the index already bounds by time), so it is redundant defence. "A counter on the call path" is covered by a test that the calls wrote no usage/graduate kinds, and the diff touches no call-path code.
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1`): keel, fmt, shape, features, clippy, cockpit lint/test/build, test build, reader rule pass. Suite: the known L1 failures (theseus-sandbox contract, theseusd sandbox) plus `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`, which fails alone in 21 s. I did not touch theseus-discord and did not check it on the base; treat it as unexplained. The phases after the suite I ran by hand: `cargo deny --offline` ok; turn bench 4 frames per turn; lifecycle bench all budgets ok except "health after the seed" p50 4.5 / p95 7.1 ms vs 5 ms (this VM; no start-path change). The protocol-types check passed through theseus-protocol's tests.
- Keel guard: 0 findings from my change (it printed the base's pre-existing rename notes).

## Live check for the maintainer
Scratch daemon, `[self] mode = "act"`, `theseus self resume` on the scratch only; load a stand-in extension, run 20 calls; `theseus extend list` should show "used 20 times in 14 days, 0 failures"; run the night (`backlog_after_serving`, or wait) and `theseus self backlog` should list `graduate ext-<name>` and one `"self"` session. I did not run this on a daemon here; the core tests do the same through `backlog_night`. I did not measure stdio vs in-process latency, so no figure to quote.

## Left / uncertain
- "Revoke the owner can undo with one word": no reload exists; the frozen copy stays on disk. Needs a reload verb (self-join's row).
- "Touches secrets or policy" is not detected beyond the keel words in the key; extensions get no secret by construction. Network is the keel signal.
- A closed or failed item is never re-filed for the same digest.
- `extend.list {usage}` reads the ledger (cached 60 s); the cockpit polls with it every 5 s.
- Docs to change (not edited): the spec's Part III item, status.md; core AGENTS.md updated.
