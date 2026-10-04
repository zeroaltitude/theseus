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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-replay`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: replay, audit and backfill for the learning ledger, step 25d (theseus-0j2.14)

Branch: `cloud/20261004-replay`. Every commit's subject carries `theseus-0j2.14`. Deadline for the report: 5 hours
after you start.

**Background.** Jev, an external judging API, judges in the daemon through `[judge]`, `JudgeService`
(crates/theseus-core/src/judge/) and versioned packs (crates/theseus-judge/packs/). Each judgment is a `judge.call` row
scoped `judge:<pack id>`; the state it sent is a blob named by the row's `context.blob`. Step 25c (learning/) added
labels (operator, system, audit), a nightly report per pack version and question from `learn.rs`, and frozen holdouts.
25d is §2.9's replay, audit and backfill. **The learning loop (a later step, theseus-0j2.12) is built on replay:**
nightly, a writer model proposes a pack's next version from the owner-labeled errors in its train split; replay checks
it on those errors (fixed?) and on the holdout (better, no class worse?); the ladder (26a, beside you) promotes it. So
the core must be able to call replay in-process.

**Read first:** docs/design/m5-judgment.md §2.3, §2.5, §2.6, §2.9, §2.14 to §2.16, and 25d's entry under "Each step's
tests and live check"; theseus-judge's pack.rs, builders.rs, judge.rs, band.rs, learn.rs, eval.rs; theseus-core
(AGENTS.md's judge and learning entries first): judge/, learning/, rpc/learning.rs, rpc/confirms.rs (`judge_act`),
config/judge.rs, provider.rs; crates/theseus/src/client.rs (`OPERATORS`).

**What the code says** (it wins over this prompt: report each difference; where it can't settle a question, build the
clear part and report the question):
- Jev reads a question's `instructions` and criteria: a Choice's options' `means`, a Score's `levels`, a Noul's
  `when_true` and `when_false` (client.rs sends them as `criteria`). A candidate changes these, the thresholds, the
  builder or its cap, or the pinned model.
- `WIRED` (judge/mod.rs) is what the points dispatch, `EMBEDDED` every version the build knows. `[judge]
  audit_limit_usd` (§2.15) doesn't exist yet; a new `[judge]` key needs a default and a template line.
- A judgment's time is its row's (`Seen::at_ms`); a holdout's window is over those times. Ledger rows need no
  `MANIFEST_FORMAT` bump (§2.14).

**What to build (25d), each a green commit:**
1. **Replay** (`judge.replay`; `theseus judge replay <candidate> [--report <id>] [--split holdout|train] [--errors]
   [--judgments <ids>]`). This adds to the design:
   - The candidate is a version not in `WIRED`: an embedded one by name, or `--pack-file <path>` (the CLI sends its
     text; the daemon parses it with `Pack::parse`, so every loader rule holds, and opens no path). Same id as the
     incumbent; a version no row holds under another sha256, so a name always means one text. It never acts.
   - The set: a report's frozen holdout (`Holdout::judgments`), its train split (answered judgments before the window,
     never the holdout's), only the labeled ones the incumbent got wrong (`--errors`), or ids. Labels: the report's
     frozen ones for its holdout, else today's; the result says which.
   - The states: each row's blob, sent as it was when the candidate's builder, builder version and cap equal the
     judgment's; else rebuilt from its inputs (step 3), or left out with the reason. A thresholds-only candidate makes
     no call: re-band the stored answers (band.rs).
   - Each call through the judge's client and breaker is a `judge.call` row with `purpose: replay`, the run and the
     judgment it re-asks, in a scope of its own (`judge.replay:<pack id>`): the nightly report never counts it.
   - The result, candidate beside incumbent on the same judgments, through `learning::report`'s `question` and
     `pack_report` (taking the candidate's `Pack`), so every number is `learn.rs`'s: per question, labeled count,
     precision and recall per Choice class, Brier, ECE, band shares; agreement; per judgment, fixed (incumbent wrong,
     candidate right by the label) and broken (the reverse); per class, whether precision or recall fell. A security
     candidate also runs eval.rs's planted-injection set, met and missed beside the incumbent's. A `judge.replay` row
     (`rpl_…`) holds the run: the candidate's name, sha256 and text (a blob), the set, counts, cost, result.
   - Spend: priced and reserved like any judgment; the run's estimate is checked first against `[judge]
     replay_limit_usd` (new; 0.50 a run), refused with the numbers. A replay never pauses shadow judging (spend.rs).
2. **Audit** (`judge.audit`; `theseus judge audit <pack> --sample <n> --profile <p>`). A model profile answers the
   pack's questions over a seeded sample of answered judgments without an audit label: one request per state through
   the profile's `Provider`, outside any session, priced from the catalog, reserved and settled (as compaction's
   summary call does, if 30c is on main). It gets each question's instructions and criteria as Jev does, and answers
   one JSON value per question id; an answer outside the options is counted and dropped. Each answer is a
   `judge.label` row, `source: audit`, weight 0.5, keyed by judgment, question and run. The run stops before it would
   pass `[judge] audit_limit_usd` (new; 5.0); a `judge.audit` row holds it.
3. **Backfill** (`judge.backfill`; `theseus judge backfill <pack> --since <date>`). Rebuild each judged point's input
   from the recorded history with the live point's own input functions, at the event's time as `now`; build the
   state; judge it in shadow (`purpose: backfill`) with the live point's context fields (session, turn, correlation
   id, class), so 25c's system labels reach it. Its holdout time is the event's (`context.event_at_ms`, which the
   report's split reads). An event that version judged is skipped (a second run writes nothing); a pack whose input
   the store can't rebuild is refused with the reason (check rerank's and CONTINUE's).
   - **Consent.** Backfill sends the owner's history to a third party, so it runs only under his recorded consent:
     `[judge] backfill_consent` (new, false by default) in his config note, which agents can't write (§2.7). Without
     it the run is refused, naming the line. Each run's `judge.backfill` row records the consent it ran under (the
     config's digest), the pack, the window, counts and cost. Its budget is replay's. Tests use scripted stores.
4. **The owner's runs.** Each run spends money, and backfill sends history: each is an `Act` of its own through
   `judge_act` (the owner, from a private place) and in `OPERATORS`. Each runs on a task of its own, off the start
   path and every turn's, at low priority like the tender (FAST). The CLI prints a replay's two versions side by side.

In the report, write the paragraph §2.9's "Replay, backfill, and audit" should gain (you don't edit docs/design/).

**Proof, offline**, with the fake Jev and the simulator, on seeded stores: §3's 25d tests (a replay over a frozen
holdout; a state rebuilt when the builder version changed, refused with the reason when it can't be; the audit's
per-run cap; backfill's states from scripted history equal the live builders', byte for byte). Also: a replay's
incumbent side equals the report's numbers; a reworded criterion, against a fake scripted per state, fixes and breaks
the judgments it should; a thresholds-only candidate makes no call; backfill refused without consent; every run
refused from a job's shell and a shared place; a second backfill or audit writes nothing; the turn and lifecycle
benches unchanged. Planted reverts, each naming the test it breaks: replay rows written into `judge:<pack id>`; the
consent check dropped; the audit past its cap; backfill keyed by run, not by event.

**The live check is the maintainer's**, with `[secrets]` entries `jev_api_key` and a GLM key. Exact commands for a
scratch daemon (its own `--config`, `--socket`, fresh `--state-dir`; a GLM profile; `[judge] enabled = true`; Discord
and the web off):
1. Four `ask` turns; `theseus judge label <jdg> right` (or `wrong`) on three `loop.v1` judgments from `theseus judge
   log`; `theseus judge report` writes `rpt_<date>_loop.v1`.
2. `theseus judge audit loop.v1 --sample 5 --profile glm`: `theseus --json ledger -k judge.label` shows five `source:
   audit` rows.
3. /tmp/loop.v2.toml: packs/loop.v1.toml with `version = 2` and one option's `means` reworded. `theseus judge replay
   --pack-file /tmp/loop.v2.toml --report rpt_<date>_loop.v1` prints both versions; `--split train --errors` too.
4. `theseus judge backfill loop.v1 --since <yesterday>` is refused, naming the line. Never add it in a live check:
   the owner gives it.
5. With `THESEUS_SESSION` set, `theseus judge replay` is refused.

**Leave alone:** the ladder (26a, `cloud/20261004-ladder`: `pack.mode`, config/judge.rs's modes, `WIRED`; add your
keys, change none of theirs); route.v1 (judge/inbound.rs, report.rs's `ACTING`, a system label); rerank made live
(per-item grading in labels.rs and report.rs); security.v3's notices (gate.rs); the memory pass (pack.rs's lists);
compaction's roots (30c); independence (28a); extensions loading (43b); the hands' network (40). Edit no pack file.
