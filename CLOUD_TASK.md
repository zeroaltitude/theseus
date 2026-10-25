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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3b-busy-wheel`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
Do not bump it in this row (see Part 3: a batching that needs a new stored layout becomes a follow-up).

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every Rust file not listed there caps at 2,500 lines).
At the base: `crates/theseus-core/src/rpc/people.rs` 456, `rpc/proposals.rs` 838, `rpc/ontology.rs` 626,
`crates/theseus-core/src/ontology.rs` 525, `crates/theseus-store/src/wal.rs` 2,443 (57 lines of room: add nothing
there); `cockpit/src/components/People.tsx` 194, `components/OntologyParts.tsx` 66.

**Your lane: `integ/p3-cockpit`.** Its other rows, fired at about the same time on the same base:
- `p3b-human-time` (theseus-hnm37) OWNS the shared time formatter `cockpit/src/lib/figures.ts` (new `span`, `until`,
  `when`; `ago`, `ms`, `uptime` follow a human rule). Do not edit `figures.ts`; if you show a duration, import its
  names (they may not exist at your base: then show none, and say so).
- `p3b-context-chart` (theseus-11cvd): `cockpit/src/views/Context.tsx`'s `Timeline` and `lib/explorer.ts`. Not yours.

**ANOTHER LANE edits the same widget: read this.** `p3b-onto-never` (theseus-648f8, lane `integ/p3-onto`) adds Reject
and **Never** controls to the Ontology proposals panel (`cockpit/src/components/People.tsx` `ProposalsPanel`
:123-193): per-row buttons on topic rows (a new cell after the confidence cell, ~:185), a Never beside the person
rows' ✓/✗ (:175-178), and bulk Reject/Never buttons placed immediately AFTER the header's `Accept` `<Act>` (:161).
To keep the two merges clean:
- In `People.tsx` you change ONLY: the two checkbox `disabled=` attributes (:169 and :183: add the busy lock), the
  existing person-row buttons' `disabled` if needed (they already read `busy`), and ONE new element for the progress
  line, placed right after `<Refused words={refused} />` (:163). Do not restructure the table, the header row, or
  `answer()` beyond what Part 2 needs; if `answer()` must change for Part 3's batching, keep the change inside it and
  name the lines in the report.
- The busy state lives in `useOntologyWrite` (`OntologyParts.tsx` :23-49); the onto row will read `busy` from it for
  its new buttons, so keep the hook's existing return names (`write, writeEach, busy, refused, clear`) and only ADD
  fields (for example `progress`).

Other lanes of this wave work in the voice code and the trust marks; the wave-1 lane works in recall, the catalog and
the index. Do not edit `docs/status.md` (the integrator writes it at the join).

## Your task: p3b-busy-wheel, one ship's-wheel busy indicator, a proposals panel that locks while it files, and why accepts are slow (theseus-6o8q8)

Branch: `cloud/20261024-p3b-busy-wheel`.

Every commit subject carries `theseus-6o8q8`. Deadline for the report: about 3 hours after you start.

**What the owner said (2026-10-24):** "when accepting, it takes it a while to add them -- right now, the proposal
widget no longer shows an accept button, but and that's disabled, so good, but it does leave the checkboxes
check/uncheckable, and, it should show some kind of header rotating wheel animation (making it a ships wheel would be
amazing -- if one is possible, use throughout for waiting or working) that shows work is being done".

### Part 1: one ship's-wheel busy component, used everywhere for waiting or working

- New `cockpit/src/components/ShipsWheel.tsx`: an inline SVG ship's wheel (a rim, a hub, eight spokes with handles
  past the rim: it should read as a helm at 12 px), rotating by CSS. Sizes `sm` (12 px, inline in buttons and lines)
  and `md` (16-18 px, panel headers). `role="status"`, `aria-busy="true"` and an `aria-label` (default "working",
  overridable: "filing 3 of 12"). Colour from the current text colour (`currentColor`) so it fits each theme.
- Motion: it must stop rotating (and still show) under `prefers-reduced-motion: reduce` AND under the cockpit's Calm
  / unstirred modes (`cockpit/src/index.css` :517-524 switch off only `live-sweep`, `animate-ping` and
  `animate-pulse-soft` today; a new animation must be added to that list or it keeps spinning in Calm). Read how
  those modes are set and follow the same pattern.
- Replace the ad-hoc busy indicators with it: `components/Heartbeat.tsx` :386 (`animate-spin` on a `RefreshCw`, the
  only `animate-spin`), and the ontology `Act` control (`OntologyParts.tsx` :59-65), which shows `…` while busy: show
  the wheel (`sm`) and keep the button's label beside it. Search for any other "waiting/working" indicator
  (`rg -n "animate-spin|animate-pulse|Loader|Spinner|…'|'\.\.\.'" cockpit/src`) and move each that means "work is in
  flight" onto the wheel. Leave the pulses that mean "live" (for example `Shell.tsx` :197/:221,
  `TimeMachine.tsx` :261, `Transcript.tsx` :470) unless they mean waiting: list each you kept and why.

### Part 2: the proposals panel locks while an accept or accept-all runs

- While `busy`: every checkbox (person rows :169, topic rows :183) is disabled, "select all" (:160) is disabled, the
  kind filters (:158) may stay live (they only filter the view); the person rows' ✓/✗ are already disabled by `busy`.
- The panel header shows the wheel (`md`) and a progress line: `filing N of M` where M is the number of proposals
  being filed and N how many are done. `writeEach` (`OntologyParts.tsx` :26-48) sends its calls one after another;
  report progress per call (add a `progress` field to the hook's return), and if Part 3 makes one call carry many
  proposals, report what the daemon tells you (or show `filing M` without N if the call gives no progress: say so).
- When the write ends (done or refused), the lock clears and the picked sets clear as today (:148).
- Test (cockpit's runner is node's own test runner over `cockpit/test/*.test.ts`; a module it tests must be pure, with
  `import type` only from the protocol and imports named with `.ts`: read `cockpit/AGENTS.md` ~:373-380): extract the
  lock/progress state into a small pure module (for example `cockpit/src/lib/filing.ts`) and test it: busy -> every
  pick control disabled; progress counts 0..M; refusal clears busy. Red at the base (the module or its rule does not
  exist / the checkbox ignores busy). Plant P1: drop `busy` from the checkbox rule: red.

### Part 3: why accepts are slow (a FINDING for the report, and a fix if it is safe)

A read-only look before this task found (confirm each with the code, and measure if you can):
- The cockpit: `answer()` (`People.tsx` :134-149) sends ONE `ontology.proposal.accept_all {min_confidence:0,
  judgments:[...topics]}` for all picked topics, plus ONE `accept_all` (or `reject_all`) PER picked person row
  (`rowAccept`/`rowReject`, `lib/people.ts` :95/:99), awaited one after another (`writeEach`, :35).
- The daemon: `ontology.proposal.accept_all` dispatches at `crates/theseus-core/src/rpc/ontology.rs` :394-412 to
  `ontology_proposal_accept_all` (`rpc/people.rs` :291), which calls `judge_act` once (:304-310; a permission check
  that writes only on refusal, `rpc/confirms.rs` ~:705), then `listed_now()` (:267) -> `proposals_listed` ->
  `proposals_scan` (`rpc/proposals.rs` :236), which **re-scans three ledger scopes from position 0 on every call**.
- Then PER PROPOSAL: `accept_bulk` (`people.rs` :373) -> `accept_open` (`proposals.rs` :442): a session read, a
  snapshot, **one `runner.ontology.write(...)`** (~:520; `ontology.rs` :97-121), which **clones the whole ontology**
  and does **one `store.append` of a single frame**, which **fsyncs** when fsync is on (`theseus-store/src/wal.rs`
  ~:816), then two or three `announce` notifications.
So accepting M proposals costs about M frames, M fsyncs, M full-ontology clones, and (for person rows) one full ledger
scan per row. Your job:
1. **Measure** in a test (or a bench-free timing test that only counts): the number of `store.append` calls (or
   frames) and ledger scans for an accept_all of, say, 50 topic proposals and 10 person rows. Report the counts at the
   base. Do not add a timing assertion (wall time is flaky); count operations.
2. **Fix what is safe:**
   - **Cockpit side:** send all picked person rows in as few `accept_all` calls as the RPC allows (read its params:
     if one call can carry several people's judgments with their first names, use one call; if not, say why).
   - **Daemon side:** stage every proposal of one `accept_all` and write the ontology ONCE (one frame, one fsync, one
     clone): `Board::stage` (`ontology.rs` ~:133) is the nearest building block. Keep the per-proposal ledger rows and
     announcements exactly as today (the record of each accept must not change), and keep the confirm/refusal
     behaviour. Do it ONLY if the single frame is an existing frame kind (no new stored layout). If batching needs a
     new frame layout or a store-format bump, STOP the daemon half: write the design (what frame, what changes) in
     the report as a follow-up row, and ship the cockpit half and the counts.
   - Scan once per call: `listed_now()` should not re-scan per row; if a person-row batch removes the repeated scans,
     say so.
3. A test that counts: after the fix, an accept_all of 50 topic proposals is ONE ontology write; each accepted
   proposal still has its ledger row and its effect (the topic/person is in the ontology after). Red at the base
   (50 writes). Plant P2: restore the per-proposal write: red. Keep green every existing proposals test
   (`crates/theseus-core/src/tests_categorize.rs` ~:813 new-topic accept/reject, the people proposal tests under
   `judge/people/`, and the `rpc/proposals.rs`/`rpc/people.rs` tests).

## Not in this row

No new Reject or Never control (the onto row owns those). No change to what an accept records or announces. No new
timer or refetch. No store-format bump.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. A fix that
  would change any message of that flow STOPS that part: write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove or weaken no test; raise no budget, cap or ceiling; do not edit `scripts/keel-guard.py` or its
  tests; rename no test. Before the report run `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet`
  and paste its output.
- **Cockpit data rule:** nothing deleted, no proposal silently dropped; the lock never hides an error (a refusal's
  words still show).
- **Public repo:** no person's name, no real organisation, no address, no email, no key. Fixtures use invented content
  under `example.test` (invented people and topics). Write "the owner".
- **Red first;** right over compatible.
- **The gate:** in `cockpit/`: `npm ci`, `npm run lint`, `npm test`, `npm run build`; after the change load the
  Ontology view and one view with the old spinner from a scratch daemon if the cockpit's AGENTS.md shows how, and say
  whether you could. Rust: `cargo fmt --check`, `cargo clippy -p theseus-core -D warnings`, `cargo nextest run
  --workspace -E 'package(theseus-core)'` with `TZ=America/Phoenix`, then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched), any ceiling, OWNER CALL or STOP.
(2) **WHY ACCEPTS ARE SLOW:** the counts at the base (frames, fsyncs, clones, scans, RPCs) for the measured case, the
cause in one paragraph, and the counts after. (3) **The fix:** cockpit and daemon halves, done or follow-up (with the
design if follow-up). (4) **The wheel:** the component's API (props), the motion rules, and every site moved onto it
or kept. (5) **The lock:** what is disabled while busy, the progress line. (6) **Tests** and plants P1-P2. (7) **Gate**
commands and results. (8) **Keel** output. (9) **DM golden** diff result. (10) **Join notes:** every file touched,
and the exact `People.tsx` and `OntologyParts.tsx` lines you changed (the onto lane's row edits the same panel).
(11) **Follow-ups.**
