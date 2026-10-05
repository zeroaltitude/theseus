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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-cockpit-tabs`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the cockpit's Budgets, Ledger and Policy tabs, step 42b (theseus-ext.15)

Branch: `cloud/20261005-cockpit-tabs`. Every commit's subject carries `theseus-ext.15`. Deadline for the report: 4 hours
after you start.

**Background.** Step 42a gave the operator two reads. `budget.list`: each open execution's money (its limit and where
it comes from, spent, reserved, held unknown, available, lifetime), its tasks under it with their carves, its last
reset and any waiting budget question, the totals, and the judge's day budget. `policy.explain`: for a session's place,
or the CLI and every bound place, each tool's layers in the gate's order, the call-dependent ones as conditions with
their entries. The CLI shows both (`theseus budgets`, `theseus policy explain`). Step 42b puts them in the cockpit,
live, and grows its Ledger. It is cockpit work: change Rust only where a read lacks a field, and say why.

**Read first:** docs/design/m7-surface.md §2.6 and 42b's entry under "Each step's tests and live check";
cockpit/AGENTS.md whole (read only what is on screen; every control confirmed first; state in the address; the past
folded, never invented; one copy of the ledger); cockpit/src/views/Money.tsx (its `Budgets` panel), Ledger.tsx,
Boundaries.tsx (the tightenings and their undo), Systems.tsx (the approval channels); components/ConfirmCard.tsx;
lib/rpc.ts (`useRpc`, `PUSHED`, `bindPush`), lib/history.ts (`useHistoryRows`), lib/derive.ts (`providerCalls`),
lib/timemachine.ts; test/*.test.ts; theseus-protocol's budgets.rs and explain.rs, and their TypeScript in
cockpit/src/protocol.gen/.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The cockpit is the web UI** (`cockpit/`, served at `/`): there is no `web/`, and node's test runner covers its pure
  modules. Much of the three tabs exists: Ledger (a treemap of kinds, family chips by a kind's first segment, a session
  filter, a time brush, search, a row's JSON), Money's Budgets panel (spent, held and left per execution, from
  `execution.list`), Boundaries (the tightenings with their undo), Systems (the approval channels). Nothing reads
  `budget.list` or `policy.explain` yet.
- **The Ledger view polls** `ledger.tail` for 20,000 rows every 5 s (`useLedger`), against "one copy of the ledger".
- **ladder (26a) joined:** the Judgment view's Ladder panel (`pack.list`): the judge's packs stay there.
- **smalls-tools joined:** AWS hands' runaway mode, in health's `AwsHandsStatus` (`runaway`,
  `runaway_until_unix_ms`) beside the hour's meter and line. With the judge's day budget, that is the money outside
  executions.
- **42a's choices:** totals add the top rows only (a task's spend is its parent's too); `limit_from` is `config`,
  `place`, `carve` or `pinned`; the last reset is one ledger page (`last_reset_unread` while the index's shape is
  built); explain probes a call inside the roots; a session is named by its full id.
- **The push:** `PUSHED` reads are read again on `execution.changed` and the confirms; `bindPush` reads nothing on
  `policy.tightened` or `policy.untightened`.

**What to build (42b),** each a green commit:
1. **Budgets,** in Money, in place of its Budgets panel: `budget.list`'s rows, each session's tasks under it with
   their carves, where each limit comes from, held unknown, available, lifetime; burn per hour from the history's
   `provider.call` rows; recent resets; the budget questions waiting, with their buttons (ConfirmCard); the totals and
   their rule; the judge's day line; the AWS hands' lines from health, runaway said plainly. `budget.list` joins
   `PUSHED`.
2. **Ledger:** on `useHistoryRows()`, its own poll gone; saved filters (the address's query, named, in localStorage);
   a follow mode (the newest rows as they land, paused while you scroll); an export of the rows shown, as a JSON
   download made in the browser.
3. **Policy,** a view of its own (`/policy`, in the nav): explain without a session (the CLI and each bound place with
   its class and ceiling; each tool's result and the layers that raised it), a tool opening its layers and conditions,
   and `?session=` for one session. The tightenings with their undo (Boundaries' list, shared, confirmed first), and a
   link to Systems' approval channels. Read again on a tightening, an undo, and a session's hold or trust.

Each view keeps its state in the address. Under the time machine each says it shows the present, and its acts are off.

**Proof, offline:** pure modules with node tests: the budget tree, burn per hour over its window, totals equal to the
top rows; explain's summary per tool; a saved filter's round trip; the export equal to the rows shown. The gate (the
cockpit's lint, tests and build; the daemon's tests of `/` serve the build). Planted reverts, each naming the test it
breaks: totals over every row (a task counted twice); burn over all time; the export of every row, not those shown.

**The live check is the maintainer's,** in a browser. Exact commands for a scratch daemon of the build (the cockpit
built first), `[web]` enabled on a port other than 7433, on 42a's rig: `theseus-sim discord rig --dir <dir>` with a
format-2 bindings file whose channel has `posture_floor = "approve"`, and the stand-in model with a rule that calls
`task.create` twice (each with an arrangement quoting the message):
1. `/money`: the session with its two tasks under it, each with its carve; the totals equal `theseus --json budgets`';
   a turn's cost appears without a reload.
2. `/policy`: `theseus policy tighten proc.run` raises proc.run's tightening for the CLI and each place; the undo from
   the view (confirmed) lowers it; the floored channel's tools show the floor.
3. `/ledger`: a saved filter survives a reload; follow mode shows a new turn's rows; the export holds the rows shown.
4. Each view loads with no console or page error.

**Leave alone:** the task board (39b, beside you: Actions' Tasks panel, a task graph, `task.changed` in lib/rpc.ts:
keep both at the merge); the Judgment view (the ladder, security notices); the Ship; 42a's reads and the gate's order
(`toolrun/order.rs`); the memory rows (31b to 35a).
