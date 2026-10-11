# F10: the first ten minutes

F10 is the delight yardstick (theseus-wy6v, theseus-qy2a): the same twelve steps a person takes in their first ten
minutes with a coding agent, for Theseus and for Claude Code, on the same scratch project, each step with yes or no
checks, 42 in all, in a 120x40 terminal and again at 80x24. **Match** is all 42; five **beat** checks (B1 to B5) are
ones Claude Code failed or did not show on 2026-10-08. F10 measures the *client*, not the model: "it fixed the bug"
belongs to the coding benchmarks, so the Theseus arm runs on a scripted stand-in model and costs nothing, and no check
reads what a model chose to say.

The driver is in the repository, `scripts/f10/`, standard library only:

| File | What it is |
|---|---|
| `driver.py` | Runs one arm at one or both sizes in a private tmux server, captures each step's screens, keeps its records, and writes the scorecard. |
| `arm_theseus.py`, `arm_claude.py` | How each arm is driven: its commands, its keys, and the records only it can read (a session's history, a transcript). |
| `steps.py` | The steps, the checks' ids and words, the prompts both arms are given, and the stand-in's scripted answers. |
| `checks.py` | One function per check: it reads a run's captured screens and records and answers yes or no, with the evidence line. |
| `scorecard.py` | The table below with a yes or no per check and per run, each step's count, the totals, and every answer's evidence; Markdown and JSON. |
| `tmuxio.py`, `procs.py` | The private tmux server (`tmux -L f10-<pid> -f /dev/null`, never the default socket or anyone's tmux.conf), and what the driver reads from `/proc`. |
| `project/` | The scratch project: an invented harbour's tide-table library (`tides.py`) with one planted bug, its unittest file, a README, and `slow_survey.py`, which prints its progress for 90 s. |
| `fixtures/` | Screens for both arms, so the checks are tested without a terminal: Theseus's recorded by the driver on main, Claude Code's drawn from the check definitions (`draw_claude_code.py`) until a local run replaces them. |
| `test_f10.py` | The checks against the fixtures, the scorecard's totals, the stand-in's rules, the project, and the tmux isolation. |

## The steps and checks

The ids are the scorecard's and every later report's: never renumber them.

| Step (weight) | Checks |
|---|---|
| S1 Open (H) | a. one command, no arguments, opens a conversation in this directory · b. a prompt to type into appears in 2 s or less · c. the first screen names the model and the directory · d. it says in one line how to get help or how to use it |
| S2 Ask "what does this project do?" (H) | a. the reply's markdown is rendered (headings, emphasis, code) · b. each tool call names what it touched (path or command) · c. something moves while it waits (spinner or elapsed time) · d. lines wrap at word boundaries at 80 columns |
| S3 Run the tests (H) | a. the command is readable before approving · b. approving takes one key · c. what failed can be seen without leaving the conversation · d. no id has to be copied |
| S4 Fix, with a reviewed edit (H) | a. the edit's diff shows before approving · b. it is coloured, with the changed words marked · c. the diff shows in the conversation afterwards · d. the file and the line numbers show |
| S5 Don't ask again (M) | a. the prompt offers "this command, for the rest of the session or project" · b. that grant covers the command's prefix, not the whole tool · c. one key switches to accepting file edits for the session |
| S6 A 90-second command (M) | a. elapsed time shows while it runs · b. its output so far can be seen · c. you can keep talking while it runs · d. its end is announced and the agent goes on by itself |
| S7 Interrupt (H) | a. one key stops the turn · b. the conversation stays open and asks what to do instead · c. work started earlier keeps running |
| S8 Paste a 60-line log with a question (H) | a. paste and question are one message · b. the paste is folded so the prompt stays readable · c. nothing is lost or reordered |
| S9 Undo (H) | a. the agent's edits since a chosen prompt can be reverted · b. the conversation can be taken back to that prompt · c. undoing the last edit takes 6 keys or fewer |
| S10 Context and cost (M) | a. how full the context is, as a share of the window · b. what the session has cost · c. what fills the context, by part |
| S11 Leave and come back (H) | a. leaving never stops work in flight · b. one short command reopens the last conversation in this directory · c. reopening shows the transcript · d. past conversations can be searched and picked |
| S12 Plan first (M) | a. a read-only planning mode one key away · b. the plan shows for approval before any edit · c. progress through the plan's steps shows |
| Beat | B1. a ready prompt 100 ms or less after the command · B2. every turn's cost shows without asking · B3. a stopped command is verified gone (its processes) · B4. work in flight survives a restart of the agent's own process · B5. the client stays under 20 MB resident |

## What each check reads

Each check is a function in `checks.py` whose docstring is its rule; this is the same list, short. A *screen* is a
`tmux capture-pane -e` of the pane, colours kept; a *record* is a fact the driver measured that no screen holds.

- **S1a** the open command is the binary alone, and a program other than the shell is left in front, taking input.
  **S1b** the driver's time from the command's Enter until the pane shows a prompt to type into; a command that
  returns to the shell has none. **S1c** the first screen holds the model's name (its id, or its family and version in
  words) and the project's path or last part. **S1d** a line names `--help`, `/help`, `? for help`, `help` or `usage`.
- **S2a** below the prompt no markdown marker is left raw (a line starting `#`, a `**`, a backtick) and some line
  is drawn bold, italic or underlined. **S2b** a project file's name is on the screen and no tool call shows as its
  name alone (a line that is only `fs.read`). **S2c** two captures a second apart while the reply is awaited differ.
  **S2d** no line filled to the last column ends in a letter with the next line starting with one in its first column
  (the terminal's own wrap through a word), and the reply reaches the edge at all; read at each size, the 80x24 run is
  the one the check names.
- **S3a** the question's screen shows `python3 -m unittest` as typed, not as JSON. **S3b** one key from the question
  to the approval, and the tests then ran (the module's bytecode appeared). **S3c** the tests' own failure output
  (`FAIL: test_tidal_range`, the `AssertionError`) is on the conversation's screen; a model's sentence about it does
  not count. **S3d** no id was typed to get here.
- **S4a** the question shows the removed and the added line as lines, not inside the call's JSON. **S4b** both are
  coloured, differently, and a line holds two or more colours (the changed words marked). **S4c** the same two lines
  after the edit. **S4d** `tides.py` and the change's line number (50) beside the diff or as `tides.py:50`.
- **S5a** the question offers "don't ask again" or "for this session/project". **S5b** that offer names `python3`.
  **S5c** one key, pressed once, and the screen says edits are accepted.
- **S6a** a time on a line that grows between two captures 3 s apart while the survey runs. **S6b** the survey's own
  `survey: station N/30` lines while it runs. **S6c** a message sent while it runs is answered while it still runs.
  **S6d** its end on the screen unasked, and an answer of the agent's after the end with no message from the person
  between (the session's record).
- **S7a** one key while a long turn runs, and the screen says it was interrupted, the held answer never shown.
  **S7b** after the key, the conversation (not the shell) is still in front. **S7c** the survey started before the
  long turn still runs (its process, found by its working directory and command line).
- **S8a** the session recorded one message holding the question and the log. **S8b** before sending, the input shows
  the paste folded and fewer than 10 of its lines. **S8c** the message holds the 60 lines whole and in order.
- **S9a** after the arm's undo, `tides.py`'s bytes are as before the chosen prompt's edit. **S9b** that prompt is
  offered again and its answer is gone. **S9c** the undo took six keys or fewer, and worked.
- **S10a** a percentage or `N/M` tokens on a line about context; **S10b** a dollar amount; **S10c** three or more parts
  (system, tools, messages, memory, free space) named with an amount.
- **S11a** the survey started before leaving runs after the client left. **S11b** a command of at most two words
  reopened this directory's last conversation and shows one of its messages. **S11c** two or more of the earlier
  prompts on the reopened screen. **S11d** a picker listed the conversation after a word of S8's question was typed.
- **S12a** one key (the same key, once or more) enters a mode the screen calls planning. **S12b** a question about the
  plan, with `tides.py` unchanged. **S12c** a checklist of two or more steps, one done or under way.
- **B1** the conversation's ready time, at most 100 ms: the open command's, else the command the driver opened the
  conversation with. **B2** a dollar amount on each of S2's, S3's and S4's after-screens. **B3** the arm's stop of the
  survey left no process of it, and the screen says the stop was verified. **B4** the survey ran on through a restart
  of the agent's own process (Theseus: `theseusd` stopped and started; Claude Code: its exit). **B5** the client's
  largest resident set sampled (the program in front of the pane's shell, not a daemon behind it) under 20 MB.

## How each arm is driven

Both arms get the same prompts (`steps.PROMPTS`) in the same order, on a fresh copy of `project/`.

- **Theseus** runs a scratch `theseusd` on a state dir of its own, with `theseus-sim fake-model --rules` as its model:
  each step's prompt asks for its tool calls and answers its text (`steps.stand_in_rules`), so the run costs $0 and
  every run sees the same model. Its config is the defaults with the stand-in, the scratch project as the workspace,
  Discord, the web UI, the index and the judge off, `[tools] proc_sync_secs = 5`, and `proc.run` and the file writers
  set to `approve`, the template's own advice for the shell, so the approval steps have a question. S1 types the bare
  `theseus`; where that opens no conversation, the driver opens one the documented way, `theseus ask "<S2's
  question>"`, and from S3 on drives the terminal UI, `theseus tui` (enter opens the session, `i` the input line, `y`
  answers a question). S7's interrupt is Ctrl-C, the key a person presses; B3's stop is the TUI's (`s`, `s`). A step
  with no key or command in the arm (S5's mode, S9's undo, S11's reopen and picker, S12's planning mode) records none.
- **Claude Code** runs the real model, **locally only, never in a cloud session**, its version pinned
  (`--expect-version`) and recorded. Its `--max-budget-usd` works only with `--print`, and S1 must open the bare
  command, so the driver caps the run itself at $1: a watchdog prices the session transcript's usage every two seconds
  at rates above any current model's and, past the cap, stops the client and every step after it.

## Running it

```bash
# Theseus, on a release build (the stand-in model: $0). tmux 3.x and python3 are all it needs.
scripts/build.sh --profile release-thin
python3 scripts/f10/driver.py theseus --bin-dir target/release-thin --out /tmp/f10-theseus

# Claude Code: on the owner's machine only, in a terminal of its own; about $1 a run at most.
python3 scripts/f10/driver.py claude-code --out /tmp/f10-claude --expect-version "$(claude --version | cut -d' ' -f1)"

# One scorecard over both arms' runs:
python3 scripts/f10/scorecard.py /tmp/f10-theseus/theseus-120x40 /tmp/f10-claude/claude-code-120x40 \
  /tmp/f10-theseus/theseus-80x24 /tmp/f10-claude/claude-code-80x24 --out /tmp/f10-scorecard

# The tests (no terminal needed, but the isolation test uses tmux when there is one):
python3 -m unittest discover -s scripts/f10 -p 'test_*.py'
```

A run takes about six minutes a size (S6's survey is 90 s, and S7 and S11 start it again). `--keep` keeps each run's
scratch directory (the daemon's log, the stand-in's, the state dir). Each run directory holds `run.json` (the arm,
size, version, model, and every record), `screens/<step>.<moment>.ansi`, and `driver.log`; `cat -v` or `less -R`
shows a screen with its colours.

## Writing a report from a run

A run is a benchmark run, so it gets its report here, named `<date>-f10-<what>.md`, from the scorecard:

1. Run both arms at both sizes, and the scorecard over the four runs. Keep the run directories with the report's
   data (the scorecard's JSON is its data file).
2. Answer first: each arm's match and beat totals per size, and what moved since the last report.
3. The table, then each check that differs between the arms or from the last report with its evidence line, read
   against its screen. Where a check's answer looks wrong, open its screen before believing either.
4. Name what each arm was: the commit and build of Theseus, Claude Code's version and model, and the cap's estimate
   of what the run spent (`run.json`'s `cost`).
5. Threats: the stand-in answers instantly and the real model does not, so a timing check (S2c, S6c) can differ for
   that reason alone; Claude Code's screens differ between versions, and a check written against one version's
   words may miss another's (read its evidence); the drawn fixture is not a recording.
