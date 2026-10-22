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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail2-route`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of `main` for this wave (it wins over anything below about `main`)

This task was written against an older `main`. Your branch starts from the integration branch `integ/threads` at `8b641b6b` (2026-10-21: `main` `6918e3c1` plus threads round 2: threads-home-claim, threads-routing-rest, xlri5-families, and part of the protocol split); that branch, not `main`, is your base. Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 49 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 49 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (77 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
to touch your area:
- Google part 1 (`f897d6b4`, with join fixes `897c3ea7`, `26b3d91b`): the `theseus-google` crate, `[google]`,
  sign-in, `google.call` (it classifies the filled path and refuses a query or an escape in a raw path).
- The data rule (`63596193`): sensitive names go only to models of US origin, failing closed; a routed turn that keeps
  failing asks and moves up a model; Jev warns on fishing for personal details.
- Turn-end facts (`9b4d2a33`, join fixes `dfd3b55e`, `69c8d74a`): a command's result names the new files it left; a
  turn about to end beside them is told once.
- Bench measurement (`5e0bd195`), the first request's persona layer per place (`fa5832c7`), the tool catalog built
  once (`17d6deec`), `blocking` as one function (`6c1332c1`).
- Needs-you cards (`37d1edb2`), the task home `#theseus-tasks` (`cf243efe`), agent answers labelled (`32512fbd`),
  self-improvement fails open (`051fc186`), a tool `tools_find` finds is callable in the same turn (`05655588`).
- Voice: echo cancel (`6bdfe9b1`), the join guard (`5a7bcbb9`), Jev checks an unclear transcript (`a86a4ade`).
- The ontology (`dc2f5f71` core, `c7ee0bb7` Jev files each inbound message, `38745067` pictures, `3a75e094` it
  reaches routing, context and recall, `7895bb45` the nightly sweep, `580c8c37` the labelled replay and the +onto
  bench arm, `1d092ded` the cockpit's Ontology view).
- Threads: a place's home splits into threads (`fd1f2e51`); a thread's recall no longer pulls another stream's facts
  (`eb43b3d4`, the thread lens); a thread adopts its task's running turn (`76237a96`).
- Since 1d092ded (`87696a7c`, 2026-10-21): task questions and reports never stall a task (`owner_ask`, `turn/join_step.rs`, `task.close`; `afa16ac5`); `theseus chat` answers the router's question and keeps an explicit session (`5f36ef45`); herdr actions take the focused pane (`6f8487f2`); the threads bench scorer (`4abaceb2`); the lifecycle bench's margins (`b87a46c4`); three load flakes waited on as events (`87696a7c`).
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,405 / 3,525
- `crates/theseus-index/src/vectors.rs` 2,561 / 2,650

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- Google part 2 rows: gw-mail, gw-calendar, gw-drive, gw-sheets-slides (`crates/theseus-google/`, each its own
  service module, `fake/<service>.rs`, one registration line each).
- Harness rows: job-truths (job command lines, `theseus-kernel/src/job.rs`, fs/file read results), effort-on-evidence
  (turn checkpoints' effort), turn-end follow-ups and long blind waits, recall abstention and the `service` orphan.
- Threads rows: routing (`crates/theseus-core/src/thread_route.rs`, working sets, claims) and cross-stream reach
  (`theseus-memory/src/threads.rs`, `thread_recall.rs`, a place digest, Jev's near-miss check, thread links).
- Ontology follow-ups (`crates/theseus-ontology/`, the sweep, the dream's guidance, the replay), test-infrastructure
  rows (Discord task-thread tests, leaking shell loops), the China-origin model rows (`data_gate.rs`, session data),
  rough edges in progress words, detours and `theseus why`.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail2-route, five follow-ups around the route step: a held turn whose compile fails, a detour's row and its words, a judge test that races the minute (theseus-cljlz, theseus-npcps, theseus-nfrlj, theseus-q9qx5, theseus-h9jec)

Branch: `cloud/20261021-tail2-route`. Every commit's subject carries the Beads id of what it changes (`theseus-cljlz`,
`theseus-npcps`, `theseus-nfrlj`, `theseus-q9qx5`, `theseus-h9jec`). Deadline for the report: 4 hours after you start.

**Why.** Five follow-ups that the reviews of the shared-place hold and the detours-why row left open, plus one test that
fails the lane gates about once an hour:
- **A held turn whose compile fails is failed, not held (theseus-cljlz).** In a shared place, Jev's verdict can say a
  message reaches for the owner's personal details: the turn is then held (no model call, the requester is answered
  `I don't share that here.`, the owner's DM is told; theseus-i1z13). The verdict is read in `compile_first` only after
  the compile succeeded: a compile `Err` (for example `context_overage`, a message past the window) returns before
  `read_verdict`, so `t.route.held` is never set and the turn fails with the compile's error. The requester sees a
  failure instead of the fixed line, and the owner is never told about the reach.
- **Unchecked subtractions in `compile_detour` (theseus-npcps).** `tail.standing = compiled.est_tokens - before;` and
  `tail.persona = compiled.est_tokens - before;` are plain `u64` subtractions. Both steps only add today; a later step
  that trims the estimate would panic in a debug build or wrap in release and record an absurd part in the
  `context.detoured` row.
- **A steered detour's words (theseus-nfrlj).** `detouring` records `detour_kind` from a `Steer::Mode` steer or the
  verdict's mode; any other steer (`Steer::Profile` naming a detour profile) records `""`, so `theseus why` prints
  `detour: ran as a  detour on …` (empty word, double space). And a steered `quick` detour records `kind = "quick"` but
  `detour_recall = false` (a steered turn sends no recall), while `detour_left` keys the line "this turn's own recall:
  … carries none" off `kind != "quick"`: the line is missing although the recall was left out.
- **A detour's parts show `bytes: 0` (theseus-q9qx5).** `context.detoured` records tokens per part, never bytes, so
  `context.explain` gives every detour part `bytes = 0`. The cockpit's table already blanks a zero cell, but its
  footnote says every part is counted from its bytes, which is false for a detour, and the wire field says nothing.
- **The judge test races the minute (theseus-h9jec).** `crates/theseusd/tests/judge.rs`
  `a_turn_is_judged_after_it_and_asks_the_model_the_same_bytes` compares the model's raw request with the judge on and
  off byte for byte, but each turn's prompt carries its wall-clock minute (`[Terminal · Wed 2026-10-21 07:13 -07:00]
  Say done.`). When the two turns straddle a minute boundary the test fails. It was red in four lane gates on
  2026-10-21 (04:08 vs 04:09, 07:13 vs 07:14, …) and passes alone.

**Risk: low to medium.** cljlz changes what a person sees on a rare failure path (held line in place of an error);
everything else is local. No store layout change is expected (see nfrlj below).

**Prerequisite: base `origin/integ/threads` at `8b641b6b`** (the threads lane's round 2, green; it contains `main`
`6918e3c1` plus the threads rows that changed `route_step.rs`, `thread_route.rs` and `judge.rs`). Your branch starts
there and joins `integ/threads` as its round 3. Another lane (`integ/cn`, the China-origin rows) also edits
`turn/route_step.rs` (its `RouteState` and the route row); the release stage merges both. So keep every hunk in
`route_step.rs` small and local: no reordering, no renaming, no reformatting of lines you do not change.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail2-route` | theseus-cljlz, theseus-npcps, theseus-nfrlj, theseus-q9qx5, theseus-h9jec | a held verdict honoured when the first compile fails; saturating detour parts; a steered detour's word and its recall line; a detour's explain says it has no byte counts; the judge test's minute stamp normalised | `crates/theseus-core/src/turn/route_step.rs`, `crates/theseus-core/src/turn/personal_step.rs` (visibility of one fn at most), `crates/theseus-core/src/turn.rs` (one small hunk), `crates/theseus-core/src/turn/detour_step.rs`, `crates/theseus-core/src/rpc/context.rs` (`detour_of` only), `crates/theseus-protocol/src/context.rs` (`ContextDetoured`, `ContextDetour`, `ContextPart.bytes` doc), `crates/theseus-protocol/src/why.rs` (`detour_words`, `detour_left`), `crates/theseus-protocol/tests/why.rs` + `tests/why/*.txt` goldens if a case is added, `cockpit/src/protocol.gen/{ContextDetour,ContextDetoured,ContextPart}.ts` (regenerated), `cockpit/src/components/ContextTurn.tsx`, new `crates/theseus-core/src/tests_hold_compile.rs` (+ one `mod` line), `crates/theseus-core/src/tests_detour.rs`, `crates/theseusd/tests/judge.rs` | M |

## What exists (lines at `8b641b6b`; search by name, a line may have moved)
- **The hold.** `crates/theseus-core/src/turn/personal_step.rs`: `verdict_asides` (near :135) calls `engage_verdict` and
  the private `personal_warn` (near :142), which, for this turn's own verdict (`v.turn == t.tc.turn_id`) whose
  `personal` answer `warns(shared)`, builds the owner's text, sets `t.route.held = holds(shared, t.tc.task.is_some())`,
  posts to the owner's DM (folded once per window) and records `PrivacyWarned`. `hold_reply(t, i)` (near :194) writes
  the fixed line as the turn's assistant node. `crates/theseus-core/src/turn.rs` near :1840-1866: `compile_routed(..)`
  then `Err(f) => return Ok(Next::Fail(f))`, then `if t.route.held { recall_end; Self::hold_reply(t, i)?; break; }`.
- **The compile and the verdict.** `crates/theseus-core/src/turn/route_step.rs`: `compile_routed` (near :390) calls
  `compile_first` on loop 0 of a routed turn. `compile_first` (near :499): `beside(compile, &mut rx, max_wait,
  wait)` (near :193: it awaits the compile, *then* waits for the verdict up to `max_wait`, so the verdict is there even
  when the compile failed), then `let compiled = match compiled? { Ok(c) => c, Err(f) => return Ok(Err(f)) };` (near
  :536-539) **before** `read_verdict` (near :542) and `verdict_asides` (near :553). Compile failures come from
  `compile_step` (`turn/compile_step.rs` near :167-172): `Self::overage` (`turn/compaction.rs` near :543,
  `OVERAGE_CLASS`, it records a `context.overage` row) and `self.admitted(..)`.
- **Hold tests to copy from.** `crates/theseus-core/src/tests_personal_hold.rs` (`shared(option, confidence, dm)`
  builds the rig: Jev reads `chat` and the `personal` answer; a session bound to a guild channel; `rows`, `warnings`,
  `said` helpers). Do not edit that file (another lane's row adds to it): put the new tests in a new
  `tests_hold_compile.rs` and copy or `pub(crate)` the helpers you need. An overage to copy:
  `tests_compaction.rs` `the_newest_exchange_past_the_window_fails_with_context_overage` (near :626).
- **The detour.** `route_step.rs` `detouring` (near :624): `detour_kind` = `Steer::Mode(m)` → `m`, another steer →
  `""`, no steer → the verdict's mode; `detour_recall = steering.is_none() && verdict.is_some_and(|v|
  detour_recalls(&v.mode))`. `compile_detour` (near :929): `tail.standing = compiled.est_tokens - before;` (near
  :986) and `tail.persona = compiled.est_tokens - before;` (near :989). `turn/detour_step.rs` `detoured_row` (near :46)
  builds `ContextDetoured` (`kind: t.route.detour_kind.clone()`, the parts; the recall part only when
  `t.route.detour_recall`). The `Steer` enum: `crates/theseus-core/src/correction/layer.rs` near :26 (`Mode`,
  `Profile`, `Thread`, `Persona`). Steer tests: `tests_steer.rs`.
- **The wire and the words.** `crates/theseus-protocol/src/context.rs`: `ContextPart` (near :31, `bytes: u64` with no
  doc), `DetourPart` (near :92, tokens only), `ContextDetoured` (near :106, the ledger row's payload), `ContextDetour`
  (near :128: `kind`, `profile`, `model`). `crates/theseus-protocol/src/why.rs`: `detour_words` (near :261, `"detour:
  ran as a {} detour on {} ({}); …"`), `detour_left` (near :270, the own-recall line when `d.kind != "quick"`).
  `crates/theseus-core/src/rpc/context.rs` `detour_of` (near :250) maps the row into the result (parts with
  `..ContextPart::default()`, so `bytes = 0`) and `out.detour`. Golden tests: `crates/theseus-protocol/tests/why.rs`
  `a_detoured_turn_says_it_detoured_and_what_it_left_out` (near :238, goldens `tests/why/detour.txt`,
  `detour_context.txt`); core: `tests_detour.rs` `why_explains_a_detoured_turn` (near :154).
- **The cockpit.** `cockpit/src/components/ContextTurn.tsx`: the parts table's `bytes` column (near :131, `p.bytes ?
  count(p.bytes) : ''`) and the footnote near :147 ("Every part is counted from its bytes at the model's figures, …").
- **The judge test.** `crates/theseusd/tests/judge.rs` near :182-230: two rigs (`Rig::new(Some(jev))`, `Rig::new(None)`),
  one `ask("Say done.")` each, then `bytes(&on) == bytes(&off)` where `bytes` replaces the rig's temp dir with `<dir>`.
  The stamp is the prompt's time header, written by the turn's input framing (find where `[Terminal ·` is built).

## What to build
1. **theseus-cljlz: a held verdict holds a turn whose first compile fails.** In `compile_first`, when the compile
   returns `Err(f)`: still read the verdict (`read_verdict` with the same arguments) and run the personal check
   (make `personal_warn` `pub(super)`, or add a small `pub(super) fn personal_on_failure` beside it; do not run
   `engage_verdict` or record a route row for a failed compile, and change `hold_reply` not at all). Then return the
   same `Ok(Err(f))`. In `turn.rs`, the one hunk: a compile `Err` while `t.route.held` is set ends the turn held
   (`recall_end` as today, `hold_reply`, `break`) instead of `Next::Fail`; every other `Err` is failed as today. Keep
   the hunk to a few lines (e.g. `Err(f) if !t.route.held => return Ok(Next::Fail(f))` and an `Option` for the
   compiled value), nothing else in `turn.rs` moves.
   - The compile's own rows stay (a `context.overage` row is true: the compile did overflow). Check what else the
     failed compile left (a park on input, a ring) and say in the report whether a held turn keeps or skips each; if a
     parked session would make the requester's next message fail, fix that inside this path, with a test.
   - "Check the verdict before the compile when it is known" (the issue's other option) does not apply: the hold reads
     only this turn's own verdict, which is never there before the compile starts (it is awaited beside it). The
     compile still runs on a held turn by design (it costs no model call). Say so in the report.
   - In a private place, and for a task's turn, nothing changes: a compile failure still fails (`holds` is false).
2. **theseus-npcps:** both subtractions become `saturating_sub`. Nothing else in `compile_detour` changes.
3. **theseus-nfrlj: the word and the recall line.**
   - The word: a steered detour whose steer is not a mode gets a word, never `""`: prefer the mode the steered profile
     is a detour of (`routing` knows which detour modes list it; if there is no clean lookup, use `steered`). Decide in
     `detouring` (route_step.rs) so the row stores the word; `detour_words` also defaults an empty `kind` (rows already
     stored) to `steered`, so old rows never print a double space.
   - The recall line: record whether the detour carried its own recall in the row. Add `#[serde(default,
     skip_serializing_if = "Option::is_none")] recall: Option<bool>` to `ContextDetoured` and to `ContextDetour`
     (filled from `t.route.detour_recall` in `detoured_row`, copied in `detour_of`), and key `detour_left`'s line off
     `d.recall.unwrap_or(d.kind == "quick")`, so rows stored before this row read as today. This is a new optional
     field in a ledger row's JSON payload: it is **not** a store format change (no `MANIFEST_FORMAT` bump; the
     detours-why review says the same of the row itself). If you find AGENTS.md says otherwise for ledger payloads,
     stop and follow AGENTS.md, and say so.
   - Regenerate `cockpit/src/protocol.gen` the way `crates/theseus-protocol/AGENTS.md` says; only `ContextDetour.ts`,
     `ContextDetoured.ts` and `ContextPart.ts` may change (no new type, so `index.ts` stays byte for byte).
4. **theseus-q9qx5: a detour's explain says it has no byte counts.** No stored field for bytes (the detour's request
   is never stored, and tokens are what the row can say honestly). Instead:
   - `ContextPart.bytes` gains a doc comment: its UTF-8 length, `0` on a detour's parts, whose row records tokens only.
   - The cockpit's `ContextTurn.tsx`: when the result has a `detour`, the parts table has no `bytes` column and the
     footnote says the parts are the detour's row's token counts (one short sentence). A compiled turn's table and
     footnote are byte for byte today's.
   - `theseus why` prints no byte figures for parts today; check it and the TUI (`theseus-tui/src/chat/usage.rs`)
     and say in the report that nothing else shows a detour's bytes.
5. **theseus-h9jec: the judge test compares the request, not the clock.** First look for a test clock the daemon
   honours (`crates/theseusd/tests/common/`, the kernel's clock, an env var the other theseusd tests set); if one pins
   the wall clock for both daemons, use it. Otherwise normalise: a small helper in `judge.rs` (or `common/` if another
   test can use it) that replaces the time header's stamp (the text between `[Terminal · ` and `]`, matched exactly as
   the framing writes it) with `<now>` before comparing, so the rig's `<dir>` and the stamp are the only two things
   named as differing. The assertion still compares every other byte.

## Tests (each with a planted revert; the first is the key test)
- **`a_held_turn_whose_compile_fails_is_still_held`** (new `tests_hold_compile.rs`): a shared place, Jev reads the
  message as a reach (`elicit` at a holding confidence, as `tests_personal_hold.rs` scripts it), and a message large
  enough that the first compile fails with `context_overage` (copy the overage test's sizing). The turn returns `Ok`,
  its output is `I don't share that here.`, no model request was made, one `privacy.warned` row says `held: true`, and
  the owner's DM got the case. Plant: return before reading the verdict on a compile error (today's code); the test
  must fail.
- `a_compile_failure_in_a_private_place_still_fails` (same file): the same message and verdict in a private place (or
  a task's turn) fails with the overage class, as today. Plant: hold on every compile error; the test must fail.
- `a_steered_detour_names_its_kind_and_its_left_out_recall` (in `tests_detour.rs`, or `tests_steer.rs` if its rig is
  the one that steers): a turn steered to a detour by `Steer::Profile` stores a non-empty `kind` in its
  `context.detoured` row, and `why`'s words have no double space; a turn steered to `Steer::Mode("quick")` stores
  `recall: false`, and `why` lists "this turn's own recall". Plant: drop the new `recall` key in `detour_left` (back to
  `kind != "quick"`); the second half must fail.
- In `crates/theseus-protocol/tests/why.rs`: a stored row with no `recall` and `kind: "quick"` renders exactly the
  `detour.txt` golden (old rows unchanged); a row with `kind: ""` says `steered`. Plant: drop the default word.
- `npcps`: no behaviour change today (both steps only add): say so; no plant. The existing `tests_detour.rs` suite
  must pass.
- Cockpit: a test (the cockpit's own runner, `node --test` or the one `cockpit/AGENTS.md` names) that a detour's
  result renders no `bytes` header and a compiled one still does. Plant: render the column always.
- `judge.rs`: a unit test of the helper: two request texts that differ only in the stamp (`07:13` vs `07:14`, and a day
  change `23:59` vs `00:00` of the next date) compare equal after it, and two that differ in one other byte do not.
  Plant: make the helper return its input unchanged; the unit test must fail. Then run the daemon test itself
  (`cargo nextest run -p theseusd --test judge`) and say it passed (it needs the built `theseusd`; see the L1 note
  below).

## Not in this row
- The thread router, thread links, home claims, the judge's breakers, Jev families (`thread_route.rs`,
  `rpc/thread_flow.rs`, `rpc/thread_link.rs`, `rpc/home_claim.rs`, `turn/families_step.rs`, `tests_cross_judge.rs`):
  sibling rows on this same base own them.
- Docs: no `AGENTS.md`, `docs/status.md` or spec edit. The threads docs row of this batch owns them; list in your report
  the sentences a maintainer should add for cljlz and nfrlj (one line each).
- The protocol's `lib.rs`, the TUI, the Discord crate, the store layout, `tests_personal_hold.rs`.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias. Stored data stays readable (old `context.detoured` rows render as today).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure refactor needs no plant: say so.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). FAST: nothing new on a plain turn (a
  plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`); cljlz's work runs only on a
  failed compile.
- **Line ceilings** (`scripts/long-files.txt`, length / ceiling at `8b641b6b`): `crates/theseus-core/src/turn.rs`
  3,430 / 3,525 (add at most ~6 lines), `crates/theseus-core/src/tests_m3.rs` 8,050 ceiling (add nothing there).
  Unlisted files stay at 2,500 lines or fewer (`route_step.rs` is 1,182, `rpc/context.rs` 699, protocol `why.rs`
  678). Never raise a ceiling; never add a file to the list.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Changing the judge
  test's comparison is allowed only as described (it still compares every byte but the stamp and the dir); justify it
  in the report's keel section. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet` and paste its output in the report.
  It should be empty; if it flags the judge test, quote the finding and your justification. You cannot write a
  `Keel:` trailer: never try to.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. **Store format on your
  base: 48** (`MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`); this row should not change it. If you find
  you must, bump to 49 and add the old layout's sample to `tests_layouts.rs` as AGENTS.md says, and say so first in
  the report.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
  Fixture paths and names are invented.
- **Commits:** signed (`git commit -S`; if signing is not set up in your session, commit unsigned and say so), the Beads
  id in each subject, and the repo's usual `Co-Authored-By: Tabitha/Claude` trailer (copy the whole line from a recent
  commit on your base: `git log -3 --format=%B 8b641b6b | grep Co-Authored`). One commit per Beads id
  where the code allows.
- Read the AGENTS.md of every crate you touch first (`theseus-core`, `theseus-protocol`, `theseusd`, and
  `cockpit/AGENTS.md`). Keep CLOUD_REPORT.md honest: what you ran, what passed, what you could not run in the cloud
  (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM; say which you ran and which you
  could not). Run each touched crate's tests (`cargo nextest run -p theseus-core`, `-p theseus-protocol`,
  `-p theseusd --test judge`), the cockpit's checks for the file you changed, and
  `cargo clippy --workspace --all-targets -- -D warnings` before the last commit.

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty) with: what changed per Beads id; each test and its plant, with the failing output's line; for
cljlz, what the failed compile leaves behind on a held turn (rows, park, ring) and what you kept or skipped; for nfrlj,
how you chose the steered word and whether the store format moved (it should not); for h9jec, whether you pinned a
clock or normalised, and why; the files outside the row table you touched (none expected); what could not run on the
VM and the exact commands for the maintainer; the keel guard's output; the docs sentences for the threads docs row;
follow-ups you found. Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
