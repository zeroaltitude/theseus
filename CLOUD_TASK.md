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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3g-graph-forms`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
- `git commit --allow-empty -m "theseus-hl0r1: session started, setup done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3g-graph-forms`

After that, push after EVERY commit, not only at the end. On 2026-10-24 three rows of sixteen ended holding nothing but
their task commit, and because every other row pushed only at the very end there was no way to tell a session that died
at minute 5 from one that worked for two hours and lost it. A run that ends early must leave its work on the branch.

**Store format: untouched.** This row changes no stored record and no wire type. Do not bump `MANIFEST_FORMAT`, and do
not regenerate `cockpit/src/protocol.gen/`.

**Line ceilings.** `scripts/long-files.txt` lists Rust files only; no TypeScript file has a ceiling. Never raise one
anyway. At your base: `ops/graph.ts` 438 lines, `ops/GraphNode.tsx` 180, `ops/GraphLens.tsx` 256, `ops/BoardLens.tsx`
146. If a file would grow unwieldy, put the new code in a new module beside it (`ops/forms.ts` is a good name) rather
than letting one file swell.

## What odj0m already landed — read this before you write a line

Read `cockpit/src/ops/state.ts`, `cockpit/src/ops/board.ts` and `cockpit/src/ops/graph.ts` first. The one work set is
already built, and **you must consume it, never re-derive it**:

- **`boardRows()` (`ops/board.ts`, ~:110) is the one work set**: every card once, each with its one `bucket` and
  whether it `counted`. It already adds a row per stored record that is not a task's own, with `kind: 'step'`; it takes
  the queue's asks and maps each to its card; an ask with no card becomes an ask row of its own.
  `RowKind` (~:20) is `'task' | 'conversation' | 'job' | 'wake' | 'routine' | 'watch' | 'step' | 'ask'`.
- **`ops/state.ts`'s buckets** are `needs | working | scheduled | paused | todo | rest | ended`. `bucketOfCard` is the
  one path; `COLUMN_OF` (~:86) maps a bucket to a Board column, nullable (`todo` has none).
- **`OWNER_CALLS`** (`state.ts`, ~:31) is the page's one setting for the owner's calls, and **every field is already
  `true`**: the owner answered all fifteen design questions on 2026-10-24 with the verbatim words **"All recs!"**.
  `RECOMMENDED` beside it is the same object. The fields are `needsPerCard`, `todoApart`, `agentsOnly`, `hideRest`,
  `scheduledLabel`, `stalledIsPaused`.
- **The layout already runs off rows:** `layoutRows(rows, lanes, prev, opts)` (`graph.ts`, ~:137) — every node is a
  row and every row a node. `opsLayout` (~:130) is the views/records wrapper over the same path.
- **The counter filter already works:** `shown()` (`board.ts`, ~:376), `inBucket` on `row.bucket` (~:329),
  `chipWords` (~:433) giving `paused·queued · 4`, and the context set (`board.ts`, ~:435).
- The parity test `cockpit/test/ops-counts-parity.test.ts` exists and must stay green.

**Do not re-open any of those decisions.** Your row is the *forms*: what each noun looks like on the Graph and the
Board, now that the set beneath them is right.

## Your lane: `integ/p3-ops2`

Four other rows run beside you off this same base, all firing concurrently. Their write scopes are disjoint from yours
except where named:

- **`theseus-0zraw` (`cloud/20261025-p3g-explainers-words`) joins BEFORE you.** It owns `lib/explainers.ts`,
  `test/explainers.test.ts`, the `components/TaskGraph.tsx` heading, and **the one line in `ops/CommandBar.tsx` that
  mounts the `ops.graph` help button**. **You must not add a help button, must not add an explainer entry, and must
  not touch `explainers.ts` or `CommandBar.tsx` at all.** `cockpit/test/explainers.test.ts` fails both directions — a
  panel whose help id has no entry, *and* an entry no panel uses — so if you mounted the button your own branch would
  be red, and that is precisely why 0zraw goes first.
- `theseus-gnhnk` (`-p3g-card-selection`) owns `ops/Inspector.tsx`, `ops/InspectorSections.tsx`, `ops/sections.ts`,
  `ops/regions.ts` and three small hunks of `views/Ops.tsx` (~:183-191, ~:197, ~:491). **You may add at most one prop
  on `views/Ops.tsx` ~:440** where the lens is rendered, and nothing else in that file; if you can do without it, do.
- `theseus-brcs4` (`-p3g-threads-lens`) owns `ops/ThreadsLens.tsx` and `lib/threadboard.ts`.
- **`theseus-99l9z` (`-p3g-soft-retire`) joins AFTER you** and will add a visibility checkbox group to the Graph
  lens's toolbar and read `row.bucket` for its classes. Leave the toolbar's structure easy to add to, and say in the
  report exactly where its checkbox group should go.

**Your write scope, and nothing else:** `cockpit/src/ops/GraphNode.tsx`, `cockpit/src/ops/GraphLens.tsx`,
`cockpit/src/ops/graph.ts`, `cockpit/src/ops/BoardLens.tsx`, a new `cockpit/src/ops/forms.ts` if you want one,
`cockpit/test/ops-graph.test.ts`, a new `cockpit/test/ops-forms.test.ts`, and at most one prop at `views/Ops.tsx`
~:440. Touch no other file. If you believe you must, write `OWNER CALL:` in the report and leave it undone.

---

## Your task: p3g-graph-forms, the nouns' forms on the Graph and the Board (theseus-hl0r1)

Branch: `cloud/20261025-p3g-graph-forms`.

Every commit subject carries `theseus-hl0r1`. Deadline for the report: about 3 hours after you start.

**Why.** The set is right and the drawing is not: a plan, a step and an attached task are all drawn as the same work
card, so the Graph cannot show a plan's progress, a step's state, or that a task is carrying out a step rather than
being a second piece of work beside it. The owner's six nouns each get one form.

### The forms

| noun | form | colour / state |
|---|---|---|
| **thread** | The swimlane (today's `Lane`, `GraphNode.tsx` ~:157). The thread's own conversation is drawn as the lane's **head card** at column 0, with the thread's title, warmth and seal, its plan-mode *n/m* and its turn's live line. Clicking the head selects the thread. | the lane glows amber when anything in it needs the owner today |
| **conversation** (Home) | A navy anchor card at column 0, as today. | |
| **plan** | A card with a **folder tab** on its top edge, a chevron ▸/▾, "PLAN" and its short id, the title, an **n/m bar** (done in the ok tone, the open segments by bucket), and a one-line summary when folded ("1 working · 3 to do · 2 done"). Unfolded, its children sit in the next column, joined by solid "under" edges (today's parent edge). | its own bucket's tone; a folded plan holding a needs card shows "● 1 needs you" and that child is drawn anyway |
| **step** | Today's work card, labelled "STEP · ‹short id›", with a **checkbox glyph** in its state colour: ☐ to do, ◐ doing, ■ blocked, ● needs you, ⏸ paused, ☑ done, ⊘ abandoned, ✗ failed. | bucket tones (`bucketTone`): **to do is ink-dim, not grey-faded; only `ended` is faded** |
| **task** (attached) | **Not a card.** A badge strip along the bottom of the step or plan it carries out: its state glyph, the word "task", the model chip, the spend bar, and its live progress line (`progressLine`). A claim draws as "worked by ‹title›". | cyan glow while running (today's `ring`); amber when it asks |
| **job** | A pill beside the card whose task or turn runs it (today's `Job` node, kept). | its own state tone; not counted |
| **ask** (loose) | A small amber card in its lane with the ask's words and its keys. | amber |
| **wake / routine / watch / project** | Behind "+ future", as today (`Future`). A project draws as a plan card with a ◈ and its dates. | think tone |

**Folding.** A plan opens by default while it is open and folds when it ends. The ended-work fold "✓ N done" and the
node cap's "+N folded" stay as they are (`graph.ts` ~:198-249), and a folded plan counts as **one** node toward the
400-node cap (`NODE_CAP`). **The owner's folds live in the lens's memory, as `opened` does (`GraphLens.tsx` ~:67), not
in the URL.**

**Escalation in place.** When something is listed under a step, it becomes a plan: **the same card, the same history**.
Draw it from the first child step listed under it, derived in place — **no daemon row, no new wire field, no Rust**.
That is the owner's Q5 answer. The card must not jump, change id, or lose its selection when it morphs.

**Progress.** Add `progressOf` to `graph.ts` (or `forms.ts`): for a plan row, its *n of m* and the per-bucket counts of
its open children, derived from the one work set's rows alone. Nested plans count their own subtree once.

**The Board.** `BoardLens.tsx` draws plan cards with the same fold, *n/m* bar and summary. A step draws with its
checkbox glyph. The columns and which card lands in which are already `COLUMN_OF`'s and `columnsOf`'s job — do not
change them.

**The counter filter must keep working through the new forms.** When a counter click hides everything else (`hideRest`
is on), a collapsed plan that holds a kept card **opens for the filter**, and a lane with no kept card shrinks to its
head. Kept slots stay sticky so nothing moves. That behaviour exists; prove with a test that your forms did not break
it.

---

## Tests (each red at the base, with a planted revert)

The suite is `npm test` in `cockpit/` (node's own runner over `test/*.test.ts`). Red-first means: the new assertion
fails at `0fda4d57`, or under a planted revert of your change, and passes after. **A test that would pass before the
change is not a test.** The layout and the forms are pure functions over rows — test them as data, not by rendering
React.

New `cockpit/test/ops-forms.test.ts`, over a fixture with a thread, a plan with nested steps, a task-carrying plan with
a doing step and a job, a suspended step, an ask on a task, a loose ask, a to-do backlog and a wake due today:

1. **The step glyph** is the one its bucket names, for each of the eight states; a `todo` step is ink-dim and **not**
   faded, and only an `ended` one is faded. Plant P1: map `todo` to the faded look — red.
2. **`progressOf`** gives the plan's *n of m* and its per-bucket summary; a nested plan counts its subtree once, not
   twice. Plant P2: double-count a nested plan's children — red.
3. **The task is a badge, not a card:** the attached task produces no node of its own; its glyph, model chip and
   progress line hang on the step or plan it carries out. Plant P3: emit a task node beside the step — red, and the
   parity test should notice too.
4. **Escalation in place:** a step with one child listed under it renders as a plan **with the same node id and the
   same history**, and the selection survives the morph. Plant P4: give the morphed card a new id — red.
5. **Folding:** a plan folds when it ends and opens while it is open; a folded plan counts one node toward
   `NODE_CAP`; a folded plan holding a needs card still draws that child and shows "● 1 needs you". Plant P5: count a
   folded plan's children against the cap — red.
6. **The filter still works through the forms:** under a counter filter, a collapsed plan holding a kept card opens, a
   lane with no kept card shrinks to its head, and kept nodes keep their slots.
- Keep green: `ops-graph.test.ts`, `ops-graph-state.test.ts`, `ops-board.test.ts`, `ops-counts-parity.test.ts`,
  `ops-page.test.ts`, `ops-selection.test.ts` and the rest of the suite; `npm run lint`; `npx tsc -b`.
- **Where an existing `ops-graph.test.ts` expectation must change because the form changed**, change the assertion
  rather than delete it, put that change in a commit of its own whose message says why, and list it under **Keel
  findings expected** in the report. Never lower an assertion count.

## Not in this row

- **No `lib/explainers.ts`, no `test/explainers.test.ts`, no `ops/CommandBar.tsx`, no help button** — 0zraw's, and
  adding one would turn your own suite red.
- No `ops/Inspector.tsx`, `ops/InspectorSections.tsx`, `ops/sections.ts`, `ops/regions.ts` (gnhnk's); no
  `ops/ThreadsLens.tsx` or `lib/threadboard.ts` (brcs4's); no visibility checkbox group (99l9z's).
- **No change to `ops/board.ts` or `ops/state.ts`.** The buckets, the counters, `COLUMN_OF` and the one work set are
  odj0m's and are correct. If a form genuinely needs a new field on `OpsRow`, say so in the report under **OWNER CALL**
  and derive it in your own module instead.
- No protocol, no Rust, no wire change, no `protocol.gen/` regeneration.
- No benchmark, no model call, no key, no spend, no AWS.

## Rules for this row

- **The Discord DM task flow is LOCKED.** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. Nothing here
  should go near it; if some part seems to need a Discord word changed, STOP that part and write `OWNER CALL:`.
- **Keel:** remove, rename or weaken no test; lower no assertion count; add no `#[allow]` or skip; raise no budget, cap
  or ceiling (`NODE_CAP` included); do not edit `scripts/keel-guard.py` or its tests. **You cannot sign, so you cannot
  ack** — any expectation you must move goes in its own commit and under **Keel findings expected**, and the maintainer
  brings it to the owner. Before the report run `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet`
  and paste its output.
- **No ceiling raised and no cap loosened.** `NODE_CAP` stays 400.
- **Public repo:** no person's name, no real organisation, address or email. Write "the owner". Fixtures use invented
  subjects under `example.test`.
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

Report sections: (1) **For the maintainer:** store format (untouched), any ceiling, OWNER CALL or STOP. (2) **Each
form**, with the file and line where it is drawn, and a short rendering of the fixture's graph before and after, in
text. (3) **`progressOf`:** its signature, where nested plans are handled, and its edge cases. (4) **Escalation in
place:** how the morph keeps the id, the history and the selection. (5) **Folding and the cap:** how a folded plan
counts one, and where the fold state is kept. (6) **The filter through the forms:** what you proved. (7) **Tests** and
plants P1-P5, red at the base and green after, with the `npm test` test count before and after. (8) **Gate** commands
and results, with counts. (9) **Keel** output and any **Keel findings expected**, each with its rule, path and why.
(10) **DM golden** diff result. (11) **Join notes:** every file touched with its hunks' line ranges; **say explicitly
that you touched neither `explainers.ts` nor `CommandBar.tsx`**; name the one `views/Ops.tsx` prop if you added it; and
say **where 99l9z should put its visibility checkbox group in the toolbar** so its join is a plain merge. (12)
**Follow-ups.** (13) One line on the data floor: nothing new is sent to a model or routed anywhere.
