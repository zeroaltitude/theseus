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
dm-cards-fix: a due question's card always reaching the DM, "Ask again" on an expired card (needs_you.rs, pacer.rs, theseus-discord's cards); agent-answers: who answered (an agent's CLI vs the owner), money and AWS answers the owner's own (judge_act, approval.rs, the digest).

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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-self-repair-fixes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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


## Your task: self-repair-fixes, a self task starts in its own tree and knows its path; it may read the trees root; an ended item leaves no tree; the owner can retry a failed item (theseus-qh2tt, theseus-czlzg)

Branch: `cloud/20261019-self-repair-fixes`. Every commit's subject carries the issue id of its step (`theseus-qh2tt`
for steps 1-2, `theseus-czlzg` for steps 3-4). Deadline for the report: 5 hours after you start.

**Why.** On the owner's host tonight, a self-improvement task (a backlog item run in a `"self"` session) spent its
life asking the owner for permission to search, and each question expired unanswered:

- The item's first moves were `fs.glob "tsk-<item>*"` and then `fs.grep /people\.v1/`, both in `<state>/self/trees`,
  the PARENT of every self tree. The session did not know where its own tree was. The tree is opened lazily: only
  the first `self.propose_change` call opens it (`rsi/pr.rs`, "Tree open for ... at ..."), so the first turn starts
  with no tree and no path, and the model looks for one.
- The own-tree rule (`rsi/own_tree.rs`, theseus-kq4z4) lets a self session's file tools work freely INSIDE its own
  tree only. The trees root sits under the state dir, which the approve list holds, so a search of the root asks.
  A `git.log` in its own tree asked as well (`git.log {"path": "<state>/self/trees/<its slug>"}` -> approve): the
  rule covers the file tools, not the read-only git tools.
- When a question expires the turn ends with no green branch, the item fails, and (when the backlog's sweep marks it
  failed before the pipeline sees its end, `rsi/backlog/start.rs` `backlog_sweep`) its tree stays. `capacity.rs`
  `trees_open` counts that tree, the pipeline has one slot (`[self] pipeline_builds`), and nothing else starts until
  someone runs `git worktree remove` by hand.
- A failed item cannot be run again. `pipeline_ended` sets `TaskState::Failed` (`rsi/pipeline.rs`); failed counts as
  closed; `self move` refuses it (`rsi/backlog/word.rs`); `self drop` never refiles its cause; `self halt`/`resume`
  return only building items (`pipeline_halt` sets Proposed for those); `task.retry` says "not a task". Two items that
  failed because of harness bugs fixed since (theseus-kq4z4) sit failed with no way back.

## What exists (read these; lines move)
- `crates/theseus-core/src/rsi/`: `own_tree.rs` (the rule, `of`, `root_of`), `pipeline.rs` (`Local::start`,
  `pipeline_ended`, `pipeline_halt`, `clear`), `pipeline/queues.rs`, `capacity.rs` (`trees_open`, `check`), `pr.rs`
  (`self.propose_change`, `trees_root`, the lazy `tree::open`), `tree.rs` (`slug`, `find`, `open`, `close`, branches
  `self/<date>-<slug>`), `backlog.rs` (`objective_of`: the brief), `backlog/start.rs` (`backlog_sweep`,
  `backlog_start`: opens the `"self"` session with `SessionOpenParams`, which has a `dir`), `backlog/tender.rs`
  (`backlog_run`: the first turn's input is the objective), `backlog/word.rs` (the owner's `drop`/`move`).
- `Core::judge_act` in `crates/theseus-core/src/rpc/confirms.rs`: `Act::SelfBacklog` is the owner's word for drop and
  move (refused from a job's shell). The CLI: `crates/theseus/src/self_cmd.rs` (`SelfCmd`).
- Tests: `rsi/tests_pr.rs`, `rsi/tests_tree.rs`, `rsi/pipeline/tests.rs` (opens a real tree with `tree::open`), and the
  backlog's tests beside it.

## What to build
1. **The tree first, and its path in the brief (theseus-qh2tt).** When a self fix item starts (`Local::start` /
   `backlog_start`), open its tree before the first turn (as `self.propose_change` does now, the same `tree::open`,
   off the runtime's worker), make the session's working directory that tree (`SessionOpenParams.dir`, or however the
   session's cwd is set: read how the file tools resolve a relative path), and add one sentence to the objective
   naming the tree's absolute path and branch ("Your tree is <path> on branch <branch>; read and edit there, by
   absolute path."). `self.propose_change` then finds the open tree as it already does. A mint (pattern) start is out
   of scope unless it shares the code path; say which in the report. If opening the tree fails, the item does not
   start (held, with the reason, as the pipeline's other holds say why), never a session with no tree.
2. **Read-only in the trees root (theseus-qh2tt).** A `"self"` session's read-only tools (fs.glob, fs.grep, fs.list,
   fs.read, and the read-only git tools: git.log, git.diff, git.status, whatever the catalog marks read-only) may
   read the trees root and every tree under it without asking; its writes stay inside its own tree only, as today.
   Extend own_tree.rs's rule in place, with the same safety: a path that resolves out of the trees root by a link or
   `..` is judged where it lands; the rest of the state dir, the store, the secrets and the sealed sets keep their
   holds; `proc.run` is untouched. A non-self session gets nothing new. Fix own_tree.rs's module doc.
3. **An ended item leaves no tree (theseus-czlzg).** Whatever ends a self item without a join (the pipeline's
   `pipeline_ended`, the backlog's sweep marking it failed or done-without-a-branch, a cancel, a halt that drops it),
   its tree goes: `tree::close` (git worktree remove + the build tree), and its branch is deleted when it has no
   commits beyond its base (a branch with commits stays: it is evidence, and `self branches` lists it). Prefer one
   place that does this on the pipeline's tick for every tree whose item is in a closed state, so a path nobody
   thought of is covered too, with one `cleared` row as `clear` writes now. A tree whose item is still building,
   reviewing, joining or held at the keel (`keel_mark`) is never touched.
4. **`theseus self retry <item>` (theseus-czlzg).** The owner's: re-queue a failed item (Failed -> Proposed, its rank
   back as the halt path does, `session` cleared, an evidence row "retried by the owner"), so the pipeline starts it
   again on its next free slot. It refuses, with words, an item that is not failed (proposed, building, done,
   dropped). It is judged like `self move` (`Act::SelfBacklog`, or an `Act` of its own beside it if that reads
   better): refused from a job's shell, the owner's anywhere else. Add it to the RPC, the CLI (`SelfCmd`), its help
   line, and wherever `self move`/`self drop` are listed (the CLI's help, the Discord slash command if `self move` has
   one, theseus-core's and theseus's AGENTS.md). One name: `retry`.

## Tests (each with a planted revert; the first is the key test)
- KEY: a self item started through the pipeline (the pipeline tests' rig, a real tree) has its tree open before its
  first turn, the session's working directory is the tree, and the first turn's input names the tree's absolute
  path. Plant: drop the eager open (leave it to propose_change) -> the test fails on "no tree open at start".
- A `"self"` session's `fs.grep` and `fs.list` on the trees root, and its `git.log` in its own tree, run without
  asking; its `fs.write` in ANOTHER tree under the root still asks; a non-self session's `fs.grep` on the root still
  asks; a link inside a tree pointing at the store is judged at the store. Plant: remove the read-only branch of the
  rule -> the root search asks.
- An item failed by the backlog's sweep (not by `pipeline_ended`) has no tree after the next tick, `trees_open` is 0,
  and a Ready item starts in the freed slot; a branch with a commit beyond its base stays, one with none is gone.
  Plant: skip the closed-state sweep -> the tree stays and the next item is held "every slot is taken".
- `self retry` on a failed item makes it Proposed and the pipeline starts it; on a proposed or done item it refuses
  with words; from a job's shell it is refused. Plant: let it accept a done item -> the test fails.

## The checks for this row (these replace the preamble's gate line)
- `cargo fmt --all --check`; `bash scripts/shape.sh`; `cargo clippy -p theseus-core -p theseus --all-targets -- -D
  warnings`.
- `TZ=America/Phoenix cargo nextest run -p theseus-core -E 'test(/rsi|self|own_tree|backlog|pipeline/)'`, then
  `TZ=America/Phoenix cargo nextest run -p theseus-core -p theseus`.
- `python3 scripts/keel-guard.py --base <your base> --head HEAD --quiet` prints nothing.
- Then the full gate line from the preamble once at the end, if time allows; say if it did not.
- The live check for the maintainer: with self-improvement on, a Ready item starts, `theseus history <its session>`
  shows its first turn naming its tree, no question in its transcript for a read in the trees root, and after
  `theseus self retry <a failed item>` the item shows Ready in `theseus self pipeline`.

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

