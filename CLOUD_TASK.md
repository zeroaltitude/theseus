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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-security-shadow`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: `security.v1` in shadow at the gate, step 24 (theseus-0j2.5)

Branch: `cloud/20261004-security-shadow`. Every commit's subject carries `theseus-0j2.5`. Deadline for the report: 5
hours after you start.

**Background.** Step 23a wired Jev, an external judging API, into the daemon in shadow (`[judge]`, `JudgeService` in
crates/theseus-core/src/judge/, a sink that records every judgment as a `judge.call` row, the shadow day budget, and
`loop.v1` at a turn's end). Step 24 adds the operator's risk classifier, "this is risky: 0-100%", asked of every call
that acts, after the gate decides. In shadow it changes nothing: Jev may one day make a call's treatment stricter,
never looser, and is never the only guard (the external-text hold stays the deterministic floor). It is also the
integrity path for text laundered through files, since the file-hash design was dropped for it. The packs are in
crates/theseus-judge/packs/: `security.v1` (the incumbent), `security.v2` (a candidate with computed facts and the
session's reads), and `security.v3` (v2 with `steered` deciding beside `risky`, above a provisional 0.75).

**Read first:**
- docs/design/m5-judgment.md: §2.4's `security.v1` row, §2.8b, §2.9's label sources, §5's Q2 and Q12, and §3's 24
  entry; the spec's Item 95 (docs/spec/16-part3-item-86.md);
- crates/theseus-judge: the three security packs, builders.rs (`SecurityInput`, `Input::Security`,
  `Input::Security2`), decision.rs, eval.rs, and fake.rs;
- crates/theseus-core (AGENTS.md first): toolrun.rs (`admit`, then `start`: the call planned, its notice sent, then
  run), policy.rs, external.rs and tests_external.rs (the hold, the floor), tighten.rs and rpc/policy.rs (the "should
  have asked" press, which names a call's correlation id), fact/tool.rs (`ToolNotified`), judge/, tests_judge.rs; and
  where the CLI, the Discord binding, and the cockpit draw a notice (the cockpit's press is
  src/components/ShouldHaveAsked.tsx).

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- v2 and v3 came after the design. v3 is v2 plus `steered` deciding, so it replaces v2 as the candidate. Wire v1 and
  v3, both in shadow; leave v2 unwired, since its questions are v3's and would be asked twice, and say so in the
  report. `tainted_paths` stays empty: file provenance waits for v3's shadow data (Item 95).
- The place rule replaced labels and audiences.
- The cockpit (cockpit/, served at `/`) replaced the Observatory.
- The notice is sent as the call is planned, and the gate never waits, so the score is live progress that follows its
  notice; the `judge.call` row is the record. The design gives judgments no notification method, but the CLI's `ask`
  and the Discord binding hear only notifications: give the score one, as the judgment's fact, with its readers (the
  reader rule), and report it as a divergence.
- A plain turn writes 5 frames, and each tool loop adds 4. The sink's own frames can land inside a turn's window:
  jev-wire-in's join found one inside the turn bench's measured turn (theseus-0j2.3, settled at that join; read how
  before you add judgments inside turns). A turn's own frames never change.
- Declines and approvals are system labels that the learning ledger (25c) derives nightly (§2.9). 24 writes only the
  operator's press.

**One convention, four sessions** (you, 23b's surfaces, and the inbound and compile points, each dispatching
judgments inside turns; keep to these names so the merges stay mechanical). The core mints a judgment's id at its
dispatch: `theseus_judge::Ask` gains `pub id: Option<String>` (`Ask::new` sets `None`), and `Judgment::pending` uses it
when set, else mints one as now. Each dispatch marks the turn's trace with a zero-length span: name `judge`, kind
`mark`, attributes `pack` (as `security.v1`), `point`, `mode`, and `judgment` (the id its row will carry). Put your
point in a module of its own under judge/ (a child module sees the service's private parts): `judge/mod.rs` gains a
`mod` line and its `WIRED` lines, and 23a's loop path stays as it is.

**What to build (24):**
1. The gate point: at `start`, once the call is planned and its notice sent, every call whose class is not `read`,
   and every `http.fetch` and `web.search` in a session that holds external text, goes to one decision point with v1
   and v3, spawned and marked. Its state is built in the spawned task (the tool, class, posture and why, argv, paths,
   URL, other arguments, the operator's last ask, the hold, the last calls, and for v3 the session's recent reads),
   scrubbed and capped by the builders. On the call's path: the choice to judge and the mark, nothing else.
2. The record: each judgment names the call's correlation id, its posture, and whether the hold made it wait, so the
   hold's asks can later be set beside the scores (theseus-hnc8).
3. The score on notices: a notified call's line gains `risk N% (shadow)` (v1's `risky`) on the CLI, on Discord's tool
   line, and in the cockpit, when its judgment lands.
4. Labels: a "should have asked" press naming a call (`theseus policy tighten TOOL --call ID`, and the cockpit's and
   Discord's press) writes a `judge.label` row (`lbl_…`, scoped `judge:security`, source operator, weight 1.0) for
   each of that call's judgments, in the press's frame. A press inside the sink's window still finds them. Keep
   §2.5's label shape: another change (28b) writes these rows too.

**Proof, offline**, against the fake Jev: §3's 24 tests: a property test over (posture, hold, floor, scripted score),
where every decision equals the judge-off one; in a holding session, a call scored 1% still waits; the floor still
asks; a `web.search` in a holding session is judged, and one in a clean session is not; a press with `--call` writes
the label for the matching judgment; a notice shows `risk N% (shadow)`; with Jev slow (5 s), `tool.started` comes as
fast as with the judge off, under load. T1's floor tests pass unchanged, and a judged tool loop keeps its frame
budget. Planted reverts: make `start` await the judgment before the call runs, and show the `tool.started` test fail;
judge every `web.search`, and show the clean-session test fail.

**The live check is the maintainer's**, with a real key under `[secrets] jev_api_key`. Write it as exact commands on
a scratch daemon with a fresh state directory, a GLM profile, and `[judge] enabled = true`: `proc.run echo hi` at
`notify` shows its score on the CLI's notice line; a deletion of scratch files scores higher; `web.search`, then `ls`:
the `ls` waits under the hold whatever its score; `theseus policy tighten proc.run --call <id>` writes a `judge.label`
row (`theseus ledger --json -k judge.label`).

**Leave alone:** what the gate decides (policy.rs's postures and floor, external.rs's rules, broker.rs); the security
packs and their eval (a pack's change is a new version); 23b's surfaces (the sentences, the metrics, `judge.list` and
`get`, the cockpit's Judgment section); the other judge points; the MCP, terminal, LSP, and extension tools other
changes are adding (judge them by class, as any call); cancel.rs; and places.rs.
