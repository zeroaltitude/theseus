# The view's measuring kit

The context design (`docs/design/context-point-of-view.md`, §10) says nothing about the view is to be trusted
before it is measured. These three tools answer three of §10's questions, and the end-to-end proof (below) a
fourth. Each runs on a **copy** of a store, or on an empty store it fills by script, never on the live one, and writes **ids, kinds and counts only**: no message, no piece's text, no place's name,
and no daemon diagnostics. `scripts/tests/test_view_kit.py` holds that on daemon answers, errors and stderr full
of marker strings, and `scripts/tests/test_view_real.py` runs each tool on the real daemon and CLI, whose stamps
name this checkout's commit (what that does not prove is below), on an empty store.

There is one context, the view, since the cut: the tools set it beside what a turn recorded, beside a plain
baseline (every earlier line of the message's own place), or beside a judge. None of them compares two builds.

| Tool | What it answers | What it costs |
|---|---|---|
| `replay.py` | For a place's last N exchanges: the context recorded as sent, beside what that turn's own view record laid. | Nothing: no model is called. |
| `cases.py` | Does a context hold what a good one must? A frozen set of labelled cases, the scorer, and a builder that makes each case's store by script. | Nothing: the builder's model is a local stand-in. |
| `yardstick.py` | What does the view miss that a judge would find? A judge reads every earlier exchange in overlapping windows; its answers are set beside what each turn's view laid. | A model call per window, under a cap that is a bound. Refused without `--cap-usd`. |

`scratch.py` is what they share: a scratch `theseusd` on a copy of the copy it is given.

- It refuses `~/.theseus`, `$THESEUS_STATE_DIR`, anything inside or above either, a directory a running process
  names, a directory with no `store/wal`, and a directory holding any link that leads out of it.
- It refuses an output directory (`--out`, its links resolved) inside or above a live root or the input, before
  anything is written.
- The copy is made with links followed, so it holds none, in `<out>/daemon/` (mode 0700).
- The daemon and the CLI get a minimal environment: the path and the locale, a home inside `<out>/daemon/`, and
  no vault token, token file or provider key of yours.
- Every provider points at an address nothing listens on, the spend limit is a cent, and Discord, the web UI,
  the MCP server and the index are off.
- Its own words are the owner's: the daemon classes a CLI message as an agent's when an agent host is above the
  process that sent it, or when that process has no controlling terminal. The scratch daemon's settings name no
  agent host (`[policy] agent_processes = []`), and the case builder sends each line from a terminal of the kit's
  own (a pty), so a synthetic line is marked the owner's under an agent, a service or a cron alike. Without that,
  his words in another place (slot F) hold nothing and every `callback_elsewhere` case is missed for that reason
  alone. The daemon's rule is not changed.
- The daemon's stderr is discarded; a failure is a fixed code (`E_CLI`, `E_LEDGER`, `E_HISTORY`,
  `E_DAEMON_EXIT`, `E_DAEMON_TIMEOUT`, `E_JUDGE_…`).
- On every end it stops the daemon and removes the copy, and that includes a failed start, an error and
  SIGTERM. SIGTERM is trapped from before the copy starts until the removal has finished; one that arrives
  during the removal waits for it. A removal that fails is printed with its path.
- The teardown runs to its end whatever one step raises: a shutdown that fails or hangs is followed by a kill
  of the daemon's own process group, then the removal, then the prior SIGTERM handler is restored.

**What a SIGKILL leaves.** The first line every run prints (on stderr) is its scratch directory's absolute path:

```
view kit: scratch directory /tmp/replay/daemon (its store copy, state/, is removed at the end of the run; the directory itself stays)
```

At a normal end the kit removes the store copy (`state/`) and leaves the rest of that directory (its config, an
empty home), which holds nothing of the store. Killing the kit with SIGKILL (or a crash of the machine) skips its
cleanup: the store copy stays too, and the scratch daemon may still run. The kit has no verb that kills or deletes
on its own. Remove it by hand:

```bash
d=/tmp/replay/daemon                       # the path the run printed, exactly
pgrep -af -- "$d" || echo "nothing runs on it"
# if a theseusd is listed with --state-dir "$d/state", stop that one process: kill <its pid>
# then CONFIRM it has exited before deleting anything: this must print nothing
pgrep -af -- "$d"
rm -rf -- "$d"
```

A teardown that could not kill or reap its daemon says so as `E_CLEANUP <scratch directory> pid <pid>`: check
that pid and that directory the same way.

The kit needs Python 3.11 or newer (`tomllib`).

## The copy

Copy a daemon's state directory while that daemon is stopped, or from a backup, into a directory of your own,
following links (the kit refuses a copy with a link that leads out of it):

```bash
cp -aL ~/.theseus /tmp/state-copy        # with the daemon stopped
```

The kit copies it again before it starts a daemon on it, so the copy you made is left as it was. Delete it when
you are done: it holds everything the store holds.

## The paired replay

```bash
python3 bench/view/replay.py --bin-dir target/release --store /tmp/state-copy \
  --place 'local:/path/of/the/place' --turns 50 --out /tmp/replay
```

The place is named as the ledger names it (`theseus ledger exchanges` lists the places). For each of the place's
last N exchanges, `rows.jsonl` holds one line with two sides:

- **the recorded side**, read from the measuring row the turn wrote (`context.turn`): the sent context's parts in
  tokens, and whether the previous exchange was in it;
- **the view's side**, from the turn's own view record (`theseus view show <turn> --tally --json`, never
  `--text`): its first RETURNED readable view (`view_basis` in the summary), its tokens by slot, what the budget
  left out and why, its pieces by kind, and whether it held the previous exchange. That answer is read from the
  ranges its pieces lay, interiors included, against each place's exchange order: `ids` when an `exchanges` piece
  lays it, `folded` when only a `folds` piece covers it (a fold's first and last are exchange ids); a range no
  place's order resolves makes it unknown, not "absent".

**Which view, and what a record is.** `view show` reads a turn's newest 32 views and returns the readable ones
among them, in the order they were built; one that cannot be read is left out. So the first view returned is the
turn's first only when the turn had no more than 32 and its first reads: a turn compiled 33 times answers its
second first. How many came back does not tell which: fewer than 32 returned does not prove that nothing was
cut (a turn compiled 34 times, two of its newest 32 unreadable, returns 30), and 32 returned may include the
turn's first. `returned_views` is how many came back, not how many the turn had; `at_the_read_limit` marks a
turn that returned 32, and the summary counts them (`returned_views_at_the_read_limit`). A record is read
through today's source checks, so the kit measures the record as it can be inspected now, not a reproduction of
everything that was sent then: a piece whose source was retracted since, or that today's reader does not list
(a fold, a task: below), is not in it.

A turn with no view record (one from before the cut) has no view side; it is counted under `no_view_record`,
never as left out. `summary.json` sums them. Every name it writes is checked against its field's closed set (the
daemon's own enums) or the store's id shape; anything else is written `other`.

## The labelled cases

```bash
python3 bench/view/cases.py check bench/view/cases/synthetic.v2.json
python3 bench/view/cases.py build bench/view/cases/synthetic.v2.json --bin-dir target/debug --out /tmp/vc
python3 bench/view/cases.py score bench/view/cases/synthetic.v2.json /tmp/vc/held-view.json
```

The format, `view-cases.v1`, is in `cases.py`'s own text; it is frozen, and a change is a new format name. The
first set, `cases/synthetic.v1.json`, is 24 cases in §10's six classes, written for the kit: a callback here, a
callback from another place, "the second option" later, a corrected fact, a task update, and a forbidden
disclosure. A case's id, its history ids and its symbolic places are safe names (`[A-Za-z0-9_.-]{1,64}`,
never `.` or `..`): the builder joins ids and places as directories under `--out`, checks each one lies beneath
it before the first write, and refuses a set that names anything else.

`cases/synthetic.v2.json` is the set to measure with: v1 less its four forbidden-disclosure cases, since nothing is
withheld after 2026-11-22 and their verdict states a rule that no longer exists. v1 stays as it was, byte for byte.

`build` makes each case's store in a scratch daemon on a stand-in model (bench/recall's `standin.py`), then
scores two contexts: the baseline, every earlier line of the message's own place (`held-transcript.json`, the
name of the strategy it once stood for), and the view's (`view preview --message`). The bar is that no class is
worse for the view than for the baseline. What the view lays only inside a fold is reported apart
(`held_folded`), never scored as held. The builder cannot make a task's record, so the 4 task cases are
`not_built` in every score, `build`'s and `score`'s alike, and never scored (v1's 4 forbidden cases too). Making
one would take a scripted model that calls `task_create` for a "New task" line, a wait for each task to end, and a
mapping from a `task` piece back to the line that made it.

## The yardstick

```bash
# What it would cost, calling nothing (it still copies the store and starts a scratch daemon):
python3 bench/view/yardstick.py --store /tmp/state-copy --bin-dir target/release \
  --place 'local:/path/of/the/place' --turns 20 --back 96 --out /tmp/yard-plan --dry-run --cap-usd 1
# The run:
ANTHROPIC_API_KEY=... python3 bench/view/yardstick.py --store /tmp/state-copy --bin-dir target/release \
  --place 'local:/path/of/the/place' --turns 20 --back 96 --out /tmp/yard --cap-usd 1
```

For each of the place's last `--turns` exchanges, the judge (`--model`, Haiku 5.5 unless named) reads up to
`--back` exchanges before it, one line each (the person's words, cut at 480 characters), 24 to a request, each
request starting 12 lines after the last, so every line is judged twice; its higher score is kept. Of the lines
the judge answered for, the comparison counts, per turn and in sum, those it marked helpful that the rule held
(`both`), that it did not (`judge_only`: what the rule misses), and the reverse. A line no window answered for is
in no cell; `coverage` counts the lines, those judged, and those of the judged that are unknown (below). The
kit computes no share or score from the cells; any computed from them must count the `unknown_*` cells beside
the hits and misses, never drop them from the whole.

**The rule is what each turn's view laid.** On a store it is `view_record`: the turn's own view record (its
first returned readable view, as the replay reads it, with the same limits). An earlier exchange an `exchanges`
piece laid counts as held; one that lies only inside a laid fold has a cell of its own (`folded_helps`,
`folded_not`, by the judge's mark), for the turn saw its fold and not its words. Zero `folded_*` is not evidence
of zero folds while `view show` lists no fold piece (below). A laid range whose ends no available order resolves
(an exchange with no measuring row, such as adopted history) leaves unknown which lines it covers: on that turn
a line the record did not lay whole is neither a hit nor a miss, and goes to `unknown_helps` or `unknown_not`,
even when a resolved fold also covers it, for the unresolved range could have laid it whole;
`unresolved_ranges` counts those ranges, in the plan and in the result. Fewer misses (`judge_only`) with more
unknowns means less knowledge, not better recall. A turn with no record (before the cut)
is not judged and is counted (`no_view_record`). Only the chosen place's exchanges are judged. On the cases it is a strategy's `--held` file,
or `own_place_lines` without one. Every output says which (`rules_basis`). The cases the builder cannot make are
judged as fixtures only (`not_built_fixture_only`), never in the totals. The `did` line the design puts beside
each judged line is not sent: whether a reply's summary goes to the judge is the owner's to say.

**The cap is a bound.** The model and the endpoint must be known: a model with no price in the catalogue, or any
`--api-base` but Anthropic's, is refused unless you state the prices (`--price-in`, `--price-out`, dollars a
million tokens). Before each call it reserves the call's worst case: every byte of the request counted as a
token, plus the message's framing, at the higher of the input and cache-write prices, plus 400 output tokens.
It never starts a call whose reservation would pass the cap. A call's reservation is replaced by what the
provider reported only when the stream finished (`message_stop`) after a final usage report with nonnegative
whole counts, read from the four counters the kit knows (input, output, cache read, cache write; anything else
is dropped). With no final report, a malformed one, or a negative count, the reservation stays spent. `--dry-run`
prints the calls and the summed reservations, unrounded, against the cap.

**It sends the conversation's words to a model.** That is the owner's to allow, on his own store. The judge
reads only `ANTHROPIC_API_KEY` from your environment. `--judge fake` (the tests') calls nothing.

## The end-to-end proof

```bash
python3 bench/view/proof.py --bin-dir target/debug --out /tmp/proof                 # every case, 30-turn cost table
python3 bench/view/proof.py --bin-dir target/debug --out /tmp/proof --cases wake,fork --cost-turns 0
```

`proof.py` reads no store. Each case runs on a scratch daemon of its own (`scratch.py`, with its rules) on an
EMPTY store, so every word in it is the suite's own, with a stand-in model on 127.0.0.1 that answers by script,
counts what it is sent and charges nothing. The cases: a message and a follow-up; a steer during a turn; a loop
of tool calls; a job that ends after its turn (a continuation); a daemon killed in the middle of a turn and
started again; a wake; a task's report; a task's brief and a second turn; a check; a fork and its first turn; a
turn of large tool results past the budget (a working window of 40,000 tokens); a provider refusal for size;
`/new`; a shared place with another person; a settings file that
names every key the cut retires. Each case asserts what a person needs (an answer arrived, or the turn ended in
a plain failure) and ends with `case <name>: ok|FAILED <why>`.

There is one context, so each case runs once. The table is a row a case: `ok`, `hand-back(<reason> xN)` (a
`context.view_fallback` row, which no build since the cut writes), `FAILED` or `not driven`, each with the case's
compilations by strategy (`context.compiled`). Every row must read `ok [view N]`.

The cost table is one place, 30 short messages, a tool call every seventh: per turn, the stand-in's count of the
turn's FIRST request (the compiler's rule, `bench/recall/tokens.py`) and the length of its byte-identical prefix
with the previous turn's first request (tools, system, messages, in that order, cache marks taken out), which
is what a provider cache could read; then their totals. Those totals are of the first request of each turn
only, not of everything sent. The `all requests` line under it counts every request the stand-in was sent in
the run, each once, and its parts: the first requests, a tool turn's follow-ups, the calls off the turns, the
extra calls, and any outside every turn's span (`all_calls`, `all_tokens` and the `*_calls`, `*_tokens` keys in
`proof.json`). Each request is given one class in one pass over the turn's span, in whatever order its rounds
arrive, so the parts add to the total; the run checks that in code and stops if they do not.

`--cost-long` adds two longer conversations: 120 short turns in one place (folds land at the 50th
exchange and every 20th after it, four in all), and 20 turns of three 6,000-character tool results each. Per turn they add the turn's own model
calls, the calls made OFF the turn (a call that offers no tools: the ladder's `did` lines and folds, a
compaction's summary) with their tokens, and the turn's compilations by strategy; their summary is the same two
lines, `first requests` and `all requests`. A `/new` break and a day
change are not driven: the terminal has no `/new`, and the daemon reads the machine's clock.

The cold start (`cold_start_61`, part of `--cost-long`): 60 short turns, then the 61st, whose whole request is what
the first turn after the provider's cache has lapsed writes; it is priced at the design's cache-write prices.
`--expect-cut` is the cut's acceptance: it exits 2 unless every driven case is ok, no hand-back exists and every
compilation (cases and cost runs) is the view's, and it names each offending cell.
`sh bench/view/proof-all.sh [out]` does the whole proof in one command on a machine that builds: the binaries,
the daemon test below, then every case with every cost run under `--expect-cut`, ending with the case table, the
cost summaries and the acceptance.

`/new` and a shared place with a second person are driven in Rust instead, on the stand-in Discord:
`crates/theseusd/tests/view_proof.rs` (`cargo nextest run -p theseusd --test view_proof`).

**What the proof cannot tell.** The stand-in has no cache and no judgement: a shared prefix is what a cache
could read, not what one did, and no answer's quality is measured. `/new` is the chat's and a second person is a
Discord place's, and the kit keeps Discord off, so those two cases say `not driven`. A retired key's warning is
on the daemon's stderr, which the kit discards: that case shows the daemon starts and answers, not the words.
`proof.json` in `--out` holds every case's counts.

## Saved output from an earlier kit

Output saved before the one-context kit reads differently, and a script that reads it will notice:

- `proof.py`: `proof.json`'s `cost` is one run's object (it was a list, a run per mode), and each
  `cost_long` entry is one run (it was a list); results carry no `mode`; the output directories have no mode
  level (`<out>/<case>`, not `<out>/<mode>/<case>`); a cost run adds `all_*`, `follow_*` and `outside_*`; the
  printed summaries say `first requests` and `all requests` where they said `sent` and `all calls`.
- `replay.py`: a row's view side comes from the turn's record, not a preview: it has `returned_views`,
  `at_the_read_limit` and `kinds`, and no `fallback`, `overflow`, `window`, `folds` or `budget`; the recorded
  side has no `strategy`. The summary has `with_a_view_record`, `no_view_record`, `view_basis`,
  `returned_views_at_the_read_limit` and `pieces_by_kind`, and no `with_a_view_row`, `recorded_strategy`,
  `view_fallback`, `notes` or `next_turn_view`.
- `yardstick.py`: on a store `rules_basis` is `view_record` (it was `same_session_heuristic`); the output adds
  `no_view_record`, `unresolved_ranges`, `coverage.unknown` (always), and the `folded_*` and `unknown_*` cells
  when they are not zero.

## What none of it can tell

- The replay reads a turn's record as today's reader returns it: the readable ones among its newest 32 views,
  through today's source checks. It says nothing of a turn from before the cut, which has no record. Adopted history has
  exchanges but no measuring rows.
- The real-binary tests (`scripts/tests/test_view_real.py`) run only on binaries whose stamps name the commit a
  clean checkout is at, and skip, saying why, on anything they cannot check. A stamp names a commit, not what was
  compiled: a binary built from an edited tree that was then restored bears the same stamp and is admitted. It
  identifies a build; it is not build provenance.
- `view show` lists no `folds` piece today: its record's source check has no case for a fold, so it fails
  closed (`view/record/source.rs`, `may_show`), while `view preview` lays folds through a check of its own. On a
  real store the replay's `folded` and the yardstick's `folded_*` cells therefore read 0 until `view show`
  lists folds; the record's tally still counts their tokens by slot.
- The measuring row counts tokens by the compiler's estimate (3.6 bytes a token), not the provider's.
- The yardstick judges only the chosen place's exchanges; what the view laid from another place is not scored.
- No tool says whether an answer would have been better. §10's paired replay with a rubric compared two builds,
  and there is one build now.
- The synthetic cases are short and written for the kit; they say whether a context holds a line, not how often
  a person's own conversations need one.

## What this kit does not protect against

- **It is not an OS sandbox.** The scratch daemon and the CLI run as your user, with your file permissions; the
  kit's isolation is its checks of paths and its minimal environment, not a confinement.
- **A race by a process of the same user.** The checks run before the copy and the writes; a process of your user
  could swap a directory or a link for another between a check and its use.
- **The yardstick sends conversation lines to a model provider.** That is what it is for: run on a real store,
  the person's own words leave the machine, under the cap they set.
- **The case builder's lines are marked the owner's.** Its scratch daemon names no agent host and it sends each
  line from a pty of its own. That is for its own synthetic store only; the replay and the yardstick send no
  message.
