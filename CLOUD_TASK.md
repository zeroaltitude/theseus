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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3g-bench-index`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base (it wins over anything above about main, the base, or what may be edited)

Your branch starts from `main` at `0fda4d57`. That is install #83's gated tree: release 1.0.2 plus the joined patch3
work. Fetch it and branch from `0fda4d57`. Where the release preamble above disagrees with this note about the base,
the ceilings, or **what may be edited**, **this note wins**.

**Two explicit overrides of the preamble, for this row only:**
1. The preamble says not to edit `docs/benchmarks/`. **This row may add exactly one new file there,
   `docs/benchmarks/index.json`, the manifest.** It may edit **no other file under `docs/benchmarks/`** — not a
   report's `.md`, `.json`, `.csv` or `.svg`, not `README.md`, and **not `omnibus.json`**.
2. The preamble says no task runs a benchmark. That stands and is absolute here: **this row runs no benchmark,
   spends nothing, calls no model, and uses no AWS.** Every number it produces comes from datasets already committed
   in the repository.

**Early push (required, before any code).** Do setup first (`cargo --version` alone, then the rest), and `npm ci` in
`cockpit/`. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-jda0z: session started, setup done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3g-bench-index`

After that, push after EVERY commit, not only at the end. On 2026-10-24 three rows of sixteen ended holding nothing but
their task commit, and because every other row pushed only at the very end there was no way to tell a session that died
at minute 5 from one that worked for two hours and lost it. A run that ends early must leave its work on the branch.

**Store format: untouched.** No stored record, no wire type. Do not bump `MANIFEST_FORMAT`; do not regenerate
`cockpit/src/protocol.gen/`.

**Line ceilings.** `scripts/long-files.txt` lists Rust files only; no TypeScript or Python file has one. Never raise a
ceiling anyway.

**Your lane: `integ/p3-ops2`.** The four other rows in it are cockpit Operations-page rows
(`ops/`, `views/Ops.tsx`, `lib/explainers.ts`, `lib/threadboard.ts`, `views/Fleet.tsx`,
`components/TaskGraph.tsx`, `lib/taskgraph.ts`, `lib/fullgraph.ts`). **You share no file with any of them.** Stay out
of every path in that list and the join is trivial.

**Your write scope:** `bench/report/manifest.py` (new), `bench/report/test_manifest.py` (new), a small hunk of
`bench/report/draft.py` (the report step's manifest append), `docs/benchmarks/index.json` (new),
`cockpit/src/lib/benchindex.ts` (new), `cockpit/src/lib/benchfiles.ts` (the one glob that embeds the manifest),
`cockpit/src/views/Benchmarks.tsx` (the summary section only), `cockpit/test/benchindex.test.ts` (new), and
`bench/README.md` (the manifest's paragraph). Touch no other file. If you believe you must, write `OWNER CALL:` in the
report and leave it undone.

---

## Your task: p3g-bench-index, the Bench page driven by the latest datasets (theseus-jda0z)

Branch: `cloud/20261025-p3g-bench-index`.

Every commit subject carries `theseus-jda0z`. Deadline for the report: about 3 hours after you start.

**Why — the owner's own words, 2026-10-24:**

> are the bench results in the ui up to date with final results? it would be interesting to drive these off of the
> latest bench datasets, and have every bench run simply put a dataset in, develop a human report as you have been,
> edit the omnibus report as you have been, and then use the newest dataset to drive the ui in the bench page in the
> web ui

Today the Benchmarks page is a list of published reports: `cockpit/src/lib/benchfiles.ts` embeds every
`docs/benchmarks/*.md|json|csv` at **build** time with `import.meta.glob`, and `cockpit/src/views/Benchmarks.tsx`
folds the per-trial tables into frontier points through `cockpit/src/lib/bench.ts`. There is no machine-readable
index of **what the newest measurement of each thing is**, so nothing on the page can say "this is the current number,
as of this date" — a reader has to know which report is the latest.

### Step 0 — verify one claim before you build on it

The issue says `omnibus.json` holds "the figures the UI draws" and is stale. **At `0fda4d57` I can find no reference
to `omnibus` anywhere under `cockpit/src`.** Check it yourself (`grep -ri omnibus cockpit/src`), and report what you
find. If the claim is wrong — if `omnibus.json` feeds only the `omnibus.md` document and not the Benchmarks page —
**say so plainly in the first section of your report**, because it changes what "stale figures in the UI" means and
the maintainer needs to know. Do not edit `omnibus.json` either way.

### Step 1 — the manifest, `docs/benchmarks/index.json`

One row per published dataset, built from the `docs/benchmarks/*.json` files **already committed**. Those files
already carry what a row needs: look at `docs/benchmarks/2026-10-23-terminal-bench-retest-max.json`, whose top level
has `report`, `suite`, `date`, `tier`, `commit`, `arms`, `summary`, `title`, `model`, `kind`.

A row holds at least: **benchmark** (the suite, e.g. `terminal-bench@2.0`), **tier**, **arm**, **harness**,
**model**, **date**, **commit**, the **metrics with their confidence intervals** as the dataset states them, and the
**dataset path**. Design the schema yourself from what the datasets actually contain; do not invent a field no
dataset can fill. Give the file a `version` and an `about` line, and sort it deterministically so a regeneration is a
no-op when nothing changed.

Write `bench/report/manifest.py`, **standard library only** (`bench/`'s Python rule), with two entry points:
- **build/refresh** — scan `docs/benchmarks/*.json`, emit the whole manifest;
- **append one run** — add or replace the row for one report, idempotently.

Handle the files that do not fit: `omnibus.json` is not a run (skip it by its `kind`), and the older gate-bench and
memory-exam datasets have different shapes. **Skip what you cannot parse rather than guessing**, and have the tool
print a line per skipped file with the reason. List every skipped file in the report — that list is itself a finding.

Then **commit the generated `docs/benchmarks/index.json`**.

### Step 2 — every run's report step appends its row

`bench/report/draft.py` is already the tool every run's report step uses: it writes `<report>.json`, `<report>.csv`,
`img/<report>/*.svg` and the `<report>.md` skeleton. Add the manifest append to it, so a run that writes its dataset
also writes its manifest row and nothing extra has to be remembered. Keep the hunk small, make it idempotent, and
make it fail loudly rather than silently writing a half row.

`bench/`'s Python tests are **not run by the gate** — run them yourself before each commit
(`bench/README.md` says how) and paste the counts. Add `bench/report/test_manifest.py` beside the existing
`test_draft.py` and `test_charts.py`, in their style.

### Step 3 — the Benchmarks page summary reads the manifest

- `cockpit/src/lib/benchfiles.ts`: embed `docs/benchmarks/index.json` the same way the rest is embedded, through
  `import.meta.glob`. One addition; do not disturb the existing globs or `figureUrl`.
- `cockpit/src/lib/benchindex.ts` (new, pure): parse and validate the manifest, and expose **the newest row per
  (benchmark, tier, arm)** plus each row's history, oldest to newest, for a sparkline. Validate rather than trust: a
  malformed manifest must give an empty result and a named reason, never a crashed page.
- `cockpit/src/views/Benchmarks.tsx`: a **summary section** above today's run list, one line per
  (benchmark, tier, arm) showing the newest value with its interval, **an "as of ‹date›" and its commit**, a link to
  that run's report, and a small history sparkline. **Leave the frontier chart and the run list exactly as they are**
  — they read the per-trial CSVs and are correct; this is an addition, not a replacement.
- **Do not add a `Panel`, `ChartPanel` or `Card` with a new `help=` id.** `cockpit/test/explainers.test.ts` fails a
  panel whose help id has no explainer entry, and `lib/explainers.ts` belongs to another row in this lane. Put the
  summary inside an existing panel, or use a plain section with no `help` prop. If you judge the summary truly needs
  its own panel, say so under **OWNER CALL** and leave it out.

### What this row does NOT do, and why — read this, it is the scope cut

The issue also asks that the omnibus figure generator move into the repository and run in every run's report step.
**The generator is not in the repository and is not reachable from this VM** — it lives on the owner's machine
(`make_figs.py`, 952 lines, and `build_md.py`), outside the clone. You cannot move a file you cannot read.
**Leave that half undone, do not attempt a reimplementation, and say so in your report** so the maintainer can do it
locally. The same goes for regenerating `omnibus.json` from the newer runs: that needs the external generator and is
a local, docs-only job.

The optional fourth part of the plan — the daemon serving `docs/benchmarks` so a new run appears without a rebuild —
is **out of scope** here. Say in the report what it would take, in a few lines, so it can be filed.

---

## Tests (each red at the base, with a planted revert)

Red-first means: the new assertion fails at `0fda4d57`, or under a planted revert of your change, and passes after.
**A test that would pass before the change is not a test.**

**Python, `bench/report/test_manifest.py`** (run them yourself; the gate does not):
1. **Build:** over a small fixture directory of invented dataset files, the manifest has the expected rows, sorted
   deterministically; a second build over the same input is byte-identical. Plant P1: drop the sort — red.
2. **Append is idempotent:** appending a run twice leaves one row, with the second run's values. Plant P2: make it
   append blindly — red.
3. **Skipping:** a dataset the tool cannot parse is skipped with a named reason and does not abort the build, and an
   `omnibus`-kind file never becomes a row. Plant P3: let an unparseable file through — red.
4. **`draft.py`'s hook:** a drafted run writes its manifest row. Plant P4: remove the hook — red.

**TypeScript, `cockpit/test/benchindex.test.ts`** (`npm test` in `cockpit/`):
5. **Newest per key:** given rows for one (benchmark, tier, arm) at three dates, the newest wins and the history is
   oldest-to-newest. Plant P5: take the first row instead of the newest — red.
6. **Validation:** a malformed manifest gives an empty result and a named reason, and does not throw. Plant P6: trust
   the input — red.
7. **The real manifest parses:** the committed `docs/benchmarks/index.json` loads, and its newest Terminal-Bench rows
   are the 2026-10-23 retest ones, not an older run. That is the owner's actual complaint, as an assertion.
- Keep green: the whole `npm test` suite (including `bench.test.ts`, `figures.test.ts` and `explainers.test.ts`),
  `npm run lint`, `npx tsc -b`, and `bench/report`'s existing Python tests.
- **Where an existing expectation must change**, change the assertion rather than delete it, put that change in a
  commit of its own whose message says why, and list it under **Keel findings expected**. Never lower an assertion
  count.

## Not in this row

- **No benchmark run, no model call, no key, no spend, no AWS.** Absolute.
- No edit to any `docs/benchmarks/` file except the new `index.json` — **not `omnibus.json`, not `README.md`, not a
  report**.
- No move or reimplementation of the external omnibus figure generator.
- No daemon change, no new RPC, no protocol change, no Rust change.
- Nothing under `cockpit/src/ops/`, `cockpit/src/views/Ops.tsx`, `cockpit/src/views/Fleet.tsx`,
  `cockpit/src/lib/explainers.ts`, `cockpit/src/lib/threadboard.ts`, `cockpit/src/lib/taskgraph.ts`,
  `cockpit/src/lib/fullgraph.ts` or `cockpit/src/components/TaskGraph.tsx` — four sibling rows own those.
- No new dependency: `Cargo.lock` and the `package-lock.json` files gain no package, and `bench/`'s Python gains no
  import beyond the standard library.

## Rules for this row

- **The Discord DM task flow is LOCKED.** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result.
- **Keel:** remove, rename or weaken no test; lower no assertion count; add no `#[allow]` or skip; raise no budget, cap
  or ceiling; do not edit `scripts/keel-guard.py` or its tests. **You cannot sign, so you cannot ack** — anything the
  guard flags goes under **Keel findings expected** with its rule, path and why. Before the report run
  `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` and paste its output.
- **No ceiling raised and no cap loosened.**
- **Public repo:** no person's name, no real organisation, address or email. Write "the owner". Python fixtures use
  invented report names and values; do not copy a real report's numbers into a fixture.
- **Red first; right over compatible.**
- **The gate:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. A commit touching only Python, Markdown
  or `docs/` changes nothing the gate builds, so for those commits `bench/report`'s own Python tests are the gate —
  run them and say so; run `scripts/gate.sh` itself before your first commit and before your last. For the cockpit
  commits: `npm test`, `npm run lint` and `npx tsc -b` in `cockpit/`, each with its counts. **The gate's cockpit step
  hides its own errors when `node_modules` is cold**: "cockpit ok" with no test count is not evidence, so paste the
  number of tests that ran, from `npm test` itself.

## Your report

Commit `CLOUD_REPORT.md` at the repository root as the branch's last commit, subject `cloud report (not for main)`, and
push. Write it with your editor tools, **never a shell heredoc** (a heredoc on a full temp filesystem has committed an
empty report here before), and check after the commit that it is not empty. End with a short final message that starts
`CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** **the step-0 finding about `omnibus.json` first**, then store format
(untouched), any ceiling, OWNER CALL or STOP, and the scope cut (the external generator, and the daemon-serving
option with what it would take). (2) **The manifest schema:** every field, where in a dataset it comes from, and why
you chose it. (3) **Coverage:** how many datasets became rows, and **every file skipped with its reason** — the
list matters. (4) **`draft.py`'s hook:** the exact hunk and how it is idempotent. (5) **The page:** what the summary
shows, where it sits, and the screenshot-in-words of one row. (6) **The newest Terminal-Bench numbers the page now
shows**, with their dates and commits, so the maintainer can check them by eye against the reports. (7) **Tests** and
plants P1-P6, red at the base and green after, with the Python and `npm test` counts before and after. (8) **Gate**
commands and results, with counts. (9) **Keel** output and any **Keel findings expected**. (10) **DM golden** diff
result. (11) **Join notes:** every file touched with its hunks' line ranges, and a line confirming you touched
nothing under `cockpit/src/ops/` or the other four rows' files. (12) **Follow-ups.** (13) One line on the data floor:
nothing new is sent to a model or routed anywhere, and no benchmark was run.
