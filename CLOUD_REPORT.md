# CLOUD REPORT: f10 (theseus-qy2a)

Branch `cloud/20261010-f10`, from main at 2fd1f654 (store format 26, not bumped: nothing stored changed).

## Step: the F10 driver, checks, scorecard and fixtures

### What I found

- The baseline reproduces: on main, the driver's Theseus arm scores **11 of 42** (match) and **5 of 5** (beat) at
  120x40 and at 80x24, against the hand-run baseline's 8 of 42 and 4 of 5. The step-by-step counts match the north-star
  plan's table except in three checks, each explained below: S3a (a rule choice), S8a and S8c (fixed on main since
  2026-10-08 by paste-order and tui-paste).
- `theseus-sim fake-model --rules` could not script a turn that reads a file and then answers: the call carrying the
  tool results always got "Done.". Fixed in the sim with a test (a rule's `then`).
- A method page at `docs/benchmarks/f10.md`, where the brief put it, fails the gate: the cockpit's Benchmarks tab
  reads every `.md` at the top of `docs/benchmarks/` as a run report, and its index test (`cockpit/test/bench.test.ts`,
  "the index lists every report…", "a report's parts…") failed 19 != 20 and "f10 has its answer first". I moved the
  how-to to **`docs/benchmarks/f10/README.md`** (the glob is not recursive). If the owner wants it at `f10.md`, the
  cockpit's glob and test need an exclusion list; that is the cockpit's to decide.
- Claude Code 2.1.296's `--max-budget-usd` "only works with --print" (its `--help`), and S1a needs the bare command,
  so the arm caps itself: a watchdog prices the session transcript (`~/.claude/projects/<cwd>/*.jsonl`) every 2 s at
  rates above any current model's (15/75/18.75/1.5 dollars per million tokens: in, out, cache write, cache read) and
  stops the client and every later step past $1. The estimate lands in `run.json` (`records.cost`).

### What I changed

- `71b6811` sim: a fake-model rule's `then` answers its calls' results (`prompt_text`, `answered_turn`; the test
  `a_rules_then_answers_its_calls_results`; theseus-sim's AGENTS.md and the subcommand's help).
- `3af4318` f10: `scripts/f10/` (driver.py, arm_theseus.py, arm_claude.py, steps.py, checks.py, scorecard.py,
  tmuxio.py, procs.py, project/, fixtures/, test_f10.py) and `docs/benchmarks/f10/README.md`.
- `3bac0d4` f10: the Theseus fixture replaced by the release build's recorded run (same answers as the debug run).

### How the Theseus arm reaches each step (the rules, also in the README and arm_theseus.py's docstring)

S1 types the bare `theseus`. It errors, so S2 opens the conversation the documented way, `theseus ask "<question>"`, and
S3 onwards drive `theseus tui` (Enter opens the session, `i` the input line, `y` answers). The daemon is the defaults
plus the stand-in, the scratch project as workspace, Discord/web/index/judge off, `proc_sync_secs = 5`, and `proc.run`,
`fs.edit`, `fs.write`, `fs.patch` = approve (the template's own advice for the shell), so the approval steps have a
question. S7's interrupt is Ctrl-C (the key a person presses). B3's stop is the TUI's `s s`. B4 stops and starts
`theseusd` while the survey runs. Steps with no key or command in the arm (S5c, S9, S11b-d, S12) record none.

Where a check was ambiguous on Theseus's screens, the rule used:
- **S3a yes**: the TUI's card prints `confirm proc.run: python3 -m unittest -v test_tides`, the command as typed. The
  baseline counted S3 as 2 of 4. I can't tell which of its checks failed (likely `watch -i`'s `y`+Enter for S3b). With the
  TUI, S3a, b and d hold.
- **S1d yes**: clap's `For more information, try '--help'.` counts as one line saying how to get help.
- **S2c no**: `theseus ask` prints nothing while it waits (the stand-in holds 2.5 s).
- **S6d yes**: the late result's line `← proc.run ok · exit 0 · 90029 ms · late` is its end announced, and the session's
  history shows an assistant message after it with no person's message between (the continuation turn).
- **S8b no**: the TUI's input line shows the paste flattened with `↵` and only its tail. That is not a fold.
- **S10a no**: only `~1004 tokens`, no share of the window. **S10b yes**: the TUI's row shows `$0.00`.
- **B1**: the conversation's ready time is `theseus tui`'s (67 ms at 120x40, 73 at 80x24, polled through tmux every
  10 ms, so an upper bound), since the bare command opens none.
- **B3 yes**: `verified: process tree, 1 process` shows in the conversation when the next turn takes the stopped
  result (on S8's screen), not on the stop's own screen. The check reads both.
- **B5 yes**: 5.6 MB (`theseus-tui`; the client in front of the pane's shell, not the daemon).

### How I proved it

- `python3 -m unittest discover -s scripts/f10 -p 'test_*.py'`: **20 tests, OK**. They cover every check on both fixtures
  (Claude Code's drawn: 42 yes, 5 no; Theseus's recorded: the answers above), the totals, rule edges (a word cut at the
  edge, raw markdown, a bare tool name, the JSON-only edit, a reorder), the stand-in's rule order, the project (the bug
  fails, the fix passes, line 50), and the tmux isolation (every argv carries `-L f10-… -f /dev/null`, the private
  socket exists, and the default socket is neither created nor touched).
- Planted reverts (Python), each restored and the suite OK after:
  1. S2a always yes: caught by `TheseusFixture` [S2a] "True != False : S2a: planted", `test_raw_markdown_fails_rendering`,
     `test_the_totals`, and the markdown totals test.
  2. S3c reading the wrong capture (S3.ask instead of S3.after): caught by `ClaudeCodeFixture` [S3c] "the test's own
     failure output is not on the screen", and the markdown totals test.
  3. A totals bug (beat checks counted into match): caught by `test_all_yes_is_42_and_5`,
     `test_totals_count_each_step_and_the_beat_checks_apart`, `test_the_totals`, and the markdown totals test.
- Planted revert (Rust): `answered_turn` back to "Done." failed `a_rules_then_answers_its_calls_results`
  (`left: "Done." right: "# Tides…"`). Restored, `touch`ed, 5 of 5 pass.
- No runs under load: nothing here is timing-bound Rust, and the driver's waits are bounded polls.

### Live check on this VM (release-thin build, the stand-in, $0)

`scripts/build.sh --profile release-thin && python3 scripts/f10/driver.py theseus --bin-dir target/release-thin --out
/tmp/f10-live` (about 14 minutes for both sizes):

| Step | Check | theseus 120x40 | theseus 80x24 |
|---|---|---|---|
| S1 Open (H) | a. one command, no arguments, opens a conversation in this directory | no | no |
| S1 Open (H) | b. a prompt to type into appears in 2 s or less | no | no |
| S1 Open (H) | c. the first screen names the model and the directory | no | no |
| S1 Open (H) | d. it says in one line how to get help or how to use it | yes | yes |
| S1 | | **1 of 4** | **1 of 4** |
| S2 Ask "what does this project do?" (H) | a. the reply's markdown is rendered (headings, emphasis, code) | no | no |
| S2 Ask "what does this project do?" (H) | b. each tool call names what it touched (path or command) | no | no |
| S2 Ask "what does this project do?" (H) | c. something moves while it waits (spinner or elapsed time) | no | no |
| S2 Ask "what does this project do?" (H) | d. lines wrap at word boundaries at 80 columns | no | no |
| S2 | | **0 of 4** | **0 of 4** |
| S3 Run the tests (H) | a. the command is readable before approving | yes | yes |
| S3 Run the tests (H) | b. approving takes one key | yes | yes |
| S3 Run the tests (H) | c. what failed can be seen without leaving the conversation | no | no |
| S3 Run the tests (H) | d. no id has to be copied | yes | yes |
| S3 | | **3 of 4** | **3 of 4** |
| S4 Fix, with a reviewed edit (H) | a. the edit's diff shows before approving | no | no |
| S4 Fix, with a reviewed edit (H) | b. it is coloured, with the changed words marked | no | no |
| S4 Fix, with a reviewed edit (H) | c. the diff shows in the conversation afterwards | no | no |
| S4 Fix, with a reviewed edit (H) | d. the file and the line numbers show | no | no |
| S4 | | **0 of 4** | **0 of 4** |
| S5 Don't ask again (M) | a. the prompt offers "this command, for the rest of the session or project" | no | no |
| S5 Don't ask again (M) | b. that grant covers the command's prefix, not the whole tool | no | no |
| S5 Don't ask again (M) | c. one key switches to accepting file edits for the session | no | no |
| S5 | | **0 of 3** | **0 of 3** |
| S6 A 90-second command (M) | a. elapsed time shows while it runs | no | no |
| S6 A 90-second command (M) | b. its output so far can be seen | no | no |
| S6 A 90-second command (M) | c. you can keep talking while it runs | yes | yes |
| S6 A 90-second command (M) | d. its end is announced and the agent goes on by itself | yes | yes |
| S6 | | **2 of 4** | **2 of 4** |
| S7 Interrupt (H) | a. one key stops the turn | no | no |
| S7 Interrupt (H) | b. the conversation stays open and asks what to do instead | no | no |
| S7 Interrupt (H) | c. work started earlier keeps running | yes | yes |
| S7 | | **1 of 3** | **1 of 3** |
| S8 Paste a 60-line log with a question (H) | a. paste and question are one message | yes | yes |
| S8 Paste a 60-line log with a question (H) | b. the paste is folded so the prompt stays readable | no | no |
| S8 Paste a 60-line log with a question (H) | c. nothing is lost or reordered | yes | yes |
| S8 | | **2 of 3** | **2 of 3** |
| S9 Undo (H) | a. the agent's edits since a chosen prompt can be reverted | no | no |
| S9 Undo (H) | b. the conversation can be taken back to that prompt | no | no |
| S9 Undo (H) | c. undoing the last edit takes 6 keys or fewer | no | no |
| S9 | | **0 of 3** | **0 of 3** |
| S10 Context and cost (M) | a. how full the context is, as a share of the window | no | no |
| S10 Context and cost (M) | b. what the session has cost | yes | yes |
| S10 Context and cost (M) | c. what fills the context, by part | no | no |
| S10 | | **1 of 3** | **1 of 3** |
| S11 Leave and come back (H) | a. leaving never stops work in flight | yes | yes |
| S11 Leave and come back (H) | b. one short command reopens the last conversation in this directory | no | no |
| S11 Leave and come back (H) | c. reopening shows the transcript | no | no |
| S11 Leave and come back (H) | d. past conversations can be searched and picked | no | no |
| S11 | | **1 of 4** | **1 of 4** |
| S12 Plan first (M) | a. a read-only planning mode one key away | no | no |
| S12 Plan first (M) | b. the plan shows for approval before any edit | no | no |
| S12 Plan first (M) | c. progress through the plan's steps shows | no | no |
| S12 | | **0 of 3** | **0 of 3** |
| Beat | B1. a ready prompt 100 ms or less after the command | yes | yes |
| Beat | B2. every turn's cost shows without asking | yes | yes |
| Beat | B3. a stopped command is verified gone (its processes) | yes | yes |
| Beat | B4. work in flight survives a restart of the agent's own process | yes | yes |
| Beat | B5. the client stays under 20 MB resident | yes | yes |
| **Match** | of 42 | **11** | **11** |
| **Beat** | of 5 | **5** | **5** |

### theseus 120x40 (theseus 0.0.1)

- S1a no: `theseus` returned to the shell: '$'
- S1b no: `theseus` never showed a prompt to type into
- S1c no: the first screen lacks the model and the directory
- S1d yes: For more information, try '--help'.

Evidence, 120x40 (80x24 differs only in numbers: B1 73 ms, B5 5.3 MB, S2d cut at `works out t|he tidal`):

- S1a no: `theseus` returned to the shell: '$'
- S1b no: `theseus` never showed a prompt to type into
- S1c no: the first screen lacks the model and the directory
- S1d yes: For more information, try '--help'.
- S2a no: raw markdown: '# Port Wenlow tides'
- S2b no: a call shows only its tool: '→ fs.read'
- S2c no: nothing moved in a second of waiting; the screen ends '⟳ context recompile (new_session, transcript) · 1 message(s) · ~14002 tokens'
- S2d no: cut: 'he high and low waters b' / 'etween their neighbours,'
- S3a yes: out 12 · $0.0002 │ you: run the tests: python3 -m unittest -v test_tides │ ── turn turn_
- S3b yes: one key: y
- S3c no: the test's own failure output is not on the screen
- S3d yes: no id typed
- S4a no: no diff before approving
- S4b no: no diff lines to colour
- S4c no: the diff is not in the conversation after the edit
- S4d no: no line number beside the file's name
- S5a no: no offer to stop asking
- S5b no: no offer to scope
- S5c no: the arm has no key that accepts edits
- S6a no: no elapsed time moves while it runs
- S6b no: none of its output so far is shown
- S6c yes: answered while it ran: The low water is the 12:00 reading, at 0.60 m.
- S6d yes: │ ← proc.run ok · exit 0 · 90029 ms · 830 B · late
- S7a no: C-c did not stop the turn
- S7b no: the key left the conversation for the shell: '$'
- S7c yes: the earlier survey still runs
- S8a yes: one message: question and log
- S8b no: no fold and no log on screen
- S8c yes: all 60 lines, in order
- S9a no: the arm has no undo
- S9b no: the arm has no undo
- S9c no: the arm has no undo
- S10a no: only '│ ⟳ context append · 29 message(s) · ~1004 tokens'
- S10b yes: ○ ready what does this proj $0.00 │ ses …7fb573 · what does this project do? · claude-sonnet-5-5 · ○ ready
- S10c no: parts: tools?, messages?
- S11a yes: the survey still runs after leaving
- S11b no: no command reopens the last conversation here
- S11c no: nothing reopened
- S11d no: no picker of past conversations
- S12a no: the arm has no planning mode
- S12b no: the arm has no planning mode
- S12c no: the arm has no planning mode
- B1 yes: a prompt after 67 ms (`theseus tui`)
- B2 yes: ○ ready what does this proj $0.00 │ ses …7fb573 · what does this project do? · claude-sonnet-5-5 · ○ ready
- B3 yes: │ ← proc.run stopped by the TUI · 315 B · late · verified: process tree, 1 process
- B4 yes: the survey ran on through theseusd's stop and start
- B5 yes: 5.6 MB (theseus-tui (pid 27594))

### The live checks for the maintainer

1. The Theseus arm, as above: `scripts/build.sh --profile release-thin && python3 scripts/f10/driver.py theseus --bin-dir
   target/release-thin --out /tmp/f10-theseus`. It should print the same table (11 and 5 at both sizes).
2. **The Claude Code arm, local only** (real model, at most about $1 a run; the watchdog's estimate is in
   `run.json`'s `records.cost`): `python3 scripts/f10/driver.py claude-code --out /tmp/f10-claude --expect-version
   "$(claude --version | cut -d' ' -f1)"`. Expect 42 of 42 and 0 of 5, as the baseline found. Where a check says no,
   read its evidence and its screen (`less -R /tmp/f10-claude/claude-code-120x40/screens/<step>.ansi`). The arm's keys
   and screen patterns (ready `? for shortcuts`, busy `esc to interrupt`, the ask `Do you want to…`, the rewind
   `Esc Esc Enter Enter`, plan mode by `BTab BTab`, `/context`, `/cost`, `claude -c`, `claude --resume`) are written from
   2.1.x and were never run here, so expect to adjust a pattern on the first run. Then replace the drawn fixture with
   its 120x40 run (`cp -r /tmp/f10-claude/claude-code-120x40 scripts/f10/fixtures/`, drop driver.log) and rerun
   the tests. `ClaudeCodeFixture` then holds the checks to real screens.
3. One scorecard over both: `python3 scripts/f10/scorecard.py /tmp/f10-theseus/theseus-120x40
   /tmp/f10-claude/claude-code-120x40 /tmp/f10-theseus/theseus-80x24 /tmp/f10-claude/claude-code-80x24 --out
   /tmp/f10-scorecard`. The first run's report goes in `docs/benchmarks/` (the README says how).

### Left, uncertain, and for the owner

- The Claude Code arm is untested against a live Claude Code (by the brief). Its S9 undoes a fresh edit (the S9.edit
  prompt, the latest) rather than S4's, so "since a chosen prompt" is the latest prompt. S11's restart for B4 is
  Claude Code's own exit. S12 waits up to 300 s for a plan.
- The stand-in answers at once, so timing checks (S2c, S6c) can differ between arms for that alone. The S2 rule holds
  its answer 2.5 s so S2c has a wait to watch.
- S3b's `ran` comes from the module's bytecode appearing (a unittest run imports `tides`).
- **Siblings**: h2h-bench's pty driver and this tmux driver should share a module later: the terminal layer
  (`tmuxio.py`'s private server, capture, keys, paste) and `/proc` reads (`procs.py`) are general. The checks and
  steps are F10's own. Not merged now, as the brief says.
- Docs the maintainer may want: `docs/benchmarks/README.md` could link `f10/README.md` as the method page. The
  north-star plan's delight table ("8 of 42") should read 11 of 42 and 5 of 5 on main today. scripts/AGENTS.md could
  name `scripts/f10/`.

## Keel findings expected

None. `THESEUS_KEEL_BASE=2fd1f65 python3 scripts/keel-guard.py`: `keel: ok (2fd1f65..the working tree, 0 findings)`.
(Against this clone's stale local `main`, a9ad950, the guard listed five erosions from commits already on main between
the two. Once `origin/main` was fetched, the gate's own run used 2fd1f65 and found none.)

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before the commits: keel, fmt, shape, features, clippy and
cockpit (306 of 306) passed. The suite ran 3,683 tests: 3,650 passed, 33 failed, 44 skipped. The 33 are all the known L1 cases (root VM, no job cgroup,
theseus-pv6i): theseus-sandbox's contract tests and `spawn_100`, and theseusd's `sandbox` tests. No other failure, and
no timing flake this time. The phases after the suite, run by hand: protocol types unchanged; turn bench `frames_plain
5 (budget 5)`, `frames_tool 9 (budget 9)`; `cargo deny --offline check`: advisories, bans, licenses, sources ok.
The final gate, before the report's commit (head 3bac0d4): the same. The suite ran 3,683 tests: 3,650 passed, and 33 failed, every one an L1
case listed above. No other failure. Protocol types unchanged, the turn bench at 5 and 9 frames, and deny ok. **Green by
the brief's rule.** Python under scripts/f10 is outside the gate: its suite (20 tests) passed before each commit.
