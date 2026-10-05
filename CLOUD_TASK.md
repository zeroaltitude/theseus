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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-wal-sync`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: a failed fdatasync's frames never come back, and a reopened log's directory synced before its first frame is reported durable (theseus-ljgm; also theseus-c67g)

Branch: `cloud/20261005-wal-sync`. Every commit's subject carries its issue id. Deadline for the report: 5 hours
after you start.

**Background.** The WAL is the truth (AGENTS.md, "The WAL and frames"). Two gaps in its sync, both in
crates/theseus-store/src/wal.rs:
1. **theseus-ljgm.** When the store's writer's `fdatasync` fails, every frame of its batch is answered with the error
   (store.rs: "a failed group sync failed each one it led"), but the frames stay in the segment, unindexed. Later
   batches write after them and sync, and the next open replays them as committed: callers told "failed" see their
   records after a restart. Worse, a later `fdatasync` can return Ok after the kernel dropped the failed pages' dirty
   state (PostgreSQL's fsyncgate, 2018), and `Wal::sync` then advances `synced` past frames that may not be on disk,
   so every later frame's mark (theseus-7nfj) claims them.
2. **theseus-c67g.** A segment's name is as durable as its frames (theseus-xprd): the sync that makes a new
   segment's first frame durable syncs the log's directory too (`unsynced_dirs`). That holds within one process. If
   the process that created segment N+1 dies before that sync, the next open finds N+1 and appends to it
   (`append_segment` with a last segment returns no directory to sync), so the new process's syncs never sync the
   directory until its next roll. ext4's ordered mode hides it (an fdatasync that grows a file commits the journal
   with the earlier entry); POSIX does not promise it.

**Read first:** theseus-store's AGENTS.md (the crash test, the traps); wal.rs: the module docs, `Writer` (`broken`,
`short_write`), `open_from`, `append_segment`, `write_timed` (a write cut short is cut back off, or the log is
`broken`), `sync`, `synced`, `sync_with_first_frame`, `dir_syncs`, `cut_next_write`, and the tests in its `mod
tests` and wal/tests/ (`a_new_segments_name_is_synced_before_its_first_frame_is_reported_durable`); store.rs's writer
(the batch, its one sync, the answers) and store/tests.rs (`the_writer_commits_every_queued_frame_with_one_sync`);
crates/theseus-follow (`WalFollower`, `FollowError::Rewound`); theseus-index's tender.rs and theseus-core's
aws/durable.rs, each meeting `Rewound`; crates/theseus-sim's `crash-test`; the AGENTS.md files.

**What changed since the issues were written** (the code wins; report each difference):
- **The log already has a broken state:** a frame write cut short that can't be cut back off sets `broken`, and every
  later `write` is refused ("a restart's open cuts the torn tail"). A failed sync has no such path yet.
- **Recommended for ljgm** (the issue's second option, with the first as its fallback): when `sync`'s fdatasync
  fails, cut the segment back to the end of the last frame a sync covered (`set_len`), sync the cut, and roll the
  writer's state back with it (its length, its next position, `synced` untouched). If the cut or its sync fails, the
  log is `broken`. Then decide whether a cut that succeeded takes frames again or stays broken until a restart, and
  say why. A roll in between already `sync_all`ed the segment before it, so unsynced frames are only ever in the last
  segment; check that.
- **Followers read the page cache.** theseus-index's tender follows the log unbounded, so it may hold frames you cut;
  the durability tender is being bounded to synced frames by a sibling session (durability-fixes). If positions are
  reused after a cut, a follower that read the cut frames must meet a rewind (`FollowError::Rewound`: the index
  rebuilds, the tender ships again from the start), never the same positions holding other records. Prove what
  theseus-follow does with a cut it read past, and fix theseus-follow only if it can miss one (say so first).
- **A failed index write** after a good sync also fails every frame of its batch, though they are durable, and they
  come back too. Not this step: say what it would take.
- **Recommended for c67g:** an open that appends to a last segment it did not create puts the log's directory in
  `unsynced_dirs`, so the first sync after the open syncs it once. The start path's first frame is the kernel's
  startup frame, so that is one more directory sync before serving: count it, measure one here (say the time), and
  say whether it could wait until after serving without a frame reported durable before it. The lifecycle bench's
  budgets are the maintainer's to run.
- **No format change:** the frame layout and the marks stay as they are. FAST: nothing new on a sync that succeeds.

**What to build,** each a green commit:
1. **c67g.** The directory synced at the first sync after an open that recovered a last segment. A test counting
   directory syncs, as the theseus-xprd test does: an open of a log whose last segment exists, one write and sync,
   `dir_syncs` is 1; a second sync, still 1; an open that created segment 1, as today.
2. **ljgm.** First a test-only way to fail the next fdatasync (beside `cut_next_write`), and tests that fail on
   main's code (keep their output for the report): the failed batch's frames come back at the next open, and a later
   good sync moves `synced` past them. Then the fix, as you decided, in the same commit: after a failed sync, no
   frame of that batch is read back by the next open; `synced` never covers a frame no sync made durable; frames
   written after a cut that took frames again are durable and read back, in order, with a gapless sequence; a failed
   cut leaves the log broken, refusing frames with a message that says why. The store's writer answers as today.
3. **The writer's view.** A store test through the writer thread: a batch whose sync fails is answered failed, the
   next batch (if your rule takes frames) succeeds, and a reopen holds exactly the records answered Ok.

**Proof, offline:** the tests above, plus: the writer's tests (store/tests.rs) as they were; a follower that read a
failed batch then sees a rewind (a test in theseus-follow, or the index tender's own); `theseus-sim crash-test
--restarts 8 --writers 4` green (say the iterations and seeds); every theseus-store, theseus-follow and theseus-index
test; the wal and store tests 5 times under load. Planted reverts, each naming the test it breaks: the cut skipped
(frames back at the open); `synced` advanced past a failed batch; the directory left out after an open; a failed cut
not marking the log broken.

**The live check is the maintainer's.** Exact commands, no fault injection on the real disk:
1. The install build's `theseus-sim crash-test --iterations 50 --restarts 8 --writers 4 --tear true` on a scratch
   dir: every iteration recovers.
2. A scratch daemon started twice on one fresh state dir (SIGKILL between), the second start under `strace -f -e
   trace=fsync,fdatasync -o <file>`: the log directory's one fsync before its first frame is reported durable (your
   report says how to find it); then `theseus-sim bench lifecycle --runs 10 --check` stays inside its budgets.

**Leave alone:** durability-fixes (beside you: a read accessor for `Wal::synced` in store.rs, theseus-follow's
bounded read, aws/durable.rs: change no line of theirs; theseus-follow only as said above); store.rs beyond the
writer's answers (batch 6's rows and lane files renumber `MANIFEST_FORMAT` there); lane speed (the turn's store
writes and the Discord outbox: it may cap the writer's group commit; keep the batch's shape as it is); the kernel,
theseus-core, and theseus-sim beyond running its crash test.
