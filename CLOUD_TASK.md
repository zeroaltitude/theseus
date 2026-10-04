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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-mcp-prompts`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: MCP prompts, step 36c (theseus-ext.4)

Branch: `cloud/20261004-mcp-prompts`. Every commit's subject carries `theseus-ext.4`. Deadline for the report: 4 hours
after you start.

**Background.** Step 36b (merged) put the operator's MCP servers' tools in front of the model: `McpBoard`
(crates/theseus-core/src/mcp/) starts each `[mcp.servers.<name>]` after serving, keeps each server's last good tool
list as a META record (`mcp.tools.<server>`), and offers its tools in private places only. theseus-mcp's client
already lists and gets prompts (`list_prompts`, `get_prompt`), and its fake serves three (`greet { name }`,
`brief { topic? }`, `review { path }`), but the board only counts them (health's `prompts`). Step 36c lets the
operator run a server's prompt as a turn's input: `/prompt` on Discord, `theseus prompt`, and a picker in the cockpit.

**Read first:**
- docs/design/m7-surface.md §2.1's "Prompts (36c)" and "Seen in", and the 36c entry under "Each step's tests and live
  check";
- crates/theseus-core/src/mcp/ (mod.rs, tool.rs, tests.rs), rpc/mcp.rs, fact/mcp.rs, external.rs (the session's
  hold), places.rs, and the AGENTS.md; crates/theseus-mcp (AGENTS.md, client.rs, types.rs, fake.rs);
  theseus-protocol's mcp.rs and `TurnSubmitParams`; theseus-discord's runtime.rs (`commands()` and the interaction
  dispatch) and runtime/voice.rs, a command in a module of its own; crates/theseus/src/mcp.rs; and the cockpit's
  Composer.tsx and AGENTS.md.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The place rule replaced labels:** a place is private or shared. MCP is not public (36b offers its tools in
  private places only), so a prompt runs only for a session in a private place; a shared place's `/prompt` or
  `turn.submit { prompt }` is refused, saying why.
- **The cockpit replaced the web UI and the Observatory:** the picker goes beside the cockpit's composer. The
  protocol's TypeScript is generated into cockpit/src/protocol.gen/.
- **One store format number,** `MANIFEST_FORMAT`, 7 on main. A node's `Origin` has no `mcp` value today. A new value,
  or a new field on a stored record, is a layout older builds can't read, so it bumps the number with a layout sample
  in tests_layouts.rs (AGENTS.md's version rule). A new META key, like 36b's stored list, needs no bump.
- **Near their ceiling** (scripts/long-files.txt): theseus-discord's runtime.rs is at about 3,435 of its 3,500 lines.
  Put `/prompt` (its autocomplete, its modal, and the modal's parse) in a new module beside runtime/voice.rs, and add
  only the command and the dispatch lines to runtime.rs. Protocol types go in theseus-protocol's mcp.rs.

**What to build (36c):**
1. The board keeps each server's `prompts/list` (the arguments, and a digest of each definition), lists it again on
   `notifications/prompts/list_changed`, and stores it under a META key of its own (`mcp.prompts.<server>`), so a
   start lists prompts at once, as 36b's tools are. `mcp.prompt.list` reads them.
2. `turn.submit { prompt: { server, name, arguments } }`: the core calls `prompts/get`, waiting for that server
   alone as a tool call does, and the messages become the turn's input nodes: user-role, origin `mcp`, author
   `prompt:<server>/<name>`. Text stays text, an image becomes an attachment, and an embedded resource becomes text
   with its URI. A server with `external = true` (the default) gives the session T1's hold in the input's own frame,
   as an external result does. A missing required argument, an unknown prompt, or a server that is down is an error
   before any node is written.
3. A prompt whose definition changed since its last use: `mcp.prompt_changed` (a fact in fact/mcp.rs, ledgered and
   narrated) and an operator notice through the outbox, on the model of 36b's `mcp_changed` post. The use goes ahead.
4. The CLI: `theseus prompt <server/prompt> [--arg k=v]… [--session <id>]`, which streams the reply as `ask` does.
   `theseus mcp` lists each server's prompts too.
5. Discord: `/prompt name:<server/prompt>` (a bare name), with autocomplete from `mcp.prompt.list` (at most 25
   choices, filtered by what was typed). Then a modal with one text input per argument (up to 5), or one `args`
   input of `k=v` pairs for a prompt with more.
6. The cockpit: a prompt picker beside the composer, with the prompt and a field per argument (required ones
   marked). It sends `turn.submit { prompt }`.

**Proof, offline,** with theseus-sim's `fake-mcp` and the in-process fake. The design's 36c tests:
- the input nodes, with their origin and author; the hold for an external server, and none with `external = false`;
- the autocomplete answer, the modal, and its parse (5 arguments, 6, and a missing required one);
- the CLI's arguments;
- a changed definition gives its row and notice once.

Also: a shared place's prompt is refused, and a restart lists the stored prompts before the server is up. Run the
cockpit's lint, test, and build. Planted reverts: drop the hold for an external server's prompt, and show its test
fail; skip the changed-definition notice, and show its test fail.

**The live check is the maintainer's.** Write it in the report as exact commands, on a scratch daemon with a fresh
state dir, the fake MCP server as `[mcp.servers.fake]`, and the fake Discord:
- `theseus prompt fake/greet --arg name=Ada` gets a reply that greets Ada;
- the history shows the input node with author `prompt:fake/greet`, and the session holds external text (an
  `fs.write` asks next);
- `theseus mcp` lists the three prompts, and after a restart lists them before the server is up.

It costs only the model's tokens. `/prompt` itself needs a person to type it, so it goes into the owner's testing.

**Leave alone:**
- 43a's extensions: another session adds proposed servers to the same board, so keep your edits to mcp/mod.rs
  small and put prompts in a module of their own;
- the MCP server side (41b);
- `classify.v1` and `role.v1` at inbound: another session changes the inbound path;
- bindings format 2: another session changes places and the Discord runtime;
- the cockpit's Judgment and Ontology views, which other sessions are adding.
