# The view's measuring kit

The context design (`docs/design/context-point-of-view.md`, §10) says nothing about the view is to be trusted
before it is measured. These three tools answer three of §10's questions. Each runs on a **copy** of a store,
never on the live one, and writes **ids, kinds and counts only**: no message, no piece's text, no place's name.
`scripts/tests/test_view_kit.py` holds that on daemon answers full of marker strings.

| Tool | What it answers | What it costs |
|---|---|---|
| `replay.py` | For a place's last N exchanges: what the transcript's request was made of, beside what the view would have held. | Nothing: no model is called. |
| `cases.py` | Does a context hold what a good one must? A frozen set of labelled cases, the scorer, and a builder that makes each case's store by script. | Nothing: the builder's model is a local stand-in. |
| `yardstick.py` | What do the rules miss that a judge would find? A judge reads every earlier exchange in overlapping windows; its answers are set beside the rules' choice. | A model call per window. Refused without `--cap-usd`. |

`scratch.py` is what they share: a scratch `theseusd` on a copy of the copy it is given, with its own state,
socket and config, every provider pointed at an address nothing listens on, a spend limit of a cent, and
Discord, the web UI, the MCP server and the index off. It refuses `~/.theseus`, `$THESEUS_STATE_DIR`, a
directory a running process names, and a directory with no `store/wal`. It removes its copy when it stops.

## The copy

Copy a daemon's state directory while that daemon is stopped, or from a backup, into a directory of your own:

```bash
cp -a ~/.theseus /tmp/state-copy        # with the daemon stopped
```

The kit copies it again before it starts a daemon on it, so the copy you made is left as it was. Delete it when
you are done: it holds everything the store holds.

## The paired replay

```bash
python3 bench/view/replay.py --bin-dir target/release --store /tmp/state-copy \
  --place 'local:/path/of/the/place' --turns 50 --out /tmp/replay
```

The place is named as the ledger names it (`theseus ledger exchanges` lists the places). For each of the place's
last N exchanges, `rows.jsonl` holds one line: the transcript's side, read from the measuring row the turn wrote
(`context.turn`: the sent context's parts in tokens, and whether the previous exchange was in it), and the view's
side, from `theseus view preview <place> --turns N --json` (its tokens by slot, what the budget left out and why,
the fallback a turn would have taken, the window and the folds, and whether it held the previous exchange).
`summary.json` sums them. It sends the preview no message, so the view is assembled for an empty one.

A daemon whose preview has no `--turns` form gives only the place's next turn's view (`next_turn_view` in the
summary); a daemon with no preview gives the transcript's side alone. The summary's `notes` say which.

## The labelled cases

```bash
python3 bench/view/cases.py check bench/view/cases/synthetic.v1.json
python3 bench/view/cases.py build bench/view/cases/synthetic.v1.json --bin-dir target/debug --out /tmp/vc
python3 bench/view/cases.py score bench/view/cases/synthetic.v1.json /tmp/vc/held-view.json
```

The format, `view-cases.v1`, is in `cases.py`'s own text; it is frozen, and a change is a new format name. The
first set, `cases/synthetic.v1.json`, is 24 cases in §10's six classes, written for the kit: a callback here, a
callback from another place, "the second option" later, a corrected fact, a task update, and a forbidden
disclosure. `build` makes each case's store in a scratch daemon on a stand-in model (bench/recall's
`standin.py`), then scores the transcript's context and the view's. It cannot make a task's record or a seal, so
the task and forbidden classes are listed as `not_built`, never scored.

## The yardstick

```bash
# What it would cost, calling nothing:
python3 bench/view/yardstick.py --store /tmp/state-copy --bin-dir target/release \
  --place 'local:/path/of/the/place' --turns 20 --back 96 --out /tmp/yard-plan --dry-run --cap-usd 5
# The run:
ANTHROPIC_API_KEY=... python3 bench/view/yardstick.py --store /tmp/state-copy --bin-dir target/release \
  --place 'local:/path/of/the/place' --turns 20 --back 96 --out /tmp/yard --cap-usd 5
```

For each of the place's last `--turns` exchanges, the judge (`--model`, Haiku 5.5 unless named) reads up to
`--back` exchanges before it, one line each (the person's words), 24 to a request, each request starting 12 lines
after the last, so every line is judged twice; its higher score is kept. The comparison counts, per turn and in
sum, the lines the judge marked helpful that the rules held (`both`), that they did not (`judge_only`: what the
rules miss), and the reverse. On a store the rules are the transcript's; on the cases (`--cases`, with a
strategy's `--held` file), the strategy's.

**It sends the conversation's words to a model.** That is the owner's to allow, on his own store. It is refused
without `--cap-usd`, never starts a call whose worst case could pass the cap, and `--dry-run` prints the calls
and the worst case first. `--judge fake` (the tests') calls nothing.

## What none of it can tell

- The replay's transcript side is what the request WAS; its view side is assembled NOW, from the store as it
  stands, for an empty message. Anything the store learned since (a `did` line written later, a fold, a seal, a
  correction) shapes the view and did not exist then.
- History from before the owner's mark has no proven place: no view holds it, so a replay over it measures the
  fallback, not the view.
- The measuring row counts tokens by the compiler's estimate (3.6 bytes a token), not the provider's.
- Neither tool says whether an answer would have been better; §10's paired replay with a rubric does, and it is
  not built.
- The synthetic cases are short and written for the kit; they say whether a context holds a line, not how often
  a person's own conversations need one.
