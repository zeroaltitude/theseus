# The view's measuring kit

The context design (`docs/design/context-point-of-view.md`, §10) says nothing about the view is to be trusted
before it is measured. These three tools answer three of §10's questions. Each runs on a **copy** of a store,
never on the live one, and writes **ids, kinds and counts only**: no message, no piece's text, no place's name,
and no daemon diagnostics. `scripts/tests/test_view_kit.py` holds that on daemon answers, errors and stderr full
of marker strings.

| Tool | What it answers | What it costs |
|---|---|---|
| `replay.py` | For a place's last N exchanges: the context recorded as sent, beside what the view would have held. | Nothing: no model is called. |
| `cases.py` | Does a context hold what a good one must? A frozen set of labelled cases, the scorer, and a builder that makes each case's store by script. | Nothing: the builder's model is a local stand-in. |
| `yardstick.py` | What does a rule miss that a judge would find? A judge reads every earlier exchange in overlapping windows; its answers are set beside the rule's choice. | A model call per window, under a cap that is a bound. Refused without `--cap-usd`. |

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
  tokens, and whether the previous exchange was in it. The row does not record which strategy built that context,
  so every row says `strategy: not_recorded`;
- **the view's side**, from `theseus view preview <place> --turns N --json`: its tokens by slot, what the budget
  left out and why, the fallback a turn would have taken, the window and the folds, and whether it held the
  previous exchange. That last answer is read from the exchange ranges its pieces lay, interiors included; a
  range the place's exchange order cannot resolve makes it unknown, not "absent".

`summary.json` sums them. The replay sends the preview no message, so the view is assembled for an empty one.
Every name it writes is checked against its field's closed set (the daemon's own enums) or the store's id shape;
anything else is written `other`.

A daemon whose preview has no `--turns` form gives only the place's next turn's view (`next_turn_view` in the
summary); a daemon with no preview gives the recorded side alone. The summary's `notes` say which.

## The labelled cases

```bash
python3 bench/view/cases.py check bench/view/cases/synthetic.v1.json
python3 bench/view/cases.py build bench/view/cases/synthetic.v1.json --bin-dir target/debug --out /tmp/vc
python3 bench/view/cases.py score bench/view/cases/synthetic.v1.json /tmp/vc/held-view.json
```

The format, `view-cases.v1`, is in `cases.py`'s own text; it is frozen, and a change is a new format name. The
first set, `cases/synthetic.v1.json`, is 24 cases in §10's six classes, written for the kit: a callback here, a
callback from another place, "the second option" later, a corrected fact, a task update, and a forbidden
disclosure. A case's id, its history ids and its symbolic places are safe names (`[A-Za-z0-9_.-]{1,64}`,
never `.` or `..`): the builder joins ids and places as directories under `--out`, checks each one lies beneath
it before the first write, and refuses a set that names anything else. `build` makes each case's store in a scratch daemon on a stand-in model (bench/recall's
`standin.py`), then scores the transcript's context and the view's. It cannot make a task's record or a seal, so
the 8 task and forbidden cases are `not_built` in every score, `build`'s and `score`'s alike, and never scored.

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
in no cell; `coverage` counts the lines and those judged.

**The rule is not the transcript's real selection.** On a store it is the `same_session_heuristic`: every
earlier exchange of the turn's own session counts as held, with compaction, summaries, recall and other
sessions' place tails ignored, and only the chosen place's exchanges judged. On the cases it is a strategy's
`--held` file, or `own_place_lines` without one. Every output says which (`rules_basis`). The 8 cases the
builder cannot make are judged as fixtures only (`not_built_fixture_only`), never in the totals.

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
python3 bench/view/proof.py --bin-dir target/debug --out /tmp/proof                 # every case, both modes, 30-turn cost table
python3 bench/view/proof.py --bin-dir target/debug --out /tmp/proof --assembly view --cases wake,fork --cost-turns 0
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

Two modes: `view` (`[context] assembly = "view"` in the scratch daemon's settings) and `default` (no setting).
The table is case x mode: `ok`, `hand-back(<reason> xN)` (the `context.view_fallback` rows), `FAILED` or
`not driven`, each with the case's compilations by strategy (`context.compiled`). After the cut both columns
must read `ok [view N]`.

The cost table is one place, 30 short messages, a tool call every seventh: per turn, the stand-in's count of the
turn's first request (the compiler's rule, `bench/recall/tokens.py`) and the length of its byte-identical prefix
with the previous turn's first request (tools, system, messages, in that order, cache marks taken out), which
is what a provider cache could read; then totals and the ratio of the two modes.

`--cost-long` adds two longer conversations in each mode: 120 short turns in one place (folds land at the 50th
exchange and every 20th after it, four in all), and 20 turns of three 6,000-character tool results each. Per turn they add the turn's own model
calls, the calls made OFF the turn (a call that offers no tools: the ladder's `did` lines and folds, a
compaction's summary) with their tokens, and the turn's compilations by strategy. A `/new` break and a day
change are not driven: the terminal has no `/new`, and the daemon reads the machine's clock.

The cold start (`cold_start_61`, part of `--cost-long`): 60 short turns, then the 61st, whose whole request is what
the first turn after the provider's cache has lapsed writes; it is priced at the design's cache-write prices.
`--expect-cut` is the cut's acceptance: it exits 2 unless, in both modes, every driven case is ok, no hand-back
exists and every compilation (cases and cost runs) is the view's, and it names each offending cell.
`sh bench/view/proof-all.sh [out]` does the whole proof in one command on a machine that builds: the binaries,
the daemon test below, then every case in both modes with every cost run under `--expect-cut`, ending with the
case table, the cost summaries and the acceptance.

`/new` and a shared place with a second person are driven in Rust instead, on the stand-in Discord:
`crates/theseusd/tests/view_proof.rs` (`cargo nextest run -p theseusd --test view_proof`).

**What the proof cannot tell.** The stand-in has no cache and no judgement: a shared prefix is what a cache
could read, not what one did, and no answer's quality is measured. `/new` is the chat's and a second person is a
Discord place's, and the kit keeps Discord off, so those two cases say `not driven`. A retired key's warning is
on the daemon's stderr, which the kit discards: that case shows the daemon starts and answers, not the words.
`proof.json` in `--out` holds every case's counts.

## What none of it can tell

- The replay's recorded side is what the request WAS, by whatever strategy built it; its view side is assembled
  NOW, from the store as it stands, for an empty message. Anything the store learned since (a `did` line written
  later, a fold, a seal, a correction, adopted history) shapes the view and did not exist then.
- History from before the owner's mark has no proven place: no view holds it, so a replay over it measures the
  fallback, not the view. Adopted history has exchanges but no measuring rows.
- The measuring row counts tokens by the compiler's estimate (3.6 bytes a token), not the provider's.
- The yardstick's rule on a store is a heuristic, not the context a turn was sent; it cannot see other places.
- Neither tool says whether an answer would have been better; §10's paired replay with a rubric does, and it is
  not built.
- The synthetic cases are short and written for the kit; they say whether a context holds a line, not how often
  a person's own conversations need one.

## What this kit does not protect against

- **It is not an OS sandbox.** The scratch daemon and the CLI run as your user, with your file permissions; the
  kit's isolation is its checks of paths and its minimal environment, not a confinement.
- **A race by a process of the same user.** The checks run before the copy and the writes; a process of your user
  could swap a directory or a link for another between a check and its use.
- **The yardstick sends conversation lines to a model provider.** That is what it is for: run on a real store,
  the person's own words leave the machine, under the cap they set.
- **The replay's "recorded" side is not the transcript strategy's selection.** It is what the measuring row
  recorded as sent, by whichever strategy built it; the yardstick's rule on a store is a same-session heuristic.
  Neither reconstructs how a past turn's context was chosen.
