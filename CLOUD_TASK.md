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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3b-context-chart`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
This row is cockpit-only and needs no store change. Do not bump it.

**Line ceilings.** Rust ceilings (`scripts/long-files.txt`) do not apply to the cockpit, but keep files reasonable:
`cockpit/src/views/Context.tsx` is 481 lines; put new logic in `cockpit/src/lib/explorer.ts` (236) or a new lib module.

**Your lane: `integ/p3-cockpit`.** Its other rows, fired at about the same time on the same base:
- `p3b-human-time` (theseus-hnm37) OWNS the shared time formatter `cockpit/src/lib/figures.ts` (new `span`, `until`,
  `when`; `ago`, `ms`, `uptime` change). Do not edit `figures.ts`; month labels here are `YYYY-MM` keys, not times.
- `p3b-busy-wheel` (theseus-6o8q8): a new `components/ShipsWheel.tsx`, `components/Heartbeat.tsx`'s spinner,
  `components/People.tsx` and `components/OntologyParts.tsx` (the Ontology proposals panel). Not yours.
**Shared with you:** nothing, if you stay in `views/Context.tsx`'s `Timeline` (:264-298), `lib/explorer.ts`'s
`monthBins` (:193-206) and new files. If you need a sqrt/log scale helper, put it in `lib/viz.ts` or your own new
module (not `figures.ts`). Other lanes of this wave work in the ontology code, the voice code and the trust marks;
the wave-1 lane works in recall, the catalog and the index. Do not edit `docs/status.md`.

## Your task: p3b-context-chart, a readable "as of · episodes a month" chart (theseus-11cvd)

Branch: `cloud/20261024-p3b-context-chart`.

Every commit subject carries `theseus-11cvd`. Deadline for the report: about 2 hours after you start.

**What the owner said (2026-10-24):** "The context page as of; episodes widget displays in table mode but does not
seem to work anymore in chart mode".

**Why (the maintainer's read of the owner's `import.sessions` facets after a re-import):** 84 months have episodes.
The first are `1970-01` (5 episodes) and `1973-11` (5), then `2006-07` (323) ... up to `2026-10` (19,778). `Timeline`
(`cockpit/src/views/Context.tsx` :264-298) asks `monthBins` (`cockpit/src/lib/explorer.ts` :193-206), which fills
EVERY month from the first to the last with gaps as 0 (capped at 1,200): about 680 bars at `min-w-[3px] flex-1`
(:283) in a 132 px panel; and heights are linear against the max (`Math.max(3, count / max * 100)%`, :286), so
against 19,778 nearly every bar is 0-3 px. It looks empty. Table mode (`ChartPanel`'s toggle,
`components/ChartPanel.tsx` :38-44, `lib/chartview.ts` :7, kept in `?table=`) still works and must keep working. The
data comes from `useRpc('import.sessions', query, 30_000)` (`Context.tsx` ~:133), `facets.months`, oldest first
(the protocol promises it: `crates/theseus-protocol/src/import.rs` :525-526). A click on a bar keeps that month
(`pick`), and `picked` may be a range `a..b`: keep both working.

**The bad dates are data.** Episodes stamped epoch 0 or in 1973 are almost certainly bad timestamps from an import.
**Do not delete, rewrite or re-date any episode, in the cockpit or the daemon.** Show them honestly; if you can see
where they come from (read the importer: `crates/theseus-core/src/rpc/import*.rs` and the import crate, read-only),
describe it in the report as a follow-up row; do not fix it here.

### What to build

1. **Undated chip.** Months before a plausible floor (before 2000-01; make the floor a named constant with a comment
   saying why) are not drawn as bars. Their episodes show as one chip beside the chart, `undated · 10 episodes`
   (hover/tip lists the months and counts). A click on the chip keeps those episodes the way a bar click keeps a month
   (a range `first..1999-12` if `pick` takes ranges: check how `picked` is applied to the query; if the query cannot
   express it, the chip only informs and says so in its tip). The table keeps every month, the undated ones included
   (it is the honest full view), marked as undated.
2. **A sensible window.** Default to the last 24 months that have data (ending at the newest month). A small range
   control in the panel header: `2y` (default) / `5y` / `all`, kept in the URL like the table toggle (`lib/
   chartview.ts`'s pattern) so a reload keeps it. `all` still starts at the floor, not 1970.
3. **Long gaps compressed.** Inside the window, a run of more than 6 empty months is drawn as one narrow gap mark
   (a thin dashed column with a tip `2009-02 to 2012-07: no episodes`), not as dozens of empty columns. Short gaps stay
   as empty months.
4. **Heights that show the shape.** A height scale option `linear` / `sqrt` (default `sqrt`), in the same header
   control style; a non-zero month is always at least visible (keep the 3 % floor). Count labels on the biggest bars
   (the top 1-3 that fit), and the footer keeps `most: N in a month`.
5. Keep: the tip on each bar, click-to-keep, the `on`/opacity rule for a picked month or range, the light/dark accent,
   the empty state.

### Tests (red first)

Cockpit's runner is node's own test runner over `cockpit/test/*.test.ts` (a tested module must be pure, `import type`
only from the protocol, imports named with `.ts`; read `cockpit/AGENTS.md` ~:373-380). Put the logic in pure functions
in `lib/explorer.ts` (or a new pure module) and test them in `cockpit/test/explorer.test.ts` (`monthBins` is tested at
~:93-95) or a new test file:
1. The owner's shape as a fixture (invented counts are fine, same shape): `1970-01` 5, `1973-11` 5, `2006-07` 323, a
   few months to `2026-10` 19,778. Assert: the undated group holds the two old months (10 episodes); the default window
   is the last 24 months ending `2026-10`; no bin before the floor is drawn; with `all`, the first drawn month is
   `2006-07`; the bar count is small enough to read (for example at most 60 columns for `5y`, gap marks included).
2. Gap compression: a fixture with a 40-month gap draws one gap mark, a 3-month gap draws three empty months.
3. Sqrt heights: a month of 323 against a max of 19,778 is visibly tall (above 10 %) under `sqrt` and tiny under
   `linear`.
4. Existing `monthBins` tests stay green (if you change its signature, keep the old behaviour available for any other
   caller: `rg -n monthBins cockpit/src`).
Plants: P1, draw from the first month again (no floor): test 1 red. P2, linear default: test 3's default-scale case red.

## Not in this row

No change to the episodes, their dates, the importer, the daemon or the protocol. No new timer or refetch. No
change to table mode's rows except marking undated months.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical (you touch no
  Rust). Run `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result.
- **Keel:** remove or weaken no test; raise no budget, cap or ceiling; do not edit `scripts/keel-guard.py` or its
  tests; rename no test. Before the report run `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and
  paste its output.
- **Cockpit data rule:** no data lost or hidden without a trace: every episode is counted somewhere on the panel
  (window, gap marks, or the undated chip, plus the table).
- **Accessibility:** the range and scale controls are buttons with `aria-pressed`; the chip and gap marks have labels.
- **Public repo:** no person's name, no real organisation, no address, no email, no key. Write "the owner".
- **Red first;** right over compatible.
- **The gate:** in `cockpit/`: `npm ci`, `npm run lint`, `npm test`, `npm run build`. If `cockpit/AGENTS.md` shows how
  to load a view from a scratch daemon, load the Context page with an import fixture and say what you saw; if you
  cannot, say so.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched), OWNER CALL or STOP if any. (2) **The
chart as built:** the floor constant, the default window, the range and scale controls and their URL keys, gap
compression's threshold, labels. (3) **The undated episodes:** how they show, and what you found about where the bad
dates come from (a follow-up row, not fixed). (4) **Tests** and plants P1-P2. (5) **Gate** commands and results.
(6) **Keel** output. (7) **DM golden** diff result. (8) **Join notes:** every file touched, and confirmation that you
touched none of the lane's other rows' files. (9) **Follow-ups.**
