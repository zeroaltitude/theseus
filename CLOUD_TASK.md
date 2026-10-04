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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-ladder`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the ladder: pack modes, arms, promotion and rollback, step 26a (theseus-0j2.15)

Branch: `cloud/20261004-ladder`. Every commit's subject carries `theseus-0j2.15`. Deadline for the report: 5 hours
after you start.

**Background.** Jev, an external judging API, judges in the daemon through `[judge]` and `JudgeService`
(crates/theseus-core/src/judge/). A pack's mode is its line in `WIRED` (judge/mod.rs), under the config's ceiling
(`JudgeConfig::mode_of`: `max_mode` and a pack's own `mode` lower it, never raise it). 26a is §2.7's ladder, on 25c's
learning ledger: modes in the store, canary arms, promotion, and rollback by each pack's rules. Three packs go live
before it, outside it, and it adopts them: `route.v1` (Jev's model per interaction mode), `rerank.v1` (recall's
rerank, bounded, with its own breaker) and `security.v3`'s notices (after a call the gate let run that it is 90% sure
was risky). M6's memory canary uses the same arms (roadmap row 44), and the learning loop (a later step,
theseus-0j2.12) promotes its pack versions here.

**Read first:** docs/design/m5-judgment.md §2.4, §2.7, §2.8 (b), §2.9's holdouts, §2.14 to §2.16, and 26a's entry under
"Each step's tests and live check"; theseus-judge's learn.rs and pack.rs; theseus-core (AGENTS.md first): judge/,
config/judge.rs, config/memory.rs, learning/, rpc/confirms.rs (`judge_act`, `Act`), extend/mod.rs's `ask` and
extend/answer.rs (a card answered by `action.confirm`); crates/theseusd/tests/config_copy.rs (a restart in place);
crates/theseus/src/client.rs (`OPERATORS`); cockpit/src/views/Judgment.tsx.

**What the code says** (it wins over this prompt: report each difference; where it can't settle a question, build the
clear part and report the question):
- `PackMode` is off, shadow, canary, live. There is no `rolled_back` yet.
- security.v1's file names §2.7's notice rules (`notices_per_day` 30, `labels_per_day` noise 3); security.v3's names
  none. 25c kept the labels `noise` and `useful` for these rules: they grade nothing.
- The live packs' sessions build beside you (`cloud/20261004-route`, `-rerank-live`, `-security-notices`). Adopt
  whichever is on main when you clone as it is; for the rest, build the adoption, the rule and its event, and report
  the one call each point needs at the merge.
- Ledger rows need no `MANIFEST_FORMAT` bump (§2.14).

**What to build (26a), each a green commit:**
1. **The mode in the store.** A `pack.mode` row per change: the pack version, its mode (`off`, `shadow`, `canary`
   with a share, `live`, `rolled_back`), the mode before, who and through what, why, the report it cites with its
   holdout's bounds, `forced`, and a rollback's rule and words (`Fired::why`). Scope them per pack id on their own, so
   a first read is a few rows. The latest row is the version's mode, read at the first judgment and kept in memory;
   with none, `WIRED`'s line. `rolled_back` acts as shadow, and moving it up cites a report written after it, or is
   the owner's. Every point asks one function, `mode_for(pack, session)`: off, shadow or live for this session, under
   `mode_of`'s ceiling, unchanged.
2. **Arms.** A canary pack acts where `learn::arm(session, pack, share)` is canary and judges the control in shadow.
   Nothing is stored on the session; each judgment's row records its arm; a larger share keeps every canary session.
   Memory's canary (config/memory.rs) is 30b's minimal assignment: keep its hash, so a running experiment's sessions
   keep their arms, and report what moving it onto these arms takes.
3. **Promotion.** `pack.promote`; `theseus packs promote <pack> --canary <share> | --live [--report <id>]`; a button in
   the cockpit's Judgment section. The design's bar (a report whose frozen holdout has 200 labeled per deciding
   question and 30 per acting class, `learn::sufficient`, the version beating its baseline) gates **automatic**
   promotions: give the core a function for them, refusing one short of the bar with the numbers ("work_state:
   labeled 37 of 200"). **The owner can promote at any time:** below the bar his row says `forced`, with the numbers.
   **A security pack's promotion** (`security.*`), his or automatic, is always his approval card: a question answered
   as every question is (`action.confirm`: `theseus confirm`, Discord's buttons, the cockpit), judged by `judge_act`,
   as extend.ack's is. Approval writes the mode; a decline or no answer writes a declined row. A question is a planned
   action on an execution, and a promotion has none: decide where it lives, and say why.
4. **Rollback.** Each pack's rules are checked as each event lands (`learn::check_all`) and again by 25c's nightly
   run. After a restart, the day's events are read back first, so no count resets. A rollback is a row
   (`rolled_back`, `who: system`, the rule, its words), a notice on the owner's surfaces the way the judge's other
   notices go (find them on main), and health's line. Moving down needs nobody, but `pack.rollback` (`theseus packs
   rollback <pack>`) by hand is the owner's act, as a promotion is: a job's process that could roll back security's
   notices could silence them. Both are `Act`s through `judge_act`, and in `OPERATORS`.
5. **Adoption.** The three live packs' modes stand as the owner's promotions of 2026-10-04. At the ladder's first read
   (after serving, never on the start path), write each one's row once: `live`, `who: owner`, `why: "decision of
   2026-10-04"`; a pack the build lacks is adopted at its first judgment once it joins. Their rules then apply, from
   an adoption table keyed by pack id (a later version keeps them), read beside each file's own:
   - `route.v1`: the owner pins a different profile on 3 routed turns in a local day (he names a profile for a
     message within 10 minutes after a routed turn in that session, and not the routed one: route's `pinned` choices);
   - `rerank.v1`: its own breaker opens twice in a local day (the `judge.circuit` rows that name it);
   - `security.v3`'s notices: the design's, more than 30 notices, or 3 of its judgments labeled `noise`, in a day.

   The first two are new `RollbackRule` and `CanaryEvent` variants in learn.rs, with the loader's checks. Each of the
   three is a day's brake: its rollback lapses at the next local midnight (an `until` on the row), as the notices
   step's own brake does, and a brake that step recorded today (a `judge.paused` row, `what: "notices"`) reads as
   one; every other rollback stands until a promotion.
6. **Surfaces.** Health's judge line gives each pack's mode, share and why (`route.v1: live (owner: decision of
   2026-10-04)`, `security.v3: rolled back until 00:00 (notices_per_day)`); `theseus packs` lists each version's mode,
   rules and last rows; the cockpit's Judgment section shows them, with promote and roll-back buttons.

Not in this step: the report's canary outcomes and JUDGE_STOP's nudge (26b), the roles table (26c).

**Proof, offline**, with the fake Jev: §3's 26a tests (the config lowers a mode and never raises it; an automatic
promotion short of the bar refused with the numbers, the owner's ledgered as forced; a job's process and a shared
place refused; a security promotion is a card only the owner's answer from a private place carries, and a decline or
an expiry writes no mode; a synthetic rule trigger rolls a pack back; `max_mode = "shadow"` caps every pack after a
restart in place). Also: the adoption written once, after serving; each adopted rule fires on scripted events (3
pins, 2 breaker opens, 31 notices, 3 `noise` labels), not on a near miss; the brake lapses at midnight (tokio's paused
clock); a restart keeps the day's count; arms sticky and monotone; the lifecycle and turn benches unchanged. Planted
reverts, each naming the test it breaks: the config raising a mode; a security promotion without its card; the
adoption written at every start; the day's count reset at a restart.

**The live check is the maintainer's**, with `[secrets]` entries `jev_api_key` and a GLM key. Exact commands for a
scratch daemon (its own `--config`, `--socket`, fresh `--state-dir`; a GLM profile; `[judge] enabled = true`; Discord
off; the cockpit on a free port):
1. `theseus packs`: the adopted packs this build has are `live`, who owner; `theseus --json ledger -k pack.mode` has
   one row each, and a restart adds none.
2. `theseus packs promote loop.v1 --canary 1.0`: a `forced` row; health says `loop.v1: canary 1.0`; the next `ask`
   turn's judgment records its arm. `theseus packs rollback loop.v1`: a `rolled_back` row and its notice.
3. `theseus packs promote security.v1 --live`: a card, no mode; `theseus confirm <id> --approve` writes it. With
   `THESEUS_SESSION` set, the promote is refused.
4. An `ask` turn that runs `proc.run echo hi`, then `theseus judge label <jdg> noise` on three `security.v3`
   judgments from `theseus judge log`: security.v3 rolls back until midnight, with its notice.
5. `max_mode = "shadow"` and a restart in place: health shows every pack in shadow.

**Leave alone:** replay, audit and backfill (25d, `cloud/20261004-replay`: learning/, rpc/learning.rs, new `[judge]`
keys); route.v1 (judge/inbound.rs, `[routing]`); rerank made live (recall's live path, its breaker); security.v3's
notices (gate.rs, their key and brake); the memory pass (pack.rs's lists); compaction's roots (30c); independence
(28a); extensions loading (43b); the hands' network (40). Edit no pack file: a change to one is a new version.
