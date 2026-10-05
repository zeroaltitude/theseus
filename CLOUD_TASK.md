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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-health-words`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: health's missing words: a `web:` line, the 1-hour cache price and count, `binary` in the cockpit, and a row and a notice when free disk space crosses a line (theseus-jxau; also theseus-4v1z, theseus-od13, theseus-f337)

Branch: `cloud/20261005-health-words`. Every commit's subject carries its issue id. Deadline for the report: 4 hours
after you start.

**Background.** Four small gaps, each where a fact health already holds reaches no one who doesn't read JSON:
- health's `web` section (refusals by host, origin and peer, the owner check, the dev origin and its uses) has no
  line in `theseus health`'s text;
- the catalog has a fifth price, `cache_write_1h_per_mtok` (2 × input on Anthropic's models), and `Usage` a 1-hour
  write count, `cache_creation_1h_input_tokens`, but `theseus catalog` shows four prices and health's tokens line
  no 1-hour writes;
- health's `binary` (whether this daemon's jobs can write the binary it runs) is in the CLI's text, not the cockpit;
- health's `disk` (`ok`; `low` under `[server] disk_warn_mb`; `below_floor` under `disk_floor_mb`, where jobs are
  refused) changes state silently: no row and no notice when free space crosses a line or comes back.

**Read first:** crates/theseus's render.rs (`health_lines`, `disk_line`, `binary_line`) and render/ (the
`push_health` modules), cmd.rs (`catalog`, `health`); theseus-protocol's lib.rs (`HealthResult`, `WebStatus`,
`DiskStatus`, `BinaryStatus`, `Usage`) and ledger.rs (the kinds' registry); theseus-core's disk.rs (`Disk`,
`FixedSpace`), rpc/driver.rs (`heartbeat`), fact/mod.rs, fact/tool.rs (`JobRefused`), outbox.rs (`to_operator`,
`post_restart_notice`), catalog.rs; theseus-discord's courier.rs (the operator arms: `restarted`'s); the cockpit's
views/Systems.tsx, components/Shell.tsx (the header's indicators, `webTone`, `webTitle`), components/DiskSpool.tsx;
the AGENTS.md files, cockpit/'s included.

**What changed since the issues were written** (the code and AGENTS.md win; report each difference):
- **The cockpit replaced the web apps**, and already shows the web section (Systems' "Web UI · access" panel and a
  header indicator), the 1-hour price (Systems' catalog table) and the 1-hour writes (Money). Those halves of jxau
  and 4v1z are done: build the CLI's.
- **`approval.open` is gone.** The `[approval]` matrix and health's approval lines were removed when approvals became
  the place rule's (an answer counts only from a private place, by the owner), and Systems states the rule. od13 is
  `binary` alone.
- **crates/theseus's render.rs is near its ceiling** (3,062 of 3,100): new code goes in a module of its own under
  render/, as the places', judge's and parked tasks' health lines do, and render.rs gets the `mod` line and the call.
  theseus-discord's render.rs is at its ceiling: the disk notice's words go elsewhere.
- **A job below the floor is refused** (`JobRefused`; `JobStoppedBelowFloor` for one stopped there). Nothing retries
  it when space returns, and this step adds no wake.
- **FAST:** nothing on the start path. The heartbeat runs every 60 s and at each wrapper's notice; a statvfs is
  cheap, but say whether the check runs on notice beats, which come in bursts.

**What to build,** each a green commit:
1. **`web:` in `theseus health`** (jxau): one line, as `web: refused host 3, origin 1, peer 2; dev origin
   localhost:5173 (served 4)`, or `web: ok` when nothing was refused and the dev origin is off. The owner check off
   (`peer_unchecked`) is loud (the bad tag) with its reason. Say what the line says when the web UI is off. The words
   match the cockpit's (`webTitle`).
2. **The 1-hour cache** (4v1z): `theseus catalog` gains a `$c.1h` column from each entry's
   `cache_write_1h_per_mtok`; health's tokens line shows the 1-hour writes within the cache writes (`cache-write
   1,200 (1h 300)`), only that format string changed in render.rs. JSON output is unchanged.
3. **`binary` in the cockpit** (od13): Systems shows health's `binary` (path, state, detail); `jobs_can_write` is
   loud (the fault tone, at the top of Systems, and a header indicator beside web's, titled in the CLI's words),
   `unknown` waits, `ok` is one quiet line. The tone and words in a pure function under lib/ (as lib/disk.ts's
   are), tested in cockpit/test/ for each state.
4. **Disk crossings** (f337): the heartbeat reads `Disk::status()` and, when its state changes, writes one row
   (`disk.low`, `disk.below_floor` or `disk.ok`, registered in ledger.rs, a fact of its own, with free, total, warn
   and floor in MB and the state it left) and one notice on the operator's lane (`Outbox::to_operator`, as the
   restart's notice is written), which theseus-discord's courier.rs posts as it posts the restart's: one arm, its
   words in a function of their own. At most one per crossing: a margin (say what) keeps free space that wobbles at
   a line from writing a row a minute. Decide and say: whether a start whose first read is not `ok` writes one (the
   state held in memory starts unknown) or reads the last `disk.*` row after serving; what `unknown` (statvfs
   failed) writes.

**Proof, offline:** the `web:` line for zeroes, refusals, the dev origin and the owner check off (render tests, as
`disk_line`'s); the catalog's column from a fixture entry; the tokens line with and without 1-hour writes; the
cockpit's binary words in their three states (`npm test` and `npm run build` in cockpit/); disk, with `FixedSpace`
driving free space ok → low → below the floor → ok over many beats: exactly one row and one notice per crossing,
none in a steady state, none for a wobble inside the margin, and the courier's plan for a `disk` body; the core's
output golden moved only where your lines are. Planted reverts, each naming the test it breaks: a row written every
beat; the owner check's line not loud; the 1-hour column reading the 5-minute price.

**The live check is the maintainer's.** Exact commands for a scratch daemon: fresh state dir, Discord off, the web
on:
1. `theseus health` shows `web: ok`; after one request to the web port with a foreign `Host` header, it shows
   `web: refused host 1`.
2. `theseus catalog` shows the fifth price, 2 × input on an Anthropic model; after a turn that writes the 1-hour
   cache (an Anthropic profile), health's tokens line shows its 1-hour writes.
3. `[server] disk_warn_mb` above the free space: one `disk.low` row within a beat of the crossing your start rule
   gives (`theseus --json ledger --kind disk.low`), and the outbox's one operator post; back below it, one `disk.ok`.
4. The cockpit's Systems view, the daemon run from a directory its user can write (the build's target/): the loud
   card and the header indicator; installed as its own user, the quiet line.

**Leave alone:** the cockpit's Budgets, Ledger and Policy views and lib/rpc.ts (cockpit-tabs, under review: name in
the report the words its Ledger view should give the disk rows), its task graph (task-board); health's `aws:` lines
in render/aws.rs (aws-fixes adds unknown keys to them); the Discord outbox and streaming beyond your one arm (the
speed lane); rpc/server.rs (push-once); rpc/driver.rs beyond your call in `heartbeat` (kernel-fixes may mark a
restart's in-process calls at the driver's first tick); cmd.rs beyond `catalog` (prove-wire-in adds a `judge prove`
dispatch there); the job path's floor check.
