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

**What main holds.** You clone main at e6f90af3 or later, with store format 16. These joined main last night, so your clone has them:
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
- the refusal fallback: a refused request goes once to its model's fallback (Sonnet 5.5's is Sonnet 5), and every surface says so.

**Other changes in flight.** About twenty other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Under review now, merging into `main` one at a time over the next hours (your clone may hold some of them):
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop: labeled examples into packs, thresholds re-fit from calibration.

On the owner's machine:
- files: every surface accepts any file, and PDFs reach the model (the Discord attachments, `AttachmentContent`, `fs.read`, `web/fetch.rs`, the compiler's document blocks);
- speed: a confidence bar per routing mode, Jev's connection kept warm, and a reply that reaches Discord before its settle's sync (`config/routing.rs`, `turn/route_step.rs`, the judge client, the Discord outbox and streaming);
- context-honesty: the persona says how a request is assembled, and recall's notes say the harness chose them (`turn.rs`'s `PERSONA`, `recall/render.rs`).

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
- prove-wire-in: `theseus judge prove` from the ledger.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (16 on main today; the changes under review take it to 17, 18 or 19, and the files change one more), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-aws-fixes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: four AWS fixes: runaway mode yields to a raised line, unknown `[policy.aws]` keys are named, a blackhole route's test, and the hand image script's checks (theseus-6hkx; also theseus-snhr, theseus-rx7m, theseus-i7bz)

Branch: `cloud/20261005-aws-fixes`. Every commit's subject carries its issue id. Deadline for the report: 4 hours after
you start.

**Background.** Four findings in the AWS toolset (docs/design/aws-toolset.md), each its own commit:
1. **theseus-6hkx.** Runaway mode (crates/theseus-core/src/aws/hands/runaway.rs) refuses new AWS actions once the
   period's spend reaches `runaway_factor` × a line (`hourly_alert_usd`, or `daily_budget_usd`), writing a META mark
   `aws.runaway.<account>` so a restart in its period doesn't announce it again. Its refusal says to raise the
   line or the factor and restart. But `Sink::admit` and `observe` call `current()` first, which returns the mark
   while its period holds and never compares it with the config: a daemon restarted with a raised line, in the same
   hour or day, still refuses every new group, quoting the old line.
2. **theseus-snhr.** A `[policy.aws]` key naming no service or operation (`"ec2:TerminateInstance"`, `"cloudformaton"`)
   loads and never matches, so the call falls to its class's line or `enforcement`: looser than the operator wrote,
   and with C2's writes a write that should wait may run. The loader checks only each key's form (`aws_policy_key`):
   the catalog stays undecoded on the start path (§3.10).
3. **theseus-rx7m.** Discovery in an existing VPC (aws/hands/network.rs, `unrouted`) runs Fargate only when each
   subnet's table sends 0.0.0.0/0 to a NAT whose route is not `blackhole` (a NAT deleted under it). Dropping that check
   passes every test: the fake's route tables hold no blackhole route.
4. **theseus-i7bz.** infra/aws/hand/build.sh fails its own checks on a good image. `file` says a Rust musl binary is
   `static-pie linked`, which `grep -q 'statically linked'` rejects. And under `set -euo pipefail`, `docker run … hand
   2>&1 | grep -q 'THESEUS_HAND is not set'` takes docker's status: `theseusd hand` without its spec prints that line
   and exits 1, so the script says the image does not run. A copy with both fixes built and ran the image.

**Read first:** aws-toolset.md §3.9 and §3.10; runaway.rs and its tests in aws/hands/tests_part2.rs (`runaway_rig`
and the two runaway tests); config/aws.rs (`aws_policy_key`, the account's lines); config.rs's `policy.aws`;
aws/mod.rs (`check_all`, `status`); theseusd's `aws.check` phase (crates/theseusd/src/main.rs); theseus-aws-catalog's
lib.rs; theseus-protocol's `AwsStatus`; health's `aws:` lines (crates/theseus/src/render/aws.rs, `aws_lines`);
aws/hands/network.rs, tests_network.rs (`route_tables`); infra/aws/hand/ and infra/aws/test/; the AGENTS.md files.

**What changed since the issues were written** (the code wins; report each difference):
- **The config was split** (theseus-sk47): `aws_policy_key` is in config/aws.rs, not config.rs.
- **The catalog's costs:** `Catalog::embedded()` decodes only its index, on first use; `entry(name)` finds a service by
  id, signing name or endpoint prefix (a name that hits twice reads as none: call it ambiguous if you can tell);
  `service(name)` decodes that one service and caches it, and `operation` names its operations. A service key needs
  the index alone; an operation key decodes its own service.
- **Runaway mode joined tonight** with the tools smalls. Its live check couldn't run: no hands stack is deployed.
- **Long files:** crates/theseus-protocol/src/lib.rs is at its ceiling (2,709): add `AwsStatus`'s field only, and raise
  its line in scripts/long-files.txt by exactly those lines in the same commit, appending the reason as that line's
  history does. Health's `aws:` lines are built in render/aws.rs: change them and add their tests there, not in
  crates/theseus/src/render.rs (3,062 of its 3,100).
- infra/aws/test's stdlib tests (`python3 -m unittest discover -s infra/aws/test -p 'test_*.py'`) are not in the gate:
  run them and give the counts.

**What to build,** each a green commit:
1. **theseus-6hkx.** Pick one of the issue's fixes and say why: `current()` ignores a mark whose line × factor is below
   what the config now gives for that line (or whose line the config no longer sets), so `reached` decides again; or
   the start drops a mark whose line or factor differs, with a row. The first is a read at admission, never on the
   start path (FAST), and keeps a lowered line's mark; the second writes at the start. Tests beside the existing two:
   enter runaway mode, raise the line, rebuild the account from the new config on the same store, and the same group
   is admitted; the factor raised, the same; the line lowered, still refused; the day's line removed, its mark no
   longer refuses. Health's runaway words follow the same rule.
2. **theseus-snhr.** After serving, in or beside the `aws.check` phase, check each `[policy.aws]` key naming a service
   or an operation against the embedded catalog (class keys need nothing), and name each unknown one in `AwsStatus`
   (a list, absent when empty), in health's `aws:` line, and in the log, one warning each. Never on the start path,
   and never failing the start: the issue's other option, failing the config's confirmation, is not this step.
   Regenerate cockpit/src/protocol.gen/. Measure the check's time and what it adds to RSS.
3. **theseus-rx7m.** `route_tables` gains a blackhole case (the subnet's 0.0.0.0/0 route names a NAT with
   `<state>blackhole</state>`): `fargate_in_an_existing_vpc_needs_its_routes_and_runs_in_its_subnets`, or a test of
   its own, asserts the subnet is refused, named, and no `RunTask` sent.
4. **theseus-i7bz.** In build.sh: `grep -qE 'statically linked|static-pie linked'`, and the image check captures first
   (`out=$(docker run … 2>&1 || true)`), then greps the text. The proof is a stdlib test in infra/aws/test/ needing no
   cargo, Docker or network: it copies build.sh and the Dockerfile into a scratch git repository under /tmp laid out as
   this one, with a stub `scripts/build.sh` that writes a stand-in binary where build.sh looks
   (`$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release-thin/theseusd`), and stub `file` and `docker` first on PATH.
   Never `--push`. The cloud's Docker cannot build the real image.

**Proof, offline:** the tests above, and: `static-pie linked` and `statically linked` pass, `dynamically linked` is
refused; a docker that prints the hand's line and exits 1 passes, one that prints anything else fails; a typo'd
operation and a misspelt service are named, a right service and operation are not; the lifecycle's phases unchanged;
the existing hands and aws tests pass. Planted reverts, each naming the test it breaks: `current()` blind to the
config again; the blackhole check dropped; the old grep; the old pipeline; operations left unchecked.

**The live check is the maintainer's.** Write exact commands for a scratch daemon (fresh state dir, the stand-in model
`theseus-sim fake-model --rules`, Discord and the web off, an `[aws.accounts."<account>"]` with the owner's key):
1. `[policy.aws]` with `"ec2:TerminateInstance" = "approve"`, `"cloudformaton" = "notify"` and `"s3:ListBuckets" =
   "notify"`: after the check, `theseus health`'s `aws:` line names the first two and not the third, `theseus --json
   health` lists them, the log warns once each, and the daemon serves.
2. On the owner's machine, `infra/aws/hand/build.sh` (no `--push`): it builds, says the image answers as a hand, and
   prints its name. No AWS cost.
3. Runaway mode, once a hands stack is deployed and with the owner's go (cents): a line low enough that one small
   group enters runaway mode; raise `hourly_alert_usd`, restart within the hour: the same group runs.

**Leave alone:** durability-fixes (beside you: aws/durable.rs, durable/ and their tests); health-words (beside you:
render.rs, health's other lines, the cockpit's health view: put nothing in cockpit/ but the regenerated types); the
cockpit's Policy view (batch 6's cockpit-tabs); the other hands modules.
