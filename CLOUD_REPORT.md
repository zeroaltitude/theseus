# Cloud report: telemetry3 (theseus-kxyc, theseus-6xwq, theseus-qdk5, theseus-gfi4)

Branch `cloud/20261005-telemetry3`, from `main` at 60b43fb6 (store format 20; this branch changes no stored
record, so no format bump). Started 20:22 UTC, report at 22:21 UTC by `date` (deadline 01:22).

| Step | Commit | What |
|---|---|---|
| 1. kxyc | `d0b6e453` | a test: a late result taken by its own turn's `finish` is traced and counted there |
| 2. 6xwq | `62a43087` | a test: a continuation's fresh calls each traced for its own call, in their groups |
| 3. qdk5 | `86993e3c` | `theseus.cancel{backend,state}`, fed by `Stops::count`, health's own count |
| 4. gfi4 | `b49fd1ff`, `b20e7e63` | `theseus.index.*` gauges and restarts, sampled after serving; the second commit makes its tests hold under load |
| docs | `45f23e07` | theseus-core's AGENTS.md names both metrics |

No new dependency; Cargo.lock and the package-locks are untouched. No protocol type changed (protocol.gen
unchanged). No config key added. Line counts: toolrun.rs, turn.rs, turn/calls.rs and telemetry/tests.rs are
untouched; `INSTRUMENTS` grew from 34 to 40 at its end (`CANCELS`, then the five `INDEX_*`).

## Step 1: theseus-kxyc, a late result taken by `finish`

**Found.** As the issue says: `TurnRunner::take_late` (turn/calls.rs) pushes the late spans of results that settled
while the turn ran, at the trace's top level; with it pushing nothing, every test passed. One difference from the
issue's picture: every turn's trace has a `continuation` span (`catch_up`'s `CaughtUp` fact writes it even when
it caught up nothing), so "no `continuation` parent" is asserted as "the late span's parent is the root, and no
`continuation` span holds a `tool` span", not as "no `continuation` span".

**Changed** (`d0b6e453`, telemetry/tests_resumed.rs only). `a_late_result_taken_as_its_turn_finishes_is_traced_there`:
a proc.run of `sleep 1.3` with `proc_sync_secs = 1` answers `background`; the model's next call (its answer to the
placeholder) is held by a provider wrapper (`Held`) until a task, draining the spool with `core.heartbeat` every
50 ms as `wait_queued` does, sees the job's correlation id in the execution's `queued_results`, then releases it.
**Deterministic:** the turn cannot finish before the completion is queued (the model is blocked on it), and the
completion is queued by the drain, not by the turn (whose own result would go to `own.settled`), so `finish`'s
`take_late` always takes it; no sleep decides anything. Asserts: two `tool proc_run` spans (the placeholder,
`background`; then the late one), the late one's parent the root `turn`, `late: true`, `result: ok`,
`tool_use_id: t1`, `run_ms` ≥ 1,300; one `theseus.tool.calls` series, `proc.run`/`ok` = 1, and its duration's min
≥ 1,300 ms. `rig` gains `rig_with(provider, …)`.

**Proved.** Passes; planted revert, `take_late` pushing no span (its loop replaced by `let _ = late_spans;`): only
this test fails (`assertion left == right failed: its placeholder and its late result`, 1 span for 2), every other
telemetry test passes (54 of 55). File restored and touched; `git status` clean.

## Step 2: theseus-6xwq, the continuation's fresh calls

**Found.** As the issue says. With `index: r.index` or `group: r.group` in `ResumeOutcome::ran_batch`, every test
passed.

**Changed** (`62a43087`, telemetry/tests_resumed.rs only). `a_continuations_fresh_calls_are_traced_each_for_its_own`:
calls `[a1 proc_run (echo), b1 fs_read one.txt, c1 fs_read two.txt]`, `enforcement = open` and `[policy.tools]
"proc.run" = "approve"`; approved. Asserts the continuation's trace holds exactly
`(continuation, tool proc_run, a1, ok)`, `(tools, tool fs_read, b1, ok)`, `(tools, tool fs_read, c1, ok)`; one
`tools` span (`calls: 2`) whose parent is `continuation` and which starts after A's span ends; the
continuation's children are `[tool proc_run, tools]` in that order; `theseus.tool.calls` is proc.run 1 and
fs.read 2, two series. The two reads did run together (one group), so the `tools` span is asserted, not optional.

**Proved.** Passes. Planted reverts, each alone, toolrun.rs restored and touched after each:
- `index: r.index`: only this test fails (171 tests of the telemetry, tests_m3 and continuation suites run; 170 pass).
- `group: r.group`: only this test fails (170 of 171 pass): A and the reads share group 0, so all three sit
  under one `tools` span.

## Step 3: theseus-qdk5, `theseus.cancel`

**Found.**
- The issue's premise has moved, as the brief says: a count outside a turn has a path (`record_push`,
  `record_judgment`, …). So no new export path: one `record_cancel` call where health counts.
- **Choice: `Stops` holds a telemetry handle** (`OnceLock<Telemetry>`, `Stops::export_to`), set where the judge's is:
  in `Core::build` when the parts bring a pipeline (tests), and in `Core::build_telemetry` once the daemon builds
  it after serving (the daemon's telemetry is built after serving, never "as the core is built", so it's set
  there, beside `runner.judge.export_to`). `Stops::count`, health's own counter, records the metric in the same
  call, so health and the metric agree by construction, and every stop path (cancel, task cancel, `/stop`, the
  disk's floor, a stop at launch) is covered without finding each one. toolrun.rs gains no line; cancel.rs gains
  the field, `export_to`, and three lines in `count` (its stops and verdicts are untouched).
- **The label question (not changed; reported as asked).** An L0 job stopped by its own cgroup (a delegated daemon,
  theseus-a5nv) has a `VerifiedBy::Cgroup` verdict, scope `cgroup`, and `job_backend` names it `l1`. The action does
  **not** tell its level in general: the kernel keeps a proposal on the action only for a call that asks
  (`plan(…, keep_proposal)`, true only in `plan_confirm`), and an L0 proposal carries no class key anyway, so an
  open or notify L1 call's action looks like an L0 one's. Old L1 records wrote the same `VerifiedBy::Cgroup` and
  scope `cgroup` (job_l1.rs before 13783824). What does tell: since theseus-gyin no L1 wrapper of this build writes
  `Cgroup`, and `terminate_all` names only verdicts of a stop it ran just now, so a live `Cgroup` verdict is an L0
  job's unless its wrapper is a pre-gyin binary still running across a swap. **Recommendation:** name `Cgroup`
  `l0` in `job_backend` (it's only ever called on live verdicts), with a test in theseusd's tests/cgroup.rs where a
  delegated scope exists; or keep a job's class on its action. On a delegated daemon today, health and the metric
  both say `l1` for every L0 job cancelled.

**Changed** (`86993e3c`): `CANCELS` (`theseus.cancel`, IntSum, attributes `theseus.cancel.backend` and
`theseus.cancel.state`, valued `verified`/`uncertain`/`unsupported` and `l0`/`l1`/`async`/`inproc`/`job`/hands
backends exactly as health names them), `Metrics::cancel`, `Telemetry::record_cancel`, `Stops::export_to`, and the
two `export_to` calls in rpc/mod.rs. Tests in telemetry/tests_cancel.rs (new), a whole core posting to the receiver:
- `each_cancel_is_counted_by_its_backend_and_state_as_health_counts_it`: an http.fetch that hangs, cancelled (as
  tests_cancel.rs does), and a background proc.run whose wrapper is a `sh job-wrapper --correlation-id <id>`
  process in its own group (the in-process launcher leaves no pid; the same stand-in tests_m3 uses), cancelled:
  verdicts `task` and `group`; health `[(async, verified, 1), (l0, verified, 1)]`, and the metric's points equal it.
- `a_cancel_nothing_reaches_is_counted_unsupported`: a background job with no wrapper: `inproc`/`unsupported` in
  health and the metric alike.

**Proved.** Both pass; the core's tests_cancel.rs passes. Planted reverts (cancel.rs restored and touched after
each): the feed removed fails both tests; every cancel fed as `verified` fails
`a_cancel_nothing_reaches_is_counted_unsupported` (which is why it exists: both cancels of the first test are
verified). The old-exporter conformance test passes unchanged (no cancel there, so no new point).

## Step 4: theseus-gfi4, the tender's gauges and restarts

**Found / chosen.**
- **Names**, as §3.20's others (`theseus.durability.lag_ms`, `theseus.node_cache.bytes` in `By`):
  `theseus.index.lag_bytes` (By), `theseus.index.lag_ms` (ms), `theseus.index.documents`, `theseus.index.rss_bytes`
  (By), gauges (`Kind::IntLast`) of the tender's answer; `theseus.index.restarts`, a counter (IntSum).
- **Restarts** are fed the rise of `TenderStatus.restarts` between samples (`Metrics.index_restarts` keeps the last,
  as `node_cache` does), so the supervisor gains no hook. The first sample creates the series at 0.
- **The sampler: a task after serving** (`Core::sample_index_after_serving`, tender/sample.rs), started from
  theseusd's `after_serving` beside `core.index.run()` (the socket daemon only), and only when
  `[telemetry] otlp_endpoint` is set and `[index]` is on; it returns whether it started. Every
  `metrics_interval_secs` it takes `health_block()` and records it (`Telemetry::record_index`). It holds the core
  by `Weak`. `IndexTender::sample` asks nothing while telemetry is off (not yet built, or failed). Why not the
  exporter's tick: the tick is the sender's loop, and an ask there would put a socket call (up to 100 ms) inside
  the export, and telemetry/export.rs would gain a callback into the core; the task keeps the exporter as it is.
  The cost: a gauge may be up to one interval older than the post that carries it.
- **What the gauges show while the tender is down or late:** `health_block` asks no socket unless the supervisor
  runs a tender. Down (backoff, absent, not yet started): no status, so the four gauges keep the last answer they
  had, and `restarts` moves when it starts again. Late (running, no answer in 100 ms): `health` gives its last
  answer, which is sampled again (the same values). So a flat lag can hide an outage; `restarts` and health show
  it. If the owner wants the gauges to drop out while down, `Metrics::index` can remove the four points when
  `status` is `None`; I didn't, to keep series continuous.
- **FAST:** nothing on the start path or a turn's; at most one ask (≤ 100 ms) an interval, after serving; nothing
  without an endpoint.

**Changed** (`b49fd1ff`): metrics.rs (five instruments, `Metrics::index`), telemetry.rs (`record_index`), tender.rs
(`mod sample;`), tender/sample.rs (new), theseusd's main.rs (one call, beside the tender's start),
tests_tender.rs (helpers `pub(crate)`; the stand-in now answers a lag of 4096 B / 250 ms and an RSS of 48 MiB, and
returns a count of the requests it read). Tests, telemetry/tests_index.rs (new): a sample records the stand-in's
numbers (documents 3, not its nodes 2); a kill and restart raise the counter by one over three samples; a hung
stand-in keeps each sample within the deadline (≥ 100 ms, < 2 s) and the gauges keep its last answer; nothing
asks before the tender runs (only `restarts`, 0, is recorded) or with telemetry off; a core without an endpoint, or
with `[index]` off, starts no sampler; a core with both samples each interval (1 s).

`b20e7e63` (load): under the load recipe, two of these tests failed three times in six runs (`nothing_asks…`
twice, `a_sample_records…` once; that commit's message says "once each", which is wrong), each on its
**positive** check: a sample's one ask under the 100 ms deadline gave up before the stand-in, on the same
current-thread runtime at nice 19, answered (`[None, None, None, None, Some(0)]` for the numbers; `left: 0, right:
1` for "a sample asks a running tender"). The negative assertions (nothing asked before the tender runs, nothing
with telemetry off) held in every run. The tests now take the status once under `index.status`'s 2 s deadline
before sampling, so a late sample records that last answer (the designed behaviour), and the "it asks" check
samples until the stand-in read a request (at most 50 tries).

**Proved.** All five pass. Planted reverts (metrics.rs restored and touched after each): restarts fed the total
instead of its rise fails `a_kill_and_restart_raise_the_restarts_by_one` (3 for 1); `documents` read from `nodes`
fails `a_sample_records_the_tenders_numbers` and `a_hung_tender_keeps_each_sample_within_the_deadline` (2 for 3),
checked again after `b20e7e63`.

## Proof across the steps

- Offline: `cargo nextest run -p theseus-core -E 'test(/telemetry::|tests_tender|tests_cancel/)'`: 71 tests, all
  pass unloaded (every telemetry test, the core's tests_cancel.rs, tests_tender.rs, the conformance test against
  `testdata/old-exporter.json` unchanged).
- The core's output golden is unchanged (no golden line moved; `tests_output` passes in every gate with
  `TZ=America/Phoenix`).
- **Under load** (four `while :; do :; done` loops at nice 0, the tests at nice 19, after building unloaded), the
  same 71 tests, 3 runs after `b20e7e63`: 70 of 71 pass each run; the one failure each time is
  `telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is` (`each turn's trace: left 4, right 3`),
  the known theseus-qjd6 (the exporter's retry counted as a fourth trace; timing-flakes fixes it). Before
  `b20e7e63`, two sets of three runs showed the step-4 failures above: `nothing_asks…` in run 1 of the first set
  (with all 71 tests) and run 2 of the second (step 2 to 4's files only), `a_sample_records…` in run 1 of the
  second.
- Note: a nice-19 `cargo nextest run -p theseus-core` under the loops first rebuilds theseus-core's test binary
  (its `-p` features differ from the gate's workspace build), which starved for 30 minutes; I stopped it by its
  pids and built unloaded first.

## The live check

I ran both checks on a scratch daemon here (debug build of `b20e7e63`, fresh state dir, Discord and the web off),
and they showed what the brief asks; the maintainer's run on the install build is still the record. Commands, with
`S=/tmp/live` and `B=` the build's directory:

```bash
mkdir -p $S && cd $S
# a loopback OTLP/HTTP sink: each POST saved as posts/<n>-v1_<kind>.json
cat > sink.py <<'PY'
import http.server, sys, os, itertools
os.makedirs("posts", exist_ok=True); n = itertools.count()
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("content-length", 0)))
        open(f"posts/{next(n):05d}-{self.path.strip('/').replace('/', '_')}.json", "wb").write(body)
        self.send_response(200); self.send_header("content-length", "2"); self.end_headers(); self.wfile.write(b"{}")
    def log_message(self, *a): pass
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
PY
echo '[{"when": "run the tide", "calls": [{"name": "proc_run", "input": {"argv": ["sleep", "60"]}}], "text": ""}]' > rules.json
echo sk-test > key && chmod 600 key
cat > theseus.toml <<EOF
[model]
api_base = "http://127.0.0.1:9448"
api_key_secret = "anthropic_api_key"
[secrets]
anthropic_api_key = "file:$S/key"
[server]
state_dir = "$S/state"
socket = "$S/theseus.sock"
[discord]
enabled = false
[web]
enabled = false
[tools]
proc_sync_secs = 1
[policy.tools]
"proc.run" = "open"
[telemetry]
otlp_endpoint = "http://127.0.0.1:4319"
metrics_interval_secs = 5
EOF
python3 sink.py 4319 & $B/theseus-sim fake-model --rules rules.json &
$B/theseusd --config $S/theseus.toml > daemon.log 2>&1 &    # [index] is on by default; theseus-index beside theseusd
T="$B/theseus --socket $S/theseus.sock"
# a small reader of the last metrics post
last() { python3 -c 'import json,glob;b=json.load(open(sorted(glob.glob("posts/*v1_metrics.json"))[-1]))
for m in b["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]:
  if m["name"].startswith(("theseus.cancel","theseus.index")):
    for p in m["sum"]["dataPoints"]: print(m["name"],{a["key"]:a["value"]["stringValue"] for a in p.get("attributes",[])},p["asInt"])'; }
```

1. The cancel:
   ```bash
   $T ask "run the tide script"      # the cwd ($S) is outside the roots, so it asks: approve it
   $T confirm <the act_ id it prints>  # the call answers `background` after 1 s
   $T executions                      # the exe_ id, waiting on 1 call
   $T executions cancel <exe_ id>     # "⏹️ cancelled proc.run … (verified: process tree, 1 process)"
   $T health | grep cancels           # "cancels since the start: l0 1 verified"
   sleep 6; last                      # theseus.cancel {backend: l0, state: verified} 1
   ```
   Seen here: exactly that (one point, `l0`, `verified`, 1), health agreeing.
2. The tender:
   ```bash
   $T index status                    # documents ("N nodes in N chunks": the chunks are `documents`), "0 B behind", rss
   sleep 6; last                      # theseus.index.documents N, lag_bytes 0, lag_ms 0, rss_bytes ≈ the rss, restarts 0
   $T health | grep ^children         # "index tender running (pid P, 0 restarts)"
   kill -9 P; sleep 12
   $T health | grep ^children         # "index tender running (pid P2, 1 restart)"
   last                               # theseus.index.restarts 1
   ```
   Seen here: documents 5 (index status: "5 nodes in 5 chunks"), lag 0 and 0, rss_bytes 22,028,288 (status:
   "rss 21.0 MB"); after the kill, restarts 1 and a new pid.
3. Stop: `$T shutdown`, then kill the sink's and the stand-in's pids.

## Left, uncertain, and for the owner

- **The `l1` label of an L0 cgroup stop** (step 3 above): reported, not changed; a one-line change with a test
  where a delegated scope exists.
- **Gauges while the tender is down keep the last answer** (step 4): a choice the owner may want the other way.
- **qjd6** fails under load in every run, as listed; nothing of mine touches it.
- **Docs the maintainer may want** (not edited, per the brief): the spec's §3.20 table
  (docs/spec/04-part1-s3.10.md, the "turns, tokens, …" row's neighbours) could list `theseus.cancel` (backend,
  state) and `theseus.index.lag_bytes`, `lag_ms`, `documents`, `rss_bytes` (gauges, sampled each metrics interval
  after serving) and `theseus.index.restarts`; m4-boundaries.md §2.11 and m6-memory.md §2.13 could note they are
  built; docs/status.md's telemetry line. theseus-core's AGENTS.md is updated on this branch.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before every commit (six runs): fmt, shape,
features, clippy, cockpit, test build, and the reader rule pass each time; the suite runs 2,790 to 2,798 tests and
fails only the 33 known L1 tests of a root VM (theseus-pv6i): theseus-sandbox's 19 contract tests and its bench's
`spawn_100`, and theseusd's 13 sandbox tests (counted in the last gate's log). Every other test passes, the core's golden included (no flaky retry
needed). After the suite I ran the phases it stops before: the protocol types (unchanged), the turn bench
(`theseus-sim bench turn --check --runs 5 --burst 0`: 5 frames plain, 9 with a tool, both at budget), and
`cargo deny --offline check` (advisories, bans, licenses, sources ok); the lifecycle and jobs benches are skipped by
`THESEUS_GATE_NO_BENCH`. So each commit counts as green under the brief's rule.
