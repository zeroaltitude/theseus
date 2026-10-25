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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3g-threads-lens`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
work, including **`theseus-odj0m`, the Operations page's one work set**, which this row is written on top of. Fetch it
and branch from `0fda4d57`. Where the release preamble above disagrees with this note about the base, the ceilings,
what may be edited or the work in flight, **this note wins**.

**This is a cockpit-only row. No Rust file changes.** You still run the Rust gate once at the end, as below.

**Early push (required, before any code).** Do setup first, then `npm ci` in `cockpit/` and one `npm test` to see the
suite green at the base. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-brcs4: session started, setup done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3g-threads-lens`

After that, push after EVERY commit, not only at the end. On 2026-10-24 three rows of sixteen ended holding nothing but
their task commit, and because every other row pushed only at the very end there was no way to tell a session that died
at minute 5 from one that worked for two hours and lost it. A run that ends early must leave its work on the branch.

**Store format: untouched.** This row changes no stored record and no wire type. Do not bump `MANIFEST_FORMAT`, and do
not regenerate `cockpit/src/protocol.gen/`.

**Line ceilings.** `scripts/long-files.txt` lists Rust files only; no TypeScript file has a ceiling. Never raise one
anyway. At your base `ops/ThreadsLens.tsx` is 99 lines and `lib/threadboard.ts` is 187.

## What odj0m already landed — read this before you write a line

Read `cockpit/src/ops/state.ts` and `cockpit/src/ops/board.ts` first. **The one work set already exists, and your lens
must read it rather than deriving its own:**

- **`boardRows()` (`ops/board.ts` ~:110) is the one work set**: every card once, each with its one `bucket` and
  whether it `counted`. It already adds a row per stored record that is not a task's own, with `kind: 'step'`; it maps
  each of the queue's asks to its card; an ask with no card becomes an ask row of its own in its lane.
  `RowKind` (~:20) is `'task' | 'conversation' | 'job' | 'wake' | 'routine' | 'watch' | 'step' | 'ask'`.
- **The buckets** (`state.ts`) are `needs | working | scheduled | paused | todo | rest | ended`; `bucketOfCard` is the
  one path, `COLUMN_OF` (~:86) maps a bucket to a Board column and is nullable.
- **`OWNER_CALLS`** (`state.ts` ~:31) is the page's one setting, and **every field is already `true`**: the owner
  answered all fifteen design questions on 2026-10-24 with the verbatim words **"All recs!"**.
- **The counter filter already works:** `shown()` (`board.ts` ~:376), `inBucket` on `row.bucket` (~:329),
  `chipWords` (~:433) giving `paused·queued · 4`, and the context set (`board.ts` ~:435).
- `cockpit/test/ops-counts-parity.test.ts` exists and must stay green.

## Your lane: `integ/p3-ops2`

Four other rows run beside you off this same base, all firing concurrently. Their write scopes are disjoint from yours:

- **`theseus-0zraw` (`-p3g-explainers-words`) joins FIRST.** It owns `lib/explainers.ts`, `test/explainers.test.ts`,
  the `components/TaskGraph.tsx` heading and the one `ops/CommandBar.tsx` help-button line. **Do not touch any of
  them, and add no help button or explainer entry** — `explainers.test.ts` fails an entry with no panel *and* a panel
  with no entry, so either half alone turns a branch red.
- **`theseus-hl0r1` (`-p3g-graph-forms`) joins after 0zraw** and owns `ops/GraphNode.tsx`, `ops/GraphLens.tsx`,
  `ops/graph.ts`, `ops/BoardLens.tsx`. It defines the nouns' forms — the plan card with its fold and *n/m*, the step's
  checkbox glyph, the task as a badge strip. **Do not touch those four files**, and do not reimplement their forms:
  where your lens shows a plan's progress, derive it from the rows the same way and say so, so the integrator can fold
  the two onto one helper later if they diverge.
- `theseus-gnhnk` (`-p3g-card-selection`) owns `ops/Inspector.tsx`, `ops/InspectorSections.tsx`, `ops/sections.ts`,
  `ops/regions.ts` and three hunks of `views/Ops.tsx` (~:183-191, ~:197, ~:491).
- `theseus-99l9z` (`-p3g-soft-retire`) joins after hl0r1 and adds a visibility checkbox group whose classes read
  `row.bucket`. **Your lens must not filter out ended or stale rows on its own** — that choice is 99l9z's, and if you
  hard-code a hide here the two will fight. List what the one work set gives you and let the filters decide.

**Your write scope, and nothing else:** `cockpit/src/ops/ThreadsLens.tsx`, `cockpit/src/lib/threadboard.ts`,
`cockpit/test/threadboard.test.ts`, and a new test file if you want one. Touch no other file. If you believe you must,
write `OWNER CALL:` in the report and leave it undone.

---

## Your task: p3g-threads-lens, the Threads lens over the one work set (theseus-brcs4)

Branch: `cloud/20261025-p3g-threads-lens`.

Every commit subject carries `theseus-brcs4`. Deadline for the report: about 3 hours after you start.

**Why.** The Operations page's Threads lens is the owner's view of work *by the line of work it belongs to*. Since
odj0m there is one work set with plans, steps and their attached tasks in it, and the lens still lists the older,
thinner thing. It must now list, under each thread, **its plans and steps with their attached tasks** — and, like
every other lens, it must respond to a counter click.

### What to build

1. **`groupThreads` and `listed` in `lib/threadboard.ts` read the one work set.** A thread groups the rows whose lane
   it is; under it come its plans, each with the steps listed under it, and under each step or plan its **attached
   task** — not as a second entry beside it, but as part of that card's line, the way the Graph draws it as a badge
   strip. A step with no task says who carries it. Home is the lane for work belonging to no thread.
2. **Nesting.** A plan holds steps and smaller plans. Show the nesting, with a plan's **n of m** done and a one-line
   summary of its open children by bucket ("1 working · 3 to do · 2 done"). A nested plan counts its own subtree
   once, not twice.
3. **A counter click filters the lens, exactly as it filters the Graph and the Board.** The page already computes
   which rows a filter keeps (`shown()`) and a context set of each kept row's ancestors. The lens's rule:
   **a thread is listed when it holds a kept card, and under it only the kept cards** (its ancestors drawn as faint
   context, the way the Graph draws them, so a kept step never appears without the plan it sits under). A thread with
   no kept card is not listed. The clear chip that `chipWords` gives (`paused·queued · 4 ×`) is the page's and the
   Graph's — do not draw a second one in the lens.
4. **The counters themselves are not yours.** `countersOf` and `needsOf` are odj0m's and correct. Your lens must
   agree with them: for each bucket, the number of **counted** cards your lens lists under a filter for that bucket
   equals the counter. Prove that with a test (see below) rather than by re-counting.

### The owner's answered questions that bear on this row

Both were answered on 2026-10-24 with the verbatim words **"All recs!"**:

- **Q1 — "Needs you" counts one per card**, however many asks that card holds. So a card with three asks is one line
  in your lens with one needs mark, never three. Read `needsPerCard` from `OWNER_CALLS`; do not hard-code it.
- **Q2 — a step listed and not started is its own state, `todo`**: **no counter and no Board column**, and it is
  carried in **its plan's n/m**. So a to-do step appears in your lens inside its plan's progress, and is not counted
  as paused·queued. Read `todoApart` from `OWNER_CALLS`; do not hard-code it.

---

## Tests (each red at the base, with a planted revert)

The suite is `npm test` in `cockpit/` (node's own runner over `test/*.test.ts`). Red-first means: the new assertion
fails at `0fda4d57`, or under a planted revert of your change, and passes after. **A test that would pass before the
change is not a test.** `threadboard.ts` is pure — test it as data, not by rendering React.

Over a fixture with two threads and Home, a plan with nested steps, a task-carrying plan with a doing step and a job,
a to-do backlog, a suspended step, two asks on one card, a loose ask and a conversation at rest:

1. **The shape:** each thread lists its plans and steps with their attached tasks; a task never appears as a sibling
   of the step it carries out; a taskless step says who carries it; Home holds the threadless work. Plant P1: emit the
   attached task as its own entry — red.
2. **Progress:** a plan's *n of m* and its by-bucket summary are right, and a nested plan's subtree counts once.
   Plant P2: double-count a nested plan — red.
3. **Q1 one per card:** a card with three asks is one line with one needs mark under `needsPerCard`. Plant P3: count
   asks instead of cards — red.
4. **Q2 to-do:** a to-do step is inside its plan's *n/m* and is **not** in paused·queued under `todoApart`.
   Plant P4: fold `todo` back into paused — red.
5. **The filter:** under a counter filter for each bucket, a thread is listed only when it holds a kept card; under it
   only the kept cards, with their ancestors as context; **and the number of counted cards listed equals that
   bucket's counter from `countersOf`** — the lens's own parity check, one assertion per bucket. Plant P5: list a
   thread with no kept card — red.
- Keep green: `threadboard.test.ts`'s existing tests, `ops-board.test.ts`, `ops-counts-parity.test.ts`,
  `ops-page.test.ts` and the rest of the suite; `npm run lint`; `npx tsc -b`.
- **Where an existing `threadboard.test.ts` expectation must change because the lens changed**, change the assertion
  rather than delete it, put that change in a commit of its own whose message says why, and list it under **Keel
  findings expected**. Never lower an assertion count.

## Not in this row

- **No `ops/GraphNode.tsx`, `ops/GraphLens.tsx`, `ops/graph.ts` or `ops/BoardLens.tsx`** — hl0r1's.
- **No `lib/explainers.ts`, `test/explainers.test.ts` or `ops/CommandBar.tsx`, and no help button** — 0zraw's.
- No `ops/Inspector.tsx`, `ops/InspectorSections.tsx`, `ops/sections.ts`, `ops/regions.ts`, no `views/Ops.tsx`
  (gnhnk's).
- **No `ops/board.ts` or `ops/state.ts`.** The one work set, the buckets and the counters are odj0m's. If your lens
  genuinely needs a new field on `OpsRow`, say so under **OWNER CALL** and derive it in `threadboard.ts` instead.
- **No hiding of ended, retired or stale rows of your own** — 99l9z owns that choice.
- No protocol, no Rust, no wire change, no `protocol.gen/` regeneration.
- No benchmark, no model call, no key, no spend, no AWS.

## Rules for this row

- **The Discord DM task flow is LOCKED.** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. If some part
  seems to need a Discord word changed, STOP that part and write `OWNER CALL:`.
- **Keel:** remove, rename or weaken no test; lower no assertion count; add no `#[allow]` or skip; raise no budget, cap
  or ceiling; do not edit `scripts/keel-guard.py` or its tests. **You cannot sign, so you cannot ack** — any
  expectation you must move goes in its own commit and under **Keel findings expected**. Before the report run
  `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` and paste its output.
- **No ceiling raised and no cap loosened.**
- **Public repo:** no person's name, no real organisation, address or email. Write "the owner". Fixtures use invented
  thread and card titles under `example.test`.
- **Red first; right over compatible.**
- **The gate** (read the root `AGENTS.md` and `cockpit/AGENTS.md` "Building and checking" first). For a cockpit-only
  change: `npm ci`, `npm test`, `npm run lint` and `npx tsc -b` in `cockpit/`, each with its counts; then
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` once before your last commit. **The gate's cockpit step
  hides its own errors when `node_modules` is cold**: "cockpit ok" with no test count is not evidence, so paste the
  number of tests that ran, from `npm test` itself.

## Your report

Commit `CLOUD_REPORT.md` at the repository root as the branch's last commit, subject `cloud report (not for main)`, and
push. Write it with your editor tools, **never a shell heredoc** (a heredoc on a full temp filesystem has committed an
empty report here before), and check after the commit that it is not empty. End with a short final message that starts
`CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (untouched), any ceiling, OWNER CALL or STOP. (2) **The
shape:** `groupThreads` and `listed`, their signatures, and a text rendering of the fixture's lens before and after.
(3) **Progress:** how a plan's *n/m* is derived from rows, and how it relates to whatever hl0r1 derives for the Graph
(say whether the two should be folded onto one helper at the join). (4) **Q1 and Q2:** where each is read from
`OWNER_CALLS`, never hard-coded. (5) **The filter and the parity check:** the per-bucket assertion and its numbers.
(6) **Tests** and plants P1-P5, red at the base and green after, with the `npm test` test count before and after.
(7) **Gate** commands and results, with counts. (8) **Keel** output and any **Keel findings expected**. (9) **DM
golden** diff result. (10) **Join notes:** every file touched with its hunks' line ranges; **state explicitly that you
touched neither hl0r1's four files nor 0zraw's, and that you hid no ended or stale row of your own**. (11)
**Follow-ups.** (12) One line on the data floor: nothing new is sent to a model or routed anywhere.
