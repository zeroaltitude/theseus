<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/ and the benchmark adapters under bench/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own; bench/ has its README), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that
  rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once
  collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This
  environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is
  refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, these timing tests can fail, and none is on the flaky list: rerun it alone, and name it in the report.
  - theseus-store's `tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs);
  - theseus-core's `term::tests::python3s_repl_computes_on_the_screen` (theseus-1n2y),
    `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` (theseus-t2yb),
    `tests_lsp_edits::the_block_adds_no_frame` (theseus-xx6w), and `tests_route`'s tests that slow Jev's verdict past
    route's wait;
  - theseus-discord's `tests_outbox::a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1);
  - theseusd's `a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker` (theseus-1n5f).
- One negative assertion failed once under load and is a finding, not a flake: theseus-kernel's `tree`
  `the_deadline_stops_the_whole_tree_too` (theseus-g11i; a batch-7 session fixes it). If it fails for you, keep its
  output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine (today: the kernel sim's put-back check, theseus-81ig; theseusd's stop on a SIGTERM or a SIGINT; a clean stop that closes the index). Any other failure is yours to explain.

**What main holds.** You clone main at faaa9df6 or later, with store format 17. These joined main last night, so your clone has them:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15), and its fix: a routed session keeps its move only while route.v1 acts for it, and `profile.use` moves it;
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the language servers' `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16);
- gliding with the place rule (`channel.post` and `channel.read`), and the cockpit Ship view's gentle roll;
- the Linux lanes: a job's wrapper and its L0 command spawn without a fork; each L0 job in a cgroup of its own where the daemon's is delegated, with a process cap and an exact stop; one sync per job completion; background passes that wait while the machine is busy; language servers that watch their own files;
- the bench fixes: the bench asks for its model's whole output cap, `[policy] private_addresses`, and `[model.retries]` (a transient failure's bounded retry inside its turn);
- rust-analyzer's compiler errors after an edit, a saved document waiting for the check after its save;
- the refusal fallback: a refused request goes once to its model's fallback (Sonnet 5.5's is Sonnet 5), and every surface says so;
- files, part one: every surface accepts any file, and PDFs reach the model (the Discord attachments, `AttachmentContent`, `fs.read`, `web/fetch.rs`, the compiler's document blocks; store format 17);
- context-honesty: the system header says how a request is assembled, and recall's notes say the harness chose them (`turn.rs`'s `ASSEMBLY`, `recall/render.rs`'s preamble);
- FSRS-6 retention and the `+retention` arm.

**Other changes in flight.** About twenty other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Under review now, merging into `main` one at a time over the next hours (your clone may hold some of them):
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop: labeled examples into packs, thresholds re-fit from calibration.

On the owner's machine:
- files, part two: notebooks, Office documents, EPUB, archives, audio and video reach the model (the attachment readers beside part one's);
- speed: a confidence bar per routing mode, Jev's connection kept warm, and a reply that reaches Discord before its settle's sync (`config/routing.rs`, `turn/route_step.rs`, the judge client, the Discord outbox and streaming);

These are other cloud sessions like you, each on its own branch:
- durability-fixes: the durability tender's missing-key read, only synced frames shipped, and two restore tests;
- aws-fixes: runaway mode after a raised line, unknown `[policy.aws]` keys in health, a network test, the hand image's build checks;
- push-once: one serialization per notification for every watcher, and every operator act's `by`;
- kernel-fixes: the deadline's stop of a whole tree, a late completion's one row, a restart's in-process calls, nested locks;
- sim2: kernel-sim drives `/stop`, wakes and the outbox under crashes;
- route-gaps: a routed session follows its base, and the route review's debts;
- bench-efficiency, bench-async and bench-recall: the benchmark program's efficiency track and two new benchmarks;
- proc-steps: `proc.run`'s steps, and `fs.patch`'s recount;
- health-words: health's web and 1-hour cache words in the CLI, `binary` in the cockpit, and disk crossings;
- prove-wire-in: `theseus judge prove` from the ledger;
- telemetry-resumed: a confirmed call's run and a background job's end counted once as tool calls;
- telemetry-calls: a failed provider call's `error.type`, and the AWS calls' metrics;
- reader: the reader rule's tool and same-name holes, and `theseus-index --version`;
- wal-sync: a failed fdatasync's frames cut back, and a reopened log's directory synced.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (17 on main today; the changes under review take it to 18, 19 or 20), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,709), crates/theseus-core/src/compiler.rs (2,560) and crates/theseus-discord/src/render.rs (2,928 of 2,930); near it, crates/theseus-discord/src/runtime.rs (3,453 of 3,500), crates/theseus/src/render.rs (3,062 of 3,100), crates/theseus-kernel/src/kernel.rs (2,992 of 3,030), crates/theseus-core/src/turn.rs (3,472 of 3,523) and crates/theseus-core/src/config.rs (2,855 of 2,910). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-telemetry-calls`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package, and bench/'s Python gains no import beyond the standard library and the Harbor its adapter already uses. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, docs/benchmarks.md, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

---
## Your task: a failed provider call's time apart from answered ones (`error.type`), and AWS calls counted and timed by service, operation and outcome (theseus-lmhp; also theseus-ku5f)

Branch: `cloud/20261005-telemetry-calls`. Every commit's subject carries its issue id. Deadline for the report: 4
hours after you start.

**Background.** v1.1's lane telemetry2 (docs/design/roadmap-v1.1.md, theme 2) makes OTel tell the ledger's story.
Two gaps in the calls' metrics:
1. **theseus-lmhp.** `theseus.provider.call.duration_ms` (crates/theseus-core/src/telemetry/metrics.rs,
   `provider_calls`) carries `gen_ai.provider.name` and `gen_ai.request.model` since theseus-yf1, so a failed call (a
   404 in 150 ms, a 429, a timeout at 600 s) lands in the answered calls' series and skews the model's percentiles.
   The GenAI semantic conventions add `error.type` to a call's duration when it fails. Add it, from the class the
   span recorded.
2. **theseus-ku5f.** C1 records each AWS request as an `aws.called` row and as a span of kind `aws` under its call's
   span, in OpenTelemetry's AWS names (aws/mod.rs, `span`: `rpc.service`, `rpc.method`, `aws.request_id`,
   `cloud.region`, with `status` and, on an AWS error, `error`, its code). The AWS design (docs/design/aws-toolset.md
   §3.8) also names two metrics: `theseus.aws.calls` (a counter) and `theseus.aws.duration_ms` (a histogram), by
   service, operation and outcome. No instrument exists yet.

**Read first:** the roadmap's theme 2; the spec's §3.20 (docs/spec/: the GenAI names, which instruments carry which
attributes); aws-toolset.md §3.8; telemetry/metrics.rs (`provider_calls`, `lsp_requests` and `files_read`, two
walks of a trace's spans, `INSTRUMENTS`, `turn`, `failure`), telemetry/spans.rs (`ProviderCall`, `provider_calls`,
`gen_ai`, `failed`), telemetry/tests.rs (`pipeline`, `Receiver`, `result_with`, `s`, `points_of`, `point_with`,
`attrs_of`, `a_provider_calls_time_carries_its_provider_and_model`), telemetry/tests_files.rs (a test file of its
own, its helpers imported from tests.rs); fact/turn.rs (`ModelCallFailed`, `ModelAnswered`, `ProviderRefused`,
`ModelRetried`); turn.rs (`settle_failed`, and the stop's cut, whose class is `stopped`); turn/retry_step.rs; aws/
mod.rs (`span`, `row`); the AGENTS.md files.

**What changed since the issues were written** (the code wins; report each difference):
- **A failed call's span** closes through `ModelCallFailed`: `error` holds the class (`ProviderError::class()`:
  `rate_limited`, `overloaded`, `timeout`, `server`, `network`, `auth`, `invalid_request` and the rest, or `unknown`
  for an error that is no provider's), and `message` its text. A call a `/stop` cut has class `stopped`. Decide what
  `stopped` gives: its own `error.type` (the call did not answer) or none (an operator's choice, as theseus-iu3a
  treats a cancelled tool call); say which, and why.
- **A refusal is an answered call:** `ModelAnswered` closes the span with `stop_reason` `refusal`, and the refusal's
  fallback (theseus-7gir.18) makes a second provider span on the fallback's model. Neither carries `error.type`; the
  refusal's own count is the ledger's. Say so in a test.
- **`[model.retries]`** (theseus-7gir.21): a transient failure's retry happens inside its turn, so a turn that
  completes can hold failed provider spans. Your split applies to them too, which is where it matters most.
- **The span itself:** semconv puts `error.type` on the call's span as well as on its duration. Add it in `gen_ai`
  when the span failed, beside the `error` attribute `flatten` already copies; leave `failed`'s ERROR status as it is.
- **AWS outcomes:** a request's `status` is `ok`, `unbound` (never sent: the account is not bound) or `error`, with
  `error` set to the AWS error code when AWS answered one (`AccessDenied`, `ThrottlingException`). Name the outcome
  attribute and decide whether the code rides as `error.type` (a bounded set: the services' documented codes); say
  why. Walk every `aws` span in the trace wherever it sits, as `lsp_requests` walks `lsp.request`: a sibling session
  is putting an approved call's AWS spans into its continuation, and those then count too.
- **Batch 6's memory rows** (under review, maybe on your main) add five instruments, a walk and two `record_*`
  methods in metrics.rs and telemetry.rs. Your two instruments go after them in `INSTRUMENTS`; the maintainer sums
  the array's length at the merge.
- telemetry/tests.rs has 2,482 of the 2,500 lines an unlisted file may hold: put your tests in a file of their own
  (as tests_files.rs is); edit tests.rs only where an existing expected point changes.
- **FAST:** both are computed at the turn's end from its trace, as today. Nothing on the start path or the turn path.

**What to build,** each a green commit:
1. **`error.type` on failed provider calls** (lmhp). spans.rs's `ProviderCall` keeps the span's error class;
   `provider_calls` adds `error.type` to `theseus.provider.call.duration_ms` when there is one (and to
   `theseus.provider.first_token_ms` only if a failed call can have a first token: say whether). `gen_ai` adds it to
   the span. Tests: a turn whose trace holds two answered calls and one `rate_limited` call gives two series of the
   model's, one with `error.type = rate_limited`; a refused call and its fallback carry none; your rule for
   `stopped`; a failed turn's calls split the same way. The existing tests' points move only where a failed call is.
2. **AWS metrics** (ku5f). `theseus.aws.calls` and `theseus.aws.duration_ms` beside the tool instruments, fed by a
   walk of the trace's `aws` spans in `turn` and `failure`, by `rpc.service`, `rpc.method` and the outcome you named.
   Tests read them back through OTLP's types: two DescribeStacks (one AccessDenied) and one unbound
   GetCallerIdentity give three series, each counted and timed; a failed turn's AWS calls count too.

**Proof, offline:** the tests above; every telemetry test, apart from the points step 1 moves (name each); the
conformance test against `testdata/old-exporter.json` passes as it did; the core's output golden unchanged (it pins
no metric); the whole telemetry suite 3 times under load. Planted reverts, each naming the test it breaks:
`error.type` dropped from the duration; a refused call given an `error.type`; the AWS walk reading only the turn's
top level; an unbound request counted as `ok`.

**The live check is the maintainer's.** Write exact commands for a scratch daemon: fresh state dir, Discord and the
web off, `[telemetry] otlp_endpoint` at a loopback OTLP/HTTP sink (a few lines of python3's http.server saving each
POST), `metrics_interval_secs = 5`:
1. `[model.retries] transient = 1`, and a profile on the stand-in model (`theseus-sim fake-model --rules`) whose
   first call fails (if the stand-in can't answer a 429, point a second profile at a closed loopback port: class
   `network`): the sink's `theseus.provider.call.duration_ms` has a series with that `error.type` beside the
   answered one.
2. With an `[aws.accounts]` entry the maintainer binds, a stand-in rule that calls `aws_whoami`: the sink shows one
   `theseus.aws.calls` point with `rpc.service = sts`, outcome ok, and its duration. An `aws_call` its session
   policy refuses shows the error outcome.

**Leave alone:** telemetry-resumed (beside you: `tool_calls`, spans.rs's `ToolCall`, the continuation's and late
results' spans, toolrun/ and turn.rs: change none of them); batch 6's memory rows' instruments and walks (keep their
lines); aws-fixes (aws/hands/, config/aws.rs, health's `aws:` lines: read aws/mod.rs, change nothing there); lane
speed (the judge client and its metrics); the provider and the turn path themselves: read them, change nothing.
