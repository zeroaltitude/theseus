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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-pa1xx-quick-recall`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: pa1xx-quick-recall, prove (and finish) "a quick detour carries the turn's recall" (theseus-pa1xx)

Branch: `cloud/20261019-pa1xx-quick-recall`. Every commit's subject carries `theseus-pa1xx`. Deadline for the report: 3
hours after you start.

**Your branch already holds the change.** Its parent of the `cloud task` commit is `eac26b6d` ("theseus-pa1xx: a quick
detour carries the turn's recall"), a signed commit on top of `main` at `bbc65530`. It was written on the owner's
machine and **never compiled**: the machine's disk was too full to build. Your job is to build it, test it, fix what is
wrong in it, prove it with planted reverts, and report. Start from your branch as cloned; never rebase or merge.

**Why.** On the owner's machine a fresh session asked, from memory only: "in the owner's recent DM, what tag name did
he choose for the 1.0.0 release, and what did he say about routing private data?" The route pack judged it `quick`, so
the turn took a detour to the small model. Recall ran and admitted the right note ("the tag will be `release-1.0.0`"),
and the detour threw it away by design (theseus-n7nc: every detour dropped recall, because a greeting once got notes
attached). The recall row said `detoured`, and the model answered "I can't find it". A quick question is often about
what was said or decided before, which only recall holds. The same root cause made the voice path's quick route read
the owner's own words (RSI, keel, lane) as their common meanings.

**The design (already in `eac26b6d`).**
- `crates/theseus-core/src/config/routing.rs`: `RECALLING_DETOURS = ["quick"]` and `detour_recalls(mode)`. Never test
  a mode by name elsewhere.
- `crates/theseus-core/src/turn/route_step.rs`: `RouteState.detour_recall`, set by `TurnRunner::detouring` where the
  turn detours (a steered turn's detour carries none); `compile_detour` reads through `recall_view` when it is set,
  so the pending `Recall` node renders after the message, and on a later loop keeps the turn's own `Recall` node (by
  `t.recall.recall_id`), never an earlier note or a summary; `recall_compiled` reports the pack's drops.
- `crates/theseus-core/src/turn/recall_step.rs`: `recall_routed` drops a detour's node, rides and footer count only
  when the detour does not carry recall; a trivial detour's row still says `detoured`.
- `crates/theseus-core/src/compiler/situation.rs`: 35a's table: a detour admits a recall note (new or old); the
  table test `each_situation_admits_its_classes` changed with it.
- Tests in `crates/theseus-core/src/tests_route.rs`: `a_quick_detour_sends_its_recall_and_writes_it`,
  `a_quick_detours_later_loop_keeps_its_recall`; in `config/routing.rs`:
  `a_quick_detour_recalls_and_a_trivial_one_does_not`. The theseus-n7nc tests (`a_trivial_detour_writes_no_recall_and_its_row_says_detoured`,
  `after_a_detour_the_next_request_carries_no_stale_note`, `a_routed_turn_still_writes_and_sends_its_recall`,
  `a_detours_window_leaves_its_recall_note_out`) must still pass unchanged.

**Risk: low to medium.** A quick turn's request grows by the recall pack (at most `[memory] budget_tokens`, 1,500 by
default) when recall admits something; a trivial turn is unchanged; a kept or switched turn is unchanged.

**Prerequisite:** none beyond your branch.

## What to do
1. Setup as the preamble says, then `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo nextest run --workspace --no-run`. Fix every compile error and clippy finding in the change (a function over
   clippy's 100-line limit gets a helper, never an `allow`). Commit fixes as `routing: <what> (theseus-pa1xx)`.
2. Run `cargo nextest run --workspace -E 'package(theseus-core) & (test(/route/) | test(/situation/) | test(/recall/) | test(/detour/))'`
   (TZ=America/Phoenix). Every test passes. If a new test is wrong about the fakes (for example the stand-in index or
   the tool the second test calls), fix the test so it proves the same behaviour, and say what you changed and why.
3. **Planted reverts**, each run with the same filter, restored with a fresh mtime (`touch`), then `git status` clean:
   - P1 `recall_step.rs`: `if t.route.keeps.is_some() && !t.route.detour_recall {` → `if t.route.keeps.is_some() {`
     (a quick detour's footer and row go back to `detoured`).
   - P2 `route_step.rs` `compile_detour`: `true => self.recall_view(t, nodes),` → `true => (nodes, crate::recall::render::Sources::default()),`
     (the note never reaches the request).
   - P3 `route_step.rs` `compile_detour`'s filter: `crate::stub::Kind::Recall => own == Some(n.id.as_str()),` →
     `crate::stub::Kind::Recall => false,` (a later loop loses the note).
   - P4 `situation.rs`: `Situation::Detour => common || matches!(class, Arrangement | RecallNote),` →
     `Situation::Detour => common || class == Arrangement,` (35a refuses the request).
   - P5 `route_step.rs` `detouring`: `detour_recalls(&v.mode)` → `false && detour_recalls(&v.mode)`.
   Each must make a named test fail. A MISSED plant is a finding: add the test that catches it.
4. The full gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`), then
   `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` (paste its output; it must be empty).
5. Measure, relative only: run `target/debug/theseus-sim bench lifecycle --runs 10 --check` (the turn path changed).

## Out of scope
The route pack's prompt, the `why` command for detoured turns (a separate issue), the recall pipeline and its
`in_context` rule, the import sync, docs/ (the maintainer writes them; the commit already updated
`crates/theseus-core/AGENTS.md` and `docs/design/m6-memory.md`: correct them only if your fixes change what they say).

## Where main is
The preamble above describes an older `main`. Where they disagree, this task is right: `main` is at `bbc65530` (v1,
store format 45). This row stores nothing new: no format bump. Line ceilings: `crates/theseus-core/src/turn.rs` 3,384 /
3,525; never raise one. Other work in flight tonight (stay out of its areas): a data-routing lane (`routing/data.rs`,
`data_gate.rs`, the route's data step, `catalog.rs`'s origin), Google Workspace rows (`theseus-google`, `src/google/`),
speed rows (the send path, first request size, idle size), a Discord test hang, a voice fix, the README.

## Rules for this row
- Keep it minimal: fix what the change needs, nothing beside it. Every behaviour change has a test and a plant.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling.
- No new dependency. No person's name anywhere: write "the owner". Fixtures are invented.
- Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run here (about 33 L1 tests fail on this
  VM), with exact commands for the rest.
