# CLOUD_REPORT: telemetry-resumed (theseus-8pei, theseus-0zm4)

Branch `cloud/20261005-telemetry-resumed`, from `main` at faaa9df6 (store format 17, unchanged: nothing stored
changed). Started 09:35 UTC, report at about 10:50 UTC.

| commit | what |
| --- | --- |
| `82a2cade` | turn: the continuation's calls and the late results are traced in the turn that answers them (steps 1, 2, and 4's fix and test) |
| `081994e8` | telemetry: each tool call counts once, at its answer, timed by its run (step 3) |
| `7d6ce7bd` | docs: theseus-core's AGENTS.md says where a call's span is built and when it counts |

## What I found (and how the code differs from the issues)

- As the brief says, since C2 a fact's `span` writes into its turn's trace, and nothing in `resume.rs` or `late.rs`
  can make one. So the calls come back to the turn: `ResumeOutcome` gains `calls: Vec<ToolUse>` and `ran: Vec<Ran>`
  (a `Ran`'s `index` points into `calls`), and `absorb` returns `Vec<LateCall>` in place of its `u32`.
- `run_fresh`'s calls (the ones after the call that asked, never gated in their turn) had `Ran`s and no span. They
  now go into `ResumeOutcome` with their groups shifted (`ResumeOutcome::ran_batch`), so a group that ran together
  still sits under one `tools` span inside the continuation.
- **A gap the issue didn't name:** `finish` absorbs too, taking late results that settled while the turn ran. Its
  late results are traced as well (`TurnRunner::take_late`, in `turn/calls.rs`), at the trace's top level rather
  than under a continuation span, because that turn has none for them.
- `run_confirmed`, `run_authorized`, `check_dispatched` and `answer_settled` now return the call's `CallOutcome`
  (they used to return an `Option<String>` job, or nothing). `answer_cancelled` returns its `ResultStatus`. The loop
  in `resume` builds `background` from the outcome. I didn't touch `late_results`' placeholder search or resume's
  filters (batch 6's lines). The late call's wire name comes from the registry by canonical name, with
  `theseus_tools::wire_name` as the fallback, not from the transcript.
- **A late result's time:** I took the recommended design. Each late result is a `tool <wire>` span: a point at its
  absorption, with `late: true` and `run_ms`. `run_ms` is the action's `settled_at_ms - dispatched_at_ms`, falling
  back to the node's `duration_ms`. `spans::tool_calls` takes `run_ms`, when present, as the call's time.
- **Counted once: moved, as recommended.** `Metrics::tool_calls` skips spans whose `result` is `awaiting_confirm` or
  `background`. Each call counts once, at its answer, by its result:
  - in its own turn, when it ran there;
  - in the continuation, when it waited for the operator (approved, declined, superseded, unknown after a restart);
  - at its late result, for a background job, timed by the job's run.

  The proposing turn's span keeps `awaiting_confirm` or `background`, so the trace still says what happened then.
  A confirmed call that goes to the background inside its continuation is traced `background` there and counted at
  its late result.
- `turn.rs` went from 3,499 to 3,439 lines. `trace_calls` and `call_result` moved to `turn/calls.rs`, which also
  holds `call_spans`, `late_spans` and `take_late`. `crate::turn::call_result` is re-exported under `cfg(test)` for
  the existing test.
- `Ran` now derives `Debug, Clone`, because `ResumeOutcome` derives them.

## How it was proved

- **New tests**, 4 in all:
  - `telemetry/tests_resumed.rs` (new file; tests.rs untouched except for the points that moved):
    - `a_confirmed_calls_run_is_traced_in_its_continuation` (step 1): proc.run under `enforcement = approve`,
      approved, then the turn it resumes. Its `tool proc_run` span is under `continuation`, result `ok`, timed by
      its 0.4 s run. The proposing turn's span still says `awaiting_confirm`.
    - `a_late_result_is_traced_once_with_its_jobs_run` (step 2): `proc_sync_secs = 1`, a 1.6 s job. The next
      turn holds exactly one `tool proc_run` span under `continuation`, with `late: true`, `ok`, `run_ms` ≥ 1,600,
      and zero length.
    - `a_confirmed_call_and_a_background_job_are_counted_once_by_their_runs` (step 3): a confirmed 0.4 s call and
      a confirmed 1.6 s call that goes to the background, across four turns. `theseus.tool.calls` has a single
      series (`proc.run`/`ok`) with value 2; the duration's count is 2, sum ≥ 2,000 ms, min ≥ 400 ms.
  - `aws/tests_resumed.rs`:
    - `an_approved_aws_calls_requests_are_spans_under_its_call_in_the_continuation` (step 4, 0zm4):
      `[policy.aws] read = "approve"`, DescribeStacks approved. Its `aws cloudformation:DescribeStacks` span is
      under its call's span in the continuation, with the row's correlation id, inside the call's bounds.
      `Aws::waits_for_trace("t_read")` (new, test-only) is false.
- **Existing tests:** all of telemetry's tests and `tests_continuations` pass. The points the rule moves:
  - `tool_calls_are_counted_and_timed_by_name_family_backend_and_outcome`: the synthetic turn's `background`
    (60 s) and `awaiting_confirm` (1 ms) series are gone, so the shell-fallback ratio is (4, 8), not (6, 10).
  - `a_turns_calls_reach_the_tool_metrics_as_the_runtime_ran_them`: the waiting proc.run is no longer a point
    (3 points, not 4). Its span still says `awaiting_confirm`.
- **The output golden** moved four lines, each a new child span under a continuation's `continuation [tool]` line,
  run with `TZ=America/Phoenix`:
  - line 347: `tool proc_run … "late":true,"result":"ok","run_ms":#`, a late result;
  - line 487: `tool fs_write … "result":"ok"`, an approved write;
  - line 622: `tool fs_write … "result":"declined"`;
  - line 759: `tool fs_write … "result":"cancelled"`, a call not run because new input came.
- **Frames and FAST:** `a_plain_turn_stays_within_its_frame_budget` passes. `theseus-sim bench turn --check --runs 5
  --burst 0`: plain 5 of 5, tool 9 of 9, after each commit. The spans are built from what the calls returned. Nothing
  waits on them or reads the store for them.
- **Under load:** four `busy.sh` loops at nice 0, and `nice -n 19 cargo nextest run -p theseus-core -E
  'test(tests_resumed)'` five times: 4 of 4 passed each time (about 2.5 s a run). The loops were killed by their
  pids.
- **Planted reverts**, each restored, `touch`ed, and followed by `git status` showing a clean tree:
  1. Continuation spans dropped (`catch_up` passed `&resumed.calls[..0]`, `&resumed.ran[..0]`). Failed:
     `a_confirmed_calls_run_is_traced_in_its_continuation`, the AWS test, and the step 3 test.
  2. A late result timed by its absorption, not its run (`spans::tool_calls` ignoring `run_ms`). Failed: the
     step 3 test ("their runs: 414.463 ms").
  3. A moved call also counted in its proposing turn (the metrics filter removed). Failed: the step 3 test,
     `tool_calls_are_counted_…`, and `a_turns_calls_reach_…`.
  4. AWS spans not taken (`call_spans` given no `Aws`). Failed: the AWS test, and the existing
     `an_aws_call_through_the_core_is_a_row_a_span_and_a_result`.

## The live check (I ran it here on a fresh scratch daemon; please rerun it on the owner's machine)

The files I used are below. Put them in a scratch directory `$D`, with `$B` the build's binaries (`target/debug` or
the release-thin ones). On the owner's machine, pick ports that are free there: 9448 for the model, 4318 for the sink.

`$D/rules.json`:
```json
[
  {"when": "short script", "calls": [{"name": "proc_run", "input": {"argv": ["sh", "-c", "sleep 1; echo tide"]}}]},
  {"when": "tide script", "calls": [{"name": "proc_run", "input": {"argv": ["sh", "-c", "sleep 3; echo tide"]}}]},
  {"when": "long job", "calls": [{"name": "proc_run", "input": {"argv": ["sleep", "8"]}}]}
]
```

`$D/theseus.toml` (replace `$D` with the path; Discord and the web are off, so no bindings file is involved):
```toml
[model]
api_base = "http://127.0.0.1:9448"
[secrets]
anthropic_api_key = "env:FAKE_ANTHROPIC_KEY"
[server]
state_dir = "$D/state"
socket = "$D/state/theseus.sock"
[tools]
projects_dir = "$D/work"
proc_sync_secs = 2
[policy]
enforcement = "approve"
[discord]
enabled = false
[web]
enabled = false
[telemetry]
otlp_endpoint = "http://127.0.0.1:4318"
metrics_interval_secs = 5
```

`$D/sink.py` (saves each POST as `<n>-v1_traces.json` or `<n>-v1_metrics.json`):
```python
import http.server, itertools, pathlib, sys
out = pathlib.Path(sys.argv[2]); out.mkdir(exist_ok=True)
n = itertools.count()
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("content-length", 0)))
        (out / f"{next(n):04d}-{self.path.strip('/').replace('/', '_')}.json").write_bytes(body)
        self.send_response(200); self.send_header("content-type", "application/json"); self.end_headers()
        self.wfile.write(b"{}")
    def log_message(self, *a): pass
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
```

Commands:
```sh
mkdir -p $D/work
python3 $D/sink.py 4318 $D/otlp & SINK=$!
$B/theseus-sim fake-model --rules $D/rules.json & MODEL=$!
FAKE_ANTHROPIC_KEY=sk-fake $B/theseusd --config $D/theseus.toml > $D/daemon.log 2>&1 &
S=$D/state/theseus.sock
# 1. A confirmed call that runs inside its continuation.
$B/theseus --socket $S ask "run the short script"        # parks: prints act_… (exit 6)
$B/theseus --socket $S confirm --approve <act_…>         # "← proc.run ok · exit 0 · ~1013 ms"
$B/theseus --socket $S --json ledger --kind turn.trace -n 1
# 2. A confirmed `sleep 8` that goes to the background (proc_sync_secs = 2), and its late result.
$B/theseus --socket $S ask "run the long job"
$B/theseus --socket $S confirm --approve <act_…>         # "← proc.run background"
sleep 10; $B/theseus --socket $S --json ledger --kind turn.trace -n 3
sleep 6; ls $D/otlp | tail -1                             # the newest *-v1_metrics.json
$B/theseus --socket $S shutdown; kill $MODEL $SINK
```

What each showed here (and should show there):
- **1.** The continuation's trace: `continuation` > `tool proc_run` `{result: "ok", tool_use_id: "toolu_fake_0"}`,
  1,019 ms. The proposing turn: `loop 0` > `tool proc_run` `{result: "awaiting_confirm"}`.
- **2.** Three traces: the proposing turn's `awaiting_confirm`; the continuation's `continuation` > `tool proc_run`
  `{result: "background"}`, about 2,006 ms; the late result's turn: `continuation` > `tool proc_run`
  `{result: "ok", late: true, run_ms: 8012}`, zero length.
- **Metrics:** in the newest metrics file, `theseus.tool.calls` has only `proc.run`/`ok` points, with no
  `awaiting_confirm` or `background` series. Here, after three proc.runs (a 3 s background job run first, then
  the two above): `asInt` 3, and `theseus.tool.duration_ms` count 3, sum 12,046.8 ms (3,015 + 1,019.8 + 8,012),
  max 8,012. So the `sleep 8` is about 8 s, once. With only the two runs above it should be count 2, about
  9,030 ms.

## Left open, uncertain, and for the owner

- **§3.23 needs this line:** "`theseus.tool.calls` and `theseus.tool.duration_ms` count each call once, at its
  answer, by its result: in its turn when it ran there; in the continuation when it waited for the operator
  (approved, declined, superseded, unknown after a restart); at its late result for a background job, timed by the
  job's run (`run_ms`). The proposing turn's span keeps `awaiting_confirm` or `background`." The spec is the
  maintainer's to edit. `docs/status.md` should note that telemetry2's last step landed.
- **Calls that never count:** a waiting call answered by `answer_after_cancel` (its execution cancelled, so no turn
  runs), and a background job whose late result no turn ever takes (a cancel's sweep writes its end). Neither has a
  span, so neither is a point. Before this change both counted once, as `awaiting_confirm` or `background`. Tracing
  the cancel's answers would need a trace outside a turn.
- **`theseus.task.changes`** reads the same spans, filtered to `ok`. A layer-1 task edit approved in a continuation
  now counts there, where before it never did. That looks right, but it is a change.
- **`TurnSubmitResult.tool_calls`** still counts only the calls `run_tools` ran (the turn's own loops). The
  continuation's answered calls and its late results are in its trace and the metrics, not in that number.
- **A late result's span has no `outcome` (Debug) attribute**, since no `CallOutcome` exists for it. It has
  `result`, which is what the metrics and `failed()` read.
- **Commit split:** the brief's steps 1 and 2 are one commit (`82a2cade`). Their code shares `turn/calls.rs` and
  `catch_up`'s one record, and step 4's test rode in the same commit, because its fix is that commit's.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Each time it failed only in its
suite phase, on this VM's known L1 cases:
- `theseus-sandbox::contract`'s clauses, including the egress_18b tests, `exit_status_and_signals`,
  `sigterm_is_forwarded_to_the_command` and `scratch_is_reported_and_discarded`;
- `theseus-sandbox::bench spawn_100`;
- 13 `theseusd::sandbox` tests (a root daemon's job has no job cgroup, theseus-pv6i).

That is 33 failed, with 2,544 of 2,577 passing in the last run. One test failed and then passed on a retry, and it is
on the flaky list: `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults` (the put-back check,
theseus-81ig). The phases after the suite, run by hand each time, all passed: the protocol types unchanged, the turn
bench (5/5, 9/9), and `cargo deny --offline check` (advisories, bans, licenses, sources ok). The timing tests on the
brief's list and `the_deadline_stops_the_whole_tree_too` didn't fail.

**A note on this VM:** one gate run (between `82a2cade` and `081994e8`) failed 94 tests, every one that runs a job.
The disk was nearly full (797 MB free) because `target/debug/incremental` had grown to 17 GB. I deleted that cache,
which rebuilds itself, and the rerun was back to the 33. A long session here may need the same.
