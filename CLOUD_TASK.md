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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3i-wakes-and-persona`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `0fda4d57`. That is install #83's gated tree: release 1.0.2 (signed tag
`release-1.0.2` at `ac298666`) plus the joined patch3 work. Fetch it and branch from `0fda4d57`. Line numbers below
were read at `0fda4d57` and are right unless a line moved; search by name anyway. **Where the release preamble above
disagrees with this note about the base, the store format, ceilings, the keel or the work in flight, this note wins.**
The preamble above is older than your base: ignore its store-format numbers, its list of changes in flight, and its
sentence that lets you raise a line ceiling. The rules here replace them.

**Early push (required, before any code).** Do setup first (`cargo --version` alone, then nextest) and your first
`cargo build -p theseus-core`. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-o1h3f.8: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3i-wakes-and-persona`

After that, push after EVERY commit, not only at the end. This is mandatory, not advice: on 2026-10-24 three of
sixteen rows ended with nothing but their task commit on the branch, and because every other row pushed only at the
very end, a session that died at minute 5 and one that worked for two hours left byte-identical branches. **A routine
that fires and pushes nothing is indistinguishable from a dead one.** This row is XS: if it is done in an hour, push
it and write the report. If it fails, push the failed attempt with your findings.

**Store format at your base: 51**; the kernel's `SCHEMA` is 2. This row touches neither.

**Line ceilings — never raise one** (`scripts/long-files.txt`). None of your files is near one:
`crates/theseus-core/src/config/wake_limits.rs` is 34 lines, `crates/theseus-core/persona/default.md` is 5 lines,
`crates/theseus-kernel/src/wake_kinds.rs` is short.

## Why this row exists

The owner's standing line, 2026-10-24 21:52: *"we should be very very lightweight on any limits to theseus tasking
itself based on what it sees and what we are working on. notify, work, and allow feedback is the model."* He answered
the design's questions **"All recs!"** (10-24 23:47) and **"Yes to all"** (10-25 09:54).

Two small things hold Theseus back from that model, and they are the cheapest two in the whole audit:

1. **`max_wakes` = 5** (design §1.1 item 14). A conversation may hold at most five check-ins. Theseus scheduling its
   own follow-ups runs out at five, and the daemon-wide `max_reminders` (100) is the real flood bound anyway.
2. **The built-in persona has no act-then-tell line** (design §1.6 item 45). A persona with no file of its own falls
   back to `crates/theseus-core/persona/default.md`, a text that nowhere gives the model permission to act first.
   The owner's own persona file has the sentence; the built-in default does not, so every fallback persona is more
   timid than he asked for. Nothing else in a tool description tells the model to ask first — `owner.ask`'s own
   description already says "otherwise decide and say what you decided" — so this one text is the gate.

## Your task: p3i-wakes-and-persona (theseus-o1h3f.8, o1h3f row H)

Branch: `cloud/20261025-p3i-wakes-and-persona`.

Every commit subject carries `theseus-o1h3f.8`. Deadline for the report: about 1.5 hours after you start.
**Your specification is the **o1h3f design check** (the maintainer holds it; it is not in this repository), section 4 row H, with §1.1 item 14
and §1.6 item 45 as its reasoning.** You do not have that file — its content is reproduced below. Do not re-derive
the design; build what is written here.

### What to build

**1. `max_wakes` 5 → 20.**

There are **three** sites, and the design named only the first. Find and handle all three:

- `crates/theseus-core/src/config/wake_limits.rs` :14-16, `pub(super) fn default_max_wakes() -> usize { 5 }` — the
  config's default. This is the one the design names.
- `crates/theseus-core/src/config/wake_limits.rs` :3, the module's own doc comment, which says "`max_wakes` (5
  check-ins a conversation)". Keep the prose true.
- `crates/theseus-kernel/src/wake_kinds.rs` :153, `max_wakes: 5` in `WakeLimits`'s `Default`. **Read it before you
  change it.** The config's `limits()` (`wake_limits.rs` :25-33) builds `WakeLimits` from the config section and takes
  `max_routines` from `WakeLimits::default()`, so the kernel default is the value used where no config section is
  built — tests, and any caller that defaults. Decide honestly whether the kernel's default should move too, say
  which you chose and **why**, and make the two agree or explain why they should not.
- `crates/theseus-core/config/theseus.example.toml` :499,
  `max_wakes = 5                           # check-ins (wake.at) one conversation may hold at once; at least 1`.
  Update the number and keep the column alignment of the comment exactly as the surrounding lines have it.

The enforcement site is `crates/theseus-kernel/src/wakes.rs` :256-258 (`if held >= limits.max_wakes`) and the limit
is read through `KernelConfig::wakes`. **Change no logic there** — `.max(1)` stays, `max_reminders` (100) stays as
the daemon-wide flood bound, `max_watches` (20) stays, and `max_wake_ahead_days` (365) stays.
`crates/theseus-kernel/src/tests_wake_owner.rs` :389 sets `max_wakes: 2` explicitly and must keep passing unchanged.

**The keel question, which you must answer with the guard and not with your own judgement.** The design checked this
and expects **no finding**: the guard's `cap-raised` rule (see `scripts/keel-guard.py`'s docstring) flags a
`fn default_*` naming **usd, spend, per_day, daily, day_ceiling, max_loops or max_turns**, and a
`theseus.example.toml` key naming **spend, usd, per_day, daily or max_loops**. `default_max_wakes` and `max_wakes`
name none of those. **So no `Keel:` trailer is owed for this half.** But run the guard and paste its whole output
anyway:
`python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet`.
**If it flags this change: STOP, put 5 back, and write `OWNER CALL: <the guard's exact line>` in the report.** You
cannot sign, so you cannot ack, and **you may not grant yourself a trailer** — if the guard disagrees with the
design, the guard wins and the owner decides.

**2. The act-then-tell line in `crates/theseus-core/persona/default.md`.**

The file is five lines: a first paragraph ("You are Theseus, a durable agent…"), a `Voice.` paragraph, and a
`Values.` paragraph. Add the owner's own sentence, **with "the owner's" in place of "his"**:

> Act, then tell: anything reversible and within your permissions, do it and report; ask only for what is genuinely
> the owner's.

- It belongs in the **`Values.` paragraph**, as a sentence beside "Lead with what you found or did. A promise to act
  later is scheduled in the same turn." Match the paragraph's voice and punctuation exactly.
- **Add this sentence and nothing else.** No rewording of any existing sentence, no reflow of the file, no change to
  the `Voice.` paragraph. See "Shared files" below: another row is changing the `Voice.` paragraph's `Spoken aloud:`
  line right now, and your branch must touch no part of that line.

**The goldens this moves.** The persona's text is compiled into the system, so three places hold a copy at your base:

- `crates/theseus-core/tests/golden/private_requests.txt` — the compile golden. It will move.
- `crates/theseus-core/src/tests_persona.rs` and `crates/theseus-core/src/tests_engage.rs` — both assert on the
  persona's words.

Run every golden. **Each golden that moves gets its own commit**, with the old and new line quoted in the commit
message and in the report. **I checked that `crates/theseus-discord/src/golden/dm_flow.txt` does NOT hold the
persona's text** (it contains neither "durable agent" nor "Spoken aloud"), so this row does **not** touch the locked
DM flow and **owes no `Keel:` trailer for the persona half either**. Prove that with
`git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result.

### Tests (each red at the base, with a planted revert)

1. **The limit.** A conversation may hold 20 check-ins and the twenty-first is refused with the existing refusal; a
   config that sets `max_wakes` explicitly still wins; `.max(1)` still floors a 0. Plant P1: put 5 back: red.
2. **The template.** The example toml's line parses, and the template test that un-comments every line still passes
   with the new number. Plant P2: leave the template at 5 while the default is 20 (the template tests should catch the
   disagreement; if they do not, say so — that is a finding worth reporting): red.
3. **The persona's line.** The compiled system for a persona with no file of its own contains the act-then-tell
   sentence, and the `Voice.` paragraph is **byte-identical to the base**. Plant P3: drop the sentence: red.
   The byte-identical half of this assertion is what protects the other row's work; do not leave it out.
4. **Nothing else moved.** A golden test proves the persona file's other two paragraphs are unchanged. Plant P4:
   reword one existing sentence: red.
- **Keep green:** the whole `theseus-core` and `theseus-kernel` suites, every golden, and `theseusd`'s wake tests.
- If an existing test has to change its expectation, **do not delete or weaken it**: rewrite it with the same number
  of assertions, keep its name, and list it under **Keel findings expected** with its line and reason.

### Not in this row

No `TaskOrigin`, `self_started` or `because` (row A). No `task.not_this`, no Ops or cockpit change, no `.tsx` edit
(row B). No tools-deferral change (row C). No admission or burn change (rows D and E). No RSI change (row F). **No
Discord change of any kind** (row G, and the DM task flow is locked). No change to `max_reminders`, `max_watches`,
`max_routines` or `max_wake_ahead_days`. No new config key. No new dependency. No benchmark, no spend, no AWS, no
model call.

## Shared files: fires are parallel, joins serialise

Eleven other patch3 rows are in flight right now against this same `0fda4d57`. That is deliberate: the review queue,
not the build, is our bottleneck, so rows fire concurrently and the integrator serialises the joins.

**Your shared file is `crates/theseus-core/persona/default.md`, and the concurrent row is `theseus-dgbcx`.** dgbcx is
changing the `Voice.` paragraph's **`Spoken aloud:`** line — two owner-approved join fixes are being applied to it
right now and gated as a merge onto main.

- **Write your row against `main 0fda4d57` as if you were the only one editing this file. Do NOT try to coordinate
  with dgbcx, do NOT stub, and do NOT guess at its new wording.** Your addition is additive and on a different line
  from theirs, which is exactly why both can fire at once.
- **Your join goes AFTER `theseus-dgbcx`**, and at the join the integrator takes `persona/default.md` from the new
  main — by then its `Spoken aloud:` line will carry the owner's longer wording — and applies your one added
  sentence on top. **Put that line in your report.** The only thing that makes that join a one-minute job instead of
  a guess is that your diff of this file is a single added sentence in a different paragraph: keep it that way.
- The same applies to the three goldens that hold the persona's text: dgbcx moves the same goldens' `Spoken aloud:`
  lines. **In the report, quote your golden diffs line by line**, so the joiner can tell your lines from dgbcx's at a
  glance.
- Nothing else you touch is shared: `config/wake_limits.rs`, `wake_kinds.rs`, `wakes.rs` and the example toml are
  yours alone in this wave. Say so.
- If you find you need to edit anything outside this list — in particular `crates/theseus-core/src/task.rs`,
  `cockpit/src/views/Ops.tsx`, or anything under `crates/theseus-discord/` — **stop, leave that part out, and write
  `JOIN SEAM: <file> — <why I needed it>` in the report.**
- **List every hunk you place, with the file, the line range and why**, in the report's join notes.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical; prove it as
  above.
- **Keel.** Run `python3 scripts/keel-guard.py` before every commit and
  `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` before the report, and paste the whole output.
  The design's reading is that this row owes **no** `Keel:` trailer, for either half. **If the guard says otherwise,
  the guard wins: STOP that part, restore the old value, and write `OWNER CALL:` with the guard's exact line.** Never
  add a `Keel:` trailer yourself: an unsigned ack counts for nothing and a branch may not ack itself.
- **This row raises a behaviour limit (5 → 20) by design, so say that plainly and prominently in
  `CLOUD_REPORT.md`** — joins quietly undo caps and two lanes can each move the same one. Name the old value, the new
  value, every site, and what still bounds the behaviour (`max_reminders` 100, daemon-wide).
- **Data floor:** the persona's sentence is text compiled into the system prompt; nothing new leaves the machine.
- **Public repo:** no person's name, no real organisation, address or email. Write "the owner" — note that this is
  exactly why the sentence says "the owner's" and not "his".
- **Red first** (each test red at `0fda4d57`, green after; prove each with its planted revert, restoring the file and
  `touch`ing it so cargo rebuilds it, and run `git status` after every restore); right over compatible.
- **The gate:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, and `cargo nextest run -p theseus-core -p
  theseus-kernel` plus every other crate whose files you touched. Then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, **never a shell heredoc**, and check
after the commit that it is not empty. Push the branch. End with a short final message that starts
`CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched) and `SCHEMA` (2, untouched); ceilings; the
raised limit stated plainly; any `OWNER CALL:`, `JOIN SEAM:` or STOP. (2) **`max_wakes`:** every site you changed with
its `file:line`, old and new, **including what you decided about the kernel's own `WakeLimits::default()` and why**,
and what still bounds the behaviour. (3) **The persona:** the added sentence quoted, where in the file, and a diff of
the whole file showing that only one sentence was added. (4) **Goldens:** each one that moved, with the old and new
line quoted, and its own commit hash. (5) **Tests** and plants P1-P4, red at the base and green after, with counts.
(6) Any existing test whose expectation you rewrote, under **Keel findings expected**. (7) **Gate** commands and
results. (8) **Keel** output, whole, with your reading of it. (9) **DM golden** diff result. (10) **Join notes:** the
line that your join goes after `theseus-dgbcx` and takes `persona/default.md` from the new main; every file touched
with line ranges; your golden diffs quoted line by line; and a statement that you edited nothing under
`crates/theseus-discord/`, no `.tsx` file, and not `task.rs`. (11) **Follow-ups.** (12) One line on the data floor.
