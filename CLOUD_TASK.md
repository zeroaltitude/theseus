<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-learn-loop`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: the learning loop, where the owner's labels rewrite Jev's packs, step 25f (theseus-0j2.12)

Branch: `cloud/20261004-learn-loop`. Every commit's subject carries `theseus-0j2.12`. Deadline for the report: 5 hours
after you start.

**Background.** Jev, an external judging API, answers each pack's typed questions (crates/theseus-judge; theseus-core's
judge/). It reads each question's `instructions` and its criteria (a Noul's `when_true`/`when_false`, a Choice's
options' `means`, a Score's levels) literally, and that text is the only prompt there is to adjust. On main:
- 25c labels judgments and reports nightly per pack version, against a frozen holdout (learning/);
- 25d replays a candidate version (a pack file not yet wired) over the stored states of chosen judgments, with its
  numbers beside the current version's through the same `learn::` functions;
- 26a's ladder moves versions between modes (`pack.mode`, promote, rollback, security's approval card).

The report only measures (§2.9: "It proposes nothing on its own"). The owner wants labels to adjust Jev's prompts.
This step closes the loop. Write it into docs/design/m5-judgment.md as a new §2.17 and step 25f in §3's table, and
update §2.9's "proposes nothing" line and §5's Q10.

**Read first:**
- the design: §2.3, §2.7, §2.9, §5 (Q9, Q10, Q15), and the 25d and 26a entries as main now has them;
- crates/theseus-judge: pack.rs (`EMBEDDED`, `Pack::parse` and the loader's rules), client.rs (`Question::wire`:
  what Jev reads), band.rs, and learn.rs (`Window`, `holdout_split`, `Split`, `sufficient`, `Minimum`);
- crates/theseus-core (AGENTS.md first): learning/ (`HOLDOUT_DAYS`, report.rs, labels.rs's `resolve`, tender.rs),
  rpc/learning.rs (`run_learning`), judge/, 25d's replay and audit, 26a's ladder, outbox.rs's `to_operator`; and
  cockpit/src/views/Judgment.tsx.

**What the code says** (the code and AGENTS.md win; use main's names for 25d's and 26a's parts). Report each
difference; where the code can't settle a question, build the clear part and report it.
- **Packs are compiled in** (`EMBEDDED`, Q10), but a learned version is born at run time, so it lives in the store:
  a row holds its whole TOML and digest, and `<state dir>/packs/<name>.toml` is written from the row (derived, as
  25c's `learning/<date>.json` is). Every version, compiled or learned, goes through `Pack::parse`. Name learned
  versions so a later build's compiled-in version can't collide (from v101 up, say), and report the rule.
- **Each point names its packs as constants** (gate.rs's `SECURITY_CANDIDATE`, inbound.rs's). A promoted learned
  version takes its parent's place at its point: one version of a lineage per role, never two. A rollback gives the
  place back. Build this on 26a's modes, not a second registry.
- **The holdout is 25c's:** the 14 days before the report's local midnight (`HOLDOUT_DAYS`); the train split is what
  came before, and today's judgments are in neither. So a store whose labels are new has no train split for two
  weeks. Keep the split for the nightly run, make its length config (`[judge] holdout_days`, 14), and report it.
  **The owner wants learning to start in days, not weeks.** Until a pack has 200 labeled judgments in its window,
  the nightly loop uses an interleaved split instead: every fifth labeled judgment, by a stable hash of its id (so a
  judgment never changes side), is holdout, and the rest are train, across all time. Once the window reaches 200,
  the loop switches to 25c's time split. Record which split each run used, and never let one judgment's label feed
  the writer and grade the candidate in the same run.
- **The security notices** (theseus-0j2.13), if on main, follow security.v3's live successor: a learned version
  promoted from v3 takes over its notices and their brake.

**What to build (25f):**
1. **The proposal.** Nightly, after the tender's report run, for each wired pack version with at least `min_errors`
   (10) new errors:
   - **An error** is an owner's label (operator source) that `resolve` reads as the answer being wrong (`noise` and
     `useful` are not), on a judgment in the train split (never the holdout's), not used by an earlier proposal in
     its lineage.
   - **The writer model** reads them, newest first, up to `max_errors` (40): each state (from its blob), Jev's answers
     and bands, the label and note, and the pack file. Its prompt states the loader's rules and Jev's: criteria are
     read literally, boundary cases are spelled out, and no question asks for math, counting or dates.
   - **It returns text only:** the next version's `instructions` and criteria, spelling out the boundary cases the
     errors show. Code checks the candidate differs from its parent in that text alone (ids, kinds, options, builder,
     state, model, point, action, rollback rules and sample stay), and that `Pack::parse` takes it.
   - One open proposal per lineage. States hold strangers' text, so the writer's output is only ever a candidate.
2. **The writer.** `[judge.learn] writer_profile = "opus"` (claude-opus-5-5; if main's template lacks
   `[profiles.opus]`, add route.v1's lines), called as 25d's audit calls its profile, priced, inside
   `writer_limit_usd_per_day` (2.0): a run that would pass it is skipped and reported. Network calls never hold the
   tender's thread. Other keys, with template lines: `enabled` (true; off with the judge), `min_errors`, `max_errors`,
   `margin` (0.02).
3. **The check.**
   - One 25d replay asks Jev the candidate on the stored states of the labeled judgments in the train split and the
     holdout. The candidate keeps its parent's builder, so the blobs are its states. It pays as 25d's replay does.
   - Thresholds are then re-fit in code from the candidate's train answers, never by the writer: a pure function in
     learn.rs (for instance, the lowest `act` whose train precision is at least the parent's, within the loader's
     bounds, unchanged below 30 labeled train answers per question). Write the rule into the design.
   - It reports the train errors fixed, and both versions' holdout precision and recall per question and class.
4. **The decision.**
   - **At 25c's minimum** (200 per deciding question, 30 per acting class): precision and recall up by `margin` on its
     deciding questions (say how you combine two), and no class worse. It is promoted through 26a: a live pack's
     candidate goes live at once, with the ladder's rollback rules as the brake.
   - **Below the minimum:** no class worse on the holdout, and some train errors fixed. A live pack's candidate goes
     to 26a's canary; a shadow pack's replaces its parent in shadow.
   - **Security packs** go to the owner's approval card (26a's) instead, with the diff and the numbers.
   - Anything else is held, with why. Promote through 26a's own act, citing the proposal; where its bar refuses what
     this allows (the canary below the minimum), extend it for learned candidates and say so.
5. **What the owner sees.**
   - A row per proposal (`judge.proposal`, or your name): parent, version, digest, the errors used, the replay's
     numbers, the decision and why.
   - One notice where operator notices go ("classify.v101 from 12 of your labels: holdout precision 0.80 → 0.86,
     recall 0.70 → 0.75; replacing classify.v1 in shadow").
   - The cockpit's Judgment section: a versions view per pack (each version's mode and source, the diff between any
     two, promote and reject; a reject is a mode row to `off`, the owner's act, judged by `judge_act`).
   - `theseus judge learn <pack> [--split <time>]` runs the loop now (an owner's act). `--split` sets the boundary
     (train before it, holdout from it until now), recorded in the row.
6. **Rollback.** Every version is a file and a row, and 26a's rollback returns a lineage to any earlier version.

**Proof, offline**, against the fake Jev and a scripted fake writer:
- 9 new train errors propose nothing and 10 propose once; a second night with no new errors proposes nothing; a
  planted holdout label's note never reaches the writer's request.
- A candidate that changes a question's id or the builder is refused; re-fit thresholds equal the rule's output.
- Better on train but worse on one holdout class: held. Better everywhere at the minimum: live for a live pack, the
  card for a security pack, shadow for a shadow pack. The writer's day budget stops a run.
- A restart rebuilds `<state dir>/packs/` from the rows; a rollback restores the parent at its point; nothing new
  runs at start (the lifecycle bench).

Planted reverts: let holdout errors into the writer's input (the leak test fails); skip the no-class-worse check (the
regression test fails).

**The live check is the maintainer's**, with `[secrets]` entries `jev_api_key`, `zai_api_key` and
`anthropic_api_key`. Write exact commands for a scratch daemon with a fresh state directory, a GLM profile for turns,
`[profiles.opus]`, `[judge] enabled = true`, `[judge.learn] min_errors = 2` (a scratch run has few real errors), and
the cockpit on a free port:
1. Send eight short messages (asks, fragments, a "stop" typed as text) and label each `classify.v1` judgment's `kind`
   (`theseus judge label <id> control -q kind`); if Jev got fewer than two wrong, give two another plausible option,
   so the check has errors. Note the time. Send four more, and label them.
2. `theseus judge learn classify.v1 --split <that time>` shows the proposal, the replay's numbers and the decision.
   The cockpit shows the diff, and `<state dir>/packs/` holds the file; if it replaced v1, 26a's rollback puts v1 back.
3. Ask for two `proc.run echo hi` calls, label both `security.v3` judgments `wrong`, and run `theseus judge learn
   security.v3 --split <now>`: the proposal is held with its reason, or waits on the approval card (`theseus confirm
   <id> --decline`).

**Leave alone:** 25d's replay and 26a's ladder beyond the hooks you add; the compiled-in pack files; route.v1 and
rerank-live (each live, with its own rollback rule); the memory pass's packs; 26b and 26c if in flight;
compaction-roots (30c), independence (28a) and extensions loading (43b).
