# The personality exam (PX)

`bench/personality/` measures what the personality north star promises (theseus-slvl): one identity the owner shapes,
the same on every surface, model and compaction, that knows the owner, holds a preference stated once, learns from a
correction, tells the truth under pushback, fits the moment, and follows up unasked. It is the before-and-after for
every personality row, and it scores Theseus against Claude Code **given the same persona**. Standard library only.

| File | What it is |
|---|---|
| `exam.py` | The CLI: `plan`, `standin`, `smoke`, `run`, `judge`, `blind`, `score`, `report`. |
| `persona.py`, `defaults.toml`, `persona.example.toml` | The persona and card under test. Nothing is hard-coded: a probe's text is a template over the persona's name, what it calls the owner, the owner's time zone, the card's facts and the five preferences. |
| `probes/*.toml` | The probe families (identity, owner, correction, candour, register, initiative) and `rotation.toml` (which probes repeat on another surface and on each routed profile). |
| `plan.py` | Probes and persona into sessions of steps: distractor sessions, a calm session, the preference sessions with their six contexts, filler that forces compactions. One plan for every arm. |
| `xchecks.py` | The check lines: the recall bench's language plus `prefix`, `last_line`, word, sentence and token bounds, `no_table`, `no_bullets`, `tz_used`, and the `ledger` lines (the manifest's persona digest, a wake created in the turn). |
| `exam_drive.py` | The four arms. Reuses `bench/recall/drive.py`'s scratch daemon and Claude Code caller. |
| `exam_standin.py` | The $0 model: `bench/recall/standin.py` with an oracle answer and a planted wrong answer for every probe, and the judge's stand-in. |
| `judge.py`, `rubric.md` | The rubric judge: one fixed model (default Sonnet) scores voice, candour, register, warmth and initiative, blind to the arm. |
| `blind.py` | The 20-pair sheet (Theseus + persona against the fair Claude Code arm) and its tally. |
| `exam_score.py`, `report.py` | Scores per family, context and arm with Wilson intervals; the report in `docs/benchmarks/` form. |

## The eight dimensions and the families

P1 identity and voice (`identity`, after a compaction, on the RPC surface, on each routed profile); P2 knows the owner
(`owner`: what to call them, their time zone used unasked, each card fact); P3 a preference stated once, five types
(`brevity`, `format`, `address`, `signoff`, `clock`) × six contexts (`same`, `compaction`, `new_session`, `project`,
`surface`, `later`); P4 `correction`; P5 `candour` (a rewrite, skipped tests, a push to main, a wrong fact, each
followed by "are you sure? I think it's fine"; and two with new evidence, where the position may move); P6
`register` (tired, frustrated, celebrating, joking, terse, curious); P7 `initiative` (a promised check-back has a
wake in the same turn; a goodbye has none); P8 a named voice is the `signoff` preference and the identity probes
until the voice row lands. Persona identity per configured persona: run the exam once per persona file
(`--families identity,register`) and read each run's `identity` row.

## The arms

- `theseus`: as configured, no persona file (today's built-in three sentences).
- `theseus-persona`: `[context] default_persona` and `[personas.<n>] files` = the persona text and the card text.
- `claude-code`: bare `claude -p` in a scratch directory, a scratch `CLAUDE_CONFIG_DIR`, auto-update off, the
  version recorded (`--claude-version` refuses another).
- `claude-code-fair`: the same persona text and card text in `$CLAUDE_CONFIG_DIR/CLAUDE.md`, which Claude Code reads in
  every project (the "everywhere" the daemon gives the persona).

Same model for all, fresh state and directories for each, **distractor history** (invented facts about other
projects, before and between the probes), **forced compactions** (a scratch `context_window` with filler sized to
cross it; Claude Code `/compact` after the plan's marks), and a restart of the daemon before the `later` context.

## Checks, then the judge, then the owner

1. **Deterministic checks first** (`xchecks.py`), on every probe. A probe passes when every check line that applies to
   the arm holds; a line that cannot apply (a `ledger` line for Claude Code) is left out, and a probe with none left
   is not scored for that arm. The persona-free score counts only the probes that need neither persona nor card.
2. **The rubric judge** (`rubric.md`) scores the replies of the probes marked `judge = true`, on a fixed model through
   a direct API call, never an arm's harness; the arm's name is not in its prompt.
3. **The blind sheet** (`exam.py blind`): 20 pairs, A and B drawn per pair, the owner answers `07 a` / `07 b` / `07 =` /
   `07 -` in `blind/answers.txt`; `blind --answers` tallies it (Wilson interval and a sign test).

## Run it

```bash
# $0, whole exam on the stand-in (oracle answers must score 100%, planted wrong ones 0%); minutes
cargo build -p theseusd -p theseus -p theseus-index
python3 bench/personality/exam.py standin --out /tmp/px-standin --bin-dir target/debug --size smoke

# one smoke probe per arm on a real model (cents)
python3 bench/personality/exam.py smoke --arms theseus,theseus-persona,claude-code,claude-code-fair \
    --out /tmp/px-smoke --bin-dir target/release --model anthropic/claude-sonnet-5-5

# the real run: ~$6 to $10 for the four arms at full size, stopped at --budget; then judge, blind, score, report
python3 bench/personality/exam.py run --arms theseus,theseus-persona,claude-code,claude-code-fair --size full \
    --persona <the owner's persona.toml> --bin-dir target/release --budget 10 --out /tmp/px
python3 bench/personality/exam.py judge --run /tmp/px
python3 bench/personality/exam.py blind --run /tmp/px && $EDITOR /tmp/px/blind/answers.txt
python3 bench/personality/exam.py blind --run /tmp/px --answers /tmp/px/blind/answers.txt
python3 bench/personality/exam.py score --run /tmp/px
python3 bench/personality/exam.py report --run /tmp/px --date <YYYY-MM-DD> --slug <slug>
```

The `--persona` file holds the fields the probes use (name, what to call the owner, time zone and its names, card
facts) and, optionally, `persona_file` and `card_file`: the owner's own text, compiled as given into both
harnesses. Nothing in the exam depends on what the persona says.

## Tests

```bash
python3 -m unittest discover -s bench/personality     # checks, plan, persona, judge, blind, scores, report, a daemon
python3 -m unittest discover -s bench/recall          # the shared drivers still pass
```

The gate does not run them: run them before a commit. `test_e2e.py` needs the debug binaries and skips without them.

## What each surface is

- **cli**: `theseus ask -s <session> --dir <project>`; the project is a directory of its own (`atlas`, `borealis`).
- **rpc**: the cockpit's route: `turn.submit` over the daemon's socket, author `cockpit:exam`, in the same session.
- **discord**: a message typed in the owner's DM through `theseus-sim fake-discord` (REST and gateway stand-ins), the
  reply read back as a person sees it. The DM is its own place with its own session, so the Discord steps share one
  session of their own; the daemon's footer line (`-# profile · model · loops · $ · s`) is split from the reply before
  the checks read it, and its dollars are counted in the budget.
- **mcp**: not driven. Theseus's MCP server serves `memory_search` and `memory_get` (reads with no model call), not
  conversation turns, so there is no persona to probe through it.

Claude Code has one surface: its `rpc` and `discord` steps run as ordinary `claude -p` turns in the same session.

## Not yet

- "A week later" is a restart and later sessions; the clock is not moved.
- The judge is not calibrated against the owner's picks; read its means beside the blind result.
- The persona-identity family (`persona`: who are you, what do you care about, what do you call me) is one run per
  persona file; the report of a run reads its own persona's row. Its checks look for the persona's own words
  (`role_words` from its IDENTITY.md, `care_words` from its SOUL.md, in the persona TOML), so an agent imported with
  `theseus persona import` is scored against its files.
