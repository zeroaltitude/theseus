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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7b-golden`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `ae420a49` (2026-10-22: release stage 6, install #74, which landed `integ/tail6` on top
of stage 5's `f6f2900e`); that is your base. Line numbers quoted below are at `ae420a49` but may still be off by a few:
search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `f6f2900e`** (`git log --first-parent f6f2900e..HEAD` lists it):
- `ae420a49`: `integ/tail6` (the paced stand-in model; the ontology sweep's titles off the worker, its pair rule, kept
  kind names and the cockpit's renames; the thread growth race, the held row's timing, confirms and the budget question
  naming their turn; the data-gate word only when the gate decided, its counters; the task home's restart test and
  `sticky_unsure`; the recall bench's later-session plan and read-only test trees).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500 (add nothing)
- `crates/theseus-discord/src/render.rs` 2,975 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/turn.rs` 3,467 / 3,525
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you:** nothing else is integrating. Two waves of rows base on `ae420a49` and join the lane
`integ/tail7`: the four `tail7-*` rows started an hour before you (ontology sweep, effort words, threads/TUI, recall
bench and data-gate word), and the six `tail7b-*` rows of this wave. A local lane also edits `scripts/keel-guard.py` and
its test: touch neither. Each row owns the files named in its task's "Rules for this row" and "Not in this row"
sections. Those win over anything older. Stay out of the other rows' files.

**The Discord DM task flow is locked** (the owner, 2026-10-22: "please do not change how it is now, all of the little
touches are so good"): the task reply, the started and thread lines, the board, the forum posts and live cards, the
approval card and the task reports. No row in this wave changes their text, order or buttons. If your change would move
one of them, stop and say so in the report instead.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail7b-golden, a golden test of the whole Discord DM task flow on the fake Discord, so no change can drift it (theseus-hd8l2)

Branch: `cloud/20261021-tail7b-golden`. Every commit's subject carries `theseus-hd8l2`. Deadline for the report:
3 hours after you start.

**Why.** The owner walked the Discord DM task flow on 2026-10-22 and said: "OK, that worked AMAZINGLY well -- please do
not change how it is now, all of the little touches are so good". The flow is now locked: any intentional change to it
needs the owner's OK first. Today it is held by many separate tests, each asserting a part (a board golden, the
approval card's inline text, link lines, post counts). Nothing holds the whole flow, so a change elsewhere (a new
message convention, a policy change, a render tweak) could alter its text or its order and every part-test could still
pass. This row adds ONE golden test that drives a whole DM task on the fake Discord and records every message, in
order, with its buttons and its edits, so any drift fails one named test. The one change the owner approved since the
walk is already on your base: the approval card's plain-words layout (`the_approval_card_reads_as_the_owner_approved_
it`): the golden records it as it is.

**Risk: low.** Tests only; no runtime code changes (a test-only seam at most).

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins the lane `integ/tail7`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7b-golden` | theseus-hd8l2 | one golden test of the DM task flow (request -> reply -> started/thread lines -> post and live card -> approval card -> press -> report -> board), normalized, with the `THESEUS_GOLDEN=write` convention | a NEW `crates/theseus-discord/src/tests_dm_flow_golden.rs` (one `mod` line in the crate's test list), its golden file under `crates/theseus-discord/src/golden/` (or beside `render/golden/`), a small normalizer in the test file; a test-only helper in `crates/theseus-sim/src/fake_discord/` only if the fake cannot list edits in order (say so) | M |

## What exists (at `ae420a49`; search by name)
- The fake Discord: `crates/theseus-sim/src/fake_discord.rs` (2,484 lines: at the 2,500 limit; add nothing there, use
  a submodule under `fake_discord/` if you need a helper): `FakeDiscord::start` near :608, `start_with_gateway` near
  :758, `Msg`, `Button`, `Typed`, `Pressed`, `Guild`; submodules for channels, threads (`FakeThread`), commands, events,
  emojis. The fake gateway: `crates/theseus-sim/src/fake_gateway.rs`.
- Rigs to copy (do not edit them; another row edits `tests_task_home.rs` and `tests_gateway.rs`):
  `crates/theseus-discord/src/tests_gateway.rs` (`Rig` with `fake: Arc<FakeDiscord>` near :52, built near :115, helpers
  in `tests_gateway/`), `tests_task_threads.rs` (`rig` near :286, `until` near :124, `replied` near :141),
  `tests_task_home.rs` (`rig` near :30, `boards`, `cards`, `report`).
- Whole-task tests to model the drive on: `tests_default_forum.rs`
  `a_dm_task_works_in_its_post_and_the_dm_gets_one_link_line` (near :393), `a_dm_task_with_a_channel_gets_an_index_
  card_linking_it` (near :859); `tests_task_home.rs` `the_task_home_holds_the_board_reports_and_one_pointer_per_task`
  (near :285), `a_tasks_card_goes_to_its_post_not_the_channel` (near :399); `tests_walk1b.rs`
  `a_dm_under_notify_starts_two_tasks_and_both_are_seen`; `tests_task_owed.rs`
  `a_fast_task_has_exactly_one_post_in_its_final_state`; `tests_task_threads.rs`
  `a_forum_post_is_the_tasks_live_card_and_its_tags_follow_its_state` (near :760).
- Golden conventions: `THESEUS_GOLDEN=write` rewrites a golden instead of comparing (`crates/theseus-core/src/
  tests_output.rs` `the_cores_output_matches_its_golden` near :570, write branch near :580; `THESEUS_GOLDEN_TO`
  redirects); the Discord board golden `render/board_v2.rs`
  `the_boards_for_0_1_8_and_30_tasks_needs_you_and_all_done_are_as_they_were` near :768 with
  `render/golden/board_v2.txt`; the approval card's inline expected text `render/approval/tests_approval.rs`.
- Where the flow's messages come from (read only): Discord `render.rs` (`card`, `report` near :991, the 🔔 notified
  lines), `render/approval.rs`, `render/board.rs` (`BOARD_HEAD`), `runtime/threads.rs` (the 🧵 and "🔔 started task"
  lines near :713/:721), `runtime/task_home.rs` (the pointer line near :502), `runtime/forum.rs`, `runtime/live.rs`,
  `runtime/live_board.rs`, `runtime/start_task.rs`, `courier.rs` (`report_post`, `failed_post`); core
  `needs_you/words.rs` (the card's words).

## What to build
1. **The drive.** On the fake Discord with the owner's shape (a DM place, the task home and forum as the default
   config makes them: check `tests_default_forum.rs` for the default), drive ONE task end to end with the stand-in
   model: the owner's DM message asks for a task; the task starts; it makes one call that needs approval (a write
   outside its tree, as in the approval card's tests) so the approval card appears in the task's post; the test presses
   **Approve** (one of the three buttons); the task finishes and reports; the board updates. If one task cannot show
   every part (for example the index card needs a channel), drive a second, short task in the same test and record it
   after the first.
2. **The record.** Every message the fake saw, in the order Discord would show it, as one text: for each create and
   each edit, one block with its place (DM, the task's post, the board, the index), the message's text, its embeds'
   text, its components (button labels and styles, in order), and whether it was an edit of an earlier block (by its
   ordinal, not its id). Reactions and pins if the flow makes any.
3. **Normalize** only what changes run to run: snowflake ids -> ordinals (`<#post-1>`, `<@owner>`), task ids -> `task-1`,
   timestamps and `<t:..:R>` -> `<time>`, durations -> `<dur>`, temp paths -> `<tmp>`, token and dollar counts if the
   stand-in model makes them vary (check: if they are deterministic, keep them). Keep every emoji, every word, every
   line break and the order. Say in the report exactly what is normalized and why each is unstable.
4. **The golden file** and its comment header: "The Discord DM task flow, locked by the owner on 2026-10-22. Change this
   file only with the owner's OK (`THESEUS_GOLDEN=write cargo nextest run -p theseus-discord dm_flow`)." Write the same
   rule in `crates/theseus-discord/AGENTS.md` (two lines, beside the task-flow section).
5. **Determinism.** Run the test 20 times (`cargo nextest run -p theseus-discord dm_flow --stress-count 20` does not
   fail on a red: count passes from its output, or loop it yourself) and 5 times under the load recipe; it must be green
   every time. Wait on events (`until`), never on sleeps.

## Tests
- **`the_dm_task_flow_is_as_the_owner_approved_it`** (the golden test).
- Plants, each must fail the golden test with a readable diff (show the first diff lines in the report):
  (a) change one word of the started line in `runtime/threads.rs`; (b) swap the order of two buttons on the approval
  card; (c) drop the board's header emoji; (d) make the report post twice. Restore and `touch` after each.
- Every existing Discord test green: `cargo nextest run -p theseus-discord` whole, and `-p theseus-sim`.

## Not in this row
- No change to any message's text, order, or buttons: this row only pins them. If the drive shows something that looks
  like a bug (a duplicate post, a missing line), record it in the golden as it is and name it in the report for the
  owner; do not fix it here.
- Do not edit the existing rigs or tests (`tests_gateway.rs`, `tests_task_home.rs`: tail7-effort edits their rigs;
  `tests_default_forum.rs`, `tests_task_threads.rs`, `render/*` tests). Copy what you need into your new file.
- The other wave rows' files: tail7b-hold (`external.rs`, `PolicyConfig.external_text`, the template), tail7b-budgets,
  tail7b-tiers (core turn path, recall, tools deferral), tail7b-cn (`draft*`), tail7b-messages (proactive messages
  outside the task flow; it must not move your golden). The tail7 rows: tail7-onto, tail7-effort (Discord
  `tests_task_home.rs` / `tests_gateway.rs` rig only), tail7-threads (the TUI), tail7-recall.
- `crates/theseus-discord/src/runtime.rs` (3,494 / 3,500): touch nothing. `scripts/keel-guard.py` and its test (a
  local lane).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; nothing here is stored.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented (the task's title, the
  file it writes).
- Read the root AGENTS.md, `crates/theseus-discord/AGENTS.md` and `crates/theseus-sim/AGENTS.md` (the fake Discord)
  first. Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the drive (what the test does, step by step); the golden file's path and its full text in a code
block (the owner reads it to confirm this is the flow he approved); what is normalized and why; the 20-run and
under-load results; each plant with the first lines of its diff; anything in the flow that looked wrong; the keel
guard's output; the store format; a `docs/status.md` sentence for the integrator; a note for the integrator: tail7b-
messages and every later row must leave this golden unchanged, so run it after each merge.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
