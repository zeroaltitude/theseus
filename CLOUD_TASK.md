<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate you touch, scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- The sandbox's contract tests that need a non-root user can fail or skip here: the VM runs everything as root, and Linux exempts root from `RLIMIT_NPROC` (theseus-pv6i).
- `theseus-core tests_push::the_snapshot_and_the_events_agree_under_the_position_rule` failed once in 20 suites here (theseus-amr2).
- A reaping or job test whose `turn.submit` is answered `internal`, `No such file or directory (os error 2)`, under load (theseus-46ya: a spool read race).

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About twenty-five other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- the Jev wire-in (`[judge]`, `JudgeService`, the judgment sink, `loop.v1` in shadow, crates/theseus-judge's client);
- the ontology wired in (crates/theseus-ontology's records, its snapshot, the context compiler's guidance walk; `MANIFEST_FORMAT` goes to 7);
- MCP tools in turns (`McpBoard`, `[mcp.servers]`, the `Tool` trait's names as `&str`);
- `theseus judge prove` (a report generator in crates/theseus-judge);
- five gate flakes;
- the user unit's restart limits and install checks (crates/theseusd/src/install/, scripts/user-service.sh);
- the MCP server (`[mcp_server]`, `/mcp` on loopback, `Surface::Mcp`);
- AWS hands on Lambda and Fargate (the `hand` role, `aws.hands.run`, the SQS poller, infra/aws/'s hands files);
- the durability tender (WAL segments and blobs to S3, index rows to DynamoDB);
- a terminal toolset (`term.*`, a pty per session);
- the gate's bench build, features recheck and cockpit build (scripts/gate.sh).

These are other cloud sessions like you, each on its own branch:
- the judgment surfaces (trace marks, `judge` spans, `judge.list`/`get`, the cockpit's Judgment section);
- `security.v1` in shadow at the gate;
- `classify.v1` and `role.v1` at inbound;
- CONTINUE's signals and `continue.v1` in shadow;
- the arrangement on `task.create`;
- the memory `Recall` node, `derived_from` edges and the `BudgetReport`;
- the `+rerank` memory arm;
- the cockpit's Ontology view;
- `categorize.v1` in shadow and `tasks.parked` in health;
- bindings format 2 (many guilds, per-place ceilings);
- MCP prompts (`/prompt`, `theseus prompt`, the cockpit's picker);
- `extend.propose`;
- `theseus restore --from s3://…`;
- AWS hands' cancellation, TTL reaper, reservations and grid;
- the LSP board and tools (crates/theseus-lsp's consumers).

On the owner's machine:
- a benchmark run (bench/, and docs/benchmarks.md at its end).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field to a stored record or a new record kind, bump it by one from main's number as you cloned it, and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. Others bump it too: the maintainer renumbers at the merge.
- **Files near their line ceiling** (scripts/long-files.txt): crates/theseus-protocol/src/lib.rs, crates/theseus-core/src/turn.rs, crates/theseus-core/src/config.rs, crates/theseus-discord/src/runtime.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/tests_m3.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-smalls-tasks`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the review's small changes to steps 39a, 38a and 28a (theseus-ext.10, theseus-ext.11, theseus-w8ys)

Branch: `cloud/20261004-smalls-tasks`. Every commit's subject carries its part's issue id: `theseus-ext.10`,
`theseus-ext.11` or `theseus-w8ys`. Deadline for the report: 4 hours after you start.

**Background.** The owner reviewed three merged steps and chose a small change to each: the task record (39a:
`TASK` records, task_graph/, layer 1 on the gate's path), bindings format 2 (38a: per-place ceilings), and check
tasks (28a: `check_of`, the exclusion, the basis, check.rs). The parts are independent: do them in this order, each in
green commits of its own, and report what's left if time runs short.

**Read first:** the root AGENTS.md; the AGENTS.md of crates/theseus-core, crates/theseus-discord and
crates/theseus-protocol; docs/design/m7-surface.md §2.3 (bindings) and §2.4 (the task graph); docs/design/m5-judgment.md
§2.11 (independence); the newest Items in docs/spec/16-part3-item-86.md.

**What the code says** (the code and AGENTS.md win: report each difference, and where the code can't settle a
question, build the clear part and report the question):
- `TaskOrigin` (theseus-protocol's tasks.rs) holds `session` and `principal`, and the principal is the execution's
  authority (`operator` in the owner's own conversation), so a record can't tell the owner's act from a tool call.
  Every record today comes from a tool call: `task.create` with a brief (a task session, its objective and
  acceptance from the arrangement's pieces), `task.create` without one (a plan item), or `task.split`. No method or
  CLI creates a task.
- Layer 1 is decided from the input alone. `authority_of` sets `Plan::authority` in `Tool::plan`, which has no store
  (`ToolCtx`), and policy.rs asks at every posture when it is set. toolrun.rs reads the record under the task's lock
  (`lock_for_call`, `proposed`).
- A decline clears the proposal in the answer's frame (rpc/confirms.rs: `lock_for_answer`, `tools::declined`, then
  `announce(…, "change_declined", …)`). `expire_question` declines the call but leaves the proposal on the record,
  so the view says a change waits when none does.
- The binding's start (theseus-discord's runtime.rs, `guilds::check_profiles`) fails the whole binding when one
  ceiling names an unknown `profile`. `Core::bind_places` (rpc/bindings.rs) only logs an unknown tool family.
- A model call reserves its output cap at the output price plus its input estimate (`reserve_micros` in catalog.rs):
  $1.28 of output alone on the template's sonnet profile.
- The task graph's view (task_graph/view.rs, `line` in mod.rs) shows each task's id, title, state, owner, deps, first
  acceptance line and version. The record has no notes field, and the view never shows evidence. A check's session
  record carries `check` (`TaskCheck`: `checked_task`, `excluded_sessions`).

**What to build:**
1. **Layer 1 guards only the owner's tasks (theseus-ext.10).**
   - Mark each new record whose objective and acceptance the model wrote: a field on `TaskOrigin`, set for a plan
     item (`task.create` without a brief, from a turn's tool call) and for `task.split`'s children. A record without
     the field, written before this change, reads as the owner's.
   - The owner's tasks keep layer 1: any the owner made, and the task sessions `task.create` makes with a brief and
     arrangement. A plan item changes freely. Its objective, its acceptance and abandoning it apply at once, still
     versioned and visible (version +1, the `task.updated`/`task.closed` rows, `task.changed`, the narrative line).
   - Decide it in the harness. toolrun.rs reads the record before the gate decides, so drop `Plan::authority` there
     for a plan item, and its call runs at its tool's own posture. `lock_for_call` and `proposed` follow.
   - The view marks the owner's tasks and its head line says only those wait; so do task.update's and task.close's
     descriptions.
   - `expire_question` clears an expired proposal in the expiry's frame, as the decline does (the task's lock
     first), with a row that says it expired.
   - A stored field: bump `MANIFEST_FORMAT` by one (once for the branch), with a layout sample of a TASK record
     without it. Regenerate the cockpit's protocol types.
2. **A bad place fails alone (theseus-ext.11).**
   - An unknown `profile` in one place's ceiling leaves only that place unbound (read like a channel the file doesn't
     name), and every other place binds. Its reason shows in health and at the binding's start: a warning in the log,
     and a row with its narrative line, recorded once a start as `PlaceViewed` is.
   - A ceiling's unknown tool family shows in health beside its place, not only in the log.
   - Health and the binding's start warn when a place's `spend_limit_usd` is below one call's output reservation on
     its profile (the ceiling's `profile`, else the live one: `reserve_micros(max_tokens, 0)`), with the figures:
     "#pier's $1.00 limit is below one call's $1.28 on sonnet (before its input)". A model the catalog doesn't price
     has no figure: no warning, and the log says why.
   - theseus-protocol's lib.rs is at its line ceiling, so new health fields go in its modules (places.rs).
     theseus-discord's runtime.rs is at 3,432 of 3,500, so put the logic in runtime/guilds.rs.
3. **A check sees the checked task by title and state (theseus-w8ys).** In a check's view, the checked task and every
   task under it show id, title and state only, and so does any record whose session is in `excluded_sessions`: no
   owner, deps, acceptance or version. The check's other lines and every other session's view stay as they are. The
   check still gets the objective and the claim through its basis (check.rs). Count the lines shown this way in
   `context.compiled`'s `tasks` summary (`restricted`), so the ledger shows it.

**Proof, offline:**
- ext.10: a plan item's acceptance change and its abandoning apply at once, with no question, and so does a split's
  child's; a task session's change still waits on its card; an old record (the layout sample) waits as the owner's;
  an expired layer-1 question leaves no proposal on its record, its row written (drive `expire_questions(now)`;
  don't sleep).
- ext.11: with one place's profile unknown, the other places bind and answer, and health names the unbound place and
  why; an unknown family shows in health; a $1 limit on a profile whose call reserves $1.28 warns, and $2 doesn't.
- w8ys: a checked task that set deps and acceptance, and a subtask under it, show id, title and state in the check's
  view; the parent conversation's view is unchanged by this part.
- The turn bench and the frame budget unchanged (run the bench for its shape; the maintainer measures).
- Planted reverts, each naming the test it breaks:
  - give brief tasks the plan item's mark: the task session's waiting test fails;
  - skip the clear in `expire_question`: the expiry test fails;
  - fail the whole binding again: the other-places test fails;
  - render the checked task with `line()`: the check view's test fails.

**The live check is the maintainer's.** Write it in the report as exact commands and what each shows:
1. **Layers.** A fresh scratch daemon with Discord and the web off, a model key in `[secrets]`
   (`anthropic_api_key`), and `[kernel] confirm_ttl_secs = 60`:
   - Ask for a plan item made with `task.create` without a brief, then a task.update of its acceptance. There's no
     card, and `theseus tasks` shows it at v2.
   - Run `theseus policy tighten proc.run`, then ask for a task with a brief that runs `uname -a` with proc.run (so
     it stays open). Ask to change that task's acceptance: the call waits on a card.
   - Answer nothing for a minute. `theseus tasks` then shows no proposal, and the ledger has the expiry's row.
2. **Bindings**, on the Discord stand-ins (`theseus-sim discord rig`, `fake-discord`, `fake-model`; no keys). Use a
   bindings file with three places: `#lab`, whose ceiling names `profile = "nosuch"`; `#pier`, with
   `tools = ["web", "nosuch"]` and `spend_limit_usd = 1`; and a DM. Health shows the binding running, `#lab` unbound
   with its reason, `#pier`'s unknown family and its limit warning, and the DM bound and answering. The daemon's log
   and the ledger say the same at the start.
3. **Checks**, with the model key: a task that reads a file and closes its record with evidence quoting it; once it
   reports, a check of it (`check_of`). The check session's `context.compiled` rows show `tasks.restricted` ≥ 1.

**Leave alone** (changes in flight near these files): extensions loading (43b: rpc/confirms.rs, policy.rs,
toolrun.rs, places.rs); `route.v1` (25e: a place's profile as its default and cap, ceiling.rs, `turn.submit`);
independence (28a, just merged: check.rs, `check_of`); replay (25d) and the ladder (26a: `judge_act`); security
notices (24: the gate in policy.rs); the memory pass (31a: judge/mod.rs). Keep your edits in confirms.rs, policy.rs
and toolrun.rs to a few lines each; put the logic in task_graph/ and the Discord crate.
