<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate you touch, scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.

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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-ontology-view`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the cockpit's Ontology view, step 21c (theseus-8kk.2)

Branch: `cloud/20261004-ontology-view`. Every commit's subject carries `theseus-8kk.2`. Deadline for the report: 3
hours after you start.

**Background.** Step 21b wired the ontology into the core (crates/theseus-ontology; crates/theseus-core's ontology.rs
and rpc/ontology.rs): a kinds table; categories (a tree of topics, and the given `channel:` and `person:` categories
made when a place binds); guidance per category; and each session's memberships, rendered into the system block at a
recompile. Its surfaces are the protocol and the CLI (`theseus ontology kinds | categories | topic add | guide |
member`). Step 21c gives the operator the same in the cockpit, the daemon's web app in cockpit/. It is a view only.

**Read first:**
- docs/design/m4-boundaries.md §2.8 (the kinds table, the data shapes, the compile walk, the guardrails, surfaces)
  and its 21c entry;
- cockpit/AGENTS.md, whole: read only what is on screen; every control confirmed first, and a protocol method; state
  in the address; the time machine; `npm test`'s pure modules;
- cockpit/src/main.tsx, components/Shell.tsx (the navigation list), views/SessionDeck.tsx (it reads
  `compilation.list` and has the Recompile control), views/Boundaries.tsx, lib/rpc.ts, lib/hooks.ts, and a lib test;
- the protocol's types: cockpit/src/protocol.gen/ (`OntologyListResult`, the three writes' params) and
  crates/theseus-protocol/src/ontology.rs; crates/theseus-core/src/rpc/ontology.rs for what each method refuses.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The cockpit replaced the web UI and the Observatory** (`web/` is gone). The design's "web UI's Ontology view" is
  a cockpit view, and the Observatory's session memberships belong in the session deck.
- **Writes are judged** by `judge_act` (the owner, from a private place). The cockpit is an owner's surface, so its
  writes count; a refusal comes back as `REFUSED` (-32005) with its words, and a given kind's membership is refused as
  invalid params. Show both.
- **No Rust.** 21b's four methods are all the view needs. If one is missing, say so in the report instead of adding it.

**What to build (21c):**
1. A view at `/ontology` (one route line in main.tsx, one entry in Shell.tsx's navigation):
   - the kinds table;
   - the category tree, depth-first, each category with its kind, `added_by`, description, and its guidance's version
     and digest;
   - a guidance editor (`ontology.guidance.set`; empty text takes guidance away), the text shown in the confirm before
     it is sent;
   - "add a topic", with a parent picker (`ontology.category.add`).
2. In the session deck, a memberships panel:
   - the session's given and interpreted memberships (`ontology.list { session_id }`), each with its origin and as-of;
   - beside them, what the newest compilation's manifest recorded (`memberships`, `guidance`), so a change not yet
     compiled reads "applies at the next recompile" next to the existing Recompile control. `CompilationInfo.manifest`
     is untyped on the wire: read both fields defensively, since an older compilation has neither;
   - add and remove a topic (`ontology.membership.set`).
3. Pure helpers in src/lib/ontology.ts (the tree's order and depth; the pending difference between the list and the
   manifest), tested in test/ontology.test.ts the way the other lib tests are.
4. The time machine. The view reads the present through `ontology.list`. While the time machine shows a past moment,
   the view says it shows the present, and its controls are off, as cockpit/AGENTS.md asks of acts.

**Proof, offline:**
- `npm ci --offline` if needed, then `npm run lint`, `npm test` and `npm run build` in cockpit/, and the gate;
- the new tests: the tree's order and depth, with a nested topic and a given category; the pending difference (one
  added, one removed, a guidance version changed, nothing pending);
- planted reverts: sort the tree by name alone, and show the order test fail; compare memberships without the
  guidance version, and show the pending test fail;
- load every view from a scratch daemon of the branch's build if a browser runs here, and check each loads with no
  console or page errors; if none runs, say so.

**The live check is the maintainer's.** Write it in the report as exact commands: build the cockpit, then the
daemon; lay out a scratch daemon with `theseus-sim discord rig --dir <dir>` (its fake Discord binds `#lab` and a DM)
and set `enabled = true` and `port = 7461` under its config.toml's `[web]` (never 7433); start the three processes
as the rig prints. Then, in a browser at that port's `/ontology`, and what each should show:
1. The categories made at the bind: `lab` (`channel:…`) and the DM's person, origin `transport`.
2. Add the topic `harbor` with a description, then `harbor-tides` under it, and set guidance on `harbor`;
   `theseus --socket <dir>/sock ontology categories` lists both, with guidance version 1.
3. In a CLI session's deck (`theseus … sessions open`, then one `ask`), add `harbor`: it reads "applies at the next
   recompile"; press Recompile; after the next turn the note is gone and the manifest lists `topic:harbor`.
4. Try to add `channel:<lab's id>`: the refusal's words show.

**Leave alone:** the cockpit's other work tonight: the Judgment section (23b), the MCP prompt picker (36c), extension
cards (43a), places with their guilds and ceilings in the Boundaries and Systems views (38a). Keep main.tsx and
Shell.tsx to one line each. categorize.v1's proposals (28b) are being built beside you: say in the report where their
panel should go. No Rust, and never edit the generated TypeScript by hand.
