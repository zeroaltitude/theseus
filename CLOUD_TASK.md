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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-extensions-load`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: an acked extension loaded, restarted with the daemon, and revoked, with `/extensions`, step 43b (theseus-ext.8)

Branch: `cloud/20261004-extensions-load`. Every commit's subject carries `theseus-ext.8`. Deadline for the report: 5 hours
after you start.

**Background.** Self-extension (M7 step 43; the spec's §3.21, "planks, never the keel") lets the model add a tool as a
small MCP server of its own. Step 36b built the MCP board (crates/theseus-core/src/mcp/: `McpBoard`, `McpTool`,
servers from `[mcp.servers.<name>]` started after serving, each server's tool list stored as `mcp.tools.<server>`,
tools offered in private places only). Step 43a let a stdio server run in L1 and built `extend.propose { name, dir,
command, description, tests?, network? }`: the proposal frozen into `<state>/extensions/<name>/<digest>/`, started in
L1 in a `proposed` state whose tools no turn is offered, tested, its manifest recorded, and put to the operator as a
card answered `extend.acked` or `extend.declined`. Nothing loads yet. Step 43b loads an acked extension, restarts it
with the daemon, and revokes it. Those are the names 36b's and 43a's prompts gave; the code on main is the truth
wherever it differs.

**Read first:**
- docs/design/m7-surface.md §2.7 (43b), §4's "decisions taken by default", §5's questions 21 and 22, and 43b's entry
  under "Each step's tests and live check"; the spec's §3.21 (docs/spec/04-part1-s3.10.md);
- crates/theseus-core (AGENTS.md first): mcp/ whole, config/mcp.rs, 43a's extension module, manifest, card and tests;
  rpc/confirms.rs (`judge_act`, `Act`); places.rs and 38a's ceilings; policy.rs (`[policy.mcp]`);
  crates/theseus-sandbox's AGENTS.md; crates/theseus/src/client.rs (`OPERATORS`, `refuse_in_a_job`);
  crates/theseus-discord's AGENTS.md, runtime.rs's commands, and how `/trust` acts as its presser.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The ack is 43a's:** `extend.acked`, the owner's answer from a private place through `judge_act`, refused in a
  job's shell. 43b loads on it, and adds no second ack.
- **"Never wider"** (§3.9), under the place rule: an extension's tools are offered only where MCP tools are (private
  places, within each place's 38a ceiling, whose `tools` may name `mcp:<server>`), and its process gets only what its
  proposal asked and the ack granted: no secret, and the network only if acked. For the design's "its ceiling is the
  proposing execution's at the ack", record that execution's place and ceiling with the load, and say how you apply it.
- **One store format number:** an `extensions` META record under a key of its own needs no bump, as 36b read the rule
  for `mcp.tools.<server>`; a new field on an existing record does.
- **This VM runs as root, and L1 refuses a root operator's job:** run the L1 tests as uid 65534, as 43a did (its
  report and tests say how).
- **Slash commands are one global list.** `/extensions` joins it from a module of its own (runtime.rs is near its line
  ceiling). The fake gateway carries no slash commands: test the command through the binding's `Place`
  (`place_for_tests`), as `/stop`'s tests do.
- **The web UI is the cockpit:** its Extensions view is the last sub-step, and may be left (say what it should show).

**What to build (43b):**
1. **Load on ack.** The ack writes the `extensions` record (name → digest, command, tools, capabilities, who acked and
   when, the proposing session) and `extend.loaded`, in its frame or the next (say which). The board starts the
   frozen copy in L1 as the server `ext-<name>`, and its tools, `mcp:ext-<name>/<tool>` (wire `mcp__ext-<name>__…`),
   are offered from the next turn's start, never mid-turn: posture `notify` unless `[policy.mcp] "ext-<name>"` says
   otherwise, class `Run`, results external unless its network is off (Q22). The `ext-` prefix is then reserved: a
   configured server so named is refused at the config's check.
2. **Restart.** Loaded extensions start after serving from their frozen copies, as configured servers do, and the
   stored list offers their tools at once. An edit of the workspace's dir changes nothing that runs.
3. **Revoke.** `extension.revoke { name }`, an operator act (`judge_act`, an `Act` of its own, in `OPERATORS`), from
   `theseus extend revoke <name>` and a Revoke button on `/extensions`: it stops the server (its whole L1 namespace),
   drops its tools from the next turn, and writes `extend.revoked`. The frozen copy stays on disk.
4. **A new version** of a loaded name is a new proposal and a new ack; the old version runs until the new one loads,
   and `extend.loaded` names the digest it replaced.
5. **Surfaces:** `/extensions` (each loaded extension: name, digest, tools, who acked and when, with Revoke); `theseus
   extend list` gains the loaded ones; health's `mcp[]` shows `ext-` servers; the narrative and telemetry as for MCP
   servers.
6. **The cockpit's Extensions view** (manifest, digest, files, tests, who acked, calls, errors, Revoke), if time
   allows.

**Proof, offline**, with 43a's fixture server or `theseus-sim fake-mcp` as the extension: the design's 43b tests (the
tools offered from the next turn, never mid-turn; a restart starts it after serving, from the frozen copy; a revoke
drops its tools and stops its process; a second version replaces the first only once acked). Also: a revoke from a
job's shell, and from a shared place, refused; a shared place's turn never offered an extension's tool; its process
gone after the daemon's stop and after its `kill -9`. Planted reverts: apply a load to the running turn's tools, and
show the next-turn test fail; restart an extension from the workspace's dir, and show the frozen-copy test fail.

**The live check is the maintainer's**, on the owner's machine (L1 refuses a root daemon). Write it as exact commands
that go on from 43a's check, whose word counter was acked, on its scratch daemon:
1. A turn asks for the word count of a scratch file: `mcp__ext-wordcount__…` runs with its notice; `theseus extend
   list` shows it loaded, and the ledger has `extend.loaded`.
2. `theseus shutdown`, then a start: `theseus mcp` shows `ext-wordcount` ready after serving, with no spawn in the
   start's frames, and the next turn calls it again.
3. `theseus extend revoke wordcount`: `extend.revoked`; `theseus mcp` no longer lists it; none of its processes is
   left; the next turn is not offered the tool.
4. `/extensions` and its Revoke button wait for the owner's own press.

**Leave alone:** 43a's propose, freeze, test and card (use them); the MCP server side (41b); 36c's prompts on the same
board (keep your edits to mcp/mod.rs small, your code in modules of its own); 42a's `policy.explain`, built beside you
(it reads `ext-` tools as MCP tools); bindings format 2's places; the terminal and LSP tools; `security.v1` beside the
gate.
