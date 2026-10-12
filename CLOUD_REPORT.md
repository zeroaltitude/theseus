# CLOUD REPORT: openclaw-sync (theseus-pjmj7)

Branch `cloud/20261011-openclaw-sync`. Commits: `08137891` (step 1, append), `e19fa58c` (steps 2 to 4). Both were pushed before the gate had finished, because the stop hook asked for it. The gate result is below. Neither commit is signed.

## Step 1: the importer appends (08137891)
- **Found:** `write.rs` rejected every changed hash. `import-a` had the same "grown file is refused" rule, which is now replaced for every source; its test `a_grown_file_is_refused_not_overwritten` is rewritten as `a_grown_file_is_appended_to_not_overwritten` and has more assertions (the keel guard shows 0 findings).
- **Changed:** `import/append.rs`, `import/write.rs`. The per-episode decision moved into `take()` to stay under clippy's 100-line limit. `ImportEpisodesResult` gained `appended` and `touched`. The CLI report prints "N appended". `SYNC_SOURCE` and `SCRUBBED_SOURCES` are in `episode.rs`.
- **Prefix proof:** it reads the stored message nodes (`session_nodes`, in write order) and compares idx and sha256 position by position, after the scrub. It does not keep a chained digest on `ImportedFrom`. So **no store-format bump**, and the row's one bump is unspent. A rejection names the first differing idx, or the first removed one. An episode that adds nothing is refused as "adds nothing". An erased session is refused.
- **Frames:** an append's nodes, the session record (hash, messages, `as_of.end_ms`, `last_active_ms`) and the tag's counts are one frame. The session keeps its original tag, place, labels and `file`. A summary is rewritten only when its text differs (not exercised by the sync).
- **Proof:** `import::tests_append` (4 tests: appends only the new nodes with one frame and counts adding up, a repeat writes nothing, rewrite/removal/reorder/different first message are named, erased is refused, scrub plus append). `import::` in theseus-core: 63 tests passed.

## Step 2: the exporter, in the repo
- **Form: a stdlib-only script**, `scripts/openclaw-export.py`, built into the CLI with `include_str!`. The rule that Cargo.lock gains no package settled it: SQLite and zstd cannot be added. Node's zlib (OpenClaw's own runtime) or the `zstd` program decodes `event_zstd` when Python 3.11 has no codec. I did not measure the cold build cost of rusqlite and zstd.
- **Shapes** come from OpenClaw's `openclaw-agent-schema.sql` (cloned and read here). They are pinned in `scripts/tests/openclaw_fixture.py`. Two guesses are marked as such: the archive blob is one event per line with seq equal to the line index, and the inbound "Sender (untrusted metadata)" JSON block carries the sender id and name.
- **Episode:** id `ep_` plus sha256 of `openclaw-sync\0agent\0session`; idx is seq; one episode per session. The owner's turns are `operator`; the agent's are `agent:<name>`; other people's channel messages are `outside` with author `person:<name>`; webhook or mail hook sessions are `outside`; cron and heartbeat prompts are `operator` with author `schedule` (the owner's own configuration; say if you want them `outside`). The place is dm, slack-channel, discord-channel, cron, heartbeat or cli (unknown channels map to cli). Sensitivity is `personal` for a DM and `company-confidential` otherwise. No partner label is set.
- **Cursor** (in META `import.sync.state`): per agent, the newest `transcript_events` rowid, the archive rowid and the rewrite-watermark time; a nightly run with nothing new is three one-row reads per agent. `floor_ms` is the first run's start and stays for every session, so each session's message list stays a prefix of its own later one. **Deviation:** the per-session "last seq exported" is not in the cursor. The session re-exports whole (the prefix proof needs that), and the people mark carries its own position. A rewrite shows through the watermark table and is refused by the daemon with its idx.
- **First run:** the default `--since` is the newest `as_of` end of the `openclaw-store` sessions. With none, it refuses. The 2026-10-06 episodes are never extended: their ids hashed their last message and their text was filtered.
- **Tag:** `openclaw-sync-YYYY-MM` for a new session; an appended one keeps its own tag.
- **Proof:** `scripts/tests/test_openclaw_export.py`, 15 tests: DM, channel with another person, cron, deleted session in the archive, zstd (needs node), rewrite, unknown event type, planted secret passed through, cursor reads only changed sessions, floor, read-only file unchanged, undecodable event. It runs in the gate via theseusd's `tests/import_sync.rs`.

## Step 3: nightly
- **Timer, not a daemon wake** (it follows from the script). `scripts/user-service.sh sync-timer install|uninstall|status [--hour N]` writes `<unit>-sync.service` and `.timer` at HOUR:40 with `Persistent=true`. Tests are in `user_service_script.rs` (2 new; they run as root too).
- **Config:** `[import.openclaw] state_dir`, `owner_ids`, `agents`; off while `state_dir` is unset; template lines are commented out.
- **CLI:** `theseus import sync [--from] [--since] [--agents] [--tag] [--dry-run] [--no-people]`. It runs the plan RPC, runs the exporter with stdout as a pipe (raw text is never in a file), sends batches, and finishes with `import.sync.record`. `import.sync.plan` and `.record` are in `OPERATORS`, so a job is refused.
- **Record:** it writes the state and an `import.synced` row in one frame, then `import people` for the touched tags (no model call), then the extraction over each touched session's nodes after its mark. The mark is META `judge.people.<session>`, the live point's key, a WAL position. The extraction runs under `[people] sweep_usd_per_day` and shares the day with the sweep (`people.sync.day`, which `people_sweep` adds to its own spend). It writes the state again with the spend and the sessions left.
- **Proof:** `judge::people::tests_sync` (3 tests: only new messages are read and the mark moves; the cap is held and the day shared; off with the sweep) and `import::tests_sync` (6 tests).

## Step 4: health
- `HealthImported.sync` (when it ran, new and appended sessions, messages, rejected, people spend, next run, error, and one `warn` sentence for a failed or missed night). It shows in `theseus health` (a line plus `⚠ import sync: …`), in the TUI header (`health` is read at each connect), and in the cockpit's Systems card. Tests: render golden, `theseus-tui` `tests_sync` (2 tests), a cockpit node test, and a core golden for the warn sentences.

## Planted reverts (each reverted, file touched, `git status` clean)
| Revert | Caught by |
|---|---|
| prefix check off | `tests_append`: `an_append_writes…`, `a_rewrite_a_removal…` fail |
| scrub skipped | `tests_append` scrub test, `tests_harness` scrub tests (4 failures) |
| cursor ignored | the exporter's tests (4 failures) |
| per-session people mark ignored | `the_people_step_reads_only_new_messages…` fails |

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel (0 findings, "Keel findings expected: none"), fmt, shape, features, clippy, cockpit lint/test/build, test build and the reader rule passed. The suite failed on 4246 tests: **about 135 failures, all of which fail on this VM without my change** (the L1/root `theseusd::sandbox` tests and theseus-sandbox's). One more, `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`, is not on the known list, times out at 21 s, and **fails identically on main's base `7005d130`** (checked by checking that tree out). It is not caused by this change. After the suite I ran the remaining phases by hand: generated TypeScript (clean) and `cargo deny --offline check` (ok). The lifecycle bench could not run (`theseus-sim bench lifecycle --runs 10 --check` fails here because its job fails under the root-L1 limit, not for any startup reason), so no timing is recorded. Nothing was added to the start path, and an idle night costs three one-row reads per agent. I did not rerun the whole gate after the last commit.

## Live check
On this VM the real CLI against a real daemon ran inside `theseusd/tests/import_sync.rs` (3 tests passed): the first run refuses and asks for `--since`; the dry run writes nothing; 5 sessions with 11 messages import; a second run writes nothing; after the fixture grows, 1 is appended and 1 is new; the planted key is in none of the daemon's files; and `health` names the sync. A hand-run scratch daemon failed on my own mis-written TOML, so there is no hand transcript; I did not run recall or the people extraction against a real model.

For the owner's state dir (replace `$STATE`; the config needs `[import.openclaw] state_dir` and `owner_ids`):
1. `theseus import sync --dry-run` should print sessions and messages per agent and record nothing.
2. `time theseus import sync` should print "sync: N sessions read… N new, 0 appended" and the people line. `theseus import list` should show `openclaw-sync-YYYY-MM`. `theseus health | grep sync` should show the line.
3. Run `theseus import sync` again: 0 new. After OpenClaw grows a session, a run shows "appended".
4. `scripts/user-service.sh sync-timer install --hour 3`, then `systemctl --user list-timers`; the morning after, `theseus health` shows the night's sync.

## Docs for the maintainer
`docs/status.md`, the spec's Part III item and `docs/user-service.md` (a `sync-timer` section) are yours to write. Note that the root `AGENTS.md` was already over 20 KB before this change. The module AGENTS.md files are updated.
