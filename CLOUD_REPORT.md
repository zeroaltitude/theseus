# CLOUD_REPORT: chat-sessions (theseus-6yka)

Branch `cloud/20261013-chat-sessions`, from `8745898f` (main at `fcdef5e0`, store format 32). No store-format bump, no
new config key, no new dependency (Cargo.lock is unchanged). Built and run on this 4-core VM.

## Commits

| Commit | What |
|---|---|
| `a93ff6f9` | core: `session.list` takes `dir` and `with_text`; every session says `started_on` (protocol types, generated TS, core tests) |
| `3c78ecd0` | chat: `theseus -c`, `theseus -r` (the picker), the transcript on reopening, `/history`, history per directory, the leaving line |
| `9bc0f97b` | f10: the Theseus arm's `s11` reopens with `-c` and searches with `-r` |
| (last) | this report |

## What I found

- The chat (theseus-cyha) already resumed a session by id and replayed 40 nodes; it had no directory-keyed list and its
  history was one file for every directory.
- `session.list` has no surface or place on a session. A DM's or a shared place's session is known only by the outbox's
  target for it, so `started_on` is derived from that (`discord`), else from a client having given it a directory
  (`cli`). A cockpit-opened session has no directory, so it is `None` and shows only when Tab widens the picker.
- The index's birth cursor is the WAL's birth time, not the record's `created_at_unix_ms`. A first attempt at a cursor from
  the record skipped sessions (the paging test caught it); the cursor is now the index's own, taken at the end of a
  scan step. Consequence, said in the module comment: a page is whole scan steps (500 births each), so it can hold more
  than `n`.
- F10's S11d wants a word of S8's question to find the conversation. S8's message is neither the first nor the last of
  the conversation, and it is a 60-line paste with the question at its end. A search over the first and last messages
  alone cannot find it. So the daemon also reads `prompts`: the head and tail (120 characters each) of each of the
  conversation's newest 12 user messages, one index page of its user-message nodes. This goes beyond the brief's "first
  and last messages"; please look at it (below, "Design choices").
- Disk: this VM's writable allowance filled during the setup build (`No space left on device`, rustc and ld died). I
  deleted `target/debug/incremental` and stale duplicate artifacts in `target/debug/deps` (inside the repository, all
  rebuildable) and built with `CARGO_INCREMENTAL=0` after.

## What changed

**`session.list { dir, with_text }`** (`crates/theseus-core/src/rpc/session_find.rs`, new). A session belongs to `dir` when
its recorded directory and `dir` have the same root: the nearest ancestor (itself included) holding a `.git` entry, else
the directory. A conversation started in `repo/src` is found from `repo` and the reverse. Reads the index's births a step
at a time until `n` found (default 200, at most 1,000), newest active first; the cursor is the answer's `older`. With the
index not built it reads every session once. `with_text` adds `first_text`, `last_text` and `prompts` (a few node pages
per session, no model); alone (no `dir`) it asks for every directory's. Protocol: `SessionListParams.dir/with_text`,
`SessionInfo.started_on/first_text/last_text/prompts` (all optional on the wire; `cockpit/src/protocol.gen/` regenerated
and added). `lib.rs` is 2,728 of 2,733.

**`theseus -c`** (`theseus-tui chat -c`; the CLI's `chat_cmd.rs`, `Cmd::Chat` gained `-c/--continue` and `-r/--resume`;
`theseus -c` and `theseus -r` as the whole command line on a terminal). One `session.list {dir, n: 50}`, then
`reopen::last_here`: a conversation (never a task), `started_on != discord` (never a DM's or a shared place's), with a
turn, not retired, with a directory, most recently active. With none: "no conversation here yet · starting a new one" and
a new session. Then `session.watch` and `session.history`, as `theseus chat SID` does.

**`theseus -r`** (`chat/picker.rs`, new; dock integration is one early return in `dock::build`). One
`session.list {dir, n: 200, with_text}`; typing filters in memory (every word of the query somewhere in the title,
labels, earlier titles, directory, first/last text or prompts, case-insensitive; nothing is sent); the chosen row's second
line shows the words around the match (or the last message); Up/Down choose, Enter continues it here, Tab reads
`session.list {n: 1000, with_text}` once and widens (Tab again goes back, both lists kept), Esc clears the query and then
starts a new conversation, Ctrl-D leaves and opens nothing. The picker lists conversations with a turn that are not
retired; tasks are not offered.

**The transcript on reopening** (`chat/reopen.rs::window`, `events.rs::history`). `session.history {n: 500}`; the last 30
turns (user message to the next) go through the existing `node_line` (your message, tool lines, failed results,
replies); older turns are one line, `N earlier turns · /history shows them`. A message over 6 lines is shown as its first
two lines, `… N more lines`, and its last two (a question follows a paste). `/history` (new command, `commands.rs`) prints
a page of the folded turns back from the first replayed node (`before`), saying whether more remain; with nothing folded it
says so.

**History per directory** (`chat/env.rs`). `$XDG_STATE_HOME/theseus/history/<last component>-<FNV-1a of the path>.json`,
0600, one file per directory. A directory with no file reads the old `chat-history.json` entry for it; that file is never
written or deleted.

**On leaving**: `left · the conversation goes on · theseus -c · theseus chat END`.

**F10 arm** (`scripts/f10/arm_theseus.py`, `s11` and a new `reopen` helper only): records `reopen_cmd`,
`reopen_foreground`, `picker_cmd`, `picker_searched`.

## How I proved it

Tests added (all pass):

- `crates/theseus-core/src/rpc/tests_session_find.rs` (6): the filter keeps one directory's sessions newest active first
  and leaves a no-directory session out; a git root matches from the root and from a subdirectory and not a sibling;
  `with_text` carries the first/last words only on request; with no `dir` it asks of every session; `started_on`
  (`cli`, `discord` through `bind_place`); paging by the cursor reads each of the directory's sessions once (3 births a
  step, 2 wanted).
- `crates/theseus-tui/src/chat/tests_reopen.rs` (11): `-c` takes the directory's last, never a task's, a DM's, a place's
  or an empty one; `-c` with none opens a new one; the picker filters by a word in a first message and Enter continues
  it; a search asks the daemon for nothing; Tab widens (one read) and back; Esc opens a new one; 30 of 35 turns replayed and
  5 folded with the exact line; a short conversation has no fold; `/history` asks `before` the first replayed node;
  `/history` with nothing folded says so; each directory keeps its own up-arrow history.
- `chat/picker.rs` (6): word search over first/last/prompts, Enter/Esc, Tab, what is not offered, a match deep in the prompts
  shows its words, and 1,000 sessions filter in under 16 ms a keystroke (debug build).
- `chat/env.rs` (4, one new): per-directory files, a directory reads the old file, 0600.
- `crates/theseus/tests/chat.rs` (+1): `theseus -c`, `-r`, `chat -c`, `chat -r` exec `theseus-tui chat -c|-r`; a session with
  `-c`, or `-c` beside another word, is exit 2. `chat_cmd.rs` (+1 unit).
- Rewritten, not removed (same assertion count): `tests.rs` and `tests/chat_pty.rs` assert the new leaving line; the
  history test's path is the new folder.

`cargo nextest run -p theseus-tui`: 165 passed. `-p theseus-protocol -p theseus`: 534 passed, 1 skipped. theseus-core
`test(session_find)`: 6 passed.

**Planted reverts** (each restored, `touch`ed, `git status` clean):

1. `last_here` without the kind check (a task's session taken): `continue_takes_this_directorys_last_never_a_task_a_dm_or_a_shared_place`
   and `continue_with_none_says_so_and_opens_a_new_conversation` fail.
2. the directory filter always true (`Some(_) if true => true`): `the_filter_keeps_one_directorys_sessions_newest_active_first`,
   `a_directory_finds_the_sessions_started_below_its_git_root` and `a_page_goes_back_by_its_cursor...` fail.
3. the replay unfolded (`window` returns `(0, 0)`): `the_last_thirty_turns_are_replayed_and_the_older_ones_folded` and
   `history_prints_the_folded_turns_from_the_cursor` fail.

**Speed** (a scratch daemon over `theseus-sim synth-store --sessions 1000` plus 150 sessions in one directory;
`session.list` timed over the socket; `release-thin`, p50 of 9 / max):

| read | release-thin | debug |
|---|---|---|
| `-c`: `{dir, n: 50}` | 5.2 ms / 8.0 | 14 ms |
| `-r`: `{dir, n: 200, with_text}` | 9.4 ms / 16.8 | 26 to 43 ms |
| Tab: `{n: 1000, with_text}` | 66 ms / 94 | 480 ms (measured before the node windows were cut from 24 to 8) |

`theseus -c` and `theseus -r` from Enter to a ready screen, in tmux polled at 10 ms, over that store: 47 to 53 ms
(both, five runs each, debug and release-thin alike: process start and tmux dominate). That is under the 100 ms p90. The
Tab read is not inside that bound and in a debug build is slow (0.5 ms a session for three index reads); I cut the
node windows read from 24 to 8 and have not cut further. The search itself asks the daemon nothing; 1,000 sessions filter
in well under 16 ms a keystroke in a debug build (the picker's test).

## Live check (done here; the maintainer's to repeat)

Scratch daemon on the F10 stand-in model (`theseus-sim fake-model`), state dir = a 1,000-session synth store plus
`theseus ask` in `tides` twice ("survey the tide log", "fix the parser") and in `reef` once ("map the reef"); a private
tmux at 100x30. Captured:

```
$ theseus -c            # in tides
theseus · Sonnet 5.5 · /tmp/live2/tides
? help · / commands
resumed · 1 turns · fix the parser

› fix the parser
The parser drops commas; fixed.

›
default · /tmp/live2/tides · Sonnet 5.5 · $0.03
```
```
$ theseus -r            # in tides, after typing "tide log"
resume a conversation · /tmp/live2/tides · tab: every directory · esc: a new one · ctrl+d: leave
› tide log
▸ survey the tide log                                              just now · 1 turn · $0.03 · cli
    “survey the tide log”
```
"reef" in tides lists nothing ("no conversation matches"); after Tab the list reads "every directory" and shows
`▸ map the reef  [reef]`; "bench 99" lists the synthetic sessions; Enter on `map the reef` continued it from `tides`.
`theseus -c` in `reef` opened "map the reef". In an empty directory: "no conversation here yet · starting a new one", and on
leaving: `left · the conversation goes on · theseus -c · theseus chat 2c1b8f`.

To repeat on the owner's machine (scratch daemon, never the operator's):
`theseus-sim synth-store --dir <state>/store --sessions 1000`, start `theseusd` on that state dir with its own
`--config/--socket`, `theseus ask --dir A "..."` twice and `--dir B` once, then in tmux `cd A && theseus -c` (expect the
second ask's conversation and its two messages), `theseus -r` + a word of the first ask (one row), Tab (the other directory's
row appears), Ctrl-D (back at the shell, no session opened: `theseus sessions` count unchanged).

## F10

`python3 scripts/f10/driver.py theseus --bin-dir target/release-thin --out /tmp/f10r` (both sizes), then
`scripts/f10/scorecard.py`. Also the same on the debug build: identical results except the timings. **Release-thin was built
(`scripts/build.sh --profile release-thin`), so S1b/B1 count: a prompt after 26 ms at 120x40 and 16 ms at 80x24.**

| Step | Main's recorded run (11 of 42) | This branch, 120x40 | 80x24 |
|---|---|---|---|
| S1 Start | 1 of 4 | 4 of 4 | 4 of 4 |
| S2 First question | 0 of 4 | 3 of 4 | 3 of 4 |
| S3 Run the tests | 3 of 4 | 4 of 4 | 4 of 4 |
| S4 Fix | 0 of 4 | 4 of 4 | 4 of 4 |
| S5 Permissions | 0 of 3 | 2 of 3 | 2 of 3 |
| S6 Background job | 2 of 4 | 3 of 4 | 3 of 4 |
| S7 Interrupt | 1 of 3 | 1 of 3 | 1 of 3 |
| S8 Paste | 2 of 3 | 3 of 3 | 3 of 3 |
| S9 Undo | 0 of 3 | 0 of 3 | 0 of 3 |
| S10 Context | 1 of 3 | 1 of 3 | 1 of 3 |
| **S11 Leave and come back** | **1 of 4** | **4 of 4** | **4 of 4** |
| S12 Plan | 0 of 3 | 0 of 3 | 0 of 3 |
| **Match** | **11** | **29** | **29** |
| **Beat** | 5 | 5 | 5 |

Main's column is `scripts/f10/fixtures/theseus-120x40` re-scored (11 of 42, the 2026-10-12 live run). The rows other than S11
changed since it was recorded by the chat and the other rows joined; I did **not** run the build before this row, so the
only checks I claim for it are S11b, S11c and S11d (no to yes in both sizes). All eleven checks that passed on main (S1d, S3a,
S3b, S3d, S6c, S6d, S7c, S8a, S8c, S10b, S11a) still pass. S11b: `theseus -c`; S11c: shows S6/S8/S11/S7.long (S8, S11,
S7.long at 80x24); S11d: `› these log lines` (the picker's match snippet line). No check, weight or the scorer was changed;
`scripts/f10/test_f10.py` passes (20 tests). A check I think is too narrow, unchanged: S11d reads the picker screen for a
line with the phrase, which the match snippet row satisfies; a picker that showed only titles would fail it however well it
searched.

## The gate

`THESEUS_KEEL_BASE=8745898f TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 scripts/gate.sh` on the tree of
commits 1 and 2 (the arm commit is Python only; `test_f10.py` run by hand).

- door, keel (0 findings, base = the task commit), keel tests, fmt, shape, features, clippy `-D warnings`, cockpit (lint,
  tests, build), test build: **pass**.
- suite: 4,789 passed, **35 failed**: 33 are the known L1 failures (theseus-sandbox `contract` 19, its `bench spawn_100`,
  theseusd `sandbox` 13); the other two pass alone: `theseus markdown rendering_costs_little_per_hundred_lines` (a
  wall-clock bound; 0.02 s alone) and **`theseusd gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`**
  (failed at 6 s under the suite, passed alone in 5.8 s). The second is not on the known-flaky list; I have not found why
  it failed under load, and have no reason to think the row touches it.
- after the suite, by hand: protocol types (no diff after `git add`), turn bench (`bench turn --runs 5 --burst 0 --check`
  passes: 4 frames), `cargo fmt` clean. **Not run**: the lifecycle and jobs benches (`THESEUS_GATE_NO_BENCH`), the size step
  (it passes on the release-thin build: `size_theseus_tui: 3.77 MB`), and **`cargo deny`**: offline it fails on a crate it
  has not downloaded (`android_system_properties`) and I did not run `cargo deny fetch`, so that phase was skipped.

**Keel findings expected: none from this branch.** The unmodified gate fails its keel phase on commits already in the
branch's base range (`merge-base(HEAD, origin/main)..HEAD`, 9fa84735c0): `crates/theseus-sim/src/lifecycle.rs:155`
`budget-raised: "seed" => Some(250.0)` (commit `1c2beb2747`, whose ack signature does not verify on this machine) and a
renamed store test (`a_checkpoint_for_close_...`, matched as the same test). I judged this branch against its own base
(`THESEUS_KEEL_BASE=8745898f`); the door's main copy shares the finding, so the plain gate stops in door.
`python3 scripts/keel-guard.py` (default base) prints exactly those two lines; `--base 8745898f`: "0 findings".

## Design choices the owner should hear about

1. **`prompts`** (above): the search reads more than the first and last messages, because F10's S11d needs it and a person
   searching for "what did we say about the log lines" needs it too. It adds up to ~3 KB a session to a `with_text` list
   (head and tail of 12 prompts); no stored record changes. Revisit by dropping `PROMPTS` to 0 in `session_find.rs`.
2. **Esc in the picker starts a new conversation; Ctrl-D leaves.** Decided by me; the brief does not say.
3. **Folding a long past message** (over 6 lines to first 2, a count, last 2) applies to replays and `/history` only; a
   message typed now is echoed as before.
4. **`started_on`** is derived, not stored: `discord` by an outbox target, `cli` by a recorded directory. A cockpit session
   with no directory has none. `-c` excludes `discord` only; it does not know a cockpit's session from a CLI's (it has no
   directory, so a directory list never holds it).
5. A page of `session.list {dir}` is whole scan steps (it can hold more than `n`), documented in the module and
   `crates/theseus-core/AGENTS.md`.
6. **An older daemon** ignores `dir` and returns every session; `-c` would then take the newest conversation with any
   directory. Client and daemon ship together, so I did not guard it.

## Where the docs should change (not edited, per the brief)

- Spec Part III: a new item for theseus-6yka (what it built, how proven, the `prompts` divergence, the disk note); the version
  line; `docs/status.md` (Updated, recently landed, the roadmap row; F10 now 29 of 42 on this build).
- `docs/benchmarks/f10/README.md`: the new column; S11b, S11c, S11d pass.
- `docs/design/roadmap-v2.md` row `chat-sessions`; the chat design's M7 picker (esc and ctrl-d).
- I edited the AGENTS.md files of `crates/theseus-tui`, `crates/theseus` and `crates/theseus-core`.

## Left or uncertain

- The Tab read of 1,000 sessions is 66 ms on release-thin and 480 ms on a debug build; if it matters, drop `prompts` from the
  widened read, or page it.
- The `gone_jobs` failure under load (above).
- `scripts/f10/fixtures/` not re-recorded (the maintainer's, per the brief).
