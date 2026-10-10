# Incidental recall

A benchmark of what an agent remembers from its own work (theseus-7gir.17). One scripted, realistic progression of
work is replayed identically to each arm: sessions of turns that run scripts, read files and change topic, with
facts seeded at known points, some said once in passing and some the topic of the moment. Probes come at set
distances after each fact and ask for it directly, need it silently, or ask after something never said. Each arm
uses its own memory, and the scorer draws each arm's recall curve and its half-life.

theseus-exam's memory exam (`crates/theseus-exam`) measures Theseus alone, from written stores, with no salience,
no distance and no indirect probe. This bench drives each arm live, through its own CLI, so the arms compare.

| File | What it is |
|---|---|
| `progression.py` | The format: sessions, turns, facts, probes, the workspace; the buckets and the validator. |
| `checks.py` | The check language, as theseus-exam's `check.rs` has it (`reply has word "…"`, `file "…" has …`). |
| `generate.py` | The generator: a progression from a seed, and the budget. |
| `drive.py` | The drivers: a progression replayed to Theseus, to Claude Code, or to Pi, each turn kept. |
| `score.py` | The scorer: every probe checked, each arm's curve, half-life, and the report. |
| `tokens.py` | Theseus's request estimate, mirrored: the census, the rates, the margin, the budget, the ring. |
| `standin.py` | A stand-in model that counts each request as the compiler estimates it, for the offline smoke. |
| `test_*.py` | Their tests. |

Standard library only, everywhere.

## The format

A progression (`progression.json`, format `recall-progression-v1`) is:

- **sessions**: a label and a date. Each session's first turn opens with its date ("It's Monday, 2026-11-09.").
- **turns**: the user's text, each in a session and a topic block, with the workspace edits the driver makes
  before sending it (`before`) and the compiler's estimate of its messages (`est_tokens`: its text, its work's
  call and result, a reply). A turn with `mark` is a compaction mark: a long log read where the script expects
  the arm's context to fill. The turn after a mark holds no fact and no probe; where the mark's one read can't
  cross the budget, it reads more logs (role `bulk`, no mark).
- **facts**: an id, a subject ("the alder relay's port"), a kind (`port`, `path`, `version`, `host`, `ticket`,
  `date`), its value, its salience, its family, the turn that states it, its source (the script that shows it, or
  `said`), its `marker` (the text whose presence in the arm's transcript proves it arrived), and what it
  supersedes.
- **probes**: the fact, the kind, the planned distance bucket, the turn that asks it, and the check.
- **workspace**: the files every arm starts from, byte for byte the same.
- **overhead_tokens**: the system prompt and tools, in tokens, that the window and the bulks were planned at: the
  daemon's own, measured at run time (see "The overhead is measured"), or a number given (`--overhead`). No constant
  stands for it. A file from before it was recorded has none and keeps its digest (the key is written only when
  set), but it has no plan to hold a daemon to: the driver asks for it to be generated again.

**Salience.** *Incidental*: said once in passing. That can be a line in a script's output ("run
./scripts/check-alder-relay.sh: did the build pass?" prints the port among the build lines), a path in an error
message, a preference in an aside, or a decision in a side remark ("Side note: Maren decided in standup that…").
*Central*: the topic of the moment ("Today's main thing is the alder relay: it moves to port 23817."). A block's
central facts are about its topic; its incidental facts are about something else.

**The world moves on.** A script that showed an incidental value is rewritten before the next turn (the log
rotated, the error fixed), so a later probe can't be answered by running it again. The arm has to remember it.

**Families**, theseus-exam's for their kinds and checks: `tool_output` (the value is in a command's output or
error), `superseded` (a value a later fact replaces, never probed itself), `time` (when something happened: the
value is its session's date), `distractor` (a same-kind value near an abstention probe, never probed itself), and
`needs_nothing` (an abstention's subject: nothing to recall). One family is added: `said`, the user's own words (an
aside, a side remark, the topic).

**Probes.**
- *direct*: "Quick question: which port is the alder relay on?"
- *indirect*: a task that silently needs the fact: "Write the URL of the alder relay's health check on this machine
  (http, path /healthz) into notes/p007.txt." It is scored by the file the task writes, never by the arm's words.
- *abstention*: a subject never stated, asked near a distractor that shares a word with it ("the cedar relay's
  port", near the alder relay's). The right answer gives no value of that kind and says it doesn't know.

**Buckets**, nearest first: `near` (a few turns on, in the same block), `topic_shift` (a later block),
`compaction` (a compaction between them), `session` (a later session the same day), `days` (a later session on a
later date), and `supersession` (a probe of a value that replaced another: the new value is right, and the old one
must be absent). The generator plans buckets by the marks. The scorer measures each arm's buckets by where that
arm actually compacted. A probe planned past a mark that its arm never compacted at is a `topic_shift` probe for
that arm.

**Each fact is probed once.** A second probe would remind the arm. The validator refuses a progression that probes
a fact twice, asks a probe before its fact, says a value anywhere but its own turn, or says an abstention's subject
anywhere at all.

## The commands

```bash
# 1. Each arm (an Anthropic key in ANTHROPIC_API_KEY; release binaries for Theseus). The Theseus arm measures its
#    daemon's system prompt and tools first, then plans the progression at that and runs it; its run directory holds
#    the progression, which the other arms read, and Pi measures its own overhead the same way.
python3 bench/recall/drive.py --arm theseus --memory-arm baseline --bin-dir target/release \
  --model anthropic/claude-sonnet-5-5 --seed 7 --size smoke --out /tmp/rc-th
python3 bench/recall/drive.py --arm claude-code \
  --model anthropic/claude-sonnet-5-5 --progression /tmp/rc-th --out /tmp/rc-cc
python3 bench/recall/drive.py --arm pi \
  --model anthropic/claude-sonnet-5-5 --progression /tmp/rc-th --out /tmp/rc-pi

# 2. A progression on its own, and its budget, at an overhead you give (a pinned plan; `--overhead` on the
#    driver does the same for a run that must match an earlier one).
python3 bench/recall/generate.py --seed 7 --size smoke --overhead 13943 --out /tmp/rc-smoke

# 3. The scores (and with `--stale retracted`, an old value named only to take it back is no longer stale).
python3 bench/recall/score.py /tmp/rc-th /tmp/rc-cc /tmp/rc-pi --out /tmp/rc-report
```

**Sizes.** `smoke` is two sessions of 15 turns, three days apart, with one compaction mark and eight probes, at
least one of each kind. `full` is three sessions of 200 turns: two on one day and the third nine days later, ten
topic blocks each, a mark in each session, and 6 probes in each of the 34 bucket × salience × kind cells (204 in
all; an abstention has no supersession). The generator prints the counts and an estimate of the tokens and dollars
per arm at the catalog's price (`--model`), each mark's reads with their bounds, and the window and its budget.
For seed 7, planned at 13,943, it says about $0.54 an arm for the smoke and about $12.02 for the full. Treat these as
a budget, not a measurement: the replies' length and the turns' recall notes are guesses.

**Run live only in a throwaway container or VM.** Neither arm's tools are confined to the workspace on a bare host.
Theseus runs the bench profile (every tool open, roots at `/`), and Claude Code runs its shell tool.

## Each arm's memory

- **Theseus**: a scratch `theseusd`, with `theseus-index` beside it, configured as theseus-exam's `daemon.rs`
  configures an arm's daemon. That means the bench profile (`bench/theseus-bench.toml`, its key
  `env:ANTHROPIC_API_KEY`), `[memory] mode = "live"` with `arm` from `--memory-arm`, Discord, the web UI and the
  MCP server off, and the index on (off for `none`, as `daemon.rs` does). The arm is any string: `none`, `bm25`,
  `baseline` today, and `+synthesis`, `+retention`, `+activation` once they land. So each memory arm is a sub-arm,
  one run each. The arm is config, never a turn's field.
  - Each progression session is one `sessions open`, and each turn is one `theseus --json ask -s <id> -`. A CLI
    session has no target, so it is private, and recall draws on every earlier session.
  - Its memory is its recall pass and its compaction's roots. A scratch `[catalog."<model>"] context_window`
    brings compaction near the marks. The generator sizes the window and the bulk reads by the compiler's rule,
    mirrored in `tokens.py` (each constant names its Rust source, and a test reads it there): a request may hold
    the window less the output cap and 4,096 tokens (the driver sets the cap to a quarter of the window, at most
    16000); a tool result is JSON at 2.4 bytes a token and text 3.3, with 3 tokens a message, 1 a block and 15 a
    tool id; from a turn's second call on, the provider's count of the last request stands and only what was
    written since is estimated; the ring runs when that passes the budget at its upper bound (the estimate × 1.4),
    and a turn whose newest exchange alone, estimated whole beside the system prompt and tools (the overhead, measured at
    run time: 13,943 tokens on main of 2026-10-09, where it was 13,599 a month before), still passes it fails before any call. So the window is the smallest where the
    turns before each mark fit at 15% over their estimate, a session with no mark fits whole, each read turn alone
    stays 10% under the budget with room for a 4,096-token summary beside it, and at 15% under their estimate the
    reads cross the budget: the mark's own, or, where one read can't do both (the smoke's short first session),
    up to three more at the turn after the mark. One tool result shows at most 30,000 characters, and `fs_read`
    numbers each line, so each log fits under that. A plan is in tokens and its logs are written in bytes, so each
    plan, once written, is worked again from its bytes (`plan_misses`), at its overhead and at `OVERHEAD_CUSHION`
    more and less, and one that misses by rounding gives way to the next: more reads in the same window, then the next
    window. For seed 7, planned at 13,943, that is 46000 for the smoke (a budget of 30,404;
    two logs of about 4,650 tokens, the second at turn 11) and 94000 for the full (73,904).
  - **The overhead is measured, never assumed** (theseus-cs8k). Every new tool definition moves the daemon's system
    prompt and tools, so no constant can stand for them. Before the progression is fixed, the driver starts a
    throwaway daemon of the run's own config (the same binaries, profile, memory arm and effort), has it take one
    short exchange in a session of its own, and reads its first `context.compiled` estimate less that message: the
    system prompt and tools, as `overhead_of` has them. It is a daemon of its own, so nothing of the probe is in
    the run's memory. On a stand-in model it costs nothing; on a real one it is one small call that writes the
    prompt's cache (about 14,000 input tokens at $2.50 a million, about $0.04; `run.json`'s `overhead.probe_cost_usd`
    says). The progression is then generated at that measure (`--seed` and `--size`), its written bounds checked
    there and at `OVERHEAD_CUSHION` (50 tokens) more and less (`plan_misses`), and run. The first real turn is read
    again (`first_turn`) and held to the plan the same way. `run.json`'s `overhead` keeps `planned`, `measured`
    (the probe's), `first_turn`, `pinned` and `past_cushion`; they are equal unless a plan was pinned, and `score.py`'s
    report prints them for every arm ("System prompt and tools"), so the overhead's growth stays visible.
    A **pinned plan** (`--overhead N` with `--seed`, or a `--progression` file, whose recorded overhead is its plan)
    is for a rerun that must match an earlier one: a daemon measured more than the cushion off it, over (a mark may
    ring) or under (the reads may not cross the budget), stops the run before its first turn (exit 3) with both
    numbers, unless `--allow-overhead`.
  - Where it compacted is read from the ledger (`context.compacted`) after every turn, with each row's outcome:
    `compaction`, a summary in the cut's place, or `ring`, the cut kept with no summary and why (the summary would
    not fit, or its call failed), and the cut's span (its message count, its first and last positions).
  - The run keeps each session's `history --full` and `memory recalled`. The daemon is stopped cleanly at the end.
    The driver fails if anything is still running that names the run's directory or works inside it (a job
    included).
  - A turn past `--turn-timeout` (900 s) has its client's process group killed and is recorded as timed out. The
    turn goes on in the daemon without its client, so the driver runs `theseus stop <session>`, and the next turn
    starts clean.
- **Claude Code**: `claude -p --output-format json` with a scratch `CLAUDE_CONFIG_DIR` and the workspace as its
  working directory.
  - Each session boundary starts a new session (`--session-id`), and each later turn uses `--resume <id>`.
  - Its tools are `Bash`, `Read`, `Write`, `Edit`, `Glob` and `Grep` (`--tools` and `--allowedTools`), which do
    Theseus's same work, with `--permission-prompts none`.
  - Its memory is its compaction and its memory files (`CLAUDE.md`, its auto-memory). The run keeps both, and its
    session logs.
  - Compaction: at a window of 100k or more (what `--autocompact` takes), `--autocompact` at the progression's
    window. Below that (the smoke, and the full at seed 7's 94k), `/compact` is sent through `-p --resume` after each mark's
    turn (`--cc-compact marks`). Print mode does run it (Claude Code 2.1.289): the session log gains a
    `compact_boundary`, and the session keeps its id. Whether it worked is checked, not assumed: a compaction
    counts only where the log shows a `compact_boundary`, and the run keeps each `/compact`'s result.
  - Variables a parent Claude Code session sets (`CLAUDECODE`, …) are taken out of its environment. A turn past
    `--turn-timeout` has its process group killed (claude and what its shell started).
- **Pi** (theseus-jp9p): `pi --print --mode json` (`@earendil-works/pi-coding-agent`, 1.0.4 as the Harbor arm
  pins it; `--pi` names the binary) with a scratch `PI_CODING_AGENT_DIR` and session directory, and the workspace
  as its working directory.
  - Each session boundary starts a new session id, and every turn of the session passes it (`--session-id`
    opens the session, or creates it). The turn's text goes on stdin.
  - Its tools are its four, `read`, `bash`, `edit` and `write` (`--tools`). It has no permission prompts.
  - Its memory is its compaction and the context files it reads (`AGENTS.md`, `CLAUDE.md`); the run keeps both,
    and its session logs.
  - Compaction: its own threshold, where the progression's context crosses its window. Pi compacts when the context
    passes its model's window less `reserveTokens`. The progression is planned at Theseus's overhead
    (`overhead_tokens`, 13,943 on main of 2026-10-09), Pi's own system prompt and tools are about 2,500 tokens
    (2,475 in the one live reading, a Harbor trial of Pi 1.0.4 on Sonnet 5.5), and one progression is read by every
    arm, so a Pi at the plain window would run about 11,500 tokens short at every turn and never compact on the
    smoke. The scratch `settings.json` sets the run model's `reserveTokens` (`compaction.modelOverrides`) to the
    model's window in Pi's catalog (`--pi-model-window`, 1,000,000 for Sonnet 5.5) less the threshold, and the
    threshold is the progression's window less what the plan's overhead holds beyond Pi's (`pi_threshold`). Seed 12's
    smoke, planned at 13,943, has a window of 49,000; Pi measured at 2,475 gets 49,000 - (13,943 - 2,475) = 37,532. Pi then reads the same bytes as the other arms and crosses where the plan does.
    `keepRecentTokens`, what a compaction keeps unsummarized, is Pi's own 20,000 or a quarter of the threshold
    (`pi_keep_recent`), the context Pi holds when it compacts: when the whole context is within it, Pi 1.0.4 has
    nothing to summarize and skips the compaction. `run.json`'s `pi_compact` keeps the window, the model's window,
    the threshold and the two. Pi's print mode sends `/compact` to the model as text, so there are no marks to
    compact at; the threshold works at any window. A compaction counts where a session log gains a `compaction`
    entry, and its summary call's tokens and dollars are its turn's.
  - Pi's overhead is measured at run time, as Theseus's is, and its threshold is set from the measure (theseus-iec1;
    no constant stands for it: the one that did was off by 242 on the first live reading, nearly five cushions). The
    threshold is written into Pi's settings before its first turn, so the measure comes first: a probe turn in a
    throwaway Pi session (its own agent and session directories, in the run's workspace, so nothing of it is in the
    run's logs, compaction or memory), and the first answer's input (input, cache read and cache write) less the
    probe's words at the model's rates. It is the provider's count on a real model and `standin.py`'s on the rig. On
    a real model it costs one small exchange (about 2,500 input tokens: a cent). `run.json`'s `overhead` has the
    Theseus record's shape (planned, measured, `first_turn`, `pinned`, cushion, `past_cushion`, `allowed`) and the
    `threshold` it was set at. `--pi-overhead N` pins the plan (a rerun that must match an earlier one): a probe
    finding Pi more than `generate.OVERHEAD_CUSHION` off it, over or under, stops the run before its first turn,
    exits 3 and names both numbers, and `--allow-overhead` (both drivers read it) runs on. Pi's first real turn is
    read again and held to the plan too. Pi has no daemon to measure the progression's own overhead: it reads the
    progression the Theseus arm planned (`--progression <its run directory>`).
  - Offline: `PI_OFFLINE=1`, beside `PI_SKIP_VERSION_CHECK` and `PI_TELEMETRY`. Without it Pi 1.0.4 overlays newer
    model-catalog data from its project's server (prices, its thinking map, compat flags), so the pinned version
    would not pin them. Its docs: "`PI_OFFLINE`: Disable automatic network activity, including model catalog
    refreshes" (docs/environment-variables.md); the code reads it in one place, the model runtime's catalog
    refresh, and a model call is not automatic network activity.
  - Pi's print mode exits 0 when the provider fails: a turn whose last answer ended on `error` or `aborted`, or
    that answered nothing, is recorded as failed (`pi_failed`). Pi's process markers and its session's variables
    (`AI_AGENT`, `PI_SESSION_ID`, …) are taken out of its environment, and a turn past `--turn-timeout` has its
    process group killed. With `--api-base`, its scratch `models.json` points the provider at a stand-in.
- **OpenClaw**, a fourth arm, is not built yet. Its memory would be its memory search and its wiki. A driver is a
  class with `drive()`, writing the same run directory, and the scorer reads any arm's.

**Effort.** Every arm runs at one reasoning effort, `--effort` (default `medium`; `low`, `high`, `xhigh` and `max`
too), and `run.json` names it. Theseus on Sonnet 5.5 sends no effort unless its profile sets one, and the model's
default is then high, where Claude Code and Pi send medium by their own defaults: the driver sets the scratch
config's `profiles.bench.effort` (a Harbor run's level is its own, in `bench/theseus-bench.toml`), passes Claude Code
`--effort` and Pi `--thinking`.

**Days are dated text.** No arm's CLI takes a clock, so each session opens with its date. What this measures is
recall of what was said on a dated day, across the arm's session boundaries and compactions. It can't measure the
effect of time itself. Theseus and Claude Code see the real date in their system prompts too (Pi 1.0.4's has
none), and a memory that weighs elapsed time (retention) sees the minutes the run took, not the script's days.

**Delivery.** Every fact's marker is looked for in the arm's transcript (Theseus's history, Claude Code's and Pi's
session logs). A probe whose fact never reached the arm (it didn't run the script, say) is excluded and counted, never
scored as a miss.

## The run directory

`run.json` holds the arm, the memory arm, the model, the progression's digest, the compactions (the turns whose
request was compacted), Theseus's `compaction_rows` (each turn's outcomes and cuts), what the stop had to kill, and the `overhead` of Theseus and Pi (planned, measured, first turn, pinned). `turns.jsonl` holds each turn's reply, exit, tokens, dollars
and latency. `delivered.json` holds each fact's delivery. `progression.json` is a copy of the progression, and
`workspace/` is the arm's workspace as the run left it. `raw/` holds each turn's stdout, the transcripts and the
daemon's log.

## Each score

- **Correct**: the probe's check holds. That uses the check language: the value present (a whole word; a date in
  the forms people write it), a superseded value absent, and for an abstention, no value of the kind and an
  admission. An indirect probe's check reads its file in the arm's workspace.
- **An admission** says so in plain words: "I don't know", "I can't tell", "no record", "no matches", "a search
  found no …", "nothing there pins (says, shows) …", "I'm not able to find", "unable to tell" (`ADMIT` in
  `progression.py`). The scorer derives an abstention's check from its kind each time it scores, so a run kept from
  before a change to the rule is scored by today's; a direct or indirect probe keeps its stored check.
- **The curve**: recall accuracy (direct and indirect) per arm by bucket and salience, with the mean turns and
  tokens since the fact. Each turn's new tokens are its input, cache writes and output. Abstention accuracy has a
  table of its own.
- **The half-life**: where accuracy falls to half its nearest bucket's. It is interpolated linearly between the
  two buckets around the crossing, in turns and in tokens. It is `not reached` if accuracy never falls that far,
  and `undefined` when the nearest bucket recalls nothing.
- **Confident-wrong**: a wrong answer that gives a value of the asked kind (for an abstention, any value) with no
  hedge.
- **Stale**: a superseded value given. By default (`--stale strict`) that is so even as history: "It moved from
  27340 to 38013" is stale and wrong, because the check wants the old value absent, as theseus-exam's superseded
  items do. With `--stale retracted`, a reply is right, never stale or confident-wrong, and counted apart under
  *Old named*, when every place it names the old value a retracting phrase governs it, and it states the new value
  at least once where none does. A phrase governs the one value of the asked kind nearest it on its side, at most
  four words off, in the same clause (a semicolon, a dash or a sentence stop ends it): before the value ("moved
  from X", "ignore the X", "previously X", "formerly X", "used to be X", "instead of X", "replaced X", "no longer
  X", "not X", the last at most one word off), after it with the value its subject ("X is no longer used", "X was
  replaced", "X isn't used anymore"), or around it ("it was X before", "earlier it was X", "before the move it was
  X"). A phrase governs each member of a list under it ("no longer X or Y", "used to be X and later Y", "X and Y are
  both retired"), and an old value named twice in a clause ("moved from X to Y, then from Y to Z") is retracted
  where its last naming is, an earlier one only a move's destination ("to Y") or governed too: named again after
  its retraction ("moved from X to Y, then back to X", "it was X before and is still X"), it is current again. A
  lead word reaches "was" four words off ("before the move it was X"). "The old X" and "the former X" only
  describe: they govern beside a retraction word of the
  clause that is about X, with no other value nearer it ("the old X was replaced", "the old port X, the one we no
  longer use, is closed"), never alone ("the old X is back") and never beside a retraction of another value ("the
  old port X is what answers, since Y was dropped"). A bare "instead" is no retraction ("use the old port X
  instead"); "instead of X" is. A phrase that cites ("as I said previously", `you wrote "moved from X to Y"`) or is
  negated ("don't forget", "X is no longer wrong", "it's no longer wrong to use X") governs nothing. So "It moved from 27340 to 38013." is right, and
  "It's 27340, previously 38013.", "It was 38013 before, now it's 27340.", "Port 27340 replaced 38013." and
  "38013 is no longer used; it's 27340." are wrong under both rules: each gives the old value as the current one.
  "The archiver is on port 27340." is stale under both, and "It's 27340, or maybe 38013." wrong under both. The report
  says which rule it scored by. **The rule's reach**: it reads phrases, not meaning, so it can score a reply as a
  careful reader would not. A move cited with no citing verb ("the note says moved from X to Y, which I can't confirm") reads as a
  retraction, so it is right; a retraction in words it doesn't list ("X was rolled back") is not one; a list joined by
  words `JOIN` doesn't hold ("X as well as Y") governs only its nearest member; and a phrase in another clause or
  sentence governs nothing here. A list member that a verb follows at once
  starts a clause and takes no phrase from before the list ("ignore Z, X is the live port, and Y is only planned":
  X is current). The three false rights this once left (`the old X` beside a retraction of another value, a negation
  ahead of a prefix, and a comma read as a list across clauses: theseus-hau2) are closed. Strict scoring is
  unaffected: each of those replies names the old value, so it is stale there, as before.
- **Cites**: of the right direct answers, those that say where (the script, or that the user said it) and when
  (its date or weekday, or a relative time).
- **Cost and latency** per probe: its turn's dollars and wall time.
- Also: accuracy by probe kind, by how the fact was said (`output`, `error`, `aside`, `remark`, `topic`), and by
  value kind, and how many probes each arm's compactions moved from their planned bucket.
- **Compactions by outcome**: each arm's `compaction` and `ring` rows, counted apart, and where each fell. Both
  move a probe: a ring drops the same leading turns as a summary does, only with nothing in their place, so a fact
  before it is as far from the arm's request either way (`MOVING_OUTCOMES` in `score.py`). Claude Code's
  `compact_boundary` and a run kept from before the outcomes count as `compaction`.

`score.py` writes `report.md`, `curve.svg` (drawn by hand: incidental solid, central dashed) and `scores.json`.

Every run of it gets its report in [`docs/benchmarks/`](../../docs/benchmarks/README.md): `python3 bench/report/draft.py recall --scores <out>/scores.json --date <day>` drafts it from the scorer's output ([`bench/README.md`](../README.md), "Every run gets its report").

## Tests

```bash
python3 -m unittest discover -s bench/recall
```

They cover:
- **The generator**: determinism (the smoke's digest pinned, the full the same twice, another seed different),
  stratification (every cell filled, each fact before its probe and probed once), and no generated name or value
  in theseus-exam's `exam-v2.toml`. They also check the prices against `catalog.rs`, `tokens.py`'s constants
  against their Rust sources, and both bounds of every mark, for the smoke and the full at three seeds.
- **The scorer**, against numbers worked by hand: a known curve's half-life, confident-wrong, stale, citations, an
  abstention, and distance measured where the arm compacted.
- **The Claude Code driver**, against a stand-in `claude` on PATH: session ids carried, a boundary opening a new
  one, `/compact` after the mark.
- **The Pi driver**, against a stand-in `pi` on PATH: a probe turn in a session of its own, a session id per
  session, the reserve and the kept tokens at the threshold the measure sets (a 2,475 reading plans at 2,475), the
  offline variables, its overhead recorded, a pinned plan and a Pi 51 off it refused before any turn (50 off not, and
  `--allow-overhead` runs on), a compaction read from its log, a parent's variables taken out, each turn's answers and
  summaries summed, and a provider's error a failed turn.
- **The Theseus driver**, end to end on this workspace's binaries (`target/debug`, or `THESEUS_RECALL_BIN_DIR`):
  the smoke planned at the overhead it measures (seed 12), a pinned plan refused before any turn, the smoke on `standin.py`, and a turn past its timeout stopped while the next one runs, on `theseus-sim
  fake-model --rules`. It is skipped when they are missing, and nothing is left running. Theseus trusts the
  provider's count, and theseus-sim's stand-in reports 40 input tokens a call: on it, a turn's history costs
  nothing, and a mark that fails live (an overage) passes. `standin.py` reports each request's estimate by the
  compiler's rule, as a provider's count would come, and does each turn's work as a model would (`fs_read` of a
  whole log); on it the smoke compacts at the mark or the turn after, every turn exits 0, and a perfect arm scores
  perfectly.

The real `claude` runs against the same stand-in too: `ANTHROPIC_BASE_URL=http://127.0.0.1:<port>
ANTHROPIC_API_KEY=stand-in` with `drive.py --arm claude-code`, and rules that name its tools (`Bash`). That checks its
flags, its sessions, its logs and a `/compact` in print mode. But the stand-in asks again for a tool call that some
of its requests have already answered, so a few turns run to their timeout. That makes it a check by hand, not a
test.

The real `pi` (1.0.4) runs against `standin.py` too: `drive.py --arm pi --pi <its path> --api-base <the
stand-in's address>`, with rules that name its tools (`bash` with a `command`, `read` with a `path`). On the
smoke, every turn exits 0 and every fact is delivered; with `--context-window 10000` it compacts at turn 11
(10,323 tokens before), and the driver records it. It is a check by hand, not a test.

The gate doesn't run them. Run them before each commit that touches this directory.
