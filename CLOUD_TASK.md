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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail6-cn`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `49352e8a` (2026-10-22: release stage 4, install #72, which landed `integ/tail4` round 1
and `integ/cn` round 5 on top of stage 3's `f1fc9b03`); that is your base. Line numbers quoted below are at `49352e8a` but
may still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `f1fc9b03`** (`git log --first-parent f1fc9b03..HEAD` lists them):
- `6854991f`: `integ/tail4` round 1 (the TUI work view, thread routing by title, voice warm tracks, the Discord approval
  card and forums, the test temp-dir leak, the ontology sweep's renames and kind names).
- `a95ec41d` + `b046ca16`: `integ/cn` round 5 (the tool screen's redirects, the China-origin config guidance, reply data,
  the draft's first pass).
- `49352e8a`: a join fix to the approval card's why line.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500 (the lane beside you adds lines here: add nothing)
- `crates/theseus-discord/src/render.rs` 2,975 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail5` (`origin/integ/tail5`, `b1cd38e9` or later) lands as the next release stage. Its rows are the branches
  `origin/cloud/20261021-tail5-effort`, `-tail5-threads`, `-tail5-discord` and `-tail5-t4fix`. Your task's "Rules for this
  row" names the files of theirs that sit near yours. Read their diff before you edit those files:
  `git diff origin/main...origin/cloud/20261021-<row> -- <file>` (three dots).
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail6-cn, the China-origin follow-ups now on main: the data-gate footer and route line, the gate's counters, the screen's redirect plumbing, reply data's refused path (theseus-4h85l, theseus-atbgl, theseus-uaqox)

Branch: `cloud/20261021-tail6-cn`. Every commit's subject carries the Beads id of what it changes (`theseus-4h85l`,
`theseus-atbgl`, `theseus-uaqox`). Deadline for the report: 4 hours after you start.

**Why.** Release stage 4 (`a95ec41d`, now on `main`) landed the China-origin lane's round 5. Three reviews left findings
the owner wants in before the release closes:
- **theseus-4h85l** (the cn-observability review, `a1wvq.11`). (1) The route footer says `data gate: verdict_private` on
  every private or unclear turn in a mode that lists a China-origin profile, even when a US model would have won the
  route anyway: the word should show (and count) only when the rule actually kept the turn off a restricted-origin model
  it would otherwise have taken. (2) `note_open` makes a `TurnRoute { reason: "no_verdict", .. }` for a turn that asked
  no route, so the CLI's status line shows `<who> · route no_verdict · open` (Discord shows `routed: open`) where before
  there was no route line: an open-only turn should show only `open`. (3) The `sensitive` gate word has no end-to-end
  test. (4) The provider chokepoint (`data_gate.rs` `Gated`) refuses without counting on `theseus.route.data_gate`,
  because it holds no telemetry handle. (5) Four background paths refuse a restricted-origin model without a
  `record_data_gate` call: compaction (`compaction/data_gate:<word>` in `turn/compaction/gate.rs` `summary_route`, the
  ring and the fallback arms), consolidation (`consolidation/restricted_origin` in `consolidate/run.rs` `unrestricted`),
  keep-warm (`keep_warm/restricted_origin` in `keep_warm.rs` `keep_warm_prepare`) and replay
  (`replay/restricted_origin` in `own_bench/replay.rs` `refuse_restricted`).
- **theseus-atbgl** (the tail3-cn review, F1). The redirect test (`tests_tool_screen.rs`
  `a_fetch_that_redirects_into_the_owners_network_fails_the_screen`) works at the `data::screen` level; `turn/screen_step.rs`
  passing `meta.final_url` is held by no test (passing `None` leaves every test green).
- **theseus-uaqox** (the tail4-cn-reply review, F2). `judge/reply_data.rs`'s module doc says every other outcome, a
  refused reservation included, writes the `data.reply` row; but when `reserve_beside` refuses, `prepare_reply` returns
  `None` and `reply_one` returns before `land_reply`. It fails closed, but bench and inspect cannot see refused asks.

**Risk: low-medium.** (1) changes when an owner-visible footer word shows; nothing changes which model a turn uses.

**Prerequisite: base `origin/main` at `49352e8a`.** Your branch joins the new lane `integ/tail6`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail6-cn` | theseus-4h85l, theseus-atbgl, theseus-uaqox | footer word only when the gate decided; no route line for an open-only turn; a `sensitive` end-to-end test; the chokepoint's and four background paths' counters; a screen-step turn test; reply data's refused row | core `data.rs`/`routing.rs` (the gate's reason, only as needed), `turn/data_step.rs` (`note_open`), `turn/route_step.rs` (the `record_data_gate` call near :860 only), `data_gate.rs` (`Gated`), `turn/compaction/gate.rs`, `consolidate/run.rs` (`unrestricted`), `keep_warm.rs` (`keep_warm_prepare`), `own_bench/replay.rs` (`refuse_restricted`), `judge/reply_data.rs`, new or existing test files beside them (`tests_bg_gate.rs`, `tests_tool_screen.rs`, a new `tests_data_footer.rs` if you need one); protocol `route.rs` (`TurnData::footer`, only if the rule lives there); core `AGENTS.md` (the data-rule section) | L |

## What exists (at `49352e8a`; search by name)
- **The gate's reason.** `crates/theseus-core/src/data.rs` near :100 `DataWhy` (`VerdictPrivate` => `"verdict_private"`,
  `NoVerdict` => `"no_verdict"`, and the others) and its doc near :212; `crates/theseus-core/src/routing.rs` near :164
  `Reason::NoVerdict`. `crates/theseus-core/src/turn/route_step.rs` near :860 `self.judge.record_data_gate("route", why)`.
- **The footer.** `crates/theseus-protocol/src/route.rs` near :86 `TurnData::footer()`: `open` when a request carried a
  pass, `data gate: <why>` when `gate` is set. Its test near :370. Discord's `runtime/route.rs` near :383 tests the
  Discord line (`routed: trivial · data gate: verdict_private`). The CLI's `crates/theseus/src/render.rs` near :508
  `status_line` (render.rs is at 3,126 / 3,128: change it by at most two lines, better not at all).
- **The open note.** `crates/theseus-core/src/turn/data_step.rs` near :139 `note_open` makes the `TurnRoute` the review
  names. The cn-observability review (`a1wvq.11`) says `· open` shows only once a request carried a pass, and a later
  `record_route` keeps `open` (route_step.rs near :803-822).
- **Counters.** `crates/theseus-core/src/telemetry.rs` near :276 `record_data_gate(what, why)`;
  `crates/theseus-core/src/judge/inbound.rs` near :496 the judge's wrapper. `crates/theseus-core/src/data_gate.rs` near
  :51 `struct Gated` and near :158 `impl Provider for Gated` (the chokepoint). `crates/theseus-core/src/tests_bg_gate.rs`
  drives the background gates (near :246: "Nothing reaches the chokepoint's refusal").
- **Background refusals.** `turn/compaction/gate.rs` near :42 `summary_route`; `consolidate/run.rs` near :306
  `unrestricted`; `keep_warm.rs` near :474 `keep_warm_prepare`; `own_bench/replay.rs` near :209 `refuse_restricted`.
- **The screen.** `crates/theseus-core/src/turn/screen_step.rs` near :53 reads `meta.get("final_url")` and passes it to
  `data::screen(...)` near :61. `tests_tool_screen.rs` holds the `data::screen`-level redirect test.
- **Reply data.** `crates/theseus-core/src/judge/reply_data.rs`: module doc, `prepare_reply` near :69, `reserve_beside`
  near :112, `land_reply` near :127, `reply_one` near :155.

## What to build
1. **4h85l (1).** The route's `data_gate` word, the footer's `data gate: <why>` and the `route` counter apply only when
   the data rule removed a China-origin (or unknown-origin) candidate the route would otherwise have taken. When the
   winning profile is a US model anyway, the row and the footer carry no gate. Decide where that test lives (the route
   decision knows the candidates it filtered) and say which in the report. Keep `route.decided`'s other data fields.
2. **4h85l (2).** A turn that asked no route but carried a pass shows `open` and no `route no_verdict`: either `note_open`
   makes a route record the surfaces do not print as a route, or the reason is one the CLI and Discord already skip.
   Pick the smaller change, preferably in core or protocol, not `render.rs`.
3. **4h85l (3).** An end-to-end test of the `sensitive` gate word: a turn whose data verdict reads sensitive in a mode
   listing a China-origin profile routes to the US model, its row carries the `sensitive` gate word, and the counter
   counts it once.
4. **4h85l (4).** The chokepoint counts each refusal on `theseus.route.data_gate` (`what` = `chokepoint`, `why` = its
   class word). Give `Gated` the telemetry handle the way its builder can reach it (search where `Gated` is made); no
   global.
5. **4h85l (5).** The four background refusals each call `record_data_gate` with the `what`/`why` pairs above. One
   test over the four (in `tests_bg_gate.rs` or beside it) asserts each pair is counted.
6. **atbgl.** A turn test: a scripted `http.fetch` result node whose `meta` carries a private `final_url` (use
   `10.0.0.10`) is not screened open (no network needed). Optional nit: in the existing data-level test's private list,
   use `10.0.0.10` instead of `192.168.1.10`.
7. **uaqox.** On a refused reservation, call `land_reply(&ask, None, None)` (or the closest equivalent) so the refused
   ask writes its `data.reply` row, and keep the module doc true. If writing the row would spend anything, word the doc
   "writes nothing" instead and say why. Either way, a test pins it.
8. **Docs.** Core `AGENTS.md`, the data-rule section: one sentence for (1) and (4)/(5) together. Search first; skip
   what `main` already says.

## Tests (each with a planted revert; the first is the key test)
- **`a_private_turn_a_us_model_wins_anyway_shows_no_data_gate`**: a private turn in a mode listing a China-origin
  profile, where the US profile wins on its own: no `data_gate` on the row, footer `None`, counter unchanged; and the
  case where the China-origin profile would have won: gate word, footer and one count. Plant: drop the new check.
- `an_open_only_turn_shows_no_route_reason` (the status line or the route record shows `open`, not `no_verdict`).
  Plant: restore the old `TurnRoute`.
- The `sensitive` end-to-end test. Plant: map `sensitive` to no gate.
- `the_chokepoint_counts_its_refusal`. Plant: remove the call.
- The background counters test. Plant: remove one of the four calls (name which).
- The screen-step turn test (atbgl). Plant: pass `None` for `final_url` in `screen_step.rs`.
- The refused reservation's row (uaqox). Plant: return before the new `land_reply`.
- `cargo nextest run -p theseus-core data`, `... bg_gate`, `... tool_screen`, `... reply_data`, `-p theseus-protocol
  route`, `-p theseus-discord route` pass, then `-p theseus-core` whole.

## Not in this row
- The China-origin config guidance, `[routing.*]`/`[profiles.*]` template blocks and `own_bench/drafts.rs`: settled.
- theseus-ms3hd (the draft's first pass equal to `cn_draft?`), theseus-iilre (the reviewer's programs ask): owner calls.
- The `sensitive_names` template line: an owner call.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Near you, not yet on main (integ/tail5):** tail5-effort edits `turn/route_step.rs` (the `effort::decide` call only),
  `routing/effort.rs`, `config/routing.rs`, the template's `[routing]` lines and core `AGENTS.md`; tail5-threads edits
  `route_step.rs`'s `fn held` and `turn.rs`'s held arm. In `route_step.rs` you edit only the data-gate lines near :860.
  Read `git diff origin/main...origin/cloud/20261021-tail5-effort -- crates/theseus-core/src/turn/route_step.rs` and the
  same for `-tail5-threads` first. Stay out of `routing/effort.rs` and `config/routing.rs`.
- **Rows beside you on `main` 49352e8a, joining `integ/tail6`:** tail6-onto (core `judge/onto/*`, `config/ontology.rs`,
  the template's `[ontology]` lines, the cockpit's Ontology view, `digest.rs`) and tail6-tests (`bench/recall/*`, core
  `scratch.rs` and the mint test fixtures). Stay out of their files.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; this row adds no stored field (if it truly must, see the note above).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus-core/AGENTS.md` (the data rule, routing, telemetry) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id and item; where the "gate decided" test lives and why; how the open-only
turn's route line reads now; reply data's refused path (row or "writes nothing") and why; each test and its plant with
the failing line; the keel guard's output; the store format; a `docs/status.md` sentence for the integrator (the footer's
owner-visible change); follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
