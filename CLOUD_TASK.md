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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-route`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: `route.v1`, Jev's model per interaction mode, step 25e (theseus-0j2.11)

Branch: `cloud/20261004-route`. Every commit's subject carries `theseus-0j2.11`. Deadline for the report: 5 hours
after you start.

**Background.** At `inbound` (25a, judge/inbound.rs), Jev judges each person's message with `classify.v1` and
`role.v1`, in shadow, in one request. The owner wants a model per interaction mode, from the cheapest for a message
that needs nothing to Opus 5.5 or Fable 5.1 for hard reasoning (step 2 has the table). 25e adds `route.v1` to that call,
**live** whenever the judge is on (not waiting for 26a's ladder), with `shadow` as a setting.

**Read first:** docs/design/m5-judgment.md §2.2, §2.5 to §2.7, §2.16; crates/theseus-judge (pack.rs, batch.rs,
builders.rs, fake.rs, packs/); theseus-core's judge/, turn.rs, turn/, rpc/methods.rs (`turn.submit`), ceiling.rs,
compiler.rs's header; crates/theseus/src/interactive.rs.

**What the code says** (the code and AGENTS.md win over this prompt; report each difference, and where a question
is left that the code can't settle, build the clear part and report the question):
- No session holds a profile: a message runs on the one it names, else its place's (38a), else the live one, and
  `last_target` (the last turn's) serves continuations. The CLI's interactive pane names the last turn's profile
  with every message (theseus-nu3z), so a named profile is not by itself the owner's choice.
- §2.6 pays live judgments from the session as kernel actions (26b's, unbuilt): keep the point's one reservation
  against the judge's day budget, as rerank.rs does.

**What to build (25e):**
1. **The pack**, packs/route.v1.toml in pack.rs's list, with a golden request fixture. Its state must equal
   classify.v1's byte for byte (`state = "inbound"`, the same cap and `jev_model`) to batch. One deciding Choice,
   `mode`: `trivial` (needs nothing: thanks, ok, a greeting), `chat` (conversation, a quick question),
   `sophisticated` (hard reasoning, design, research, careful writing), `deep_coding` (hard or novel programming,
   debugging), `routine_coding` (long, well-understood programming: repeated edits, a clear plan), and the no-match
   `other`, routed as `chat`. Add a `Baseline` and an `Action` variant; `WIRED` gives it `PackMode::Live`. It rides
   `at_inbound`'s one request (the cost split by question count), which now waits for a permit (`Urgency::Live`,
   under the client's whole-call limit) instead of being shed, and hands the turn its verdict.
2. **`[routing]`**, a config module of its own, sparse, in the template: `enabled` (true; acts only while `[judge]`
   is on), `mode` (`"live"` by default, or `"shadow"`; `[judge.packs."route.v1"]` lowers it too), `max_wait_ms` (200),
   `trivial_context_turns` (2), `cold_switch_tokens` (30000), `switch_confidence` (0.6), and ordered
   `[routing.modes.<mode>] profiles`: `trivial = ["cheapest"]`, `chat = []`, `sophisticated = ["opus", "fable"]`,
   `deep_coding = ["opus", "sonnet"]`, `routine_coding = ["glm53", "glm"]`. The first usable wins, else the next,
   else the session's own. Usable (model providers have no breaker; only Jev has): configured, its key settled, its
   model priced, and it reads the turn's images (glm-5.3 is text-only).
   `cheapest` (reserved) is the usable profile cheapest for a short turn at catalog prices. The template gains
   `[profiles.opus]`, `[profiles.fable]` and `[profiles.glm53]` (claude-opus-5-5, claude-fable-5-1, glm-5.3).
3. **FAST.** The verdict is awaited beside the first loop's compile, never before it: the call waits at most
   `max_wait_ms` after the compile ends. Real inbound calls took 255 ms (a cold connection), 101 and 100 ms, and the
   call starts before the compile: 200 covers a warm call and most cold ones. A late verdict applies from the next
   message, and the turn records `late`. Judge off, Jev's breaker open, the budget spent, a slash command, a
   continuation: no routing, no wait. A switched turn rebuilds its spec and compile; persist only the compilation the
   call uses.
4. **The cache.** `trivial` is a **detour**: that turn alone runs on the trivial profile, with the persona and the
   last `trivial_context_turns` exchanges compiled outside the session's compilation; the session's profile,
   `last_target` and compilation stay untouched, and the reply brings no thinking into later requests. A switch
   recompiles, strips the prefix's thinking (compiler.rs) and leaves the cache cold, so other modes switch the
   session's routed profile (a stored field bumps `MANIFEST_FORMAT`): at once while the compile's
   `est_tokens` is under `cold_switch_tokens`, above it when two turns in a row agree, and only at
   `switch_confidence` or more. Decide whether a detour needs that confidence too (default: yes). Weigh switches
   with the catalog's rates, not list prices alone: GLM 5.3's cache read ($0.26/M) is above Opus 5.5's ($0.20/M), so a
   switch to it pays back through output only; report the break-even your rule implies. GLM 5.3 Flash reads a cold
   100k context ($0.015) for less than Opus's warm read ($0.020), so the detour's trim is for speed and for sending
   less text to a second provider.
5. **The owner's choice wins.** A turn whose profile, provider or model the owner chose (`theseus ask -P/-p/-m`,
   `theseus prompt -P`, the cockpit composer's picker) is not routed; its verdict is recorded in shadow, reason
   `pinned`. Mark which `turn.submit` names are choices (the pane's carried one is not): no model changes under the
   pane without a word. `profile.use` changes the live profile; it is no pin. A place's profile (38a's ceiling) is
   the place's default **and its cap**: there, Jev routes only to profiles no dearer than the place's at catalog
   prices (a detour still happens; nothing climbs above the place's model), reason `capped` when the mode's pick
   was dearer.
6. **Records.** A fact per routed turn (its row in the turn's next frame): mode, confidence, profile, wait, and
   reason (`verdict`, `late`, `pinned`, `capped`, `fallback`, `cache_hold`, `detour`, `shadow`). Judgments are scoped
   `judge:route`; route.v1's acting classes go in learning/report.rs's `ACTING`, so 25c grades it. Add a system label
   (learning/system.rs, weight 0.5, like the others): when the owner chooses a profile (step 5's choices) for a
   message within 10 minutes after a routed turn in the same session, that turn's `mode` was wrong; and where the
   chosen profile is in exactly one mode's list, that mode was the right answer. The CLI's status
   line shows the profile and model the turn ran on, and the mode and reason if a `TurnSubmitResult` field does it;
   the cockpit's session header too if it is cheap, else report it.

**Proof, offline**, with the fake Jev and the simulator, profiles on different fake providers: one call for three
packs; each mode's first usable profile, then the next, then the session's; `cheapest`; after a detour, the next
prefix byte-identical; above `cold_switch_tokens`, a switch waits for a second agreeing turn; none under
`switch_confidence`; pinned turns unchanged, the pane's profile no pin; a place's cap (a detour below it, `capped`
above it); the system label (a choice 9 minutes after a routed turn labels it, 11 minutes after does not); each fake
mode (down, slow, 429, malformed)
and judge off leave the request unrouted; the turn bench (judge off) and frame budget unchanged; a paused-clock test:
a verdict that never comes releases the call exactly `max_wait_ms` after the compile, one at +50 ms then, a late one
applies next turn; timing tests also under load. Planted reverts: await the verdict before the compile (the bound
test fails); let the detour write the session's compilation (the prefix test fails).

**The live check is the maintainer's**, with `[secrets]` entries `anthropic_api_key`, `zai_api_key` and
`jev_api_key`. Write exact commands for a fresh scratch daemon with `[model] live = "sonnet"`, the five profiles,
`[providers.zai]`, `[judge] enabled = true`, `[routing]` at its defaults, and the cockpit on a free port. One CLI
session, no `-P`:
1. A hard design question (a crash-safe write-ahead log, two designs weighed) routes `sophisticated` to `opus`.
2. "thank you!" routes `trivial` to `glm` (the cheapest) as a detour; the session stays on `opus`.
3. "rename these twelve call sites the same way" routes `routine_coding` to `glm53`.
4. In a session with an open task (the task view rides the last message, behind a moved cache breakpoint), two
   routine-coding turns on `glm53`: the second request's usage shows cache reads. If it shows none, report it: GLM
   may ignore the block-level breakpoint.

Each status line names its model. `theseus judge log` shows three `route.v1` judgments, each in one call with
`classify.v1` and `role.v1`; the routed rows give each reason; the cockpit's Judgment section shows `route.v1` live.

**Leave alone:** compaction's roots and summary model (30c, just joined, with `summary_profile = "session"` the
default; its compile step now lives in turn/compile_step.rs): its `"jev"` value waits for this step, so list it as a
follow-up; the memory pass (31a: two packs, also new in pack.rs's list); independence
(28a: `check_of`, a task's `profile`); extensions loading (43b); the hands' network (40).
