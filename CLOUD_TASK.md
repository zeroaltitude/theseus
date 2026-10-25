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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3a-recall-floor`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about main)

Your branch starts from `main` at `ac298666` (release 1.0.2, signed tag `release-1.0.2`; the release the owner runs
now). Fetch it and branch from `ac298666`. Line numbers below are at `ac298666` and may be off by a few: search by
name. Where the release preamble above disagrees with this note about the base, ceilings or the work in flight, this
note wins.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`, :688).
This row should need no store change (it sets two config defaults and changes how a recalled value is worded). If a
part truly needs a bump, name it and justify it in your report's first section (the integrator renumbers it at the
join); do not bump for anything else.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines). At
the base: `crates/theseus-core/src/config/memory.rs` 633, `crates/theseus-core/src/recall/relevance.rs` 65,
`crates/theseus-core/src/recall/render.rs` 463, `bench/recall/precision.py` (read its size).

**Your lane: `integ/p3-first`** (new: the first 1.0.3 rows). Its other rows, all fired at about the same time on the
same base:
- `p3a-recall-now` (`cloud/20261024-p3a-recall-now`, theseus-mo1vm): "now / currently / still" questions rank the
  newest statement of a value first and mark older ones superseded (`recall/temporal.rs`, the exam's gold probe).
- `p3a-kimi` (theseus-rgbtz): catalog and provider rows for a new model family (`catalog.rs`, config providers).
- `p3a-mem-spike` (theseus-xxac7): a memory-use spike in `crates/theseus-index`.

**Files you share with `p3a-recall-now`, so keep both rows additive:** `crates/theseus-core/src/recall/render.rs`
(you add the subject beside a same-kind value; it adds a "superseded" mark for older statements: put your wording in
its own small function, do not restructure the header), and the recall paragraphs of `crates/theseus-core/AGENTS.md`
(add your own paragraph; do not rewrite theirs). You own `config/memory.rs`'s two defaults and `recall/relevance.rs`;
`p3a-recall-now` does not touch them. `p3a-kimi` may touch the example config's `[providers]` and profiles: you touch
only its `[memory]` comments, if any. Do not edit `docs/status.md` (the integrator writes it at the join).

## Your task: p3a-recall-floor, the recall relevance floor's defaults + the subject beside a same-kind value (theseus-96cyh)

Branch: `cloud/20261024-p3a-recall-floor`.

Every commit subject carries `theseus-96cyh`. Deadline for the report: about 3 hours after you start.

**Why.** A model-free rescore of the last recall bench run (seed 7, 60 never-said "abstention" questions) found that
Theseus misleads on 28 of 60: asked about a fact it was never told, it answers with a different fact's value of the
same kind (another service's port, another job's retention days) as if it were the asked one. A rival harness misled
on 15/60. Half of Theseus's misleading answers came after a compaction. A relevance floor was built for exactly this
(theseus-55hlu, joined 10-14): `[memory] recall_floor` and `recall_subject_bar`
(`crates/theseus-core/src/config/memory.rs` :160-171, :210, :300-301; applied in `crates/theseus-core/src/recall/relevance.rs`
:30-48). **It ships switched off**: both default to 0.0, and no bench or config turns them on, so it has never been
measured. Its review said: "the abstention fix itself is off until a default is measured (`bench/recall/precision.py`
is the instrument)". That sweep was never run. This row runs it, sets the defaults, and fixes the wording.

**What to build.**
1. **Run the sweep (M1), model-free.** `bench/recall/precision.py` (read its module doc and `bench/recall/README.md`
   first) starts a scratch daemon on a copy of a finished bench run's store, once per `recall_floor` /
   `recall_subject_bar` setting, and asks `memory.search` each probe's question. Use `--make SEED:SIZE` to produce the
   run on the stand-in model (`standin.py`: no key, no spend), seed 7 at the bench's normal size (read the README for
   the size the published runs used; if it is too slow for your session, use the largest size that finishes and say
   which). Build the binaries it needs (`--bin-dir`) from your branch. Sweep a grid (at least floor in
   {0, 0.1, 0.2, 0.3, 0.4} x subject bar in {0, 0.3, 0.5, 0.7}; widen it if the best point sits on an edge).
   Choose on the held-in half (odd probes), report the held-out half (even probes) once, as the module says.
2. **Choose the defaults.** The target: abstention probes that admit the same-kind distractor's value go DOWN as far
   as they can, while answerable probes that still admit the gold value lose at most 2 points (of the answerable
   count, as a share) against the 0/0 setting, and the pack's mean tokens do not grow. Pick the setting that meets
   that; if none does, pick the best trade, say why, and say what it costs. Set `default_recall_floor()` and the
   `recall_subject_bar` default in `config/memory.rs` to it, with a doc comment naming the sweep (date, seed, size,
   held-out numbers). If the sweep shows the floor does not help at all, leave the defaults at 0.0, say so plainly,
   and still do item 3.
3. **Name the subject beside a recalled same-kind value.** When the recalled pack gives a value whose subject is NOT
   the asked subject but is of the same kind (the "neighbour" shape: the store knows another service's port, the
   question asks this service's port), the rendered line must name that value's own subject next to the value (for
   example `port 9291 (of the pushgateway, not the asked service)`, wording your call), so the model cannot read it
   as the asked one. Find where the pack lines are rendered (`recall/render.rs`; the subject/kind the floor already
   computes in `relevance.rs`) and do it there, in one small function. This targets the post-compaction cases: say
   in the report whether a compacted session's recall goes through the same render (it should; if not, name the
   path and cover it too).
4. **Before/after numbers in the report.** A default change: the report names the sweep table (every setting: held-in
   and held-out admitted-distractor counts, gold-kept counts, mean tokens), the chosen point, and the before (0/0) and
   after numbers on the held-out half. Also save the sweep's raw output under `bench/recall/results/` only if that
   directory already holds such outputs (check); otherwise paste the table into the report and commit nothing extra.

**Acceptance (the issue's):** misleading answers at or below 15/60 and recall accuracy down at most 2 points. That
acceptance is measured by a full model run of the recall bench (M5, Theseus only, about $8-11), which is a LATER
LOCAL step, not this session: you have no key and must spend nothing. Your model-free sweep is the proxy: report
which setting would most likely meet it and why, and say in one line what M5 should run.

## Tests (each red at the base, with a planted revert)

1. `config/memory.rs`: a test pinning the new defaults (`MemoryCfg::default()` and a config with no `[memory]` keys
   both give the chosen values). The existing test that asserts the 0.0 default (near :577-579) asserts the OLD
   default: changing a default is the point of this row, so update that assertion to the new value in the same
   commit and say so in the report's Keel section (it is a deliberate default change named in the issue, not a
   weakened test; do not delete it, do not loosen it). Plant P1: put the default back to 0.0: red.
2. `recall/render.rs` (or a test file beside it): a pack with a same-kind neighbour value renders the neighbour's
   subject beside it; a pack whose value IS the asked subject's renders unchanged (byte-identical to the base). Plant
   P2: drop the subject from the neighbour line: red.
3. `bench/recall/test_precision.py`: if you change `precision.py` at all (only to make the sweep run or report), add a
   test for that change.
- Keep green: every existing recall, memory and config test; the exam's tests (`crates/theseus-exam`); the bench's
  Python tests (`python3 -m pytest bench/recall` or as the README says).

## Not in this row

No change to `recall/temporal.rs` or the "now" path (that is `p3a-recall-now`). No new config key. No model call, no
key, no spend. No change to Discord. Not the recall bench's rivals.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. A fix that
  would change any message of that flow STOPS that part: write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove or weaken no test; raise no budget, cap or ceiling; do not edit `scripts/keel-guard.py` or its
  tests; rename no test; edit no golden. The one deliberate change is the default's own assertion (Tests 1). Before
  the report run `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and paste its output; if it
  flags that assertion, paste what it prints and say it is the named default change (the integrator takes the owner's
  ack at the join).
- **Data floor:** nothing new is sent to a model or routed anywhere; the recalled text stays where it went before.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented subjects and values
  under `example.test`. Write "the owner".
- **Red first;** right over compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` "Building and checking" first):
  `cargo fmt --check`, `cargo clippy` on the touched crates with `-D warnings`, `cargo nextest run` (or `cargo test`)
  for `theseus-core`, `theseus-exam` and `theseusd`'s recall tests, the bench's Python tests; then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched, or a bump named and justified), any
ceiling, OWNER CALL or STOP, the chosen defaults in one line. (2) **The sweep:** how it ran (seed, size, binaries),
the full table, the chosen point, before/after on the held-out half. (3) **The subject wording:** where, the rule for
"same kind, other subject", an example line before and after, and the compaction path. (4) **Tests** and plants
P1-P2, red at the base and green after. (5) **Gate** commands and results. (6) **Keel** output. (7) **DM golden** diff
result. (8) **Join notes:** every file touched, and the hunks in `recall/render.rs` and `crates/theseus-core/AGENTS.md`
(line ranges) so the join with `p3a-recall-now` stays additive. (9) **What M5 should run** (the later local model run).
(10) **Follow-ups.** (11) One line on the data floor.
