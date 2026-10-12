# CLOUD_REPORT: commitments (theseus-lherl)

Branch `cloud/20261011-commitments`, based on c3248d20 (main's d50ad5e9 and the task commit). Store format
unchanged (27): no stored record gained a field; `wake.moved` is a new ledger row kind, which is no record layout.
Cargo.lock and the package locks are untouched.

## What I found

- The model had `wake.at` alone (`wake::NAMES = [AT]`); `public_tool` took every `wake.*` as public, so a
  shared place would have been offered any new wake tool.
- `take_wakes` reads the execution once and returned only the fired wakes, so "built from the one read" needed the
  rest of that read handed back, not a second read.
- Nothing in the request named the pending wakes, and the stand-in models that read "the last user text" skip the
  task view by its `HEAD` only: a second tail block would have been taken for the person's words.

## What I changed

Commit `a57f017b`: the three items. Commit `2de4edab`: kernel-sim's random operations gain the move.

1. **The commitments block** (`core/commitments.rs`, `compile_step.rs`): this conversation's pending wakes in the
   request's last block, after the task view's block (`view::attach_text`, no breakpoint of its own: the view's
   `attach_to` still moves the conversation's one, and a second block moves none). One line each, `⏰ a8cebb Fri 09:00
   check PR 123` and `🔁 a41d04 every 1d 21:00 (next Fri) evening haiku` (a series says `on mon,fri` and `until Oct 30
   18:00` when it has them), at most 10 lines and 300 tokens (the view's 3 bytes a token), then `(and N more: wake.list,
   or `theseus wakes`, shows them all.)`; nothing when none is pending. The list is `Kernel::take_wakes_listing`'s
   (the old `take_wakes` is now its wrapper), kept in the turn (`Frames.commitments`, lent as `TurnCtx::commitments`)
   and replaced by what each wake tool's frame hands back (`WakeSet.list`, `WakeMoved.list`, the cancel's execution):
   no second read of the store. An operator's cancel mid-turn shows at the next turn. The line starts with an icon and
   a short id, so a routine, a watch or a deferred task joins as another line (noted in the module and core's AGENTS.md).
2. **`wake.cancel {id}` and `wake.move {id, at | after, series?}`** (`core/wake_tools.rs`; the kernel's `move_wake`,
   one frame: the execution's record and a `wake.moved {from, to, by, series}` row; the cancel is `cancel_wake` with
   `by: "model"`). A name (a whole id, or the end of it, four characters at least) is resolved among this
   conversation's own pending wakes, so another conversation's wake is not found. A move keeps the id and the note; a
   series' next occurrence moves alone (its grid, so its `every`/`days`/`until`, stays), or with `series: true` its time
   moves (`first_ms`), the rule kept. Not public tools: a shared place is not offered them and the gate refuses them
   (`places::public_tool` is now `wake.at` alone), and each tool asks `places::refusal` again with the same words.
   `wake.at`'s description gained the sentence; both names are in `wake::NAMES` and the template's `[policy.tools]`.
3. **`WakeInfo`** moved to `theseus-protocol/src/wakes.rs` (re-exported; bytes unchanged) with `days`, `until_ms`
   and `title` (the note's first line). `theseus wakes` (`render/wakes.rs`, out of `render.rs`) and Discord's `/wakes`
   (`render/wakes.rs`, out of `render.rs` too, which was at its ceiling) print the title, a series' weekdays and its end.
   The cockpit's `WakeInfo.ts` is regenerated.
4. kernel-sim (`kernel_sim/wakes.rs`): a turn moves a pending wake now and then; a series moved off its grid is
   tracked until a turn takes it (`Sim2.moved_off_grid`), where the grid invariant had caught the first draft.

Also changed because the tools block grew (see "Design"): `tests_route_keep.rs` WINDOW 41,000 to 41,700 and BILLED
35,500 to 36,200 (41,300 still rang; 41,550 passes), `tests_tiering.rs` window 40,000 to 40,800 (40,400 rang early),
`config/tests.rs` the template's tool-line count 53 to 55, `tests_wakes.rs`' stand-in skips the block, the kernel's
frames golden (only the printed `WakeSet`, which gained `list`: its frames are byte-identical), `tests_compaction.rs`
(helpers made `pub(crate)` for the new compaction test), and the three stand-ins (`theseus-sim`'s two and theseusd's
tests/common/model.rs) use `view::is_head`.

## How I proved it

- **Tests added** (all pass; 8 rounds of the set, 22 tests, no failure; the core's nine also under TZ America/Phoenix,
  Pacific/Kiritimati and Pacific/Pago_Pago): theseus-core `tests_commitments.rs` (9): the block's golden (none, one, a
  series, a far one-shot with its date, 13 cut at ten with `and 3 more`), its token bound with 200-character notes,
  its build under 50 µs (best of 300: 20 µs in a debug build here), the block as the last block of every request while
  a wake is pending and gone when none is (and no `cache_control` on it), a forced compaction (`tests_compaction`'s rig, a
  summary on glm) whose request has no "Set wake " left and still carries the block, a move keeping id/series with one
  `wake.moved` row, a cancel by the model (`by: model`, a series ended with it), another conversation's wake refused by
  whole id and by short id (both tools, nothing written), a shared place refused both tools with the place rule's words.
  The stand-in model there reads the block's ids to name the wake, as the feature is meant to be used. theseus-kernel
  `tests_wakes.rs` (3): a move is one frame (observed: the execution and the row), keeps id and note, a series' two
  moves keep `every`/`days`/`until`, a one-shot cannot be moved as a series, a missing wake writes nothing;
  `take_wakes_listing` writes nothing with none due and lists the series put back. theseus-protocol `tests/wakes.rs` (2):
  the old bytes round-trip and the new fields ride after them. theseus `render/wakes.rs` (2) and theseus-discord
  `render/wakes.rs` (2): the old goldens, moved unchanged, and the title/days/until ones. `places::tests` and
  `tests_places`: `wake.cancel`/`wake.move` are not offered in a shared place. kernel-sim: 60 seeds, 304 moves, 24,120
  invariant checks, all held (and 20 seeds with `--p-race 0`).
- **Planted reverts** (each file `touch`ed after the restore, `git status` checked): (a) `commitments::attach` not called
  in `compile_step.rs`: 4 of 9 `tests_commitments` fail (the last-block test, the move test, the cancel test, the
  compaction test); (b) `move_wake` setting a new id (`wake.id = format!("{}x", ..)`): the kernel's
  `a_move_keeps_the_id_in_one_frame` (by its earlier name) and the core's move test fail; (c) the place check off
  (`public_tool` back to `starts_with("wake.")` and the tool's `private_only` returning Ok): `places::tests::
  a_shared_place_is_offered_the_public_tools`, `tests_places::a_shared_place_is_offered_and_allowed_only_the_public_tools`
  and `tests_commitments::a_shared_place_is_refused_the_cancel_and_the_move` fail. The tool's own check alone, with
  the gate's left on, fails nothing: the two ask the same `places::refusal`, and a test cannot reach the tool past the
  gate, so the tool's copy is defence in depth that no test isolates.
- **FAST.** 0 frames added: the block writes nothing and reads nothing. `theseus-sim bench turn --runs 5 --check`:
  plain 4 frames, tool-call 8, batch 14, syncs 2, first request 37.5 KB (all within budget); the suite's `tests_m3::
  a_plain_turn_stays_within_its_frame_budget` passes. `bench lifecycle --runs 10 --check`, debug builds on this VM, base
  c3248d20 against this branch, interleaved, health's p50 3.3 to 3.6 against 3.4 to 4.0 ms and p95 4.7 to 6.5 against
  4.3 to 7.9: noise, in both directions (its 5 ms budget is the owner's machine's: base missed it too, and
  base's clean shutdown p95 read 366 ms in one run); cold start p50 21.6 against 22.9, restart 21.5 against 23.0, swap
  23.3 against 23.2 ms. The start path is untouched.
- **The gate** (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=<the task commit> scripts/gate.sh`, the last
  run on the final tree before this report): keel ok (0 findings), keel tests, fmt, shape, features, clippy, cockpit
  (lint, test, build), test build and the registry test pass. The suite: 3,948 tests, 3,915 passed, 33 failed: the 19
  `theseus-sandbox::contract`, 1 `theseus-sandbox::bench` and 13 `theseusd::sandbox` tests, which are the known L1 ones (a
  root daemon's job with no cgroup). Nothing else failed. The phases after the suite I ran by hand: the TypeScript
  (`git status` clean after the protocol tests), `cargo deny --offline check` (advisories, bans, licences, sources ok; one
  unmatched-allowance warning), the turn bench and the lifecycle bench above.
- Seen once and not reproduced: `theseus-kernel job_wait::tests::a_pidfd_wakes_the_wait_at_the_exit_and_not_before`
  failed while the cold build ran beside it and passed alone; `theseusd gone_jobs::a_restart_settles_the_jobs_whose_
  wrappers_went_while_no_daemon_ran` failed in one gate run and twice alone ("the live wrapper's job runs on":
  `dispatched` 2, not 1, after the wait had seen 1), then passed in the next two gate runs; I have no base run of it
  (the base's test build ran out of disk), and nothing of mine touches jobs or restarts, so I call it a flake, not a
  finding, but it is unexplained. The VM's disk allowance filled once (a second target dir): one gate run died in its suite
  for it; deleting `target/debug/incremental` freed 15 GB and I reran.

## Keel

`python3 scripts/keel-guard.py` with its default base (merge-base with origin/main, a9ad950f) prints 5 findings that
are not this branch's: `theseus-tui/src/tests.rs` (one assertion fewer), two `allow`s (`theseus-index/src/vectors.rs`,
`theseus-protocol/src/tests_work_join.rs`) and two long-files ceilings (`theseus-protocol/src/lib.rs`, `theseus/tests/
golden.rs`), all from the commits between a9ad950f and the task's base, with `scripts/keel-signers` empty at that base.
With `THESEUS_KEEL_BASE=c3248d20` (this branch's range): `keel: ok (... 44 files changed, 0 findings acked)`, and it
names the two tests this branch moved (`a_wake_lists_its_due_time_session_and_note`, `slash_wakes_lists_each_with_its_
time_and_note`) as moved, not removed. **Keel findings expected: none.**
`protocol/src/lib.rs` is 46 lines shorter (2,614 to 2,568: its `WakeInfo` moved), `discord/src/render.rs` 82
(3,009 to 2,927) and `cli/src/render.rs` 107 (3,121 to 3,014): no ceiling was raised or needed.

## Live check (this VM: a scratch daemon on the scripted stand-in, `min_repeat_minutes = 1`)

The script (below, `/tmp/lh/live.py` here; copy `theseus`, `theseusd` and `theseus-sim` from `target/debug` into a
directory and run `python3 live.py <that dir> <a scratch dir>`; it needs no key, no vault and no network beyond
127.0.0.1:19448) starts `theseus-sim fake-model` with rules and `--dump`, a `theseusd` on its own `--config`,
`--socket` and `--state-dir`, with `ANTHROPIC_API_KEY=stand-in`, `[catalog."claude-sonnet-5-5"]` at 48,000 tokens so five
50 KB messages cut the context, and Discord, the web UI and the index off. Because wake ids are random, it reads them
from `theseus --json wakes` and writes a second rules file (the stand-in's rules are fixed text) for the move and the
cancel. Its result on the final build:

```
session ses_01a12dee66f6702186d3018db0833182
== theseus wakes (before)
68c443	2026-10-12 02:56:01 +00:00 (in 29m)	every 1d on mon,fri until 2026-11-30 09:00 #1	session ses_01a12dee66f6702186d3018db0833182 (waiting)	pulse
f442fb	2026-10-12 03:26:01 +00:00 (in 59m)	once	session ses_01a12dee66f6702186d3018db0833182 (waiting)	check PR 123
2cc018	2026-10-12 09:26:01 +00:00 (in 6h)	once	session ses_01a12dee66f6702186d3018db0833182 (waiting)	far errand

after filler 1 compilations: [('transcript', 'None'), ('transcript', 'new_session')]
after filler 2 compilations: [('ring', 'overflow'), ('transcript', 'None'), ('transcript', 'new_session')]
after filler 3 compilations: [('ring', 'overflow'), ('transcript', 'None'), ('transcript', 'new_session')]
after filler 4 compilations: [('ring', 'overflow'), ('transcript', 'None'), ('transcript', 'new_session')]
after filler 5 compilations: [('ring', 'overflow'), ('transcript', 'None'), ('transcript', 'new_session')]
== the last request's last block:
[Your pending wakes in this conversation, 3: to change one, move it with wake.move or end it with wake.cancel, never set a second.]
🔁 68c443 every 1d on mon,fri 02:56 until Nov 30 09:00 (next Mon) pulse
⏰ f442fb Mon 03:26 check PR 123
⏰ 2cc018 Mon 09:26 far errand
compilations: [('ring', 'None'), ('ring', 'overflow'), ('transcript', 'None'), ('transcript', 'new_session')]
'Set wake ' results in the request after compaction: 0
== move: Done.

== cancel: Done.

== theseus wakes (after)
f442fb	2026-10-12 03:26:01 +00:00 (in 59m)	once	session ses_01a12dee66f6702186d3018db0833182 (waiting)	check PR 123
2cc018	2026-10-13 21:00:00 +00:00 (in 1d)	once	session ses_01a12dee66f6702186d3018db0833182 (waiting)	far errand

== wake.moved {"rows":[{"at_unix_ms":1791771962687,"data":{"by":"model","execution_id":"exe_01a12dee66f6702186d3018ed17024af","from":1791797161125,"series":false,"to":1791925200000,"wake_id":"wak_01a12dee672372a4b1e1b7cc922cc018"},"kind":"wake.moved","position

== wake.cancelled {"rows":[{"at_unix_ms":1791771962767,"data":{"by":"model","due_at_ms":1791773761119,"execution_id":"exe_01a12dee66f6702186d3018ed17024af","wake_id":"wak_01a12dee671d7529b626c4f22568c443"},"kind":"wake.cancelled","position":367,"session_id":"s

```

What it shows: three wakes set by the stand-in in one response (a one-shot, a weekday series with an end, a far one-shot);
`theseus wakes` with the title, `on mon,fri until 2026-11-30 09:00 #1`; the context cut by a ring on overflow (the
stand-in has no summary profile, so it is the ring, not a summary: the compaction test above covers the summary);
the next request's last block is the commitments block, three lines, and holds no "Set wake " result (0 in the request);
the scripted `wake_move` of the far one to 21:00 the next day and `wake_cancel` of the pulse by the ids the block
names; `theseus wakes` after (two left, the far one at 21:00 on the 13th); one `wake.moved` row (`by: model`, from, to)
and one `wake.cancelled` (`by: model`). The stand-in's words ("Done.") are the scripts' own, not an answer from a model:
the model's reading of the block is shown by the request, and by `tests_commitments`' stand-in, which parses the ids
from the block's lines.

The maintainer's own check, on a real model: in a private conversation say "remind me at 9 to check the build and
every Friday at 4 to send the update", then send enough text to cut the context (or `theseus ask -s <id> ...` a few
long files), then "what reminders do I have?" (answered from the block with no tool call), "make the first one 10 and
cancel the Friday one", then `theseus wakes` (the id is the same, the time moved, one wake left) and
`theseus ledger -k wake.moved`/`-k wake.cancelled` (`by: model`). From a shared place, the model is not offered the two tools.

## Design choices the owner should hear about

- A series moved by its next occurrence alone sits off its grid until it fires, and the turn that takes it puts the
  next one back on the grid. That is the plain meaning of "move the next one", and kernel-sim's grid check now
  tolerates exactly those wakes. `series: true` moves the series' time instead.
- A shared place can still set a wake (public) and sees it in the block, but cannot cancel or move it: the tools are
  private places only, as the task says.
- The block is not on `context.compiled`'s row (that would be a protocol field and its fixtures): a request that
  carried it is visible only by its text. If the owner wants its digest and lines counted there, it is one optional
  field and a fact.
- `wake.cancel`/`move` are `Write`-class harness tools like `wake.at`, but not exempt from the external-text hold
  (only a one-shot `wake.at` is): in a session that read web text they wait for the operator's trust.
- `reminders` follows this row and changes `PendingWake`: I changed nothing in it (`WakeSet` gained `list`; `MoveTo` and
  `WakeMoved` are new).

## Left open

- Docs for the maintainer (I wrote none outside the AGENTS.md files): a Part III item for the step in `docs/spec/`; the
  version line; `docs/status.md` (its "Updated" line, the recently landed step, the roadmap's `commitments` row and the
  north-star's H8 row, which this closes for wakes); and `docs/design/m7-surface.md`/the north-star plan where
  they say the model "can only set a wake".
- The cockpit's Wakes panel could show the title, days and end (`WakeInfo.ts` has them; no panel change).
- Routines, watches and deferred tasks join the block when their rows land.
- No `WakeInfo`-level fixture in `tests/wire/` (that walk is for notifications): the bytes are held by `tests/wakes.rs`.

## `/tmp/lh/live.py`

```python
#!/usr/bin/env python3
"""theseus-lherl's live check: a scratch daemon on the scripted stand-in model.
Three wakes set by the stand-in, a forced compaction, the block in the next
request, "move the far one" and "cancel the pulse" by scripted calls, and
`theseus wakes` before and after. Usage: live.py <target-dir> <work-dir>"""
import json, os, signal, subprocess, sys, time, datetime, pathlib

T = pathlib.Path(sys.argv[1]); W = pathlib.Path(sys.argv[2])
sim, theseusd, cli = T/"theseus-sim", T/"theseusd", T/"theseus"
W.mkdir(parents=True, exist_ok=True)
PORT = 19448
SOCK = str(W/"sock")
env = dict(os.environ, ANTHROPIC_API_KEY="stand-in", HOME=str(W))
procs = []

def start_model(rules, dump):
    for p in [p for p in procs if p[0] == "model"]:
        p[1].terminate(); p[1].wait(); procs.remove(p)
    rf = W/"rules.json"; rf.write_text(json.dumps(rules))
    pr = subprocess.Popen([str(sim), "fake-model", "--addr", f"127.0.0.1:{PORT}", "--rules", str(rf), "--dump", str(dump)],
                          stdout=open(W/"model.log", "a"), stderr=subprocess.STDOUT, env=env)
    procs.append(("model", pr)); time.sleep(1.0)

def run(*a, check=True):
    r = subprocess.run([str(cli), "--socket", SOCK, *a], capture_output=True, text=True, env=env, timeout=300)
    if check and r.returncode not in (0,):
        print("FAILED", a, r.returncode, r.stdout[-400:], r.stderr[-400:]); 
    return r

cfg = f'''
[server]
state_dir = "{W}/state"
disk_warn_mb = 0
disk_floor_mb = 0
[model]
api_base = "http://127.0.0.1:{PORT}"
api_key_secret = "anthropic_api_key"
[secrets]
anthropic_api_key = "env:ANTHROPIC_API_KEY"
[kernel]
min_repeat_minutes = 1
spend_limit_usd = 50.0
[tools]
projects_dir = "{W}"
[policy]
enforcement = "open"
[catalog."claude-sonnet-5-5"]
context_window = 48000
max_output_tokens = 2000
[memory]
summary_profile = "session"
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = false
'''
(W/"theseus.toml").write_text(cfg)
filler = lambda n: f"filler {n}: " + ("the keeper logged the tide and the wind before the lamp was lit. " * 650)
rules1 = [
  {"when": "set three wakes", "calls": [
     {"name": "wake_at", "input": {"after": "1h", "note": "check PR 123"}},
     {"name": "wake_at", "input": {"after": "30m", "every": "1d", "days": ["mon", "fri"], "until": "2026-11-30T09:00:00+00:00", "note": "pulse"}},
     {"name": "wake_at", "input": {"after": "7h", "note": "far errand"}}], "text": "Set all three."},
  {"when": "what reminders do I have", "text": "From the block: check PR 123, the pulse, the far errand."},
  {"when": "", "text": "Noted. " + "The keeper logged six tides and the operator set three wakes: a PR check, a pulse and a far errand. " * 4},
]
start_model(rules1, W/"dump1")
d = subprocess.Popen([str(theseusd), "--config", str(W/"theseus.toml"), "--socket", SOCK, "--state-dir", str(W/"state")],
                     stdout=open(W/"daemon.log", "w"), stderr=subprocess.STDOUT, env=env)
procs.append(("daemon", d))
for _ in range(100):
    if run("health", check=False).returncode == 0: break
    time.sleep(0.3)
try:
    r = run("--json", "ask", "set three wakes, please")
    sid = json.loads(r.stdout)["session_id"]
    print("session", sid)
    before = run("wakes").stdout
    print("== theseus wakes (before)\n" + before)
    for n in range(1, 6):
        run("ask", "-s", sid, filler(n))
        rows = json.loads(run("--json", "ledger", "--kind", "context.compiled", "-n", "200", check=False).stdout)["rows"]
        strategies = sorted({(str(r["data"].get("strategy")), str(r["data"].get("trigger"))) for r in rows})
        print("after filler", n, "compilations:", strategies)
        if any(st == "compaction" for st, _ in strategies): break
    run("ask", "-s", sid, "what reminders do I have?")
    reqs = sorted((W/"dump1").glob("request-*.json"))
    last = json.loads(reqs[-1].read_text())
    blob = json.dumps(last["messages"])
    lastmsg = last["messages"][-1]["content"]
    lastblock = lastmsg[-1]["text"] if isinstance(lastmsg, list) else lastmsg
    print("== the last request's last block:\n" + lastblock)
    print("compilations:", sorted({(str(r["data"].get("strategy")), str(r["data"].get("trigger"))) for r in json.loads(run("--json", "ledger", "--kind", "context.compiled", "-n", "200").stdout)["rows"]}))
    print("'Set wake ' results in the request after compaction:", blob.count("Set wake "))
    # ids, for the scripted move and cancel
    ws = json.loads(run("--json", "wakes").stdout)["wakes"]
    by = {w["note"]: w for w in ws}
    far, pulse = by["far errand"], by["pulse"]
    now = datetime.datetime.now().astimezone()
    at = (now + datetime.timedelta(days=1)).replace(hour=21, minute=0, second=0, microsecond=0)
    rules2 = [
      {"when": "move the far one", "calls": [{"name": "wake_move", "input": {"id": far["short"], "at": at.isoformat(timespec="seconds")}}], "text": "Moved the far errand to 21:00."},
      {"when": "cancel the pulse", "calls": [{"name": "wake_cancel", "input": {"id": pulse["short"]}}], "text": "Cancelled the pulse."},
      {"when": "", "text": "ok"},
    ]
    start_model(rules2, W/"dump2")
    print("== move:", run("ask", "-s", sid, "move the far one to 21:00").stdout)
    print("== cancel:", run("ask", "-s", sid, "cancel the pulse").stdout)
    print("== theseus wakes (after)\n" + run("wakes").stdout)
    for kind in ("wake.moved", "wake.cancelled"):
        print("==", kind, run("--json", "ledger", "--kind", kind, "-n", "5").stdout[-700:])
finally:
    for _, p in procs[::-1]:
        p.terminate()
    time.sleep(1)
```
