<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-task-board`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
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
## Your task: claim leases, the board, `/tasks`, the layer-1 card, and the cockpit's task graph, step 39b (theseus-ext.14)

Branch: `cloud/20261005-task-board`. Every commit's subject carries `theseus-ext.14`. Deadline for the report: 5 hours
after you start.

**Background.** Step 39a made the task graph a stored structure: the `TASK` record (`TaskRecord`), versions under
compare-and-swap (`Store::lock_task`), the task tools and plan items, layer 1 on the gate's path (a proposal answered
by `theseus confirm`), the graph in the model's view, `task.list`'s `records`, `task.changed`, and `theseus tasks` as a
tree. It left `claim`, and the graph in chat and in the cockpit, to 39b.

**Read first:**
- docs/design/m7-surface.md §2.4 (claims, "In chat"), §5's question 14, and 39b's entry under "Each step's tests and
  live check"; the spec's §3.5 (docs/spec/03-part1-s3.md);
- crates/theseus-core (AGENTS.md first): task_graph/ (`line`, `state_now`; tools.rs's `run`; view.rs),
  fact/task_graph.rs, rpc/confirms.rs, rpc/driver.rs (the due pass), tests_task_graph.rs, tests_task_layers.rs,
  tests_explain.rs, tests_places.rs, tests_layouts.rs; theseus-kernel's wakes.rs;
- crates/theseus-discord: runtime.rs (`Control::Tasks`, `route`), courier.rs (`Lane`, `LaneMsg::Live`), render.rs
  (`card`, `tasks`), tests_outbox.rs; theseus-sim's fake_discord.rs;
- cockpit/: views/Actions.tsx (`Tasks`), components/SessionGraph.tsx, lib/rpc.ts (`PUSHED`, `bindPush`).

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **39a's choices:** the owner answers layer 1 from a private place (`judge_act`), not "the requester, else the
  owner"; a session task's running states are derived when read; the view reads every TASK record each loop (O(N)).
- **smalls-tasks joined:** layer 1 guards only the owner's tasks (`TaskOrigin.by_model`, `authority_for`, the view's
  `OWNERS_MARK`); an expired change clears its proposal (`task.change_expired`); a check's view shows the checked task
  by title and state; a bad place fails alone (`place_warnings.rs`). Store format 16.
- **The cockpit replaced the web UI.** Actions lists task sessions (`Tasks`, `task.list`'s `tasks`); nothing reads
  `records`. `task.list` is in `PUSHED`, read again on `execution.changed` and the confirms, not on `task.changed`.
- **Discord's `/tasks` exists:** it lists DD7's task sessions. A live edit is an upsert by key on a place's lane
  (`Op::Upsert`): coalesced, never replayed, its message id in memory. The router sends a notification to the place
  of the session it names, and no place routes a task session. The approval card special-cases only the budget.
- **Line ceilings:** the protocol's lib.rs is at its ceiling, Discord's runtime.rs and render.rs near theirs: new types
  and code in modules of their own (the protocol's tasks.rs), and only a field or a call in those files.
- **The store's format:** `claim` is a stored field: bump by one, with a sample of a TASK record before it.

**What to build (39b),** each a green commit:
1. **Claim leases.** `task.claim { id, version }` sets `claim { by: exe_…, until_ms }`, `[kernel] task_lease_minutes`
   ahead (new: default 30, at least 1). The holder's own edits renew it in the frame they write; a renewal alone never
   moves the version. A claim on a task another execution holds is refused whatever version it names, `blocked: claimed
   by session <short> until <HH:MM>`, never retried in silence. A close ends it. An expired lease frees the task in the
   due pass, as wakes are found: from claims kept in memory, built after serving (no scan of every record per tick),
   its record and a `task.lease_expired` row in one frame; a claim past its `until` reads free before that.
   `task.claimed` rows, `task.changed`, the claim in the view's line and in `theseus tasks`; the template's line, an
   input in tests_explain's agreement table, the catalogs in tests_places. A claim does not hold back other sessions'
   edits (CAS guards those): report whether it should.
2. **The board:** one message per place, made at its first `task.changed` (route a change by its root task's origin
   session to that session's place), upserted on the place's lane with the latest tree (id, title, state, owner,
   claim), pinned best-effort when the bot may (the fake Discord gains the pin route; a refused pin is logged once).
   After a restart, find the board or make a new one: say which; never on the start path.
3. **`/tasks`:** the records' tree (states, owners, claims, versions), then the task sessions' lines of today.
4. **The layer-1 card:** `ConfirmRequest` gains the change (the task, its title, the field, before and after, or
   abandon). Discord words it "Change the acceptance of tsk_… (title)? Before: … After: …", with Accept and Decline on
   today's approve and decline presses, so its ids parse as before; `theseus confirm` says the same.
5. **The cockpit's task graph:** Actions' Tasks panel shows the records as a tree (state, owner, claim, version, a
   waiting change and its card) and opens a graph (React Flow, as SessionGraph does), its state in the address;
   `task.changed` reads `task.list` again. It shows the present: under the time machine it says so, its acts off.

**Proof, offline:** the design's 39b tests: a lease expires under the virtual clock (tokio's paused clock: free at
`until`, not before); two executions claim one task (one `task.claimed`, the other `blocked … until`); the board
renders and edits (one message at the fake Discord, edited in place); the card's ids parse. Also: a renewal keeps the
version; an older store opens; a plain turn's 5 frames and `theseus-sim bench turn --check`; the cockpit's lint, tests
and build. The claim and lease tests 5 times under load. Planted reverts, each naming the test it breaks: the due pass
never frees a lease; a second claimer let through; a new board posted at each change.

**The live check is the maintainer's.** Exact commands on `theseus-sim discord rig --dir <dir>` (no keys), with
`theseus-sim fake-model --rules`, `[kernel] task_lease_minutes = 1` and `heartbeat_secs = 10`:
1. A rule records a plan item ("Chart the reef") from the rig's DM; read its id (`theseus --json tasks`), then rules
   claim it at its version: from the DM, then from a CLI session (`theseus ask`), whose result reads `blocked: claimed
   by session … until …`. `theseus tasks` shows the claim; `theseus ledger -k task.claimed` has one row.
2. The fake Discord's log: one board message in the DM, then edits of it, and its pin. `/tasks` said in the DM
   (`theseus-sim discord say`) answers with the tree and the claim.
3. A minute on: one `task.lease_expired` row, and the board shows the task free.
4. A brief task (with its arrangement) whose acceptance a rule changes: its card at the fake reads "Change the
   acceptance of …? Before: … After: …", and `theseus confirm <id>` accepts it.

**Leave alone:** situations (35a, beside you: the compile's admitted set; your view stays one class there); the
cockpit's Money, Ledger and Policy (42b, beside you: you both add to lib/rpc.ts's push lines and maybe the routes: keep
both at the merge); gliding (38b, a local lane: places, publish, the outbox, and new tools in the same test tables);
28a's checks; 27's arrangement; the task's wakes; the kernel's transitions.
