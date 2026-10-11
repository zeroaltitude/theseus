# CLOUD_REPORT: cache-fix (theseus-ezeg)

Branch `cloud/20261010-cache-fix`. Base 2fd1f65 (store format 26, unchanged: no stored field was added).

## What the earlier session did, and what I found

The earlier session stopped without a report, after three commits. I read all three, ran their tests and the gate, and
found them complete and working; I changed no code of theirs.

- `8cfb969` config: a profile's `cache_ttl` and `keep_warm_hours` are `Option`s that inherit `[model]`'s; an explicit
  `"5m"` now holds and serializes; `[cache]` section (`keep_warm_minutes` 55, `keep_warm_min_tokens` 20,000,
  `stub_min_tokens` 20,000, `stub_after_turns` 3); template and its tests; `Config::ttl_of`. The rewritten
  `cache_ttl_is_five_minutes_or_an_hour_per_profile` went from 10 to 11 assertions.
- `4bbee2a` the keep-warm (`keep_warm.rs`, `tests_keep_warm.rs`): request replays the next compile up to the last
  message plus a one-line user turn at `max_tokens` 1; private conversations a person wrote in only; never during a
  turn, never when stopping; reserved against the spend limit and held on the day ceiling; one `keep_warm` row and the
  cost booked in one frame (`day_ceiling::SPEND_KINDS`); a refused or failed read stops that session until its next
  message; windows rebuilt after a restart from the store (no stored field); health's `cache` block
  (`PromptCacheHealth`, `ProfileTtl` in health.rs) and `render/cache.rs`; metric `theseus.keep_warm.reads`.
- `780668b` stubs (`compiler/stubs.rs`, `tests_stubs.rs`): at a cold rewrite a big old attachment is a one-line stub;
  what a cold rewrite stubbed stays in force through the warm window; `context.compiled`'s `cache.stubbed` names them;
  the store is untouched.

Observed and acceptable: protocol `lib.rs` holds at 2,733 lines (the added field is paid for by re-wrapping three
doc comments), compiler.rs gained one `mod` line (2,556), `ts.rs` lines gained names, not lines.

## What I did

- `079f333` theseusd: `tests/keep_warm.rs`, the live check as a repeatable test (below).
- Ran everything, and the planted reverts, and the lifecycle bench (below).

## Proof

- Targeted: 27 tests (keep_warm, stubs, cache config, templates, render) all pass.
- Planted reverts (each restored, `touch`ed, `git status` clean):
  - inheritance off (`cache::inherit` removed): `cache_ttl_is_five_minutes_or_an_hour_per_profile`,
    `keep_warm_hours_is_inherited_like_the_ttl`, `an_explicit_five_minutes_holds_under_a_one_hour_model`,
    `tests_keep_warm::health_lists_each_profiles_ttl_and_the_reads` fail (4 of 5).
  - a read during a turn (the Running/Queued check removed): `tests_keep_warm::none_during_a_turn` fails.
  - the stub inside a warm window (`cold()` always true): `compiler::stubs::tests::a_big_old_attachment_is_chosen_and_nothing_else`
    and `tests_stubs::an_attachment_is_stubbed_at_a_cold_rewrite_and_not_inside_a_warm_window` fail.
  - the ceiling unchecked (`day_hold` removed): `tests_keep_warm::none_past_the_daily_ceiling_and_said_once` fails.
- Under load: not run (no timing assertion was added; the keep-warm tests run on the paused clock).
- Break-even: a 1-hour cold write is 2x input, a read 0.025x, so one write costs as much as 80 reads. The 55-minute
  interval is 5 minutes under the hour (the provider's clock starts at its receipt of the last call). A 24-hour window
  is 26 reads, 0.65x of one 1-hour write per 1x of input: cheaper than one cold rewrite per day. Below 20,000 tokens
  a cold write is cents, so the floor is that.
- Stub arithmetic, from `a_cold_rewrite_of_626k_tokens_costs_about_2_50_with_its_big_file_stubbed`: 626,000 tokens
  written at 5 minutes on Fable 5.1 is $7.83; with the 426,000-token attachment a stub, about $2.50.
- FAST: nothing on a turn's path (a turn's end notes a time in memory; the tender runs on tokio's timer, off it).
  `theseus-sim bench lifecycle --runs 10 --check` (debug build, 4 cores): the first run printed `LIFECYCLE BUDGET MISSED`
  and I did not capture which row; the second run had every budget row `ok` (cold start p95 51.2 ms against 50 + 7
  margin, shutdown 22.4/100, SIGKILL restart 36.9/150, swap 36.0/200). The VM is not the owner's machine and the
  first miss is unexplained: the maintainer should rerun it there.

## Live check (a scratch daemon, a stand-in model)

The stand-in (`theseus-sim fake-model`, and the test's `FakeModel`) reports no cache reads or writes, so it cannot show
a cache hit: it shows the daemon's side only. `crates/theseusd/tests/keep_warm.rs` runs a real `theseusd` with
`[model] cache_ttl = "1h"` and `[cache] keep_warm_minutes = 0.03`, `keep_warm_min_tokens = 1`: a CLI-surface turn, two
`keep_warm` ledger rows, read requests of `max_tokens` 1 ending on the line, their system, tools and messages up to the
line equal to the next turn's, health's `cache` showing fable and opus inherited 1h and `reads_today >= 2`. It passes
(12 s). I did not run the big-attachment cold rewrite against a daemon: it is covered by `tests_stubs` in the core.

For the owner's machine with the real model (config lines first, then the commands):

    # in the config note, once this has merged
    [profiles.fable]
    cache_ttl = "1h"
    [profiles.opus]
    cache_ttl = "1h"
    # optional, on a scratch config only, to see reads in minutes: [cache] keep_warm_minutes = 2

    theseus health | grep -A12 -i cache      # each profile's TTL, "(from [model])" if inherited; kept, reads, $ today
    theseus ledger -n 200 -k keep_warm       # one row a read: tokens, cache_read, cost; cache_read should be ~the prefix
    theseus ledger -n 200 -k keep_warm --json | jq '.rows[].data.usage'   # the result is one object; its rows are the ledger's

Expect after a turn in a long CLI conversation, one row about every 55 minutes with `cache_read_input_tokens` near the
prefix and `cache_creation` small; the next message after an hour's gap should show a cache read, not a write.

## Not done, uncertain, for the owner

- A restart cannot tell whether the last message was a job's, so a job's message into a private conversation keeps it
  warm after a restart (the module's doc says so).
- `keep_warm_hours` defaults to 24 for every profile, but it applies only where the TTL is 1 hour.
- Docs to change at review: the spec (a Part III item for cache-fix), `docs/status.md`, and the template section
  `[cache]` as already written.
- No new dependency; Cargo.lock untouched.

## Keel findings expected

None. `python3 scripts/keel-guard.py --base 2fd1f65` reports ok with 0 findings. Run without `--base`, this clone's
guard judges against a stale local `main` (a9ad950) and lists five findings in files this branch never touched
(tui tests, index vectors, protocol tests_work_join, long-files): those are the base's drift, not this branch's. The
gate was run with `THESEUS_KEEL_BASE=2fd1f65` for that reason.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=2fd1f65 scripts/gate.sh`, on the tree with `079f333`:
keel, keel tests, fmt, shape, features, clippy, cockpit, test build, reader rule pass; deny passes when run by hand
(`cargo deny --offline check`: advisories, bans, licenses, sources ok); the protocol types phase is clean (the working
tree had no change in `cockpit/src/protocol.gen`). The suite: 3,700 tests, 3,667 passed, 33 failed, 44 skipped. All 33
are the known L1 failures (theseus-sandbox's contract tests and theseusd's sandbox tests, a root daemon with no job
cgroup, theseus-pv6i); no other test failed. The suite phase stops the gate there, so the later phases (the lifecycle,
jobs and turn benches) were not run by the gate; I ran the lifecycle bench by hand as above.
