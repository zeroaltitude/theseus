# CLOUD_REPORT: telemetry-calls (theseus-lmhp, theseus-ku5f)

Branch `cloud/20261005-telemetry-calls`. Commits: step 1 `251fdb37`, step 2 `20ac831f`.

## Step 1: `error.type` on failed provider calls (theseus-lmhp)

**Found.** `provider_calls` put a failed call (class on its span's `error`, from `ModelCallFailed`) in the answered calls' series.
The code matched the brief, with these differences:
- A call a `/stop` cut has class `stopped`.
- A refusal closes through `ModelAnswered` (`stop_reason` `refusal`), has no `error`, and so gets no `error.type`. Its fallback's span is a second provider span on the fallback's model.

**Changed.** `spans.rs`: `semconv::ERROR_TYPE`, `error_type()`, `ProviderCall.error_type`, and `gen_ai` adds `error.type` to the span. `metrics.rs`: `provider_calls` adds it to `theseus.provider.call.duration_ms`.
`failed()`'s ERROR status and the flattened `error` attribute are unchanged.

**Design choices.**
- `stopped` gets its own `error.type`. The call never answered, and its time is cut at the stop, so among the answers it would pull their percentiles down. A cancelled tool call is left out of failure because no time series reads it; here the series would be skewed.
- `first_token_ms` carries no `error.type`. A call that failed mid-stream can have a first token, and that time is real, so it stays in the model's one series. A test pins this (a `stream` failure with a first-token mark).
- Spans with no `error` have no `error.type`. A span whose `error` is not text has none either.

**Proof.**
- New file `telemetry/tests_calls.rs`, 5 tests:
  - two answered calls and one `rate_limited` give two series, the failure's with `error.type` and a count of 1 and a time of 150 ms;
  - a refusal and its fallback carry none, on the metrics and on the spans, with no ERROR status;
  - a `stopped` call is its own series;
  - a failed turn's calls (`stream`, `overloaded`) split the same way;
  - the span carries `error.type`.
- `cargo nextest run -p theseus-core -E 'test(telemetry)'`: 40/40 after step 1, 43/43 after step 2.
- Points step 1 moved, both in the old-exporter conformance test:
  - **The failed call's series.** The golden's `provider.call.duration_ms` point (count 3, zai/glm-5.1) is now two points: count 2 without `error.type`, and count 1 with `error.type = rate_limited`. This is the point the task meant to move, so I edited `testdata/old-exporter.json` (replacing that one metric; 78 lines added, 5 removed).
  - **The span.** The golden's spans can't carry `error.type`, which the old exporter never wrote. `tests.rs` gains `without_error_type`, so the span picture is compared without that one attribute.
  - Every other telemetry test is unchanged. The core's output golden is unchanged (it pins no metric).
- Planted reverts (file restored and `touch`ed, `git status` checked each time):
  - `error.type` dropped from the duration: 4 fail (`a_failed_calls_time_is_its_own_series_beside_the_answered_ones`, `a_failed_turns_calls_split_by_error_type_too`, `a_call_a_stop_cut_is_timed_as_stopped`, and the old-exporter picture).
  - A refusal given `error.type`: `a_refused_call_and_its_fallback_are_answers_and_carry_no_error_type` fails.

## Step 2: AWS metrics (theseus-ku5f)

**Found.** `aws::span` records `rpc.service`, `rpc.method` and `status` (`ok`, `unbound`, `error`) on a `kind = "aws"` span. The AWS error's code is on the span as `error` only when AWS answered one (and only when `error_code` is set).

**Changed.** `metrics.rs`: `AWS_CALLS` (`theseus.aws.calls`, int sum) and `AWS_DURATION` (`theseus.aws.duration_ms`, ms histogram), after `RETENTION_NODES` in `INSTRUMENTS` (28 → 30). The new `aws_requests` walks the whole trace, as `lsp_requests` does, and is called from `turn` and `failure`.
Attributes: `rpc.service`, `rpc.method`, `theseus.outcome` (the same name `lsp.request` and `file.read` use).

**Design choices.**
- The code is not `error.type`. It is the service's own text, including an unknown one from an odd response, so as an attribute it could grow without bound. It stays on the span and the `aws.called` row.
- Service and operation are bounded because `aws.call`'s plan checks them against the catalog before any request. I read this in `aws/tools.rs`; I did not trace every other `Request` constructor (inventory, cost, alerts, logs and so on).

**Proof.**
- New file `telemetry/tests_aws.rs`, 3 tests:
  - two `DescribeStacks` (one `AccessDenied`) and one unbound `GetCallerIdentity` give three series, each counted once and timed (40 / 10 / 0.5 ms), with no `error.type`;
  - a failed turn's AWS calls count the same way;
  - a request under a continuation's span is counted.
- Planted reverts:
  - a walk that does not recurse: all 3 fail;
  - `unbound` counted as `ok`: 2 fail (`a_turns_aws_requests_are_counted_and_timed_by_service_operation_and_outcome`, `a_failed_turns_aws_requests_count_too`).

## Under load

Four `yes >/dev/null` loops at nice 0, the tests at nice 19. I used `yes` because the sandbox refused the `sh -c 'while :; do :; done'` recipe; the CPU load is the same.
- Three runs of the telemetry suite (43 tests): 42 pass, and `telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is` fails each time at about 20 s: `each turn's trace` left 4, right 3 (`tests.rs:2191`).
- I stashed my work, checked out `0b0c45cf` (before my commits), and ran that test alone under the same load twice. It failed identically, so this is **not mine** and is not on the brief's list. It passes unloaded in every run. Worth a bd issue.
- Before this, the same three runs passed 43/43, but the first overlapped the rebuild and I stopped the loops partway through. Treat the 3-run result above as the load proof.

## Live check (maintainer's)

I could not run this here (no keys, no bound AWS account), and I did not run these commands, so they are a recipe with the exact flags I could confirm, not a proven script. Fresh state dir, Discord and web off, `[telemetry] otlp_endpoint = "http://127.0.0.1:4318"`, `metrics_interval_secs = 5`, `[model.retries] transient = 1`.

1. A loopback sink: a `python3 http.server` handler that appends each POST body to a file.
2. The stand-in: `theseus-sim fake-model --addr 127.0.0.1:9448 --rules '[...]'`. Point one profile's provider `api_base` at it. If it cannot answer a 429, point a second profile at a closed loopback port (class `network`).
3. `theseusd --config <scratch.toml> --socket <s> --state-dir <d>`, then `theseus --socket <s>` to submit a turn. Stop it with `theseus --socket <s> shutdown`.
4. Expect the sink's last `/v1/metrics` body to hold two `theseus.provider.call.duration_ms` points for that model: one with `error.type = network` (or `rate_limited`), one without. The failed span in `/v1/traces` carries `error.type` too.
5. AWS: with an `[aws.accounts]` entry you bind, a rule that calls `aws_whoami` should give one `theseus.aws.calls` point (`rpc.service = sts`, `theseus.outcome = ok`) and its `theseus.aws.duration_ms`. An `aws_call` the session policy refuses shows `theseus.outcome = error`.
   Unbound `GetCallerIdentity` is `unbound`.

## Docs to change (not edited)

- Spec §3.20 (GenAI names): add `error.type` on a failed provider call's span and duration, and the `stopped` rule.
- `aws-toolset.md` §3.8: the two metrics' attributes are `rpc.service`, `rpc.method` and `theseus.outcome`, with no `error.type`.
- The instrument count is 30 here; the maintainer sums it with batch 6's at the merge.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, run for each step.
- fmt, shape, features, clippy, cockpit, test build and the reader rule: pass.
- Suite: fails only on the known 33 (theseus-sandbox `contract` tests and theseusd `sandbox` tests, from the VM's root daemon and no job cgroup, theseus-pv6i). No other test failed.
- I ran the later phases for step 1 by hand: protocol types (no `protocol.gen` change) and `cargo deny --offline check` both pass. The benches did not run (NO_BENCH). Step 2 touches no protocol types, dependencies or `Cargo.lock`, so I did not repeat those two.
