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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3f-recall-now`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

**This is a second attempt.** A first session was given this same task on 2026-10-24 on another branch and pushed
nothing in four hours. Nobody knows yet whether it died at launch or mid-run, so this run carries one extra rule: the
**early push** below. Work only on your own branch, named below. Never fetch, read or push the first attempt's branch.

Your branch starts from `main` at `b69236c1`. That is install #82's gated tree: release 1.0.2 (signed tag
`release-1.0.2` at `ac298666`) plus twelve commits of joined patch3 work. Fetch it and branch from `b69236c1`. None of
those twelve commits touches `crates/theseus-core/src/recall/`, `turn/recall_step.rs`, `crates/theseus-exam` or the
core `AGENTS.md`, so every line number below still holds; search by name anyway. Where the release preamble above
disagrees with this note about the base, ceilings or the work in flight, this note wins.

**Early push (required, before any code).** Do setup first (`cargo --version`, nextest) and your first
`cargo build -p theseus-core`. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-mo1vm: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3f-recall-now`

After that, push after EVERY commit, not only at the end. A run that ends early must leave its work on the branch.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`, :688).
This row should need no store change: fact records already carry supersession, and statements carry their times. If a
part truly needs a bump, name it and justify it in your report's first section (the integrator renumbers it at the
join); do not bump for anything else.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines). At the
base: `crates/theseus-core/src/recall/temporal.rs` 560, `crates/theseus-core/src/recall/tests_time.rs` 343,
`crates/theseus-core/src/recall/render.rs` 463, `crates/theseus-exam/src/tender.rs` 969.

**Your lane: `integ/p3-first`** (the first 1.0.3 rows). Its other rows are already written and reviewed. You do not
need to read their branches.
- `p3a-recall-floor` (theseus-96cyh) sets the recall relevance floor's defaults and adds a note naming the subject
  beside a recalled same-kind value. **It did NOT touch `recall/render.rs`.** It built a new `recall/neighbour.rs`,
  and outside it changed only:
  - one line in `turn/recall_step.rs` `hold_node`, near :553-556 after the `moved` lookup:
    `header = crate::recall::neighbour::with_note(header, item);`
  - a last field, `neighbour: Option<String>` (`#[serde(skip)]`), at the end of `RecallItem` in
    `crates/theseus-protocol/src/memory.rs` (~:519);
  - `neighbour: None` in `recall.rs` `fill`'s item literal (~:1024), and a `neighbour::apply(...)` line after each
    of the two `fill(...)` calls (~:861 and ~:897).
- `p3a-kimi` (theseus-rgbtz) adds catalog and provider rows. `p3a-mem-spike` (theseus-xxac7) works in
  `crates/theseus-index`.

**Keep the join with `p3a-recall-floor` additive.** The real overlap is `turn/recall_step.rs::hold_node` (~:517-570).
- If your "superseded" mark goes into the frozen header there, put it in its own small function. Call it on its own
  line, before or after the `moved` lookup, and never rewrite the `header` lines.
- If you add a field to `RecallItem`, put it before `retention`, not at the end, and say whether `fill` sets it.
- If the rendered pack is built in `recall/render.rs`, put the mark there instead.
- Add your own paragraph to the recall part of `crates/theseus-core/AGENTS.md`. Do not rewrite the existing ones.

You own `recall/temporal.rs`, `recall/tests_time.rs` and the exam's probe changes. Do NOT touch `config/memory.rs`,
`recall/relevance.rs` or `recall/neighbour.rs`: the recall-floor row owns them. Do not edit `docs/status.md`: the
integrator writes it. In the report's join notes, name every hunk you own in `recall_step.rs`, `recall.rs` and
`memory.rs`, with its line range.

## Your task: p3f-recall-now, current-value recall for "now / currently / still" questions (theseus-mo1vm)

Branch: `cloud/20261025-p3f-recall-now`.

Every commit subject carries `theseus-mo1vm`. Deadline for the report: about 3 hours after you start.

**Why.** Every "time" item of the memory exam (`crates/theseus-exam/exam/exam-v2.toml`, the 8 `time-*` items near
:1671-2243) is a current-value question under supersession: a value that changed twice, with generated chatter that
repeats the stale values about 8 times each ("How many days of WAL segments do we keep now?" answer 21, stale 14 and
30; the gate timeout, answer 900; the pushgateway port, answer 9291). The failed check is always the same: the answer
is a stale value. On the last exam, recall did NOT beat no memory on this family (item means: none 38%, BM25 17%,
fused 33%, oracle 100%; held-out time items 0 of 4 for both recall arms). The 10-14 time fix (theseus-q1tbn) reads
"when / first / last / how long" questions and, by design, leaves alone any question about now:
`crates/theseus-core/src/recall/temporal.rs` `read()` (near :331-335) returns `None` whenever a token is in `CURRENT`
(`now`, `currently`, `nowadays`, `anymore`, :97). Every time item contains "now", so the fix cannot help this family.

**What to build.**
1. **Read a current-value question.** Teach `temporal::read` (or a sibling the recall pipeline calls beside it) to
   recognise a question about how a VALUE stands now: a `CURRENT` word (add `still` if it reads that way: "do we still
   keep 30 days?"; judge the false-positive risk and test it) together with a value-shaped ask (how many / how much /
   what is the / which port / what timeout ...). Route it to a new `Kind` (for example `Current`, your name), NOT to
   `None`. A "now" question that is not about a value (for example "what are we doing now?") keeps today's behaviour:
   test that it is unchanged.
2. **Rank the newest statement of the asked subject first.** For a `Current` question, among the recalled hits that
   state a value of the asked subject, order by the statement's time, newest first (use the existing time the record
   carries; reuse the `Latest` machinery near :526 where it fits rather than writing a second ranker). Where a fact
   record's supersession is known (`superseded_by` / `Supersedes` edges, `recall/adjacency.rs` :45-68, :311), prefer
   that over raw time. Hits that are not statements of the asked subject keep their order after them.
3. **Mark older statements superseded** in the rendered pack (`recall/render.rs`): an older statement of the same
   subject's value carries a short mark such as `(superseded: older than the 2026-..-.. statement)` (wording your
   call, kept short), so the model reads the newest value as current. The newest one carries no mark.
4. **Measure with the exam's gold probe (M2), model-free.** `theseus-exam probe` (`crates/theseus-exam/src/main.rs`,
   `Probe` near :165; it asks a scratch daemon's index tender, no model) gives, per family, how much gold ranks in the
   top k. Run it with `--k 6` on the time family before (at `b69236c1`) and after your change, both halves (choose on
   `in`, report `out` once), with the flag that reads each task as a turn does (theseus-q1tbn's; read its doc) so the
   new `Current` path is exercised. Arms: the BM25 + entity arm always; the vector and fused arms only if the
   embedding model is available in your session (say which ran). The gold for a time item is its NEWEST statement:
   check that the probe counts it that way; if it counts any statement of the value as gold, add a time-family read
   that requires the newest one at rank 1 (gold@1-newest) beside gold@6 and report both. Report a table: item,
   before rank of the newest statement, after rank.

**Acceptance (the issue's):** M2 gold@6 on the time items improves (newest statement in the top 6, and at rank 1
where it is recalled at all), then the time family scores at least as well as no memory on the GLM exam (M8). M8 is a
later local model run (after theseus-1j7ib), not this session: you have no key and must spend nothing. Say in one
line what M8 should run.

## Tests (each red at the base, with a planted revert)

1. `recall/tests_time.rs`: the 8 exam questions' shapes (invented wording and values, not copied from the exam:
   `example.test` subjects) read as `Current`; "what are we doing now?" and a "when did we first ..." question read
   as today (unchanged); a `still` case and its false-positive guard. Plant P1: put back the early `return None` on a
   `CURRENT` word: red.
2. Ranking: three statements of one subject's value at three times (oldest repeated most, as the exam's chatter does)
   plus one other subject's value: the newest ranks first, the other subject after. With a supersession edge that
   disagrees with raw time, the edge wins. Plant P2: drop the newest-first order: red.
3. Render: the older statements carry the superseded mark, the newest none; a pack with no `Current` question renders
   byte-identical to the base. Plant P3: drop the mark: red.
4. If you add a probe read (gold@1-newest), a test for it in `crates/theseus-exam`.
- Keep green: every existing recall, temporal and exam test, `tests_time.rs` as it stands.

## Not in this row

No change to `config/memory.rs`, `recall/relevance.rs` or the relevance floor (that is `p3a-recall-floor`). No change
to the exam's items or their gold (`exam-v2.toml` is read-only here). No model call, no key, no spend. No Discord
change.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code b69236c1 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. A fix that
  would change any message of that flow STOPS that part: write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove or weaken no test; raise no budget, cap or ceiling; do not edit `scripts/keel-guard.py` or its
  tests; rename no test; edit no golden. Before the report run
  `python3 scripts/keel-guard.py --base b69236c1 --head HEAD --quiet` and paste its output (it must be empty).
- **Data floor:** nothing new is sent to a model or routed anywhere; only the order and marks of already-recalled text
  change.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented subjects and values
  under `example.test`. Write "the owner".
- **Red first;** right over compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` "Building and checking" first):
  `cargo fmt --check`, `cargo clippy` on the touched crates with `-D warnings`, `cargo nextest run` (or `cargo test`)
  for `theseus-core`, `theseus-exam` and `theseusd`'s recall tests; then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched, or a bump named and justified), any
ceiling, OWNER CALL or STOP. (2) **The read:** the `Current` rule, the words, the false-positive guards, at file:line.
(3) **Ranking and marks:** how newest is decided (time vs supersession), an example pack before and after. (4) **M2:**
how the probe ran, which arms, the per-item table before and after, both halves. (5) **Tests** and plants P1-P3, red
at the base and green after. (6) **Gate** commands and results. (7) **Keel** output. (8) **DM golden** diff result.
(9) **Join notes:** every file touched, and the hunks in `recall/render.rs` and `crates/theseus-core/AGENTS.md` (line
ranges) so the join with `p3a-recall-floor` stays additive. (10) **What M8 should run.** (11) **Follow-ups.** (12) One
line on the data floor.
