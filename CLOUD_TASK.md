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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-durability-fixes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the durability tender's fixes: a missing object heads 404, only synced frames ship, and the restore's two untested guards (theseus-iame; also theseus-mgw.12, theseus-b9x6)

Branch: `cloud/20261005-durability-fixes`. Every commit's subject carries its issue id. Deadline for the report: 4 hours
after you start.

**Background.** The durability tender (docs/design/aws-toolset.md §5, step 15; crates/theseus-core/src/aws/durable.rs)
ships the store's WAL and blobs to S3 under `durability/<deployment>/`, in a session of its own narrowed by an inline
policy (`durable::policy`). The restore (step 16: durable/fetch.rs, restore.rs, read.rs) reads them back. Three
findings, the first P1: it blocks turning durability on.
1. **theseus-iame.** A live check found the tender's session heads a missing key to 403, not 404: its policy has no
   `s3:ListBucket`. `Bucket::call` (durable/s3.rs) maps only a 404, `NoSuchKey` or `NoSuchUpload` to
   `S3Error::Missing`; a 403 is `Other`, which `s3_halt` makes `Halt::Retry`. The tender heads a key exactly when its
   cursor says the object was in flight at a crash: `put_once`, `resume_upload` (an upload S3 no longer lists), and
   `put_large`'s lost completion. A daemon that died after the cursor saved the mark and before S3 stored the object
   retries on every pass and ships nothing behind it. The restore's session has the fix: read.rs's `ListItsPrefix`
   (`s3:ListBucket` on the bucket, `StringLikeIfExists` on `s3:prefix`; its comment says why).
2. **theseus-mgw.12.** The follower (crates/theseus-follow) reads whole frames from the page cache, so a tail may ship a
   frame the machine never synced. After a power loss the log is cut and written again differently: the next start
   meets `FollowError::Rewound`, and stale tails past the new log stay in S3.
3. **theseus-b9x6.** A review planted two guards' reverts, and all seven tests_restore.rs tests passed with each:
   fetch.rs's gap (`first != Some(follows)`) and restore.rs's seed (`rotated && whole`).

**Read first:** aws-toolset.md §3.5 and §5; aws/durable.rs, durable/*.rs, tests_durable.rs (its fake S3: `answer`,
`lists_without_prefix`, the `Rig`) and tests_restore.rs; crates/theseus-follow (`WalFollower::read`, `Batch`);
theseus-store's wal.rs module docs (marked frames, theseus-7nfj), `Wal::write`, `Wal::sync`, `Wal::synced`, and
`read_frame`'s `FrameRead::Whole { mark }`; the AGENTS.md files.

**What changed since the issues were written** (the code wins; report each difference):
- **The fake heads 404 whatever the session.** tests_durable.rs's fake records each session's inline policy and
  answers a missing key 403 by it, but only for `GetObject`: its `HeadObject` arm answers 404 to anyone. Make HEAD
  answer by the policy as GET does (S3's 403 to a HEAD has no body). The issue's test fails only then.
- **A frame's mark is what was synced before it**, so its own records are covered only by a later frame's mark: marks
  alone hold the last batch back until the next write, and a quiet store would never ship its last write (against
  §5's 5 to 60 s). The writer knows more: `Wal::synced()` is the last position a sync covered, in process, and the
  tender runs in the daemon that writes the log. Nothing outside the WAL reads it, and `Store` does not expose it.
- **The follower's `Batch` carries no marks**, and theseus-index follows the same log with it: change it additively.
- **The tender's policy is code, not a template.** The one `s3:ListBucket` in infra/aws is theseus-hands.yaml's
  `ListTheHandsPrefix`, for the hand role, under `StringLike`: a hand heads a missing key to 403 too. Change it only if
  some hand code must tell a missing key from a refusal; report what you find.
- No store format change is expected (the tender's cursor has its own `layout`). FAST: nothing on the start path.

**What to build,** each a green commit:
1. **theseus-iame.** The fake's HEAD by policy; the tender's policy gains the restore's `ListItsPrefix` (its own
   prefix, `StringLikeIfExists`), its comment pointing at read.rs's; the policy's test
   (`the_tender_session_is_narrowed_to_its_prefix_and_its_rows`) asserts it, and still no delete or wildcard action.
   The issue's test: a cursor holding an in-flight object S3 never stored (the fake's `refuse = Some("PutObject")` for
   one pass saves the mark and fails the put), then passes with the refusal lifted: before the fix each fails
   `Halt::Retry`; after it the object is sent exactly once and every segment ships equal to the log. A second case
   for an upload S3 no longer lists (`resume_upload`'s head), if the fake can forget one.
2. **theseus-mgw.12.** Ship only frames at or before the last position known synced. Choose the bound's source and say
   why: the writer's `synced()` through one read-only accessor on `Store`, passed to the shipper as `Hooks` passes the
   ledger (recommended: exact and live), the frames' marks (a batch behind), or both. The follower stops at the bound
   (an additive read with an upper position; a segment read in part is never named sealed), the cursor stays before
   the held frames, and `oldest_unshipped_unix_ms` still counts them. The WAL's writes stay as they are.
3. **theseus-b9x6.** (a) Ship a store, then give segment 2's row and object a third segment's bytes (or swap two sealed
   objects with their rows): the restore's `gap` names the position, and segment 1 is restored. (b) A store whose last
   fetched segment came from its sealed object and is full enough that `store.restored` rolls into a new segment: the
   next pass sends only the new segment's tail. Then the narrow window the review saw: when `store.restored` fit in
   such a segment, its later frames ship as tails that a second restore never reads (it prefers the old sealed
   object), saying nothing. Stitch tails that start at the sealed object's end, or say them; say which.

**Proof, offline:** the tests above; a write without its sync (`Wal::write`) held back, then shipped after
`Wal::sync`; a power loss's cut (frames written unsynced, the segment truncated where the sync ended, the WAL reopened,
different frames written and synced): no `Rewound`, no tail in S3 past the log, a restore equal to it; the existing
durable and restore tests unchanged; theseus-follow's and theseus-index's tests pass. tests_durable.rs 5 times under
load. Planted reverts, each naming the test it breaks: `ListItsPrefix` dropped from the tender's policy; the bound
ignored; fetch.rs's gap guard off; the seed's `rotated && whole` off.

**The live check is the maintainer's,** with the owner's go; it spends under a cent. Write exact commands:
1. Assume the owner role with the tender's new policy as the session policy (print `durable::policy`'s JSON for a
   scratch deployment in the report), then
   `aws s3api head-object --bucket theseus-<account>-<region> --key durability/<scratch>/wal/none`: 404, and
   `get-object` of it `NoSuchKey`. Under main's policy both are refused.
2. A scratch daemon: fresh state dir, the stand-in model (`theseus-sim fake-model --rules`), Discord and the web off,
   `[aws.accounts."<account>"]` with `owner_role = "theseus-owner"`, `deployment = "<scratch>"`, `durability = true`.
   Two turns; within a minute `theseus health`'s durability line reads `caught_up`. Stop it; `theseusd restore --from
   s3://theseus-<account>-<region>/durability/<scratch>/` into a fresh state dir: `theseus sessions` lists the same
   sessions, and its next start ships nothing it restored again.
3. Teardown with the owner's credentials: the scratch prefix's objects and rows deleted.

**Leave alone:** the WAL's writes, its frame layout and the store's format (lane files bumps the format; lane speed may
touch the turn path's store writes); theseus-index's use of the follower; the hands (aws-fixes, beside you, works in
aws/hands/, config/aws.rs and aws/mod.rs); the local restore (theseus-core's restore.rs) beyond what S3's calls.
