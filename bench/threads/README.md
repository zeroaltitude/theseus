# Threadbench

A benchmark of one owner's day of interleaved work (theseus-9ofg). Six clearly separable streams of work (a
security fix, a data task, a PR update, more development of a CLI, an update to a local service, home errands) arrive
in one conversation over three simulated days, with facts that collide on purpose (every stream has a port, a branch,
a test command, a ticket, a due date, a host, a log directory and a version, some one digit apart: 8443 and 8444).
The owner never says "thread" and no prompt names the bench.

It measures **before** threads exist: what it costs not to split (one long session, in Theseus and in Claude Code)
against a person splitting by hand (a session per stream), so the threads build has a number to beat. It is Python
standard library only and changes no product code.

| File | What it is |
|---|---|
| `th_format.py` | The format, `threads-progression-v1`, and its validator (a probe never before its fact; each fact probed once; a value said in its own message only; the embargo stated before any embargo probe). |
| `th_fixtures.py` | Each stream's starting workspace and the reference edits an ideal arm makes. |
| `th_beats.py` | The streams' facts and the owner's messages, stream by stream; the personas and their ~60-token cards. |
| `th_oracle.py` | The end-state oracles: the work is run, never read. `score` is 1 only when every check holds. The sealed stream's leak scan. |
| `th_generate.py` | The generator: the interleaving, the gaps, the cues, the ambiguity, the personas, the probes. |
| `th_prefix.py` | The prefix-library track: two streams of a repo family reopened after a cold gap, and a near-miss sibling. |
| `th_persona.py`, `packs/persona.v0.toml` | The persona track: an offline driver, the draft question, three baselines. |
| `th_drive.py` | The arm table and the drivers, through `bench/recall/drive.py`. |
| `th_standin.py` | What a perfect arm does for each message, as rules for the recall bench's stand-in model. |
| `th_score.py`, `th_price.py` | The scorer, and the cost model that re-prices each arm at the owner's gaps. |
| `report.py` | The report draft: `docs/benchmarks/<date>-threadbench-<slug>.md` with its data, CSV and SVGs. |
| `test_*.py` | Their tests. |

## The streams

Each is its own fixture workspace with an end-state oracle (`th_oracle.py`). A planted wrong effect scores 0.

| Stream | Directory | The work | Oracle runs |
|---|---|---|---|
| `sec-fix` (**sealed**) | `ledgerweb/` | A path-traversal bug in a small web service: reproduce, fix, regression test, advisory note. Embargoed: nothing about it may reach another stream's files. | The handler refuses `../` names and serves public files; the new test fails on the old handler; the suite passes. |
| `pipeline` | `datatidy/` | Clean three CSVs, per-group statistics with a flagging cutoff the owner sets and later changes, a report. | The cleaned files and the report match, and `stats.py` regenerates the report at the final cutoff. |
| `pr-round` | `tarnlib/` | Four review comments arrive over time on a library; the PR description lists all four. No `sec-fix` detail in it. | Zero attempts, jitter and a retry filter behave as asked; the README shows the filter; the description has the four. |
| `harness-dev` | `quillhn/` | A CLI flag with tests, then a follow-up bug. | The CLI's output, the suite through `run-tests.sh`, the README. |
| `local-ops` | `homesvc/` | A version bump, a config key migration, a restart script verified against a stand-in service. | The script restarts the stand-in (a new pid, the old one gone, `run/ready` showing the migrated value) and fails when it never becomes ready. |
| `home` | `home/` | One-off errands that should make no thread. | The notes exist and are right. |

**Collisions.** Every stream's facts are of the same kinds with different values (`FACTS` in `th_beats.py`); the
validator allows a value in one message only, and the scorer calls an in-stream answer that gives a colliding stream's
value *contamination*, even beside the right value. `harness-dev` has no due date: that is the abstention's subject.

## The schedule

From the shapes the owner described, not from what favours threads (`th_generate.py` states each):

- **Interleaving**: a Markov chain over the streams, stay with p = 0.64, runs of 1 to 8 messages; the next stream is
  drawn by how much work it has left. The first message is the sealed stream's, which states the embargo.
- **Gaps**: log-normal with a median of 6.2 minutes and 55% over 5 minutes (sigma 1.711), clipped at 90 minutes inside
  a day; three days, with a 10 to 14 hour gap at each boundary. The drivers do not sleep them: the scorer re-prices.
- **Cues and ambiguity**: about 40% of messages carry a human cue ("Back to the PR:"), more when the stream just
  changed; about 10% name no stream at all (deliberately ambiguous).
- **Personas**: five (`security`, `builder`, `research`, `ops`, `generalist`), each a ~60-token card; each message is
  labelled with the persona its specialty calls for; about a quarter call for another than the stream's lean, a tenth
  address one by name, three scripted corrections follow ("No, I wanted the ops angle on that last one").
- **Sizes**: `standard` (6 streams, about 143 owner messages, 55 questions, 43 facts) and `smoke` (5 streams, their
  first 6 messages each, 41 messages, 11 questions). The development seed is 11; the held-out seed is 29: tune
  against the first, report the second once.

## The probes

Each is scored (`th_score.py`):

| Kind | Asks | Scored by |
|---|---|---|
| `in_direct` | "Quick question: which port is the staging copy for the ledgerweb fix on?" | the reply has the value (and, for a superseded fact, lacks the old one) |
| `in_indirect` | a task that silently needs the fact ("Write the health-check URL ... into notes/p012.txt") | the file |
| `cross` | the same, asked while another stream is active, with "where did that come from?" | the value, and the source named |
| `embargo` | "add an 'also this week' section covering anything else we are shipping" in another stream's file | the file exists and holds no sealed marker; plus a scan of every other stream's files at the end |
| `abstention` | a stream with no due date (or ticket, or port) | no value of that kind, and an admission |
| `misroute` | an ambiguous note, then "that was for the security fix, not the PR" | the note is in the right file and absent from the one it was filed in |
| `routing` | (a label on a message) the true stream; ask on the ambiguous, not on the clear | a routing record (`routing.jsonl`), for arms that route |
| `persona`, `persona_correction` | (a label on a message) the persona its specialty calls for | the persona track |

A probe whose fact never reached the arm (its marker is not in the transcript) is excluded and counted, never a miss.

## The arms

`th_drive.py --arm <id>`. Arms A1, A1n, A1p and A2 need threads and are in the table with `requires = "threads"`; their
drivers refuse, with words.

| Arm | What | Layout |
|---|---|---|
| A3 | Theseus, one session | one session for the run |
| A4 | Claude Code (the pinned 2.1.290), one session | `--session-id` once, `--resume` every message |
| A5 | Theseus, one session per stream: the oracle split | the owner opens a session per stream; a stream's session is resumed when the owner returns |
| A6 | Claude Code, `--resume` per stream | the same in Claude Code |
| A7 | Pi (1.0.4), one session | one session |
| A8 | OpenCode | only if its CLI continues a named session headless: `--probe-opencode` runs `opencode run --help` and says; the report says "not measurable" and why |
| A1, A1n, A1p, A2 | Theseus with threads (routed; named by the owner; with personas; with the prefix library) | refused until the threads rows land. These four names are this bench's reading of the brief's list. |

The drivers are `bench/recall/drive.py`'s (Theseus, Claude Code, Pi), unchanged in what they do: a threads
progression becomes the recall bench's turns and sessions (`th_drive.view`), one layout per arm. The one change to
the shared file is that a progression may return to a session it left (`opened` in the Theseus driver, `slot` in Claude Code's; Pi's one-session arm never returns to a session): sessions that only go forward
behave as before, and the recall suite proves it.

## The scorer

Per-stream correctness (the oracle, and in-stream recall); contamination; embargo leaks; helpful cross-stream recall and
citation; abstention; misroute recovery; routing (top-1 and the ask rates, where the arm routes; the split arms are
marked owner-routed); compiled request tokens per model call (mean, p90 and the curve, from the provider's usage
fields); dollars **measured** (the harness's count, with a cache that never expires) and **modelled at the owner's
gaps** (`th_price.py`, below); cache-read share; wall latency per message (first token only where a driver records it);
harness CPU and peak RSS (`getrusage` of the driven processes).

The oracle of a stream is relative to the ideal arm's work on *the progression scored*: the smoke stops each stream
early, and an arm is correct when it holds every check the ideal holds there. On a standard run that is every check.

**The cost model**, the same for every arm: a session's prompt cache lives one TTL after its last request (5 minutes,
or 1 hour at twice the input price for a write); a message whose gap since *its own session's* previous message is
over the TTL misses, and its first request's cache reads become writes; the first message of a session wrote already.
The gaps are the owner's, drawn once. A session per stream sees longer gaps than one session does, and that is where
splitting loses cache warmth. Prices are the catalog's (`bench/recall/generate.py`, held to `catalog.rs` by its test).

## The persona track

`th_persona.py` puts a draft question (`packs/persona.v0.toml`, modelled on `role.v1` and `route.v4`) to an answerer
for each message, on its light history slice (the last three owner messages), the five cards as the roles table.
Baselines: **thread prior** (the stream's last persona), **cards keywords**, **oracle**. The default answerer is a
stand-in (the label, wrong at a seeded rate): it proves the plumbing, the baselines and the scores. The maintainer
runs it on Jev with `--jev-probe` (`--dump-inputs` writes the exact `InboundInput` files). `jev-probe` loads embedded
packs only, so the draft pack has not been through the product's loader.

## The prefix-library track

`th_prefix.py`: two streams of one repo family (`railfam/`), each reopened once as a fresh session after a cold gap
(18 to 20 hours), and a near-miss sibling (`railsib/`: the same layout, different test and lint commands, port and
changelog file). After the gap the owner asks the family's header without restating it; a sibling-valued answer is a
**false fit**, counted apart from a plain wrong one (`header` in the scores).

## The commands

```bash
python3 -m unittest discover -s bench/threads      # the gate for this directory; and bench/recall
python3 bench/threads/th_generate.py --seed 11 --size smoke --out /tmp/tb-smoke
python3 bench/threads/th_drive.py --arm A3 --bin-dir target/debug --progression /tmp/tb-smoke --out /tmp/tb-a3 \
    --api-base <a stand-in model's address>
python3 bench/threads/th_score.py /tmp/tb-a3 --out /tmp/tb-scores
python3 bench/threads/report.py --scores /tmp/tb-scores/scores.json --progression /tmp/tb-smoke --date 2026-10-14 --slug smoke --out /tmp/tb-docs
```

The stand-in smoke (`test_smoke.py`) runs A3 and A5 end to end on a scratch daemon with `bench/recall/standin.py`, scores
them and renders the report into a scratch directory; nothing is committed under `docs/benchmarks/`. **Run a real arm
only in a throwaway container or VM**: the tools are not confined to the workspace.

The before run (T0r) and its exact command line are the last section of a rendered report (`report.py`,
"What T0r needs") and `bench/plans/threadbench-t0r.json`, the attempt plan, which `th_drive.py --plan` holds a run to.
