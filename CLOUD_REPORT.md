# CLOUD_REPORT: home-warn (theseus-d9je7)

Branch `cloud/20261021-home-warn`, base `integ/tail` at `7f428787`. Code commit `2fcb91fb`. Store format: unchanged (48).

## Keel guard (first section, as the brief asks)

`python3 scripts/keel-guard.py --base origin/integ/tail --head HEAD --quiet` at `2fcb91fb`: no output, exit 0.

Two things about the gate's keel phase, so the maintainer is not surprised:
- Plain `scripts/gate.sh` judges against `origin/main`, and it fails on this VM for reasons that are not this row: it flags `scripts/keel-guard.py` and `scripts/test_keel_guard.py` from an earlier commit, whose `Keel:` ack signature cannot be verified here (`%G? is E`), plus the moved lifecycle tests. I ran the gate with `THESEUS_KEEL_BASE=origin/integ/tail` instead.
- My first version renamed the unit test `the_warning_names_the_size_the_bound_and_the_commands` in `home_size_step.rs`, and the guard flagged it as `test-removed`. I gave the test its old name back (its assertions changed to the new text, their sense is the same: size, share of the window, commands), so nothing is flagged. No ack is needed.

## What I found

`home_size_step.rs` weighed a home against `[threads] working_tokens` (60,000), a cold-resume bound for threads. A home at 76,205 tokens is ordinary. The text printed the raw place and left `NAME`, `@A`, `@B` and `SPLIT` for the reader.

Divergence from the brief: `theseus thread unsplit` takes the split's id (`spl_…`, printed by `thread split`), not the thread name (`rpc/thread_home.rs::thread_unsplit` reads META `thread.split.<id>`). So the text says "`theseus thread unsplit` with the id it prints undoes it", not `unsplit <name>`.

## What changed (commit `2fcb91fb`)

- `config/threads.rs`: `home_warn_percent: u8`.
  - Default 50, range `1..=100`, checked in `validate` with the key, value, range and default named.
  - Template line (in the `[threads]` block only): `# home_warn_percent = 50                 # a place's home past this share (1-100) of the serving model's context window tells the owner's DM, once per home; large sessions are otherwise fine`
- `turn/home_size_step.rs`:
  - The threshold is `window * home_warn_percent / 100`, and the marker's `threshold` records it. `working_tokens` is no longer read by this step.
  - The window is `Turn.fill.window` (`fill_window(spec)`: `[context] working_window_tokens` if set, else the catalog's `context_window` for the model that compiled this request). Fallback is 1,000,000 (`FALLBACK_WINDOW`) when no window is known. `compile_step.rs` is untouched: `t.fill` is set on the line before the existing call.
  - The module comment's "The bound" paragraph is rewritten, and a "The text" bullet is added.
  - `warning_text(name, tokens, window, split)` and `stretch(nodes, keep, taken)` are new. The place's human name is `place_name(sid)`. A place no binding names reads "This place", never the raw target.
- `AGENTS.md` (core) home-size paragraph and one dated `docs/status.md` entry (2026-10-22).
- `theseus-discord/src/tests_gateway.rs::a_home_size_warning_goes_to_the_owners_dm_alone` also used `working_tokens`. Outside the brief's file list; it now sets `home_warn_percent = 1`, sends a long message, and matches the post on "home is … of the model's window", since a one-turn home has no command.

## The new warning, as the tests render it

With a stretch (4 turns, `resume_keep_turns` 2; place `DM @owner`, stored `dm:5150`):

> 📏 DM @owner's home is 51,492 tokens, 5% of the model's window. To see what moving its oldest turns into a thread would save: `theseus thread split home@dm:5150 earlier-2026-10-22 --first @16 --last @61 --dry-run`. Without `--dry-run` it splits, and `theseus thread unsplit` with the id it prints undoes it.

Without a stretch (one turn, default `resume_keep_turns` 6):

> 📏 DM @owner's home is 28,443 tokens, 2% of the model's window. `theseus history` shows what it holds.

The command uses the stored place name (`dm:5150`, which `thread split` accepts), so the text has no `discord:` anywhere.

## How the split is chosen

`stretch()` walks the home's transcript (`t.tc.store.transcript`, only when a warning is about to post, once per home).

- A turn is the non-recall, non-summary, non-moved nodes that share a `turn_id`. Nodes an earlier split hid (`moved::read(..).hidden`) are skipped, so a second warning never proposes turns that already moved.
- With N turns and `keep = resume_keep_turns`, the stretch is turns `0..N-keep`. `--first` is the first node's WAL position of the oldest turn, and `--last` is the last position among the stretch's turns. With `N <= keep` there is no stretch and no command.
- The name is `earlier-<local date of the stretch's last turn>` (from the turn id's time). It gets `-2`, `-3`… if a thread of that name exists.

## Proof

Test commands (run with `TZ=America/Phoenix`):
- `cargo nextest run -p theseus-core home_size config::threads`: 13 `home_size` tests plus the config tests, all pass.
- `cargo nextest run -p theseus-discord a_home_size`: 1 passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean (I formatted only the files I edited).

How the command is proven to run as written (`the_warning_names_the_place_and_runs_as_written`): the CLI's parser is not reachable from core tests, so the test parses the backticked command's words. It builds the same `ThreadSplitParams` the CLI builds (`from`, slug, `range: [first, last]`) and calls the daemon's own `thread_split_plan` (the `--dry-run` path, with its name, position and whole-turn validation). The plan must succeed with `plan.turns == 2` (the two oldest of four turns), and `--first`/`--last` must be positions in the session's transcript. The text must contain `DM @owner` and none of `discord:`, `NAME`, `@A`, `@B`, `SPLIT`, `<`.

New and changed tests:
- `tests_home_size::a_home_under_half_the_window_is_not_warned` (key test): a home of about 76k tokens (5,900 repeats) posts nothing and writes no marker. A home past 500,000 (52,000 repeats) has `threshold == 500_000`, posts once, and a second turn adds nothing.
- `the_warning_names_the_place_and_runs_as_written`, as above.
- `a_home_with_no_settled_stretch_gets_no_command`.
- `config::threads::tests::the_home_warning_is_a_share_of_the_window`: default 50, 80 read back, 0 and 101 refused with the key named, template line present.
- Existing tests: the bound moved from `working_tokens = 2_000` to `home_warn_percent = 1` (10,000 on a 1M window). `low()`, `long()` (now 1,000 repeats, so the routed test still routes), the `tokens > 10_000` assertion and the "armed again" test's raised bound (`home_warn_percent = 100`) changed. Assertions keep their sense. The old assertion on the text's "2,000" and the old split command became `"% of the model's window"`, `DM @owner` and `theseus history` (the one-turn home), plus a `threshold == 10_000` check.
- The unit test in `home_size_step.rs` was rewritten for the new `warning_text` signature (keeps its old name, see the keel section).

Planted reverts (each restored, `touch`ed, `git status` clean afterwards):
1. Compare against `working_tokens` again: `a_home_under_half_the_window_is_not_warned` failed at `tests_home_size.rs:225`, with the unexpected post "📏 DM @owner's home is 75,958 tokens, 7% of the model's window."
2. Print the raw target as the name: `the_warning_names_the_place_and_runs_as_written` failed at `tests_home_size.rs:289` (the `DM @owner` assertion).
3. Always emit a command (`keep = 0`): `a_home_with_no_settled_stretch_gets_no_command` failed at `:311`, with the text showing `--first @16 --last @16`.
4. Remove the range check (accept 0..=255): `config::threads::tests::the_home_warning_is_a_share_of_the_window` failed at `threads.rs:480` (`unwrap_err()` on an `Ok`).

I did not plant "restore the old `warning_text`" separately; plant 2 covers the place name, and the old text fails the same test on `NAME` and the raw place.

## Gate

`CARGO_INCREMENTAL=0 THESEUS_KEEL_BASE=origin/integ/tail TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. Door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build and the reader rule passed. The suite phase ran 7,610 tests: 7,575 passed and 35 failed.
- 34 failures are the known L1 ones: `theseus-sandbox::contract` (all clauses), `theseus-sandbox::bench spawn_100`, and 14 `theseusd::sandbox` tests (two of them timed out at 30 s). The VM runs as root, so L1 refuses (theseus-pv6i).
- 1 failure was mine: the Discord `a_home_size_warning_goes_to_the_owners_dm_alone` (still on `working_tokens`). I fixed it, and it passes alone (see above).
- `the_cores_output_matches_its_golden` passed with `TZ=America/Phoenix`. Neither `a_filtered_page_equals_the_scans_answer` nor `python3s_repl_computes_on_the_screen` failed.
- I did not rerun the whole gate after the Discord fix. The VM's disk allowance ran out twice (the 25 GB `target/debug/deps` plus incremental), and I wiped `target/debug`. The phases after the suite (protocol types, lifecycle, jobs, turn and size benches) did not run: benches are off by instruction, and I changed no protocol type. `cargo deny` was not run by me (the gate's deny phase is before the build and I did not check its result separately).
- The commit is unsigned: the VM has no signing key (`gpg.ssh.allowedSignersFile` is not set, `%G? N`).

## Live check for the maintainer

On a scratch daemon (fresh state dir, a DM with the owner bound, default config) with a home of more than 500,000 tokens (for example the owner's copy of a big store, not the live one):
1. Send a message and watch the owner's DM: one `home_size` post saying "📏 DM @… 's home is N tokens, P% of the model's window. … `theseus thread split home@dm:<id> earlier-<date> --first @A --last @B --dry-run` …" with real numbers. A home under 500,000 says nothing.
2. Run that command as printed. It should print the plan (turns, nodes, before and after tokens). Without `--dry-run` it splits and prints `spl_…`, and `theseus thread unsplit spl_…` undoes it.
3. `theseus config --sparse` with `[threads] home_warn_percent = 101` (or 0) should refuse the config, naming the key.

## Left or uncertain

- The window comes from `fill_window`, so a `[context] working_window_tokens` lowers the bound too. That seems right ("a home near the window") but it is a reading of "the model's window", so the owner may want to know.
- `stretch()` uses a `Range` pick, which also takes any turn-less nodes (a summary or harness note) between the first and last position. If a late background result of a kept turn landed inside the range, `thread.split` would refuse with the positions to type. The plan call in the test passes, and the warning stays a dry run, so the failure mode is a readable refusal.
- Follow-up for docs: `docs/status.md` has my one entry; the maintainer may want to reword it at the join. The spec's Part III item for theseus-jg1eg should mention the new bound.
