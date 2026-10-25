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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3i-rsi-notify`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
- `git commit --allow-empty -m "theseus-o1h3f.6: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3i-rsi-notify`

After that, push after EVERY commit, not only at the end. This is mandatory, not advice: on 2026-10-24 three of
sixteen rows ended with nothing but their task commit on the branch, and because every other row pushed only at the
very end, a session that died at minute 5 and one that worked for two hours left byte-identical branches. **A routine
that fires and pushes nothing is indistinguishable from a dead one.** If your work fails, or you run out of time
mid-step, push anyway, with your findings in `CLOUD_REPORT.md`: a branch with a failed attempt and an honest report is
worth far more to us than a silent one.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs` :688).
This row stores no new record field, so you should not need to touch it. If you find you do, bump by exactly one from
51, add the literal sample of the old layout to `theseus-core`'s `tests_layouts.rs` as the root `AGENTS.md`'s "store's
version rule" says, and name and justify it in your report's first section (a format bump is known to move two
goldens, so say which). The kernel's `theseus_kernel::types::SCHEMA` (2, `types.rs` :33) is **not** yours to touch.

**Line ceilings — never raise one** (`scripts/long-files.txt`; every Rust file the list does not name fails past
2,500 lines). Measured at your base: `crates/theseus-core/src/turn.rs` 3,498 of 3,525, `crates/theseus-discord/src/
runtime.rs` 3,496 of 3,500 (**four lines left in the whole file**), `crates/theseus-discord/src/render.rs` 2,982 of
3,009, `crates/theseus/src/render.rs` 3,079 of 3,128, `crates/theseus-protocol/src/lib.rs` 2,612 of 2,733,
`crates/theseus-core/src/tests_m3.rs` 7,982 of 8,050. Your own files are nowhere near a ceiling:
`rsi/pipeline.rs` 726, `rsi/pipeline/queues.rs` 373, `rsi/join.rs` 425, `rsi/backlog/start.rs` 547,
`rsi/backlog/word.rs` 351. Put new tests in a `tests_<subject>.rs` of their own rather than growing a long file.

## Why this row exists

The owner's standing line, 2026-10-24 21:52: *"we should be very very lightweight on any limits to theseus tasking
itself based on what it sees and what we are working on. notify, work, and allow feedback is the model."* He then
answered the design's questions **"All recs!"** (10-24 23:47) and **"Yes to all"** (10-25 09:54).

Self-improvement already acts and tells after. But three of its paths **stop and wait for a person who never comes**,
and one path works in silence:

1. The pipeline's starts say nothing to the owner until the weekly digest. The night's starts already have a line
   (`rsi/backlog/start.rs` :443-454 stages a `self_digest` post with `night_text`); a pipeline tick that starts a
   build stages nothing.
2. An item two independent reviews rejected is **parked** `waiting_human` and never tried again
   (`rsi/pipeline.rs` :393-411, `rsi/pipeline/queues.rs` :351-360). Parked items wait for a word the owner does not
   give, so in practice the item is dead with no notice.
3. A self-join that **conflicts with main waits for the owner and is never tried again** (`rsi/join.rs` :239), even
   though the right answer is mechanical: cut a fresh branch from the new main next time.
4. `backlog_starts` = 1 a night (`config/self_improve.rs` :81-82, :176) is moot while the pipeline runs, and holds
   the night back when it does not.

The owner's model is **fail open with a notice, never park**. This row is that, for RSI. The one-tap stop already
exists on every path it opens (`self halt` is anyone's, `rsi/switch.rs`; `theseus self drop <item>`), so loosening
here is safe today, which is why this row needs nothing else to land first.

## Your task: p3i-rsi-notify, RSI tells the owner and never parks (theseus-o1h3f.6, o1h3f row F)

Branch: `cloud/20261025-p3i-rsi-notify`.

Every commit subject carries `theseus-o1h3f.6`. Deadline for the report: about 3 hours after you start.
**Your specification is the **o1h3f design check** (the maintainer holds it; it is not in this repository), section 4 row F, with section 1.3
items 18, 25, 28 and 29 as its reasoning.** You do not have that file — its content is reproduced below. Do not
re-derive the design; build what is written here.

Read first: `crates/theseus-core/src/rsi/mod.rs` (the module's map), `rsi/pipeline.rs` whole (its head comment
:1-60 draws the state machine; `pipeline_tick` at :318; the transitions that start nothing at :353; what the reviews
moved at :389-435), `rsi/pipeline/queues.rs` (its head comment :1-20 names **ready**, **held** and the tick's moves;
the rejected-count read at :95-107; the parking rows at :291-360), `rsi/join.rs` (:23-25 the keel check, :239 the
conflict, :347 how a `self_join` post is staged), `rsi/backlog/start.rs` (:443-454 the night's `self_digest` stage,
`night_text` at :475), `rsi/backlog/word.rs`, `rsi/backlog.rs` ("Filing" — a cause the owner dropped is never filed
again — and `drop`), `rsi/switch.rs`, and `crates/theseus-core/src/config/self_improve.rs` (:19, :81-82, :176).

### What to build

**1. A `self_digest` line per pipeline tick that started something** (design §1.3 item 18).

`pipeline_tick` (`rsi/pipeline.rs` :318) stages no owner line today. When a tick **started** at least one build, stage
one `self_digest` post, through the same `outbox.stage_to_operator` path the night uses
(`rsi/backlog/start.rs` :454: `stage_to_operator(None, json!({"kind": "self_digest", "text": text}))`), **reusing
`night_text`** (`rsi/backlog/start.rs` :475) so the wording is the one the owner already knows. If `night_text`'s
input type does not fit a tick, factor the smallest shared helper rather than writing a second wording — one wording,
one place. A tick that started nothing stages nothing: no empty notes.

- This is **not** a task message and **not** the locked DM flow. `self_digest` is already a kind the courier knows
  (`crates/theseus-discord/src/courier.rs` :900) and it posts to the owner's DM through `jev_post`. **You add no
  Discord code at all** (that is row G, a separate issue): you stage the post, and the existing binding renders it.
- One line per tick, not one per item. If a tick started three builds, the line names three.

**2. Twice-rejected: dropped with the findings and a way back, never parked** (design §1.3 item 28).

Today `rsi/pipeline.rs` :393-411 writes `parked:rejected-twice` with "two reviews rejected it: it waits for the
owner's word", and `queues.rs` :351-360 shows that row. Replace the park with a **drop plus a notice**:

- the item leaves the pipeline as dropped, not `waiting_human`;
- a notice to the owner carries **both reviews' findings** (the evidence `queues.rs` :95-107 already reads: the
  reviews that rejected it and the last one's note) and **the exact command that brings it back**;
- nothing waits. The fail-open rule (the owner, 2026-10-19) is that Theseus never stops on a person's silence.

**The way back.** The design writes it `theseus self revive <item>`. **There is no `revive` verb at your base** — I
checked: `crates/theseus/src/self_cmd.rs` :34 `enum SelfCmd` has `Halt` (:47), `Resume` (:52), `Digest` (:54),
`Backlog` (:56), `Drop` (:62), `Move` (:67), `Retry` (:73), and no `Revive`. **Before you add one, read `Move` and
`Retry`.** If either already returns a dropped or parked item to `proposed`, **use it and say so in the report** —
name the verb and quote its help line — rather than adding a near-duplicate. Only if neither does, add `Revive`
beside them, with its RPC, in the shape `Drop` uses (`self_cmd/backlog.rs`). Either way the notice must quote the
**real** command a reader can type, and a test must assert the notice's command string against the verb that exists.

**3. A join conflict returns the item to `proposed`, with a notice** (design §1.3 item 29).

`rsi/join.rs` :239 — "conflict with main waits for the owner: never tried again". Instead: return the item to
`proposed` and stage a notice saying the branch conflicted with main and the next build will cut a fresh branch from
the new main. The next pipeline tick then rebuilds it against the moved main, which is the mechanically right answer.

- **Do not touch `rsi/join.rs` :23-25, the self-join keel check**, or :29-30 (no signing key). Those stay exactly as
  they are: they are keel (design §1.3 items 30 and 31) and this row keeps every keel gate.
- A conflict must not loop: an item that conflicts, returns to `proposed`, rebuilds and conflicts again must not
  spin forever. State plainly in the report what bounds it (the existing build capacity of one at a time,
  `config/self_improve.rs` :144, plus the budget; or a count you add). If nothing bounds it, add the smallest bound
  that does and name it.

**4. `backlog_starts` follows the pipeline's free slots** (design §1.3 item 25, "low value: fold into row F").

`backlog_starts` (`config/self_improve.rs` :81-82, default 1 at :176) caps the night's starts. Default it to the
pipeline's free build slots instead of the literal 1, keeping the config key so a set value still wins.

- **This is the one half of this row that could read as a raised cap, so handle it carefully.** `default_backlog_starts`
  names none of the guard's cap words (usd, spend, per_day, daily, day_ceiling, max_loops, max_turns — see
  `scripts/keel-guard.py`'s `cap-raised` rule), so I expect **no finding**. But the guard, not your judgement,
  decides: run `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` and paste the whole output. **If it
  flags anything, STOP that part, put the old value back, and write `OWNER CALL: <the guard's exact line>` in the
  report.** You cannot sign, so you cannot ack, and you must never add a `Keel:` trailer of your own for this half.
- The config template (`crates/theseus-core/config/theseus.example.toml`) and the template tests at
  `config/self_improve.rs` :551-561 name `backlog_starts = 1`. Keep the template honest about the new default and keep
  those tests passing on their own terms (:551 asserts the default is 1 — if the default is now computed, that
  assertion must become the right one for the new behaviour, which is a rewrite, not a weakening: keep the same number
  of assertions, keep the test's name, and list it under **Keel findings expected** with its line and reason).
- If deriving the free slots inside the config's defaults is wrong (a config default cannot see runtime capacity),
  then keep the key's default at 1 and compute the effective number at the night's start site instead, where capacity
  is known. Say which you did and why. **Do not raise the literal to a bigger literal** — that is the one shape that
  is a cap raise.

### Tests (each red at the base, with a planted revert)

Put new tests in a `tests_<subject>.rs` of their own where the crate's convention allows, or beside the closest
existing RSI tests (`rsi/tests_pr.rs`, `rsi/tests_budget.rs` are the models); name the file in the report.

1. **The tick's line.** A tick that starts one build stages exactly one `self_digest` post whose text is
   `night_text`'s wording and names the item; a tick that starts nothing stages none. Plant P1: drop the stage: red.
2. **Twice rejected drops.** An item with two `review:` rejections is dropped, not parked: no `parked:` evidence and
   no `waiting_human`, a notice staged, and the notice's text contains **both** findings and the **real** command.
   Plant P2: restore the park: red.
3. **The command is real.** Assert the notice's command string equals the verb the CLI actually has (parse it out, or
   assert against the `SelfCmd` variant's name/help). Plant P3: change the notice's verb to one that does not exist:
   red. This test is the whole point of point 2's "read `Move` and `Retry` first".
4. **A join conflict returns to `proposed`.** A join whose merge conflicts leaves the item `proposed` (not held, not
   `waiting_human`), with a notice naming the fresh-branch behaviour. Plant P4: restore the wait: red.
5. **The keel still holds on the loosened paths.** On the same rig: a cause in a keel class still files
   `waiting_human` and never starts (`rsi/backlog/keel.rs` :14-43), and a self-join in a keel class still posts the
   owner's card (`rsi/join.rs` :23-25). Plant P5: remove the keel check: red. **This test is required** — it is the
   evidence that the row loosened only what it meant to.
6. **The night's starts.** Whatever you did for `backlog_starts`, a test pins it: the night starts as many as the
   pipeline has free slots (or, in the fallback shape, the effective number at the start site), and a config that sets
   `backlog_starts` explicitly still wins. Plant P6: hard-code it back to 1: red.
- **Keep green:** the whole `theseus-core` suite, `theseus` (CLI) and `theseus-protocol`, and `theseusd`'s self tests.
- If an existing test has to change its expectation, **do not delete or weaken it**: rewrite it with the same number
  of assertions, keep its name, and list it under **Keel findings expected** with its line and reason.

### Not in this row

No `TaskOrigin` change, no `self_started`, no `because` (row A, in flight elsewhere). No `task.not_this` RPC, no
standing preference, no judge label, no Ops or cockpit change of any kind, no `.tsx` edit (row B). No tools-deferral
change (row C). No admission or burn change (rows D and E). **No Discord code at all** — not `courier.rs`, not
`runtime/`, not `render.rs`, and not `golden/dm_flow.txt`: the Drop / Halt buttons on these very notes are **row G**,
a separate issue, and the DM task flow is locked. No `max_wakes` change (row H). No new dependency. No benchmark, no
spend, no AWS, no model call.

## Shared files: fires are parallel, joins serialise

Eleven other patch3 rows are in flight right now against this same `0fda4d57`, in three lanes
(`integ/p3-fanout`, `integ/p3-ops2`, `integ/p3-selftask`). That is deliberate: the review queue, not the build, is our
bottleneck, so rows fire concurrently and the integrator serialises the joins.

**You are the one row in this wave that shares no file with anything in flight.** `rsi/pipeline.rs`,
`rsi/pipeline/queues.rs`, `rsi/join.rs` and `rsi/backlog/{start,word}.rs` are touched by no row in any lane. So:

- Write your row against `main 0fda4d57` as if you were the only one editing these files, because you are.
- **You join first, and on your own.** Say in your report that you understand that.
- If you find yourself needing to edit a file outside the list above — in particular `crates/theseus-core/src/task.rs`
  (rows A, C and `z9mze.3` all own hunks there), `cockpit/src/views/Ops.tsx` (six rows own hunks there), or anything
  under `crates/theseus-discord/` — **stop, leave that part out, and write `JOIN SEAM: <file> — <why I needed it>` in
  the report.** An unexpected hunk in a shared file is what turns a mechanical join into a guess.
- In the report, **list every hunk you place, with the file, the line range, the function and why**, so the
  integrator's join is mechanical even though you expect no conflict.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code 0fda4d57 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. You edit no
  Discord file, so this should be trivially clean; prove it anyway.
- **Keel.** The gate's keel phase (`scripts/keel-guard.py`; its docstring lists the rules) fails a change that
  deletes a test, adds `#[ignore]` or a skip, lowers a test's assertion count, weakens an assertion in place, adds an
  `#[allow]`, raises a bench budget, a spend or loop cap, or a ceiling in `scripts/long-files.txt`,
  `scripts/shape.sh` or `.config/nextest.toml`, or edits the guard's own files, unless a commit signed by the owner's
  key acks it. **You cannot sign, so you cannot ack, and you must never add a `Keel:` trailer yourself.** Run
  `python3 scripts/keel-guard.py` before every commit and
  `python3 scripts/keel-guard.py --base 0fda4d57 --head HEAD --quiet` before the report, and paste the whole output.
  **If it flags a behaviour cap, STOP that part, put the old value back, and write `OWNER CALL:` — do not raise the
  cap.** Say plainly in the report that this row loosens three gates by design (parked → dropped-with-notice,
  conflict → proposed, the night's starts), so the maintainer can carry that to the owner.
- **No ceiling raised and no cap loosened without saying so explicitly in `CLOUD_REPORT.md`.** Joins quietly undo
  caps, and two lanes can each raise the same ceiling; the report is how the integrator catches it.
- **Data floor:** nothing new is sent to a model or routed anywhere. The notices are the core's own words, staged to
  the existing outbox and rendered by the existing binding.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names under
  `example.test`. Write "the owner".
- **Red first** (each test red at `0fda4d57`, green after; prove each with its planted revert, restoring the file and
  `touch`ing it so cargo rebuilds it, and run `git status` after every restore); right over compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` "Building and checking" first):
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, and `cargo nextest run -p theseus-core -p theseus -p
  theseus-protocol` plus every other crate whose files you touched. Then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md` at the repository root. Write it with your editor tools, **never a shell heredoc** (a heredoc
on a full temp filesystem has committed an empty report here before), and check after the commit that it is not empty.
Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51: untouched, or the bump named and justified with which
goldens it moves) and `SCHEMA` (2, untouched); ceilings; any `OWNER CALL:`, `JOIN SEAM:` or STOP. (2) **The tick's
line:** where it is staged, at `file:line`, the wording it reuses, and the tick that stages nothing. (3) **Twice
rejected:** the drop, the notice's full text as a sample, and **which CLI verb the notice names, and whether you reused
`Move`/`Retry` or added `Revive`, with your reasoning**. (4) **The join conflict:** the return to `proposed`, and what
bounds a conflict loop. (5) **`backlog_starts`:** which shape you chose and why, the template's line, and the guard's
output for it. (6) **Tests** and plants P1-P6, red at the base and green after, with counts. (7) Any existing test
whose expectation you rewrote, under **Keel findings expected**. (8) **Gate** commands and results. (9) **Keel**
output, whole. (10) **DM golden** diff result. (11) **Join notes:** every file touched with line ranges and the
function, and a statement that you edited no file under `crates/theseus-discord/`, no `.tsx` file, and not
`crates/theseus-core/src/task.rs`. (12) **Follow-ups.** (13) One line on the data floor.
