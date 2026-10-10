# CLOUD_REPORT: job-handles (theseus-n8gk)

Branch `cloud/20261010-job-handles`, from `main` at 2fd1f654 (store format 26, unchanged: no stored field or
record kind was added; a job's short id rides in its placeholder result's `meta`, a JSON value).

Commits, in order on top of the task commit:

- `d5eb070` toolrun: a job's handle: proc_run {background}, job_read, job_wait, job_stop, and a job's end read
  in its own turn (theseus-n8gk)
- `60d3a4a` toolrun: job_wait and job_stop take a spooled end themselves, and the handle tests hold under load
  (theseus-n8gk)
- this report (not for main)

## The step

### What I found

- A `proc.run` past `proc_sync_secs` answered "Still running as background job act_… Its result will arrive in a
  later message", and nothing could read, wait for, or stop it. Its completion was the drain's, which queued it on
  the execution (`queued_results`); the turn read the queue only in `catch_up` (turn start) and `finish` (turn
  end, which then asked for a continuation turn). So a job that ended mid-turn waited for a later turn.
- The pieces to build on were there: the job's raw output exists while it runs (`Spool::result_path`, read by
  seek in `job.rs`'s `read_tail`); the job wait's event plumbing (`JobWaits`, the drain's `wake`, a stop's and the
  reconciler's wakes); the one stop (`Kernel::stop_call` + `ToolRuntime::terminate_all`); the late result's
  existing frame (`absorb`, which writes nothing when nothing is queued); and `memory.lookup`'s pattern for an
  async tool that needs the core (`Board` with a `Weak<Core>`).
- A defect met on the way (fixed here, with its test): on a daemon whose notify socket cannot bind (its spool
  path is past `SUN_LEN`, as a long scratch path was on this VM: "notify socket unavailable; heartbeat only"),
  only the 60 s heartbeat drains the spool. A blocking `proc.run` never noticed, since its turn looks at the spool
  itself every second, but my first `job_wait` waited on the drain alone and said "Still running after 30 s" for a
  job that had ended at 20 s. `job_wait`, `job_read` and `job_stop` now take a spooled completion themselves, as a
  turn's look does (`ToolRuntime::job_settled`), so they see an end at their next look. The socket's path limit
  itself is not mine to change: see "Left" below.

### What I changed

- **`proc_run {background: true}`** (theseus-tools `proc.rs`: the field, its schema line, and the description;
  invalid with `steps`): `run_job` skips the wait and answers at once, in the brief's shape:
  `Started job j7 (cargo test --workspace) in /app at 14:02:11; its output goes to <path>.` and
  `job_read j7 shows its latest output, job_wait j7 waits for it, job_stop j7 stops it. When it ends, you get a
  notice between your calls.` Without the flag nothing changes until the window passes; past it the answer is now
  `Still running as job j7 after 60 s (timeout 600 s). job_read, job_wait or job_stop it; its result also arrives
  by itself when it ends.` (the core output golden takes these words; a batch's step that goes on keeps its old
  words, see "Left").
- **Short ids** (`crates/theseus-core/src/toolrun/handles.rs`): `j1`, `j2`, … per session, in the order its jobs
  went on in the background, mapped to the correlation id; the placeholder's meta names it (`"job": "j7"`), so a
  restart reads the map again from the session's tool results (once a session per daemon life). The long id
  still works.
- **The three tools**: schemas and plans in `crates/theseus-tools/src/jobs.rs` (registered in
  `default_registry`, so the config's `[policy.tools]` check and the template test know them); runs in
  `crates/theseus-core/src/toolrun/jobs_tools.rs`, reached from `run_inproc`'s async family match (one line in
  `toolrun.rs`), through the core (`Board::attach`, one line in `rpc/mod.rs`).
  - `job_read {job?, lines = 40}`: `Job j7 (cargo test), running 3 min 12 s; 412 lines so far; the last 40: …`;
    an ended one says how it ended and that `job_wait` gives its result; with no job,
    `This session's 2 jobs running:` and a line each. Scrubbed and capped as every result is (`result_node`).
  - `job_wait {job, timeout_secs <= proc_sync_secs}` (default and ceiling `proc_sync_secs`): a future on each
    settle (`JobWaits::settled`, notified by the drain's take, a stop's wake and the reconciler's), with the turn
    waits' own 1 s backstop look; it gives `job_result` exactly as a blocking `proc.run` would (its external
    marker too, so a listed program's output still holds the session), or `Still running after 300 s. job_read j7
    shows its latest output; its result also arrives by itself when it ends.`
  - `job_stop {job}`: `Kernel::stop_call(corr, "a job_stop call")`, then `terminate_all` (its cgroup where the
    daemon is delegated, else its tree), each verdict's fact recorded; it answers `Stopped job j7 (…): nothing of
    it is left.` (or says the end was not verified) and the output so far, headed `[cancelled: stopped by a
    job_stop call; verified: process tree, 2 processes]`.
  - The gate (`job_planned`, one `and_then` in `ToolRuntime::gate`): the job must be the calling session's; a long
    id of another session's job is refused by name (`Invalid input: job act_… is another session's: only the
    session that started a job reads, waits for, or stops it`), a short id it never gave names nothing.
    `job_read` and `job_wait` are reads (`"job.read" = "open"`, `"job.wait" = "open"` in the template);
    `job_stop` is a run whose plan is the job's own argv and directory, as `term.send` is its terminal's
    (`# "job.stop" = "notify"` in the template).
  - A result that gives the job's end names it (`meta.delivers`); the late result the end still writes then
    reads `Its result was given by your job_wait call already.` and asks for no turn of its own
    (`LateCall.delivered`, counted out in `take_late`).
- **In-turn delivery**: at the top of each loop after the first, before the compile, `take_late` (the existing
  `absorb`, its spans and frame) takes what settled since; the existing `[Background result for your earlier
  proc.run call (…)]` block renders it before the next model call. After a turn ends, the continuation and
  spawn-ask-follow apply unchanged. (The block names the canonical `proc.run`, as it always has; the brief quoted
  `proc_run`. I left the words alone.)
- **One tail reader**: `crates/theseus-core/src/toolrun/peek.rs`, re-exported as `theseus_core::toolrun::peek`
  (and `Peek`): `peek(path, lines, output_max_bytes)`, bounded at 64 KiB read by seek for the lines, and a
  streamed line count only for an output of 8 MiB or less (else its size); it also says when the output has passed
  its file's head and the newest lines wait in the wrapper. `work.peek` can call it with
  `Spool::result_path(correlation_id)` and `[tools] job_output_max_bytes`.
- **Siblings.** `toolrun/calls.rs` is untouched: main has no class tables yet; the tools' own `class()` says
  Read, Read, Run. The parallel-calls joiner should key `job.stop`'s run by its job (`input.job`, resolved to the
  correlation id by `find_job`). output-kept had not joined; `job_read` names no kept file.
- Docs: `crates/theseus-core/AGENTS.md` (a bullet "A job's handle" before Hands) and `crates/theseus-tools/AGENTS.md`.
- Two tests changed their numbers, both because the request's tool list grew: `config::tests::
  example_template_uncommented_still_parses` counts 53 `[policy.tools]` lines (was 50), and `tests_route_keep`'s
  window and billed input rise by 1,000 tokens each (40,000/34,500 to 41,000/35,500), since with the three new
  definitions its first compile rang instead of carrying the recall drop; its doc says so. No assertion was
  removed.

### How I proved it

New tests (all pass):

- theseus-tools: `jobs::tests::a_read_and_a_wait_are_reads_and_a_stop_is_a_run`,
  `proc::tests::background_takes_one_program_and_never_a_batch`.
- theseus-core unit: `toolrun::peek::tests::a_peek_shows_the_last_lines_and_counts_them_all`,
  `toolrun::peek::tests::a_peek_reads_a_bounded_tail_of_a_big_output` (a 40 MiB output: at most 64 KiB read),
  `toolrun::handles::tests::a_short_id_is_j_and_a_positive_number`,
  `toolrun::jobs_tools::tests::a_label_is_the_argv_cut_at_sixty_characters`.
- theseus-core `tests_job_handles.rs` (in process, a task standing in for the drain):
  `a_background_run_answers_at_once_with_its_id_and_output_path` (and the map read again from the placeholder),
  `a_run_past_its_window_answers_with_its_handle`, `job_read_shows_the_newest_lines_and_lists_running_jobs`,
  `job_wait_gives_the_result_inside_its_window_and_says_still_running_past_it` (and the next request carries the
  output once, with the short late line), `job_wait_takes_the_end_itself_when_no_drain_comes`,
  `a_job_that_ends_mid_turn_reaches_the_next_request` (the stand-in's third request carries the block and the
  job's output; its second does not), `another_sessions_job_is_refused_by_name`,
  `the_gate_reads_job_read_as_a_read_and_job_stop_as_the_jobs_run` (gate records: `allow` and no argv for the
  read; `needs_confirm` under `"job.stop" = "approve"`, its plan the job's argv `sleep 1`, `Exec`).
- theseusd `tests/job_handles.rs` (real binary, real wrappers, the stand-in model):
  `a_background_job_is_read_and_waited_for` (`job_wait`'s result is `[exit code 0]\ndone\n`), and
  `a_run_past_its_window_is_stopped_by_its_handle_and_nothing_is_left` (a negative assertion: a `/proc` scan for
  the job's command line and its two pids; it prints what is left when it fails). It never failed except under
  its planted revert.
- Existing proc_run tests are untouched and pass (tests_jobs, tests_steps, job_latency, stops, …).

Planted reverts (each restored, `touch`ed, `git status` clean after):

1. Delivery left for the next turn (the loop-top `take_late` made unreachable): `a_job_that_ends_mid_turn_
   reaches_the_next_request` failed (the third request had no block).
2. `job_stop` not verifying (no `terminate_all`): theseusd's `a_run_past_its_window_is_stopped_by_its_handle_
   and_nothing_is_left` failed, printing the wrapper and the `bash` still alive ("Told job j1 … to stop; its end
   was not verified"). The plant left those processes running; I killed them by their pids.
3. `job_read` unbounded (the tail read with `u64::MAX`): `a_peek_reads_a_bounded_tail_of_a_big_output` failed,
   "read 41943040 bytes".
4. `job_wait` on the drain alone (no `job_settled` in its look): `job_wait_takes_the_end_itself_when_no_drain_
   comes` failed, "Still running after 8 s" for a 0.5 s job.

Under load (AGENTS.md's recipe: the tests at `nice -n 19`, four `yes >/dev/null` at nice 0, killed by pid): the
first round showed three of my tests leaning on wall-clock bounds (a turn takes 4 s under that load, so "answered
in under 1.5 s" and a 3 s job "still running" at the read did not hold). I made them structural (the job's action
is still `dispatched` when the turn has answered; jobs that must outlive the turn run 30 s) and reran: 3 rounds,
9 of 9 each (the tenth, the no-drain test, came after; it passed in the gate).

FAST: `bench turn --check --runs 5 --burst 0` (the lane gate's turn step), after the change:
`frames_plain: 5 frame(s) at the p95, budget 5: ok` and `frames_tool: 9 frame(s) at the p95, budget 9: ok`
(plain p50 51.8 ms, tool-call p50 107.3 ms, debug build on this VM). `tests_m3::a_plain_turn_stays_within_its_
frame_budget` and `tests_jobs::a_loop_with_one_job_costs_four_frames_and_two_looks` pass. The loop-top take reads
the execution once and writes nothing when nothing settled.

Live check on this VM (`target/debug` binaries, `theseus-sim fake-model --rules`, a scratch daemon in `/tmp/jh-live`
with `proc_sync_secs = 30`, enforcement notify), script A then B:

```
== script A: background, read, wait
[0.08 s] Started job j1 (sh -c sleep 20; echo done) in /tmp/jh-live/projects at 22:56:39; its output goes to /tmp/jh-live/state/spool/results/act_….out.
job_read j1 shows its latest output, job_wait j1 waits for it, job_stop j1 stops it. When it ends, you get a notice between your calls.
[0.09 s] Job j1 (sh -c sleep 20; echo done), running 3.1 s; no output yet.
[16.91 s] [exit code 0]
done
A: PASS (the wait's result carries done)
== script B: a 90 s job run blocking, stopped by its handle
[30.07 s] Still running as job j1 after 30 s (timeout 600 s). job_read, job_wait or job_stop it; its result also arrives by itself when it ends.
[0.09 s] Stopped job j1 (bash -c echo $$ > tree.pid; sleep 90 & echo $! > child.pid;…): nothing of it is left.
[cancelled: stopped by a job_stop call; verified: process tree, 2 processes]
(no output)
ps, the job's pids ['20823', '20824'] and its command line: nothing left
B: PASS
```

(The first live run, in a scratch dir whose socket path was too long, is how I found the heartbeat-only defect
above: A failed there, "Still running after 30 s", before the fix.)

### The live check for the maintainer

1. The scripted check (no keys): save the script below as `live.py`, then, from the reviewed checkout,
   `python3 live.py target/release-thin /tmp/jh-live` (any dir whose `state/spool/notify.sock` path stays under
   108 bytes; it starts its own `theseus-sim fake-model` on 127.0.0.1:9461 and its own daemon, and stops both).
   It should print `A: PASS` (the wait's result is `[exit code 0]\ndone`, about 17 s after the read) and
   `B: PASS` (30 s for the blocking run's window, then the stop's answer `Stopped job j1 (…): nothing of it is
   left.` with `verified: process tree, 2 processes`, or `cgroup` on a delegated unit, and `ps` shows nothing of
   the job). The script is in this report's appendix.
2. The real-model check (the capture rig's S14 to S16, at most $0.50): a scratch daemon on the owner's config with
   `[kernel] spend_limit_usd = 0.5`, a fresh workdir with a tiny web app (`index.html`), then
   `theseus --socket <scratch sock> ask "Start a dev server for this directory with python3 -m http.server 8765
   in the background, then write notes.md saying what the page holds, then check the server answers with
   curl -s localhost:8765, then stop the server and tell me how it ended."` It should show a
   `proc_run` with `background: true` answered `Started job j1 …`, an `fs_write`, a `proc_run` of curl, a
   `job_stop j1` answered `Stopped job j1 (python3 -m http.server 8765): nothing of it is left.`, and no
   `http.server` left in `ps`. For "hear its end": `ask "Run sh -c 'sleep 25; echo build ok' in the background as
   a build, meanwhile list the files here and write a summary to notes.md, then tell me how the build ended."`:
   the build's `[Background result for your earlier proc.run call …]` arrives inside the same turn (between calls,
   or through `job_wait`), and the answer names `build ok`.

### Left, uncertain, and choices for the owner

- **The notify socket's path limit** (found here, not fixed, outside this step): a daemon whose
  `<state>/spool/notify.sock` path is longer than `SUN_LEN` (108 bytes) runs "heartbeat only": every background
  job's end waits up to 60 s for the drain. The job tools now look for themselves, and a blocking `proc.run` always
  did, but a late result between turns still waits. Worth a health line or a shorter socket path (a short link in
  `/tmp`, or `--notify-socket`); I did not touch the kernel's wrapper path.
- A batch's step that goes on in the background (`steps.rs`) keeps its old words and gets no short id; its long id
  works with the three tools. Giving it the handle is a small follow-up in `steps.rs`.
- A background job finished within microseconds of its launch can have its completion left to a turn that
  stopped waiting (the launch registers the turn's wait before it spawns, and the background path drops it at
  once); the next drain or heartbeat takes it. Narrow, and the same as the existing past-window path's window.
- The short-id map is in memory, read again from the transcript's tool results at a session's first job (or first
  job tool call) in a daemon's life: one decode of the session's tool results then. Recording the counter on the
  session record would avoid it but bumps the store's format; I chose not to.
- `job_wait`'s backstop look is the turn waits' own (`waits::LOOK`, 1 s); its wake is the settle event.
- `job_stop` of a job whose argv is on an approve list waits for approval, as its run would: the brief's "judged as
  the job's own run".
- The `[Background result …]` block names `proc.run` (canonical), not `proc_run`; unchanged.
- Docs the maintainer may want: the spec's Part III item for theseus-n8gk; `docs/status.md`; the template's
  `[policy.tools]` already says what the three tools are.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run before each commit; on `d5eb070`'s tree: keel ok, fmt, shape ok (no file
over 2,500 lines but the 11 listed; `toolrun.rs` 2,464, `turn.rs` 3,438 of 3,525, `config.rs` 2,926 of 2,927),
features, clippy, cockpit, test build, reader rule, then the suite: 3,697 tests run, 3,664 passed, 33 failed, 44
skipped. The 33 are exactly the known L1 failures (theseus-pv6i: a root daemon's L1 job with no job cgroup): 23 of
theseus-sandbox's contract and bench (`spawn_100`) and 10 of theseusd's `sandbox`. No other test failed. The phases
after it, run by hand: protocol types unchanged; turn bench ok (frames above); `cargo deny --offline check`:
advisories, bans, licenses, sources ok.

On `60d3a4a`'s tree (the head): the same phases, the suite 3,698 run, 3,665 passed, the same 33 L1 failures and
no other; protocol types unchanged; turn bench `frames_plain: 5 … ok`, `frames_tool: 9 … ok`; deny ok; keel ok.
None of the listed timing tests failed in either run.

Keel guard (`python3 scripts/keel-guard.py`, local `main` set to `origin/main` at 2fd1f654): `keel: ok (… 0 findings
acked)`. **Keel findings expected: none.**

No dependency was added (Cargo.lock and the package-lock files are unchanged). No person is named anywhere.

## Appendix: live.py

```python
#!/usr/bin/env python3
"""Live check of theseus-n8gk on a scratch daemon and the stand-in model.

Usage: live.py <bin dir holding theseusd and theseus-sim> <scratch dir>

Script A: proc_run {background: true} of `sleep 20; echo done`, job_read j1,
then job_wait j1: the wait's result (what the request after it carries)
holds `done`. Script B: a 90 s job run blocking, its handle read from the
"Still running" answer, stopped with job_stop; ps shows nothing of it left.
"""
import json, os, re, socket, subprocess, sys, time, tomllib

bins, scratch = sys.argv[1], sys.argv[2]
os.makedirs(scratch, exist_ok=True)
P = lambda *p: os.path.join(scratch, *p)
for d in ("bin", "projects", "state"):
    os.makedirs(P(d), exist_ok=True)

def emit(t, prefix=""):
    """A minimal TOML writer for the template's shapes."""
    out, tables = [], []
    for k, v in t.items():
        key = json.dumps(k) if not re.fullmatch(r"[A-Za-z0-9_-]+", k) else k
        if isinstance(v, dict):
            tables.append((key, v))
        else:
            out.append(f"{key} = {val(v)}")
    for key, v in tables:
        name = f"{prefix}.{key}" if prefix else key
        out.append(f"\n[{name}]")
        out.append(emit(v, name))
    return "\n".join(out)

def val(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return repr(v)
    if isinstance(v, str):
        return json.dumps(v)
    if isinstance(v, list):
        return "[" + ", ".join(val(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{" + ", ".join(f"{json.dumps(k)} = {val(x)}" for k, x in v.items()) + "}"
    raise TypeError(v)

PORT = 9461
rules = [
    {"when": "Start it", "calls": [{"name": "proc_run", "input": {"argv": ["sh", "-c", "sleep 20; echo done"], "background": True}}]},
    {"when": "Read j1", "calls": [{"name": "job_read", "input": {"job": "j1"}}]},
    {"when": "Wait for j1", "calls": [{"name": "job_wait", "input": {"job": "j1"}}]},
    {"when": "Run the long one", "calls": [{"name": "proc_run", "input": {"argv": ["bash", "-c", "echo $$ > tree.pid; sleep 90 & echo $! > child.pid; wait"]}}]},
    {"when": "Stop it", "calls": [{"name": "job_stop", "input": {"job": "JOB"}}]},
]

tmpl = subprocess.run([f"{bins}/theseusd", "example-config"], capture_output=True, text=True, check=True).stdout
t = tomllib.loads(tmpl)
base = f"http://127.0.0.1:{PORT}"
t["model"]["api_base"] = base
for p in t.get("providers", {}).values():
    p["api_base"] = base
t["secrets"] = {k: f"op://Test/{k}/credential" for k in t.get("secrets", {}) if k != "github_token"}
for s in ("discord", "web", "index"):
    t.setdefault(s, {})["enabled"] = False
t["tools"]["projects_dir"] = P("projects")
t["tools"]["proc_sync_secs"] = 30
t["policy"]["enforcement"] = "notify"
t.setdefault("kernel", {})["spend_limit_usd"] = 100.0
open(P("config.toml"), "w").write(emit(t) + "\n")
open(P("bin", "op"), "w").write("#!/bin/sh\ncase \"$1\" in\n  read) printf 'tv-x' ;;\n  inject) sed -E 's#\\{\\{ op://[^}]* \\}\\}#tv-x#g' ;;\n  *) exit 1 ;;\nesac\n")
os.chmod(P("bin", "op"), 0o755)

def start_model(rules):
    json.dump(rules, open(P("rules.json"), "w"))
    return subprocess.Popen([f"{bins}/theseus-sim", "fake-model", "--addr", f"127.0.0.1:{PORT}", "--rules", P("rules.json")],
                            stdout=open(P("model.log"), "w"), stderr=subprocess.STDOUT)

def call(method, params):
    s = socket.socket(socket.AF_UNIX)
    s.connect(P("sock"))
    s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}) + "\n").encode())
    buf = b""
    while True:
        chunk = s.recv(1 << 20)
        if not chunk:
            raise SystemExit(f"{method}: the connection closed")
        buf += chunk
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            v = json.loads(line)
            if v.get("id") == 1:
                if v.get("error"):
                    raise SystemExit(f"{method}: {v['error']}")
                return v["result"]

def turn(text, sid=None):
    p = {"input": text, "author": "live", "attachments": []}
    if sid:
        p["session_id"] = sid
    t0 = time.time()
    r = call("turn.submit", p)
    return r["session_id"], time.time() - t0

def last_result(sid):
    h = call("session.history", {"session_id": sid})
    res = [n for n in h["nodes"] if n["kind"] == "tool_result" and not (n.get("detail") or {}).get("late")]
    return res[-1]["text"]

model = start_model(rules)
env = dict(os.environ, PATH=f"{P('bin')}:{os.environ['PATH']}", OP_SERVICE_ACCOUNT_TOKEN="test-not-a-token")
for k in ("THESEUS_OP_TOKEN_FILE", "THESEUS_CONFIG", "THESEUS_STATE_DIR", "THESEUS_SOCKET"):
    env.pop(k, None)
d = subprocess.Popen([f"{bins}/theseusd", "--config", P("config.toml"), "--state-dir", P("state"), "--socket", P("sock")],
                     env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=open(P("theseusd.log"), "w"))
try:
    for _ in range(400):
        try:
            call("health", None)
            break
        except (OSError, SystemExit):
            time.sleep(0.05)
    print("== script A: background, read, wait")
    sid, took = turn("Start it")
    print(f"session {sid}")
    print(f"[{took:.2f} s] {last_result(sid)}\n")
    time.sleep(3)
    sid, took = turn("Read j1", sid)
    print(f"[{took:.2f} s] {last_result(sid)}\n")
    sid, took = turn("Wait for j1", sid)
    waited = last_result(sid)
    print(f"[{took:.2f} s] {waited}\n")
    print("A:", "PASS" if "done" in waited else "FAIL", "(the wait's result carries done)")

    print("\n== script B: a 90 s job run blocking, stopped by its handle")
    sid, took = turn("Run the long one")
    still = last_result(sid)
    print(f"[{took:.2f} s] {still}\n")
    job = re.search(r"Still running as job (j\d+)", still).group(1)
    rules[-1]["calls"][0]["input"]["job"] = job
    model.terminate(); model.wait()
    model = start_model(rules)
    time.sleep(0.5)
    pids = [open(P("projects", f)).read().strip() for f in ("tree.pid", "child.pid")]
    sid, took = turn("Stop it", sid)
    print(f"[{took:.2f} s] {last_result(sid)[:600]}\n")
    time.sleep(0.5)
    ps = subprocess.run(["ps", "-eo", "pid,cmd"], capture_output=True, text=True).stdout
    left = [l for l in ps.splitlines() if "tree.pid" in l or l.split()[0] in pids]
    print("ps, the job's pids", pids, "and its command line:", left or "nothing left")
    print("B:", "PASS" if not left else "FAIL")
finally:
    try:
        call("shutdown", None)
    except BaseException:
        pass
    try:
        d.wait(timeout=10)
    except subprocess.TimeoutExpired:
        d.kill()
    model.terminate()
```
