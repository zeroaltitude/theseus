# CLOUD_REPORT: trim-names (theseus-rwpbg.10)

Branch `cloud/20261017-trim-names`, from `0a44e1f1`. Parts done in the task's order: **E, A, C, D, B (partly)**. B's
`confirm`, `executions` folds are **not done** (below, with why and the exact plan).

## What I found

- `[self]` had three Rust names (`config/rsi.rs`, `SelfConfig`, `Config::self_improve`).
- Every non-everyday command was on the one `theseus help` page (71 `Cmd` variants listed, plus a 50-line quick start).
- "mode" named seven things; the CLI said `theseus work`, `wakes`, `pause`, `resume`, `snooze`, `done`, `voice`, `notices
  rate`, `index`, `route report`, `turn replay`, `prefix report`, `catalog`, `context`, `watches` for what are verbs on
  a noun.
- The prompt cache's keys were in three sections (`[model]`, `[cache]`, `[threads]`).
- `crates/theseus/tests/golden.rs` was at 2,498 lines of its 2,500: the folds made rustfmt wrap some calls, so I
  compressed two catalog tests (it is 2,497 now; no ceiling raised).
- The base's keel range: this clone's `origin/main` (`9fa84735`) is 597 commits behind `0a44e1f1`, so `scripts/gate.sh`
  judges the keel from the wrong base and finds three erosions that are not mine (they are acked in signed commits the
  VM cannot verify). I ran the gate with `THESEUS_KEEL_BASE` set to my own base (see "The gate").

## What I changed (commits)

| Commit | Part | What |
|---|---|---|
| `b9d74ca6` | E | `config/rsi.rs` -> `config/self_improve.rs`, `SelfConfig` -> `SelfImproveConfig`; the field was already `self_improve`; TOML key stays `[self]`. One discord test import line follows the rename (contract 7: one line, no behaviour). |
| `0e108e2e` | A | `theseus help` = the 19 everyday commands; `help all`; `help COMMAND`. |
| `1811e840` | C + D | vocabulary and config sections, the renamed-key table (the two parts share `config/renamed.rs`, so one commit). |
| `a024902f` | B | the folds below. |

### A. Tiers

`theseus help` shows, in this order: ask chat tui history watch status tasks answer stop agenda digest remind routine
wake project thread places profile health, then the global options and a quick start that ends
"`theseus help all` lists every command." Every other command is `#[command(hide = true)]`: it parses, runs and has its
`--help`. `help_cmd.rs` renders `help all` from `Cli::command()` with hide ignored (everyday first, then the rest sorted),
and `theseus help COMMAND` prints that page. clap's own `help` subcommand is disabled (a hidden `help` variant replaces it).
Internal ids (`theseus-xxxx`, `(M7)`) are out of main.rs's about lines.

`theseus help`: 71 listed commands before (one page of 160+ lines with the quick start), 19 now (80 lines of golden,
of which the long intro is the old `long_about`). `theseus help all`: 83 lines, one per command (all 76 including `help`).
Tests: `help_cmd::tests::the_everyday_help_lists_exactly_the_everyday_commands`, `help_all_lists_every_command` (from the clap
tree), `a_hidden_command_still_has_its_page`, and `tests/help.rs` (goldens of both screens, a hidden command's page,
`help nonesuch` exits 2). **Plants:** removing `hide` from `ledger` fails the first; dropping `ledger` from the renderer fails
the second.

### C. One meaning per word

- `[routing.modes.<name>]` -> `[routing.classes.<name>]` (Rust: `RoutingModes` -> `RoutingClasses`, `ModeProfiles` ->
  `ClassProfiles`, field `modes` -> `classes`); class names unchanged. `why` prints "task class deep_coding, ...".
  "interaction mode" is out of the template, route.rs, bench configs and the CLI help.
- Pack mode -> **rung**: `[routing] mode` -> `rung`, `[judge.packs.<id>] mode` -> `rung` (TOML keys only: the Rust fields,
  `PackMode`, the `pack.mode` rows and every wire field keep their names); `theseus packs` prints "max rung" and "no rung
  written"; the cockpit's ladder label says "max rung". `[judge] max_mode` **stays** (it is also the wire field
  `max_mode` of `pack.list` and `health`, and the cockpit's label of it): suggest `max_rung` with the next protocol bump.
- `max_loops_mode` -> `on_max_loops` (Rust field too; type `MaxLoopsMode` and the values unchanged): `[model]`,
  `[profiles.*]`, template, `bench/` (toml, `harbor/test_bench.py`, `recall/test_drive.py`), `crates/theseusd/tests`, notes.
  `rg max_loops_mode` finds only `renamed.rs` and `scripts/test_keel_guard.py:71` (a fixture of a bench config inside the keel
  guard's own suite, a keel file I did not touch).
- Surfaces: `approval::display_name` is the one function from a stored `cli`/`web` to `terminal`/`cockpit`
  (others as stored); `standing::surface_name` goes through it, so `Surface::as_str` is the stored value only (labels, ledger).
  I did not add a `cli`/`web` input alias to `standing add --surface` (it takes the user spelling alone). **Not done:** the
  `status`, `why` and cockpit labels that name a surface read the stored value in places I did not find in the time; see "left".
- Place qualifiers: `census --place` says "the place kind (cli, dm, ...)". Other help text already says class or kind.
- Tests: `approval::tests::a_stored_surface_shows_its_display_name` (plant: returning the stored value fails it),
  `packs::tests::a_listing_names_each_packs_rung_why_and_rows` (plant: "max mode"), the `why` goldens and
  `protocol/tests/why/*.txt` (plant: dropping "task class"), and the renamed-key tests (D).

Every remaining user-facing "mode" and a suggestion: `[memory] mode` (-> `recall`? it is the arm's rung: `rung`),
`[self] mode` (off|act: `enabled` or `act`), `[kernel] spend_limit_mode` and `daily_spend_ceiling_mode` (-> `on_spend_limit`,
`on_daily_ceiling`, the `on_max_loops` pattern), `[judge] max_mode` (-> `max_rung`), the TUI/Discord footers' `routed: <mode>`
and `route.v1`'s protocol field `mode` (wire; footers could say "class"), `session.mode`'s `mode` (the conversation mode:
correct), `docs/design/*` and `docs/spec/*` (history, untouched).

### D. Config sections

| Old (TOML) | New |
|---|---|
| `[route]` | `[route_bench]` |
| `[persona] import_files`, `own_boost` | `[persona_import] import_files`, `own_boost` (`own_boost` boosts an imported episode of the engaged persona's own agent in recall, so it belongs with the import) |
| `[model] cache_ttl` | `[cache] ttl` |
| `[model] keep_warm_hours` | `[cache] keep_warm_hours` |
| `[cache] keep_warm_minutes` | `[cache] keep_warm_every_minutes` |
| `[threads] keep_warm_hours` | `[cache] threads_keep_warm_hours` |
| `[watches]` | `[watch_for]` |
| `[routing.modes.*]` | `[routing.classes.*]` |
| `[routing] mode`, `[judge.packs.*] mode` | `rung` |
| `[model]`/`[profiles.*] max_loops_mode` | `on_max_loops` |

Rust fields followed where contained: `Config::route` -> `route_bench`, `persona` -> `persona_import`, `watches` -> `watch_for`
(`cache.ttl`, `cache.keep_warm_hours`, `cache.keep_warm_every_minutes` (the wire health field `keep_warm_minutes` is
unchanged), `cache.threads_keep_warm_hours`). `[prefix]`, `[profiles.*] cache_ttl` and `keep_warm_hours` stay.

**The renamed-key table**: `config/renamed.rs`, `RENAMED` (14 rows) and `explain(text)`; `from_toml` turns the unknown-field
error into "`[model] max_loops_mode` was renamed to `[model] on_max_loops`", and `load_text` adds `loading config <file>` (seen
live in a daemon test: `Error: loading config /tmp/.../config.toml / Caused by: `[judge.packs.*] mode` was renamed to ...`).
Tests: `CASES` has one literal (document, message) per row, `the_table_and_its_cases_are_the_same_length`, and two proofs
that the old-shaped and new-shaped configs resolve to the same `Config`: an inline one holding every old key, and the
owner-shaped fixture (`fixtures/owner_shape_old.toml`, the base's bytes, vs `owner_shape.toml`), the table moving the keys
(`migrated`) and both loaded then serialised equal. **Plant:** dropping `("watches","watch_for")` and
`("model.cache_ttl","cache.ttl")` from `RENAMED` fails four tests (`every_old_key...`, the length test, both migration tests).

**The profile rule, merged form for the joiner** (`config/cache.rs`): `inherit(profiles, c: &CacheConfig)` is the base's
`inherit` with its two lines reading `c.ttl` and `c.keep_warm_hours` (they read `m.cache_ttl`/`m.keep_warm_hours`); the
implicit `default` profile in `from_toml` is built from `cfg.cache.ttl` and `cfg.cache.keep_warm_hours`. When trim-config-inherit
generalises `inherit` to every field, lay these two reads over its version: the cache source is `&cfg.cache`, not `&cfg.model`.
`sparse.rs` needed nothing (it is generic). `keep_warm.rs` and `keep_warm_threads.rs` change only the config reads
(`cache.keep_warm_every_minutes`, `cache.threads_keep_warm_hours`) and `ttl_of`'s source word (`cache`, was `model`).

**For the China-origin rows' writer:** the routing keys are now `[routing.classes.<name>]` (`profiles`, `switch_confidence`)
and `[routing] rung`; the Rust `RoutingConfig.classes` is a `RoutingClasses`.

**The maintainer's migration list** (`/etc/theseus/theseus.toml` at install): the owner-shaped fixture used
`[model] cache_ttl = "1h"` (-> a `[cache]` table with `ttl = "1h"`), `[routing.modes.trivial|sophisticated|deep_coding|routine_coding]`
(-> `[routing.classes.<same>]`) and profile `cache_ttl` lines (unchanged). The owner's live file also has those three shapes:
move `cache_ttl` out of `[model]` into `[cache] ttl` (a profile's own `cache_ttl` lines stay), rename each `[routing.modes.X]`
table header to `[routing.classes.X]` (keys inside unchanged), and nothing else is known to apply; run `theseusd check` after:
an old key now fails with the message above, naming the file. Other keys used by the fixture are unchanged.

**The keel**: `python3 scripts/keel-guard.py --base 0a44e1f1` against the C+D commit reports 13 `cap-raised` findings, all renamed
cap keys read as removed (`max_loops_mode` in the template and `bench/theseus-bench*.toml`, `[route] replay_day_usd` and
`whole_usd` in the template); every value is unchanged. The trailer the join's signed commit needs (the owner's
words are the DM of 2026-10-17 17:23:18 MST quoted in the task):

```
Keel: cap-raised bench/theseus-bench-routed.toml, bench/theseus-bench.toml, crates/theseus-core/config/theseus.example.toml — the owner, Discord DM 2026-10-17 17:23:18 MST: "Use is not yet habitual so also feel free to be aggressive about trimming aliases and changing patterns for consistency and simplicity-- more important to be right than to be historically conservative this early" (renamed, not raised: max_loops_mode -> on_max_loops, [route] -> [route_bench]; every value unchanged)
```

The guard's watched-key list does not name `max_loops_mode`, so no keel-file change was needed. A and B have no findings
(`keel-guard --base 1811e840`: ok).

### B. The folds

Done (old command deleted and its dispatch with it; wire methods unchanged; the protocol notes say which methods new clients use):

| Old | New | Proof |
|---|---|---|
| `wakes [--session S]` | `wake [list [--session S]]` | `tests/golden.rs` (converted, header-only golden diff), `tests/wake_verbs.rs` |
| `pause ID [--until]`, `resume ID` | `wake pause ID [--until]`, `wake resume ID` | `tests/wake_verbs.rs::pause_takes_an_until_and_resume_a_name` (no old test existed) |
| `snooze ID [WHEN]` | `wake snooze ID [WHEN]` (a pending wake) and `answer ID snooze [WHEN]` (one that fired; one handler) | `tests/wake_verbs.rs`, `tests/answer.rs::a_snooze_is_answered_with_its_time`; the old `snooze_10m|snooze_1h|tomorrow_9` options are refused naming the spelling (`..._are_refused_before_anything_is_sent`) |
| `done ID` | `answer ID done` (already a `work.answer` option) | the answer tests |
| `cancel ID` | stays, hidden; `wake cancel ID` is the same handler (task or wake by the id's kind) | `wake_verbs.rs` |
| `work` | `tasks --watch` | `tests/tasks_cli.rs::work_is_tasks_watch_and_keeps_the_threads_and_the_job_ids`, converted (same assertions) |
| `watches [add\|rm]` | `watch-for [add\|rm]` | `theseusd/tests/watches.rs` converted |
| `index status\|search\|rebuild` | `memory index status\|search\|rebuild` | `tests/golden.rs` index goldens converted |
| `voice [audition]`, `notices rate` | `persona voice [audition]`, `persona rate up\|down [ID]` | `voice_cmd`/`notices_cmd` tests; no golden |
| `route report [--week]`, `turn replay`, `prefix report`, `catalog` | `model bench [--week]`, `model replay`, `model prefix`, `model catalog` | `tests/golden.rs` catalog goldens converted |
| `context [SESSION]` | `why --short [SESSION]` | `tests/why.rs::why_short_reads_the_contexts_fill_and_nothing_else` (new: one `context.explain`, no other read, the fill's words); plant: dispatching `--short` to the turn's `why` fails it |

Behaviour-changing ones and their plants: `answer ID snooze [WHEN]` + the refusals (plant: removing the branch and the
refusal fails both tests), `why --short` (plant above). The rest are pure moves: their converted tests are named above.
The goldens differ only in the `$ theseus ...` line (I read each diff), plus `agenda`'s verbs line (`VERBS`).

**Not done, and the plan:**
- `confirm` -> `answer` (no args lists; `answer ID approve|approve_trust|decline [--note] [--no-wait]` calls `action.confirm`
  and follows the resumed turn as `confirm` does; any other option is `work.answer`). I did not start it: about 40 strings
  that tell a user to type `theseus confirm` (render.rs and its goldens, `approval.rs`, discord's `ELSEWHERE`, `status.rs`,
  `extend.rs`, `budgets.rs`, `packs.rs`, `judge_runs.rs`, core `rpc/packs.rs`) and 12 golden scenarios (`confirm_*`,
  `tests/refusal.rs`, `tests/seen_commands.rs`) change with it. It needs the follow-the-turn test pinned first (`golden.rs`'s
  `confirm_follow*` already pin it).
- `executions` -> `sessions --state` (the state, turns, outstanding actions, budget columns) and removing `executions cancel`.
- The old `executions`, `confirm` spellings therefore remain, hidden. `theseus projects` (an alias of `project`) remains.

### E. `self` naming
Pure rename, no plant.

## Proof nothing was lost

- Every old command's behaviour is reachable under its new spelling: see the table (each row names its converted or new test).
  Old spellings no longer parse: `wake_verbs::the_old_top_level_verbs_are_gone` (exit 2 for `pause resume snooze done wakes work
  watches voice notices index`).
- Every old config key fails with its rename message: `config::renamed::tests::every_old_key_fails_the_load_with_its_new_name`
  (14 literal cases) and a daemon-level sighting.
- The old- and new-shaped owner fixture resolve to the same `Config`: `the_owner_shaped_fixture_resolves_the_same_before_and_after`.
- Help goldens' diff: `tests/golden/help.txt`, `help_all.txt` (new files; later rewrites change a row's name only).
- `rg` of each old spelling over `scripts bench contrib crates docs cockpit/src` (excluding the historical `docs/spec`,
  `docs/design`, `docs/research`, `docs/notes`): left only (1) the renamed-key table, its tests and `owner_shape_old.toml`;
  (2) `scripts/test_keel_guard.py:71` (a keel file); (3) `theseus confirm` and `theseus executions` (not folded);
  (4) `docs/status.md` (the maintainer's, mentions `theseus index search`/`theseus catalog`), `docs/technical-overview.md`
  `theseus confirm` lines; (5) `crates/theseus-exam/src/item.rs:649` (`"/theseus wakes"` is an exam fixture sentence, left
  on purpose); (6) wire fields `keep_warm_minutes` (health), `max_mode` (pack.list, health).
- `docs/design/m5-judgment.md` and the `crates/theseus-judge/packs/route.v*.toml` comments still say `[routing.modes.<mode>]`
  (pack comments are Jev pack text: not mine).

## Docs the maintainer writes

`CLOUD_DOCS.patch` (this branch): brings `docs/cli.md` and `docs/glossary.md` of `cloud/20261017-trim-docs` to these names;
checked with `git apply --check` on that branch's head (clean). Re-run trim-docs' cli.md test on the joined tree (the
test that lists every command may want `wake`, `watch-for`, `model`; `help all` is built from clap's tree).
Also: `docs/status.md` and the spec's Part III should record: help tiers, the folds, the renamed-key table, store format
unchanged (no store format change in this row).

## Live check for the maintainer

```
theseus help | head -30 ; theseus help all | wc -l ; theseus ledger --help | head -3
theseus --socket <scratch> wake ; theseus --socket <scratch> wake pause <id> --until "fri 9:00"
theseus --socket <scratch> answer wak_<id> snooze 1h        # a fired reminder
theseus --socket <scratch> why --short                      # the fill
theseusd --config <a file with [model] max_loops_mode = "end"> check   # fails naming the file: `[model] max_loops_mode` was renamed to `[model] on_max_loops`
theseusd --config /etc/theseus/theseus.toml config --sparse | grep -n "ttl\|classes"   # after you migrate the file
```
Each should show: the 19 everyday commands; one line per command (76); the page of a hidden command; the wake verbs
working; the snooze answered; the fill lines; the rename message; the new key names.

## What is left or uncertain

- The two folds above; the stored-surface display in `status`/`why`/cockpit labels (only `standing`'s and the new function exist);
  `docs/` and Discord strings that tell the user `theseus confirm`.
- `help`'s one-line descriptions come from each about's first sentence (parentheses dropped, cut at a colon after 24+ characters,
  clipped at 72): a few read as fragments ("Tasks, one line each under its conversation"); a curated `about` for the 19 would be better.
- Design choice for the owner: the Rust types followed the names only where contained (`RoutingClasses`, `Config.route_bench`,
  `persona_import`, `watch_for`); `PackMode` and `RoutingMode` keep their names because they are the stored and wire vocabulary.
- Environment: this VM's disk is a 40 GB allowance, so I built with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` and deleted
  `target/` once; nothing outside the repository was touched.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=<the commit before the one judged> scripts/gate.sh`, with
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` (disk). `THESEUS_KEEL_BASE` is needed on this clone: `origin/main` is 597 commits
behind the base, so the default range holds three erosions that belong to the base's history (their signed acks cannot be
verified here: `crates/theseus-sim/src/lifecycle.rs` "seed", `scripts/keel-guard.py` / `scripts/test_keel_guard.py`). The C+D commit
fails the keel by its renamed cap keys (the 13 findings above) until the join's signed commit carries the trailer, so I judged
it with the base set to itself after committing (an honest limitation: the keel's verdict for C+D is the guard's output above).

Last full run (head `a024902f`, before one more edit to a test's fake script, which I then ran alone): fmt, shape, features,
clippy `-D warnings`, cockpit (lint, test, build), test build, reader rule: **passed**. The suite failed only on:

- the known L1 cases: theseus-sandbox's contract tests and `bench::spawn_100`, theseusd's `sandbox` tests (33; root, theseus-pv6i);
- `theseus-discord` `tests_task_threads` (4), `tests_war_room` (1), `tests_default_forum` (1), `tests_task_channels` (1): **120 s
  timeouts that also fail on the E+A commit (`0e108e2e`) before any rename** (same set in `/tmp/gate1.log`: the base's tests on
  this VM, root and the task tests' jobs; I did not chase them: Discord is out of my area);
- `tests_memory_lookup::a_proc_run_of_the_cli_in_the_owners_dm_ends_with_the_native_call`: mine (the fake CLI script read
  `$4`, the argument position that the `memory index` fold moved to `$5`); fixed, and it passes with `native` (20 tests);
- one load-flake seen once, `theseus-core tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`
  (passes alone; not on the flaky list; name it for theseus-1n2y-like treatment).

The phases after the suite, run by hand (`machine_checks`): protocol types (`cockpit/src/protocol.gen` regenerated and
committed in the C+D commit: unchanged since), the turn bench (`bench turn --check --runs 5 --burst 0`: 5 frames, within
budget), size (skipped: no release build), `cargo deny --offline check bans licenses sources`: **ok**.
Not run: the lifecycle and jobs benches (the owner's machine).

Test counts on this branch: theseus-core + theseus-protocol + theseus: 3,862 passed before B; theseus: 650 passed after B
(`THESEUS_GOLDEN=write` runs rewrote only the goldens named above).

`keel-guard --base origin/main --head HEAD --quiet` on this clone judges the stale `origin/main` (above); with the real base:
`--base 0a44e1f1` prints the 13 `cap-raised` findings of C+D and nothing else.
