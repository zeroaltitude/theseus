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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail7b-messages`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: tail7b-messages, one short convention for every message Theseus sends the owner unasked (kind tag + operational title + trigger hint), applied and tested, OUTSIDE the locked Discord DM task flow (theseus-x8b9k)

Branch: `cloud/20261021-tail7b-messages`. Every commit's subject carries `theseus-x8b9k`. Deadline for the report:
4 hours after you start.

**Why.** The owner, 2026-10-22, quoting the nightly self-improvement backlog digest ("Self backlog, last night: - filed:
Calls from harness fail (tsk_...) ... Reorder or drop with `theseus self move <item> <rank>` ..."): "these messages need
more context. I don't know what's happening when they are omitted. Please do a review of all of these wonderful
structured Theseus reach out messages, and ensure that a human coming to them for the first time would generally know
their purpose (not by including a bunch of explaining text on each one; more like with an operational title and a hint
at what triggered them, and what the human should do -- FYI (don't do anything, just telling you), Action item (you're
supposed to do something), etc.)"

**The scope cut (the owner, later the same morning, after walking the Discord DM task flow):** "OK, that worked
AMAZINGLY well -- please do not change how it is now, all of the little touches are so good". So the Discord DM task
flow is OUT of this row and must not move by a byte: the task reply, the started and thread lines (🔔 / 🧵), the task
board (📋), the forum posts and live cards, the approval and question cards (`needs_you`, its words and its ping), the
budget question card, the place-channel budget notice, and the task reports and failure posts. A sibling row
(tail7b-golden) pins that flow with a golden test; if your change would move any of it, you have gone out of scope.

**Risk: medium.** Many owner-visible texts change; each family gets a test.

**Prerequisite: base `origin/main` at `ae420a49`.** Your branch joins the lane `integ/tail7`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail7b-messages` | theseus-x8b9k | inventory; one convention and one helper; apply it to every unasked message outside the task flow; a test per family; the catalog in docs | a new core module for the helper (e.g. `owner_note.rs`, one `mod` line), the producers listed below (their text only), their tests, `crates/theseus-discord/src/render.rs` `restarted` only (34 lines of room: add at most 4), `crates/theseus-discord/src/runtime/voice/deaf.rs` (its text constants), core AGENTS.md (a short "Messages to the owner" section) or a new `docs/messages.md` (the catalog) | L |

## The inventory to start from (at `ae420a49`; search by name; core paths under `crates/theseus-core/src/`)
Nearly every core producer goes through `Outbox::to_operator` (`outbox.rs` near :776) or `stage_to_operator` (near
:787) with a body `{"kind": .., "text": ..}`; Discord routes each kind in `crates/theseus-discord/src/courier.rs` (the
match near :812-912: `jev_post` -> the owner's DM for `jev_notice`, `spend_ceiling`, `self_digest`, `self_keel`,
`self_join`, `self_narrow`, `initiative`, `digest`, `privacy_warning`, `onto_card`, `home_size`, `earned_card`,
`seal_card`, `link_card`, `retraction`; `operator_channel` for `restarted`, `mcp_changed`, `disk`). There is no shared
header helper for owner messages today (each producer writes its own first line).

| # | Message | Where it is built | In scope |
|---|---|---|---|
| 1 | Self backlog, last night | `rsi/backlog/start.rs` `backlog_tell` near :438 (kind `self_digest`) | yes (the owner's example) |
| 2 | Single backlog lines | `fact/rsi_backlog.rs` near :52-159; CLI `crates/theseus/src/self_cmd/backlog.rs` near :96 | yes (the DM/TUI line) |
| 3 | Weekly self digest | `rsi/digest.rs` `post_self_digest_if_due` near :82 | yes |
| 4 | Self notice | `rsi/digest/notice.rs` `post_self_notice_if_due` near :340 | yes |
| 5 | Self-join card | `rsi/join.rs` `card` near :342; `rsi/join/install.rs` near :200 | yes |
| 6 | Keel stop | `rsi/pr.rs` `keel` near :471 | yes |
| 7 | Revert card | `rsi/revert/doing.rs` `revert_card` near :237 | yes |
| 8 | Narrowed policy card | `rsi/narrow.rs` `narrow_card` near :130 | yes |
| 9 | Home-size warning | `turn/home_size_step.rs` `home_size` near :188 | yes (keep its runnable split command) |
| 10 | Privacy warning | `turn/personal_step.rs` `warn_owner` near :226 | yes |
| 11 | Budget notice (place channel, in a turn) | `turn/budget_step.rs` `notice` near :177 | NO (task flow) |
| 12 | Budget question card | Discord `render.rs` `card` near :808 | NO (task flow) |
| 13 | Day spend ceiling | `day_ceiling.rs` `day_refused` near :420; `turn/ceiling_step.rs` near :94 | yes |
| 14 | AWS month warning | `aws/month_warn.rs` `aws_month_warned` near :73 | yes |
| 15 | AWS hands alert / runaway | `aws/hands/watch.rs` `refresh` near :134; `aws/hands/runaway.rs` `enter` near :274; `aws/hands/grid.rs` near :203 | yes |
| 16 | Google lapse notice | `google/tend.rs` `google_lapse_told` near :42 | yes |
| 17 | Morning note | `initiative/morning.rs` `morning_note` near :162; `initiative/morning_google.rs` near :119 | yes (its first line only) |
| 18 | Initiative and digest posts | `initiative/speak.rs` `initiative_post` near :205; `initiative/digest.rs` `digest_post` near :125 | yes |
| 19 | Ontology mint/filing cards | `judge/onto/stage.rs` `card` near :87 | yes |
| 20 | Ontology filed-late and night lines | `judge/onto/mod.rs` `filed_late` near :107; `judge/onto/digest.rs` `lines` near :176, `onto_night_lines` near :251 | yes |
| 21 | Jev notice | `judge/notice.rs` `post_notice` near :382 | yes |
| 22 | Restart notice | `outbox.rs` `post_restart_notice` near :1204; Discord `render::restarted` near :1061 | yes |
| 23 | Bind notice | Discord `runtime/succession.rs` `bind_notice` near :259 | yes |
| 24 | Reminder / wake fired | `wake_home.rs` `reminder_body` near :206; `rpc/routines_watch.rs` near :425 | yes (a reminder's own text is the owner's: tag the line, never reword his words) |
| 25 | Voice deaf / lost / rejoined | Discord `runtime/voice/deaf.rs` `notice` near :312, constants near :27-37 | yes |
| 26 | Disk crossing | `rpc/disk_watch.rs` `notice` near :15 | yes |
| 27 | Approval/question ping (needs-you) | `needs_you.rs` `post_needs_you` near :191; `needs_you/words.rs` | NO (task flow; the owner approved its card) |

Find more with: `stage_to_operator`, `to_operator(`, `outbox.post(`, `json!({"kind": "notice"` (13 sites),
`pacer::UNPROMPTED` (`pacer.rs` near :27), `"kind": "` in `crates/theseus-core/src`, and the TUI's notices
(`crates/theseus-tui`: search `notice`, `toast`). Add every unasked message you find to the inventory; mark each "in"
or "out (task flow)" with the reason.

## What to build
1. **The inventory** (in the report and in the catalog): for each message: where it is built (file:fn), what triggers
   it, its channel, its kind, and what the owner is meant to do.
2. **One convention, kept SHORT.** First line: a kind tag and an operational title, for example
   `ℹ️ FYI · Self-improvement backlog: 6 new items from last night's failures`,
   `✋ Action · Approve the self-join of …`, `❓ Decision · …`, `⚠️ Alert · Voice can't hear the channel`.
   Kinds: **FYI** (nothing to do), **Action** (you need to do something), **Decision** (choose), **Alert** (something
   is wrong). Second line: a one-line trigger hint in plain words ("from last night's dream over yesterday's ledger",
   "the day's spend passed $200"). Then the body. The action, if any, last, as a command or a button. Items inside a
   digest get one plain-words line each (what failed, how often, since when), never internal labels like "Calls from
   harness fail" or a bare `tsk_...` id without what it is. No paragraphs of explanation. One helper builds the first two
   lines (for example `owner_note::head(Kind, title, trigger)`); every in-scope producer uses it, so the next message
   follows by construction.
3. **Apply it** to every in-scope message. Keep every fact a message carries today (counts, commands, links, ids where
   the owner needs them); change the framing, not the content. Keep each message's kind/route in the courier as it is
   (no channel moves).
4. **The catalog:** a short section in core AGENTS.md ("Messages to the owner": the convention, the helper, the rule
   that every new unasked message uses it, and that the task flow is locked) and the full inventory table in a new
   `docs/messages.md` (or in the AGENTS.md section if short). Do not edit `docs/status.md`, the README or the spec.

## Tests
- **`every_unasked_message_starts_with_a_kind_and_a_title`** (key): one test that builds each in-scope family's message
  (through its producer with a small fixture, or its pure text function) and asserts the first line matches
  `^(ℹ️ FYI|✋ Action|❓ Decision|⚠️ Alert) · \S` and the second is a non-empty trigger hint. Plant: drop the helper
  from one producer -> the test names that family.
- A test per family where the message has its own test today (update those expectations; list each changed assertion
  in the report with before/after). The home-size warning keeps `the_warning_names_the_place_and_runs_as_written`'s
  promises (human place name, a runnable split, no placeholders).
- **The locked flow does not move:** the Discord task-flow tests stay green UNCHANGED (`tests_default_forum.rs`,
  `tests_task_home.rs`, `tests_walk1b.rs`, `tests_task_owed.rs`, `tests_forum_archive.rs`, `tests_task_threads.rs`,
  `render/board_v2.rs`'s board golden, `render/approval/tests_approval.rs`'s
  `the_approval_card_reads_as_the_owner_approved_it`). If one of them needs an edit, you touched the flow: undo it.
- `the_cores_output_matches_its_golden` (`tests_output.rs`, run with `TZ=America/Phoenix`) and the CLI goldens
  (`crates/theseus/tests/golden.rs`): update only the lines your change moves, and list them.
- `cargo nextest run -p theseus-core`, `-p theseus-discord`, `-p theseus`, `-p theseus-tui`.

## Not in this row
- The Discord DM task flow (above), and every file only it uses: `crates/theseus-discord/src/runtime.rs` (3,494 / 3,500:
  add nothing), `runtime/threads.rs`, `runtime/task_home.rs`, `runtime/forum.rs`, `runtime/live*.rs`,
  `runtime/start_task.rs`, `runtime/tasks.rs`, `runtime/answer_cmd.rs`, `render/board*.rs`, `render/approval*`,
  `render/task_view.rs`, `render/card_v2.rs`, `render.rs` beyond `restarted`, `courier.rs`'s `report_post` /
  `failed_post`, `needs_you*.rs`, `turn/budget_step.rs`.
- The other wave rows' files: tail7b-hold (`external.rs`, the policy notice "Ran with a notice": out of scope, it is a
  tool line), tail7b-budgets (`turn/detour_step.rs`, `turn/defer_step.rs`, `config/threads.rs`), tail7b-tiers
  (`turn/route_step.rs`, recall, `config/memory.rs`, `config/defer.rs`, protocol `events.rs`), tail7b-cn (`draft*`),
  tail7b-golden (a new Discord golden test file; you edit no Discord test it adds). The tail7 rows: tail7-onto (core
  `judge/onto/{judged,renamed,mint}.rs`, `tests_onto_names.rs`, the cockpit's ontology files: you edit
  `judge/onto/{stage,mod,digest}.rs`: different files, same module; keep your edits to the text), tail7-effort
  (protocol `route.rs`, `routing/effort.rs`, Discord `tests_task_home.rs` / `tests_gateway.rs` rig only), tail7-threads
  (the TUI's `app.rs`, `chat/task_picker.rs`: if a TUI notice lives there, leave it and name it), tail7-recall
  (`bench/recall/*`, `scratch.rs`, `turn/data_step.rs`, `rpc/mod.rs`).
- `scripts/keel-guard.py` and its test (a local lane).

## Rules for this row
- Model roles: you design and write it; keep it minimal and elegant. Every line you add should earn its place. Short
  words, plain English; the convention must make messages clearer, not longer: count each message's characters
  before/after and keep the growth to the two head lines.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Keel.** Do not delete, ignore or weaken a test (changing an expected text to the new convention is not weakening;
  dropping an assertion is), and do not raise a budget, cap or ceiling. A cloud row cannot carry a `Keel:` trailer.
  Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its
  output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; a message's text is not a stored layout.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it). Commit per family group so a review can take them apart.
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read the root AGENTS.md, `crates/theseus-core/AGENTS.md` and `crates/theseus-discord/AGENTS.md` (the outbox, the
  courier, the task flow) first. Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`
  before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the inventory table (in/out and why); the convention and the helper; **for every in-scope message,
its text before and after** (one fixture each, in a code block; the owner reads this section to approve the look);
the character counts; the changed test expectations and goldens; the task-flow tests unchanged (their names and
results); each plant with the failing line; the keel guard's output; the store format; a `docs/status.md` sentence for
the integrator; follow-ups (messages you found but left, and why).

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
