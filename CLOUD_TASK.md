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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3g-card-selection`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
- `git commit --allow-empty -m "theseus-gnhnk: session started, setup done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3g-card-selection`

After that, push after EVERY commit, not only at the end. On 2026-10-24 three rows of sixteen ended holding nothing but
their task commit, and because every other row pushed only at the very end there was no way to tell a session that died
at minute 5 from one that worked for two hours and lost it. A run that ends early must leave its work on the branch.

**Store format: untouched.** This row changes no stored record and no wire type. Do not bump `MANIFEST_FORMAT`, and do
not regenerate `cockpit/src/protocol.gen/`.

**Line ceilings.** `scripts/long-files.txt` lists Rust files only; no TypeScript file has a ceiling. Never raise one
anyway. `views/Ops.tsx` is 538 lines at your base.

## Your lane: `integ/p3-ops2`

Four other rows run beside you off this same base, all firing concurrently:

- **`theseus-0zraw` (`-p3g-explainers-words`) joins FIRST.** It owns `lib/explainers.ts`, `test/explainers.test.ts`,
  the `components/TaskGraph.tsx` heading and the one `ops/CommandBar.tsx` help-button line. **Do not touch any of
  them, and add no help button or explainer entry** — `explainers.test.ts` fails an entry with no panel *and* a panel
  with no entry, so either half alone turns a branch red.
- **`theseus-hl0r1` (`-p3g-graph-forms`) joins after 0zraw and owns the Graph's and Board's forms:**
  `ops/GraphNode.tsx`, `ops/GraphLens.tsx`, `ops/graph.ts`, `ops/BoardLens.tsx`. **Do not touch those four files.**
  It may add at most one prop at `views/Ops.tsx` ~:440 — stay off that line.
- `theseus-brcs4` (`-p3g-threads-lens`) owns `ops/ThreadsLens.tsx` and `lib/threadboard.ts`.
- `theseus-99l9z` (`-p3g-soft-retire`) joins after hl0r1; it owns the Graph lens toolbar's visibility checkboxes,
  `lib/taskgraph.ts` and the Actions page's `components/TaskGraph.tsx` body.

**Your write scope, and nothing else:** `cockpit/src/ops/Inspector.tsx`, `cockpit/src/ops/InspectorSections.tsx`,
`cockpit/src/ops/sections.ts`, `cockpit/src/ops/regions.ts`, **three hunks of `cockpit/src/views/Ops.tsx` only**
(~:183-191 the selection, ~:197 the focus row, ~:491 the idle inspector tab), `cockpit/test/ops-selection.test.ts`,
`cockpit/test/ops-page.test.ts`, `cockpit/test/ops-sections.test.ts`, and a new test file if you want one. Touch no
other file. If you believe you must, write `OWNER CALL:` in the report and leave it undone.

---

## Your task: p3g-card-selection, every card opens the inspector (theseus-gnhnk)

Branch: `cloud/20261025-p3g-card-selection`.

Every commit subject carries `theseus-gnhnk`. Deadline for the report: about 3 hours after you start.

**Why — the owner's own words, 2026-10-24:**

> it should default to coming open in this case, but also, clicking on it does /not/ open the inspector window, which
> is also a bug

He had clicked a task-record card on the Ops Graph ("SUSPENDED step install33 join check", "WAITING HUMAN step Secret
lookups failing", "ACCEPTED step Self-improvement backlog") and nothing opened. **This is a P1 bug, filed with a
diagnosis — read it, then verify it against the code before you fix anything:**

The strip he saw is the **idle** tab (`views/Ops.tsx` ~:491: `{reg.inspector === 'idle' && <Tab side="right"
label="Inspector · select a card" onClick={() => pick(queue[0]?.key ?? null)} />}`), not the folded one. The page
thinks nothing is selected. `GraphLens`'s `onNodeClick` → `onFocus(n.id)` → `pick()` sets `a.sel`, but the selection
at `views/Ops.tsx` ~:183-191 is built only from `item | thread | row | event | mark`, and the `row` arm is
`rows.find((r) => selects(r, a.sel))`. The cards in the screenshot are **task-record nodes**, which under odj0m's one
work set are rows of `kind: 'step'` — so either `selects()` does not match them by the id the graph node carries, or
the row is not in the set the page searched. Find out which, name it in the report with the file and line, and fix
that, not a symptom.

The second half: **the idle tab's click calls `pick(queue[0]?.key ?? null)`, which is a no-op when Needs you is
empty.** That is the "it should default to coming open" half.

### What to build

1. **Every card opens the inspector.** Because odj0m made every Graph node a row of the one work set, no new selection
   kind should be needed: `onNodeClick` → `pick(id)` → `rows.find(selects(r, a.sel))` → `Selection { kind: 'row', row }`
   should find every card. Make that true. If `selects()` (`ops/board.ts` ~:381) genuinely cannot match a step row by
   the id its graph node carries, **do not edit `board.ts`** — map the node id to the row in your own code at the
   `views/Ops.tsx` ~:183-191 hunk, and say in the report that `selects()` deserves a follow-up. A `record` selection
   kind is acceptable if the mapping is genuinely impossible; prefer the mapping, and justify whichever you choose.
2. **`StepWhole`, the inspector body for a step or plan row**, a new body in `Inspector.tsx` drawing:
   - its noun, state glyph and state word, and its short id;
   - the breadcrumb thread › plan › …;
   - **for a plan:** its *n of m* and the checklist of its children, each click selecting that child;
   - the objective and acceptance, deps ("waits on", linked), claim and evidence — `recordFacts` and `RecordBlock`
     already exist, reuse them;
   - the history: created, the escalation line (when a step became a plan), closed, and the Ledger link;
   - **its task, when one is attached:** the existing `RowWhole` task sections (facts, Live, and the verbs pause, stop
     and cancel), acting on the task's session;
   - **a taskless step:** "carried by ‹conversation›" with Join (`c`).

   **No new RPC and no new wire type.** A record's own verbs are the model's; the owner's verbs act through the
   attached task.
3. **The idle strip opens the panel, always** (`views/Ops.tsx` ~:491). If Needs you has an ask it selects the first,
   as today. **Otherwise it opens the inspector panel with `Selection { kind: 'none' }`**, drawn as an empty state:
   *"Nothing is selected. Click a card, or press j to take the first."* It also lists the current counts, each a link
   that sets that filter, and the key hints.
   - `regionsFor` (`ops/regions.ts` ~:29-35) gains **one** input, "opened by hand", so a panel opened with nothing
     selected is `side`, not `idle`. Keep its other behaviour exactly as it is — the narrow and wide breakpoints, the
     folded tab, `comingShows`.
   - `Esc` or `]` folds it back, as today.
4. **Shift-click picks and right-click opens the verb menu for any card with a row** (`GraphLens.tsx` ~:172-175 does
   this already) — which is now every card. **Do not edit `GraphLens.tsx`**; just prove with a test that the
   selection path those handlers call now resolves for a step node, and say so in the report.

---

## Tests (each red at the base, with a planted revert)

The suite is `npm test` in `cockpit/` (node's own runner over `test/*.test.ts`). Red-first means: the new assertion
fails at `0fda4d57`, or under a planted revert of your change, and passes after. **A test that would pass before the
change is not a test.** Test the selection and the region rules as pure data; do not render React.

1. **A click on each node kind fills the inspector.** Over a fixture with a thread head, a conversation, a plan, a
   nested step, a suspended step, a step carrying a task, a job pill, a loose ask and a wake: for **every** node the
   layout emits, resolving its id through the page's selection path yields a non-`none` `Selection`, and the
   inspector body chosen for it is the right one. Plant P1: restore the base's selection arms so step rows resolve to
   nothing — **red, and it must name which kinds failed**, since that is the bug the owner hit.
2. **`StepWhole`'s facts:** for a plan row it gives *n of m* and its children's checklist; for a taskless step it
   gives "carried by ‹conversation›"; for a step with a task it gives the task's verbs. Plant P2: drop the task
   sections from a step that has one — red.
3. **The idle strip:** with Needs you empty, the click opens the panel with `kind: 'none'` and the empty state's
   words; with an ask present it selects the first, as today. Plant P3: restore `pick(queue[0]?.key ?? null)` — red.
4. **`regionsFor` with the new input:** opened-by-hand with nothing selected gives `inspector: 'side'` at wide width;
   every existing case of `regionsFor` is unchanged, with a table of width × folds × selected × by-hand. Plant P4:
   ignore the new input — red.
- Keep green: `ops-selection.test.ts`, `ops-page.test.ts`, `ops-sections.test.ts`, `ops-board.test.ts`,
  `ops-counts-parity.test.ts`, `ops-graph.test.ts`, `ops-keys.test.ts`, `ops-scroll.test.ts` and the rest of the
  suite; `npm run lint`; `npx tsc -b`.
- **Where an existing expectation must change because the behaviour changed**, change the assertion rather than delete
  it, put that change in a commit of its own whose message says why, and list it under **Keel findings expected**.
  Never lower an assertion count.

## The owner's answered question that bears on this row

**Q4, answered "All recs!" on 2026-10-24:** a counter click **hides everything else**, keeping each match's parents as
faint context, plus a clear chip (`paused·queued · 4 ×`, whose × and `Esc` clear it). That is already in the code
(`OWNER_CALLS.hideRest` is `true`; `board.ts`'s `shown`, the context set and `chipWords`). **Selection and the counter
filter share that behaviour:** selecting a card that the current filter hides must not silently show it, and clearing
the filter must not drop the selection. Prove both with a test.

## Not in this row

- **No `ops/GraphLens.tsx`, `ops/GraphNode.tsx`, `ops/graph.ts` or `ops/BoardLens.tsx`** — hl0r1's.
- **No `lib/explainers.ts`, `test/explainers.test.ts` or `ops/CommandBar.tsx`, and no help button** — 0zraw's.
- **No `ops/board.ts` or `ops/state.ts`.** The one work set, the buckets and `selects()` are odj0m's. Map in your own
  hunk instead, and file the follow-up.
- No `ops/ThreadsLens.tsx` or `lib/threadboard.ts` (brcs4's); no visibility checkbox group (99l9z's).
- No `views/Ops.tsx` outside the three named hunks.
- No protocol, no Rust, no wire change, no new RPC, no `protocol.gen/` regeneration.
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
  subjects under `example.test` — **invent the card titles; do not copy the ones from the screenshot above.**
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

Report sections: (1) **For the maintainer:** store format (untouched), any ceiling, OWNER CALL or STOP. (2) **The real
cause**, at file and line: what actually stopped a step card from selecting, and whether the filed diagnosis was right.
(3) **The fix:** mapping or a `record` kind, and why you chose it. (4) **`StepWhole`:** what it draws, per row kind,
with the file and line of each section. (5) **The idle strip and `regionsFor`:** the new input, and the table of cases
you proved unchanged. (6) **Selection × filter:** what you proved about hidden cards and clearing. (7) **Tests** and
plants P1-P4, red at the base and green after, with the `npm test` test count before and after, and **P1's list of
which node kinds failed at the base** — that is the owner's bug, written down. (8) **Gate** commands and results, with
counts. (9) **Keel** output and any **Keel findings expected**. (10) **DM golden** diff result. (11) **Join notes:**
every file touched with its hunks' line ranges; **state explicitly that you touched none of hl0r1's four files, none
of 0zraw's, and no `views/Ops.tsx` line outside ~:183-191, ~:197 and ~:491**. (12) **Follow-ups**, including
`selects()` if you had to work around it. (13) One line on the data floor: nothing new is sent to a model or routed
anywhere.
