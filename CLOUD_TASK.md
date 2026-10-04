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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-bindings-v2`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: bindings in several guilds, with a ceiling per place, step 38a (theseus-ext.3)

Branch: `cloud/20261004-bindings-v2`. Every commit's subject carries `theseus-ext.3`. Deadline for the report: 5 hours
after you start.

**Background.** The Discord binding (crates/theseus-discord) reads one bindings file at its start
(`<state>/bindings.toml`, `bindings.rs`): one `guild_id`, `[[channel]]` places and `[[dm]]` places. The place rule
(crates/theseus-core/src/places.rs) makes each place private or shared: a shared place is offered the public tools
alone, and only the owner's answers from a private place count. A trusted guild (`private = true` beside
`guild_id`) makes every channel bound there private, and its viewers are not read. Step 38a lets one daemon bind
places in several guilds, each with its own word on trust, and gives each place a ceiling the operator writes: a
posture floor, the tools offered, a spend limit, and a profile. 42a's budget and policy views build on it.

**Read first:**
- docs/design/m7-surface.md §2.3 "38a" and 38a's entry under "Each step's tests and live check";
- crates/theseus-discord: AGENTS.md, bindings.rs, bindings.example.toml, runtime.rs's start (the places, the viewer
  read, `trust_guild`, the commands' registration), runtime/audience.rs, runtime/voice.rs;
- crates/theseus-core: places.rs (`BoundPlace`, `PlaceRule`, `offered`, `refusal`, `owner_in_private`), policy.rs
  (`Posture`, `at_least`), the kernel's spend-limit follow (`follow_limit`, `budget.limit_changed`), turn.rs's
  `target_for_session`, rpc/bindings.rs; theseus-protocol's places.rs and `BindingStatus`; the CLI's render/places.rs.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The place rule replaced labels and audiences**, and the trusted guild (Item 89) puts trust in the bindings
  file. In format 2 each guild needs its own word: give each guild a table of its own (`[[guild]]` with `id` and
  `private`, unless a cleaner shape shows itself), and say what you chose. A format-1 file (a top-level `guild_id`,
  and its `private`) loads with the meaning it has today; a file that mixes the two is refused, naming the line.
- **A ceiling narrows what the place rule allows, and never widens it.** Read a place's ceiling where the place rule
  reads its class, so a task, whose target is its parent's place, inherits it. The design's copy in
  `Authority.ceilings` is optional: write it only if something reads it.
- **A tool the place does not offer** is refused as `places::refusal` refuses one in a shared place, with words
  naming the ceiling.
- **Slash commands are registered globally** (one list reaches every guild and the DMs). Keep that, and say in the
  report whether the design's per-guild registration is still wanted.
- **MCP tools come through `McpBoard`** if 36b has joined main when you start: then a ceiling's `mcp:<server>` entry
  offers that server's tools. If it has not, accept `mcp:<server>` entries, filter the built-ins, and say so.
- **The config note is sparse:** a new key needs a default and a template line. The bindings file is the operator's,
  not the config.

**What to build (38a):**
1. Bindings format 2: each `[[channel]]` names its `guild`; each guild's trust word; voice channels (`voice = true`)
   in any bound guild; `theseusd example-bindings` showing format 2, with the trust line commented as today.
2. Ceilings, `[channel.ceiling]` and `[dm.ceiling]`: `posture_floor` (a call's posture is the strictest of the
   config's, a tightening, the floor and T1's hold; never a refusal), `tools` (families such as `fs`, `git`, `web`,
   `proc`, and `mcp:<server>`; absent means no narrowing), `spend_limit_usd` (the lower of `[kernel]
   spend_limit_usd` and this, following either when it changes, as the 3pj rule does), and `profile` (the place's
   model unless a turn names one).
3. The binding per guild: the viewer read only for private channels outside a trusted guild, as today; the core told
   each guild's trust; interactions routed by channel across guilds.
4. Surfaces: health's `bindings[]` and places with each place's guild and ceiling; `theseus health` counting places
   by guild; `theseus places`; `discord.bound` with `guild` and `ceiling`; the cockpit's places panel (Boundaries) and
   Discord section (Systems), a small edit each.

Keep crates/theseus-discord/src/runtime.rs (3,435 of its 3,500 lines) and crates/theseus-kernel/src/kernel.rs to calls
and fields: new code goes in modules of its own.

**Proof, offline:** the design's 38a tests (format 1 and format 2 both parse; a floor makes a `notify` tool wait; a
tool outside `tools` isn't offered, and a call naming it is refused; the spend limit is the lower of the two and follows
changes; a task inherits the ceiling; interactions route correctly across two guilds). Also: a trusted guild beside an
untrusted one keeps each channel's class; the misplaced-key cases of today's tests still fail safe; a ceiling never
offers a shared place a private tool. Run the bindings and places tests 5 times under load. Planted reverts: take the
looser of posture and floor, and show the floor test fail; let a ceiling's `tools` add to a shared place's set, and
show that test fail.

**The live check is the maintainer's.** Write it in the report as exact commands on `theseus-sim discord rig --dir
<dir>`, with its `state/bindings.toml` replaced by a format-2 file: guild A trusted, with `#lab` and a floor of
`approve`; guild B untrusted, with a shared `#pier` whose `tools = ["web"]` and `spend_limit_usd = 1`; and the DM. Use
`theseus-sim fake-model --rules` with one rule that calls `proc.run` on `true` and one that calls `wake.at`. What each
should show:
1. `theseus health` and `theseus places`: both guilds, each place's class and ceiling; `#pier`'s session limit is $1.
2. The `proc.run` rule said in `#lab` waits, and its card posts there; the same in the DM runs with a notice.
3. In `#pier`, `compilation.list`'s newest manifest lists only the web tools, and the `wake.at` rule's call is refused
   with the ceiling's words.
4. `theseus --json ledger --kind discord.bound -n 1`: `guild` and `ceiling` on the row.

**Leave alone:** the place rule's own rules (narrow, never rewrite them); 36c's `/prompt` command and 43a's extension
cards, being added to the binding tonight; 30b's reply footer in the Discord renderer; 24's `security.v1` beside the
gate's decision; the MCP board and the terminal tools (joining main tonight); voice's pipeline; the cockpit beyond the
two places named above.
