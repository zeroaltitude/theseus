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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3b-human-time`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
This row needs no store change. Do not bump it.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every Rust file not listed there caps at 2,500 lines;
`scripts/shape.sh` checks only `crates/*.rs`). At the base: `crates/theseus/src/render.rs` 3,092 of 3,128 (36 lines of
room: add nothing there you can put in a `render/` module); `cockpit/src/lib/figures.ts` 100.

**Your lane: `integ/p3-cockpit`.** Its other rows, fired at about the same time on the same base:
- `p3b-busy-wheel` (theseus-6o8q8): a new ship's-wheel busy component (`cockpit/src/components/ShipsWheel.tsx`),
  replacing `animate-spin` (`components/Heartbeat.tsx` :386) and the ontology `Act` busy text
  (`components/OntologyParts.tsx` :59-65), and the Ontology proposals panel (`components/People.tsx` :123-193)
  locking while an accept runs.
- `p3b-context-chart` (theseus-11cvd): the Context page's month chart (`cockpit/src/views/Context.tsx` :264-298,
  `cockpit/src/lib/explorer.ts` :193-206 `monthBins`).
**You OWN the shared time formatter** (`cockpit/src/lib/figures.ts`). The other two rows were told to use it for any
duration or clock time and not to add their own. **Shared with you:** `figures.ts` is yours alone; if either other
row needs a duration it imports your names. Do not edit `People.tsx`, `OntologyParts.tsx`, `Heartbeat.tsx`'s
spinner line, `Context.tsx`'s `Timeline` or `explorer.ts`'s `monthBins` (if an `ago()` call in one of those files
changes only because `ago()` itself changed, that is fine: the call site stays untouched). Other lanes of this wave
work in the ontology code (and its proposals widget), the voice code and the trust marks; the wave-1 lane works in
recall, the catalog and the index. Do not edit `docs/status.md` (the integrator writes it at the join).

## Your task: p3b-human-time, human durations everywhere through ONE shared formatter, and clock times without seconds (theseus-hnm37)

Branch: `cloud/20261024-p3b-human-time`.

Every commit subject carries `theseus-hnm37`. Deadline for the report: about 3 hours after you start.

**What the owner said (2026-10-24, looking at Ops "Coming up": `20:00:00 draft ml comms ... in 166m`):** "This should
be human time -- 166m (minutes) is less useful than 2h 46m, and 45h would probably be 1d 21h, etc".

**The rule (from the issue; build exactly this):**
| span | future | past |
|---|---|---|
| under 1 min | `in 40s` | `40s ago` (under 5 s stays `now`, as `ago()` does today) |
| under 1 h | `in 12m` | `12m ago` |
| under 1 d | `in 2h 46m` (drop `0m`: `in 2h`) | `2h 46m ago` |
| 1 d to under 7 d | `in 1d 21h` (minutes dropped; drop `0h`: `in 1d`) | `1d 21h ago` |
| 7 d or more | `in 9d` | `9d ago` |

A bare span (no "in"/"ago") follows the same rule: `2h 46m`, `1d 21h`, `9d`. Rounding: floor each unit (a span of
2h 46m 59s is `2h 46m`); a future span under 1 min but over 0 reads `in 40s`, never `in 0m`. Overdue future times keep
their existing words (for example `due 3m ago`) but with the rule's span.

**Clock times:** `HH:MM`, no seconds, 24-hour as today, with a day word when not today: `20:00` (today), `tomorrow
08:00`, `yesterday 17:10`, `Mon 09:00` (within the next/last 6 days), and `Oct 30 09:00` beyond that. This is for
schedule-like times (Coming up, due times, wakes, routines, agenda, calendar). Log-like timestamps where seconds
matter (a transcript's line times, event and ledger lists, the model inspector) may keep seconds: list each one you
kept and why.

### Part 1 (must): the cockpit

1. **The formatter** in `cockpit/src/lib/figures.ts` (pure, no imports, so `node --test` runs it; `lib/format.ts`
   re-exports it): add `span(ms)`, `until(at, now)` (`in …` / overdue words) and `when(at, now)` (the clock rule), and
   make `ago()` (:48) follow the rule. Change `ms()` (:20) so that from 1 h up it follows the rule (`2h 46m`, `1d
   21h`, `9d`; today it prints `45h 0m`); below 1 h it keeps its latency style (`340 ms`, `4.4 s`, `3m 12s`). Make
   `uptime()` (:37) follow the rule from 1 d up (`1d 2h`, minutes dropped). Leave `clock()` (:65) as the seconds
   clock for the log-like sites, and make `stamp()` use `when` only if every caller is schedule-like (check; else
   leave it). Export names exactly; the other cockpit rows will import them.
2. **Use it everywhere.** The sites a read-only look found (confirm each, find any it missed with
   `rg -n "60_000|60000|/ 60\b|toLocaleTimeString|hour12|m ago|h ago|in \{" cockpit/src`):
   - **Ops Coming up:** `cockpit/src/ops/Queue.tsx` `ComingUp` :134-157: the time at :144/:151 (`clock()` ->
     `when()`), the inline `in {Math.max(1, Math.round((at - now)/60_000))}m` at :146/:153 -> `until()`.
   - `ops/facts.ts` :80 (`due HH:MM:SS, in 45h 0m`), `components/RoutinesTable.tsx` :85, `views/Actions.tsx` :224,
     `lib/agenda.ts` :423 (it takes a formatter: pass the shared one).
   - Local copies that print a human duration: `lib/threadboard.ts` :132 `span`, `ship/watch.ts` :154 `span`,
     `ship/words.ts` :178 `span` (`3 m 12 s` style), `lib/progressLine.ts` :17 `left`, `lib/durability.ts` :32-36,
     `lib/books.ts` :57, `lib/taskgraph.ts` :42 (`29m left`), `lib/bench.ts` :527. Replace each with the shared
     helper, unless it is a different kind of figure (a latency, a benchmark's seconds): say which you kept and why.
   - Times of day: `views/Economics.tsx` :224, `components/EventDetail.tsx` :46 (schedule-like: `when`);
     `components/Transcript.tsx` :231, `components/ModelInspector.tsx` :238 (log-like: may keep seconds).
3. **Tests** (cockpit's runner is node's own test runner over `cockpit/test/*.test.ts`, type stripping, imports named
   with `.ts`; read `cockpit/AGENTS.md` ~:373-380): a new `cockpit/test/figures.test.ts` with the boundary cases:
   0 s, 4 s (`now`), 40 s, 59 s, 60 s, 12m, 59m 59s, 1h, 2h 46m (166 minutes: the owner's case), 23h 59m, 24h, 45h (`1d
   21h`: the owner's case), 6d 23h, 7d, 9d 5h (`9d`), each future and past; `when` for today, tomorrow, yesterday, a
   weekday within 6 days, and a date beyond (pass `now` explicitly and build dates in local time so the test does not
   depend on the VM's zone; or set `TZ` in the test the way other cockpit tests do: check). A test that `ComingUp`'s
   pure data path (or the function you extract from it) gives `in 2h 46m` for 166 minutes. Red at the base: P1, plant
   the old `ago()` back: red; P2, plant the inline `…m` back in Coming up: red.
   - Existing cockpit tests that pin the OLD strings (`3h ago`, `45h 0m`, `20:00:00` for a schedule time): update
     each expected string to the new rule (the expectation follows an owner-asked format change; this is not a
     weakening) and list every one in the report.
4. `test/ops-page.test.ts` fails if the Ops page adds a timer or refetch interval: add none (pass `now` from what the
   page already has).

### Part 2 (after Part 1 is green; stop and list it as a follow-up if time runs short): the Rust surfaces

The issue asks for the same rule in the TUI, herdr and the CLI where they differ. Each surface has its own helper
today: TUI `crates/theseus-tui/src/chat/banner.rs` :47 `ago` (`3 min ago`, `yesterday`), `chat/live.rs` :60/:76,
`card.rs` :267 (`expires in 4:12`), `chat/thread_ask.rs` :55; herdr `crates/theseus/src/herdr_slots.rs` :144 `span`
(`1 h 5 m`); CLI `crates/theseus/src/render.rs` :1341 `until_due` (`in 9m`, `in 2h`: it loses the minutes, the
owner's exact complaint), `render/wakes.rs` :98, `render/postmortem.rs` :175, `routine.rs` :380; core
`crates/theseus-core/src/wake.rs` :968 `span` (tested ~:1062), `narrative.rs` :229; protocol
`crates/theseus-protocol/src/notices.rs` :236 and `work/horizon.rs` :485 (identical copies).
- Add ONE Rust helper with the same rule (in `theseus-protocol`, which the CLI, TUI and daemon already depend on: put
  it in its own small module, not in `lib.rs`, which is near its ceiling), with the same boundary tests, and move the
  CLI's `until_due`, the herdr `span` and the TUI `ago` onto it.
- **Do NOT change any text that reaches Discord or the DM task flow:** `wake.rs`'s `span`, `notices.rs`,
  `work/horizon.rs` and `narrative.rs` may feed Discord messages. Check `crates/theseus-discord/src/golden/dm_flow.txt`
  and the Discord render goldens: if moving a helper would change any of them, leave that helper as it is and list it
  (`OWNER CALL: <helper, where its text reaches Discord, old and new>` if you think it should change).
- CLI goldens under `crates/theseus/tests/golden/` and TUI snapshots that move ONLY because a duration's format
  changed: rewrite them with `THESEUS_GOLDEN=write` (the crates' AGENTS.md names the way) and commit only those
  lines; list each.

## Not in this row

No new timer or refetch. No change to what is scheduled or when; only how a time or span is written. No Discord text.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. A fix that
  would change any message of that flow STOPS that part: write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove or weaken no test (an expected string updated to the new format is listed, not weakened); raise no
  budget, cap or ceiling; do not edit `scripts/keel-guard.py` or its tests; rename no test. Before the report run
  `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and paste its output.
- **Public repo:** no person's name, no real organisation, no address, no email, no key. Fixtures use invented content
  under `example.test`. Write "the owner".
- **Red first;** right over compatible (one helper, not a new one beside each old one).
- **The gate:** in `cockpit/`: `npm ci`, `npm run lint`, `npm test`, `npm run build`. For Part 2 also `cargo fmt
  --check`, `cargo clippy` on the touched crates with `-D warnings`, `cargo nextest run -p theseus-protocol -p theseus
  -p theseus-tui` (and `-p theseus-core` with `TZ=America/Phoenix` if you touched it), then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched), any ceiling, OWNER CALL or STOP.
(2) **The helper:** its exported names and signatures (the lane's other rows import them), and the rule table as
built. (3) **Sites changed:** every file:line moved onto it, and every site kept (with why: latency, log-like).
(4) **Tests** and plants P1-P2; every old-string expectation updated. (5) **Part 2:** done or not, the Rust helper and
the surfaces moved, the goldens moved, the helpers left because their text reaches Discord. (6) **Gate** commands and
results. (7) **Keel** output. (8) **DM golden** diff result. (9) **Join notes:** every file touched, and confirmation
that you touched none of the lane's other rows' files beyond untouched `ago()` call sites. (10) **Follow-ups.**
