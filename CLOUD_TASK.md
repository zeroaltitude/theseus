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

**Other changes in flight.** Several other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Merged into `main` tonight (your clone holds them): turn-end facts, the shared tool catalog (speed-send-path), the
store's one blocking wait (speed-idle-size), the fleet's full-screen view, a self task's acts in its own tree, the
task's home thread, the owner's needs-you cards, voice at the join, and the ontology in core (store format 46).

Other cloud sessions like you, each on its own branch: the install build's size (release-size); the DM home split into
threads (home-split); the job truths; the ontology on the wire (onto-wire); per-thread benches (threadbench-threads);
timing flakes; voice's echo cancelling (voice-aec) and voice asking Jev (voice-jev); and two beside you in this batch:
self-repair-fixes: a self task in its own tree, the trees root readable, tree cleanup and `self retry` (crates/theseus-core/src/rsi/); dm-cards-fix: a due question's card always reaching the DM, "Ask again" on an expired card (needs_you.rs, pacer.rs, theseus-discord's cards).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 46 as you clone it), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-agent-answers`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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


## Your task: agent-answers, an answer from an agent's process is labelled as the agent's, and anything that spends money or changes AWS is the owner's own (theseus-5rvss)

Branch: `cloud/20261019-agent-answers`. Every commit's subject carries `theseus-5rvss`. Deadline for the report: 5
hours after you start.

**Why.** Today's digest said "✓ you answered: aws.stack.plan" and "✓ you answered: aws.stack.apply". The owner
answered neither: an agent the owner runs on the same machine, as the same Unix user, answered them with `theseus
answer <id> approve` from its own process. The daemon cannot tell that agent's CLI from the owner's own terminal
today, so it ledgered, showed and digested the answer as the owner's. The owner decided two things on 2026-10-19 at
19:11 ("5. your recommendation is good"):
1. **Say who answered.** A CLI answer from a process under an agent is recorded, shown and digested as that agent's
   answer, not the owner's ("✓ answered by an agent (<what it was>): aws.stack.apply"), in the digest, `theseus
   history`, the board and the ledger's `by`.
2. **Money and AWS are the owner's own.** A question whose answer spends money or changes AWS (every `aws.stack.*`
   call that asks, a budget/spend-limit question and the spend reset, and any run the policy marks paid) is answered
   only by the owner personally: a press of the owner's own account on a bound transport (the Discord DM's card), or
   the owner's own interactive shell. An agent's CLI answer to one is refused with words (an `approval.refused` row,
   as a job's is), and the question stays open for the owner.

This is a policy change (a keel item). This session cannot sign: see "Keel" below.

## What exists (read these; lines move)
- refs (in this prompt): **a job cannot answer approvals** (theseus-6qy, since 2026-09-29). `Core::judge_act`
  (`crates/theseus-core/src/rpc/confirms.rs`) traces the process behind an answer (approve, decline, the spend
  reset) or an undo: the peer's pid (`SO_PEERCRED` on the socket, `--stdio`'s parent, the web UI's loopback owner
  found via `/proc/net/tcp` and `/proc/*/fd`), then its ppid chain to pid 1. A live job wrapper anywhere above it
  (known by its command line, `theseusd job-wrapper … --correlation-id <id>`), or a pid that cannot be traced, is
  refused as `approval.refused` with `from_job` and `asker`. A press (`policy tighten`) is not traced. Code:
  `crates/theseus-core/src/peer.rs` (the job-wrapper match is documented there) and the callers of `judge_act`.
  Tests that need a process outside every job skip that part when the test process itself runs inside one, and say
  so on stderr. `crates/theseusd/tests/job_approval.rs` is a rig for real answers from real processes (a stand-in
  model in `tests/common/model.rs`, the CLI copied into the workspace root).
- `crates/theseus-core/src/approval.rs`: `Answerer` (`label`, `surface`, `discord`), shown and ledgered as `by`.
- `crate::places::owner_anywhere` (the owner's account on a bound transport, or a private place) and the `Act`s in
  `judge_act` that are already the owner's alone (`SelfResume`, `SelfBacklog`, `SelfNarrow`, `SelfRevert`): the
  pattern to follow.
- The digest's words: `crates/theseus-protocol/src/work_digest.rs` (`DigestKind::Answered`, "✓ you answered: {name}")
  and its renderers and tests (grep `you answered`: theseus-discord, theseus-tui, theseus, theseusd).
- The AWS tools: `crates/theseus-core/src/aws/` (`stack.rs`, `tools.rs`); the budget question
  (`theseus_kernel::BUDGET_TOOL`) and the spend reset in confirms.rs.

## What to build
1. **Who the CLI answerer is.** Extend the trace `judge_act` already does (one walk of the ppid chain, no second
   one) to classify a CLI answer as the owner's own or an agent's:
   - the owner's own: the answering process has a controlling terminal (from `/proc/<pid>/stat`'s `tty_nr`, or the
     equivalent the code already reads) and no ancestor whose command line matches the agent list;
   - an agent's: no controlling terminal (a service, a cron, a gateway's exec), or an ancestor that matches the
     agent list. Name it by the matching ancestor's program name (its `comm`, or the first word of its command line),
     else "no terminal".
   The agent list is a config key with a default, e.g. `[policy] agent_processes = [...]` (or under the config section
   where the answer rules live: read config/ first), holding program-name patterns of the common agent hosts (an
   agent gateway's process, a coding agent's CLI); keep the default generic, and say in the report what the owner's
   config should add. Add it to `theseusd example-config`'s template with its template test. A web UI answer is the
   owner's when its loopback owner is the owner's uid and the browser's process is traced as today; a Discord press
   is the owner's when its account is the owner's (unchanged).
2. **Label every answer by who gave it.** The ledger's row and the `Answerer` carry the class (`owner` / `agent:<name>`
   / `job:<session>` as now). `theseus history`, the board, the push's settle and the digest say "you answered" only
   for the owner's own, and "an agent (<name>) answered" otherwise. One field, stored data still readable: an old row
   with no class reads as today (the owner's); follow the store's version rule if a stored record gains a field.
3. **Money and AWS are the owner's own.** A question is money-or-AWS when its tool is `aws.stack.*` (and any other
   AWS tool that writes: check the catalog's read-only marks; `aws.describe` and other reads stay as they are), the
   budget question, the spend reset, or a run marked paid. An answer to one (approve or decline: decide, and say why;
   at least approve) from an agent's CLI is refused with words ("this spends money or changes AWS, so only the owner
   answers it: press it in the DM, or answer from your own terminal"), written as `approval.refused` with the
   agent's name, and the question stays open. The owner's own terminal and the owner's Discord press answer it as
   today. A job's answer stays refused as today.
4. **Docs.** theseus-core's AGENTS.md (the answer rules), theseus's CLI help for `answer` (one line: money and AWS
   questions are the owner's own), and the digest's docs if they describe the label.

## Keel
This tightens who may answer. It should need no test removed or weakened. If an existing test answers a
money-or-AWS question from the CLI as the owner (a test process with no terminal now reads as an agent), change that
test to answer as the owner explicitly (a terminal stand-in, or the owner's Discord press), keeping its assertion
count; put any change that drops or loosens an assertion in a commit of its own, run `python3 scripts/keel-guard.py
--base <your base> --head HEAD`, and in the report under "Keel" list each line it prints with the exact trailer for
it, in the form it prints (`Keel: <rule> <path>[, <path>...] — <why>`), the why quoting the owner: `the owner,
2026-10-19 19:11: "5. your recommendation is good" (agent answers labelled as the agent's; money and AWS answers the
owner's own)`. The maintainer signs that trailer at the join; this session signs nothing. Whatever the guard says,
put the policy commit's suggested `Keel: policy` trailer line in the report too, with the same quote, so the
maintainer can sign the policy change itself.

## Tests (each with a planted revert; the first is the key test)
- KEY: an `aws.stack.apply` question (a stand-in, as the AWS tests make one) answered `approve` from a CLI process
  with no terminal is refused with the words above, an `approval.refused` row names the agent class, and the question
  is still pending; the same answer as the owner (a terminal stand-in, or the owner's Discord press) runs it. Plant:
  skip the money-or-AWS check in `judge_act` -> the agent's approve runs the call and the test fails.
- The classification: a process with a terminal and no listed ancestor is the owner's; one with none is an agent's
  ("no terminal"); one under a listed ancestor is that agent's, by name. Unit tests over fixtures, as peer.rs's tests
  read `/proc` fixtures; plant: ignore the agent list -> the listed case reads as the owner's.
- The digest says "an agent (<name>) answered: fs.list" for an agent's answer to an ordinary approval, which runs, and
  "you answered" for the owner's; an old ledger row with no class reads "you answered". Plant: drop the class from
  the digest's words -> it fails.
- A budget question and the spend reset follow the money rule (refused from an agent, the owner's own works).
- theseusd: the `job_approval.rs` rig (or one beside it) shows a real CLI answer from a process with no terminal
  refused for a money question and accepted for an ordinary one, labelled as an agent's.

## The checks for this row (these replace the preamble's gate line)
- `cargo fmt --all --check`; `bash scripts/shape.sh`; `cargo clippy -p theseus-core -p theseus-protocol -p theseusd
  -p theseus --all-targets -- -D warnings`.
- `TZ=America/Phoenix cargo nextest run -p theseus-core -E 'test(/confirm|answer|approval|peer|digest|aws|judge/)'`,
  `cargo nextest run -p theseus-protocol -p theseus -p theseus-discord -E 'test(/digest|answer/)'`, `cargo nextest
  run -p theseusd -E 'test(/approval|digest/)'`, then `TZ=America/Phoenix cargo nextest run -p theseus-core -p
  theseusd`.
- `python3 scripts/keel-guard.py --base <your base> --head HEAD --quiet`: prints nothing, or only the lines your
  "Keel" section lists.
- Then the full gate line from the preamble once at the end, if time allows; say if it did not.
- The live check for the maintainer: from an agent's exec (no terminal), `theseus answer <an aws.stack question>
  approve` is refused and the question stays in the DM; from the owner's own terminal it runs; `theseus digest` then
  shows "an agent (...) answered" for an agent's ordinary answer.

## Rules for every row of this batch

- **Model roles:** you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller, and make an old config key fail the load with
  "X was renamed to Y". Stored data stays readable.
- **On, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the rollback: a
  test shows that with it off the old behaviour is byte for byte back.
- **Planted reverts, with notes for the reviewer.** Every behaviour change gets a test and a planted revert: break it,
  see the named test fail, restore the file and `touch` it, then `git status`. The row's **key test** (named first under
  Tests) must be shown failing on its plant. In the report, give each plant as a note the reviewer can replay: the file,
  the exact edit (the line before and after), and the test that failed with its failure line. A pure move needs no plant:
  say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. **Never edit a
  keel-guarded file** (`scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`,
  `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`,
  `crates/theseus-core/src/rsi/sealed*`, `crates/theseus-exam/exam/`, and the first request's budgets in
  `crates/theseus-core/src/first_request.rs`): any change there needs a signed commit with a `Keel:` trailer quoting
  the owner's yes, which this session can't make. If your design needs one, stop at that point and put the exact change
  in the report.
- **Signing:** you can't sign here. Commit without `-S`; sign nothing. The maintainer re-signs at the join.
- **First-request budgets stay** (cli/task 8,000, dm 15,000 tokens; `tests_first_request.rs` must pass on the
  defaults). A plain turn stays 4 frames. Nothing new on the path before a request leaves.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner"
  and "collaborator". Fixture names, guild and channel ids, and paths are invented. Run anything that leaves through
  `core.tools.scrubber` as the code already does.
- **Docs in code.** Where your change makes a crate's or the cockpit's AGENTS.md wrong, fix those lines in the same
  commit, in plain words, in the file's existing style. Read the AGENTS.md/CLAUDE.md of every crate you touch first.
- **CLOUD_REPORT.md last.** It is the branch's last commit, as the preamble says, and your final message starts
  `CLOUD REPORT COMPLETE`. Keep it honest: what you ran, what passed, what you could not run here and the exact commands
  for the maintainer.

