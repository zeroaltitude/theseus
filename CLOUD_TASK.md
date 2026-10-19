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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-needs-you-cards`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: needs-you-cards, every question that reaches the owner's DM is answerable there, in plain words (theseus-suvpe)

Branch: `cloud/20261019-needs-you-cards`. Every commit's subject carries `theseus-suvpe`. Deadline for the report: 5
hours after you start.

**Why.** At 05:31 the owner's DM got "🔔 3 questions need you": one post, no buttons, each line the raw reason a tool
call was held, cut at 120 characters (paths, ids, a word cut to "fa…"), each asker a raw task title such as "Fix the
cause `action.failed:harness`: …", and the only way to answer a `theseus answer <id> approve` typed in a shell. He asked
why it reads like that, and why he must run a command to answer. Nobody answered; the questions expired after 15 minutes
and the work they held was lost. The owner's frame: tune the mechanism so Theseus operates better in the channel, not
only nicer words. A question that reaches the DM is there to be answered, in the DM, in one tap or one word.

**Risk: medium.** Presses and replies answer real approvals. The core judges every answer as it does today (the owner,
from a private place); nothing here widens who may answer or what an answer does.

**Prerequisite: `main`.**

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| D1 | `needs-you-cards` | theseus-suvpe | each DM question its own answerable card (a batch too); plain words from structured fields; a DM reply answers the question it replies to; human askers for self tasks | `theseus-core/src/needs_you.rs` (+ a module beside it if the words grow), `theseus-discord/src/courier/ask.rs`, `render/ask.rs`, `runtime/ask.rs`, a new `runtime/reply_answer.rs` (or the name you choose), `courier.rs` (the `"needs_you"` arm and one call in the landed-write hook only), `tests_gateway/ask.rs` and a new `tests_gateway/` file, `theseus-core/src/tests_m3/ask.rs` (the batch test's expectations) | M |

## What exists (on your base; lines at `5e0bd195`)
**The core's post** (`crates/theseus-core/src/needs_you.rs`):
- `escalate_questions` (near :51): a question is due after `wait_ms` (`config/notify.rs` near :73: 60 s with no screen
  open, 300 s with one) and before its expiry; questions already carded or pinged are skipped. A post goes only when the
  outbox's pacer opens (`[notify] unprompted_pace_secs`, 1,800 s; `pacer.rs` near :34), so everything due inside one
  window goes in one post.
- `post_needs_you` (near :114): one question gets `body_of`, several get `batch_body_of`; written with
  `outbox.stage_to_operator`.
- `asker_name` (near :131): the request's `task.title`, else `task <short>`, else the session record's `title`, else
  "Theseus". For a self task the approval's `TaskRef.title` is `None` (`toolrun.rs` near :1387-1391) and the session's
  title comes from its first turn, the backlog's objective ("Fix the cause `<key>`: <title>.", `rsi/backlog.rs` near
  :153). The backlog item itself is a `TaskRecord` whose `title` is already human ("Calls from harness fail",
  `rsi/backlog/read.rs` near :176-179); the self session is opened with label `self: <title>`
  (`rsi/backlog/start.rs` near :116).
- `body_of` (near :150): `prompt` is the ask's question, else `cut_marked(req.reason, 300)`; fields `kind`, `question`,
  `text`, `asked_by`, `prompt`, `choices`, `expires_at_ms`; an approval's text ends with a `theseus answer <short>
  approve` hint. `batch_body_of` (near :191): the 120-character cuts (near :197, :206), at most 10 lines, fields `kind`,
  `text`, `questions[]`, documented as "no buttons and no settle: a nudge, not a card".
- The inputs: `ConfirmRequest` (`theseus-protocol/src/lib.rs` near :2409: `tool`, `input`, `resource`, `reason`,
  `task`, `change`, `external_text`, `options`, `preview`, `grant_offer`) and `TaskRef { task_id, short, title }` (near
  :1823). `crate::rpc::answer::kind_of(req)` gives the question's kind.
- Core test: `tests_m3/ask.rs` near :391 `unprompted_pings_go_one_a_window_and_the_rest_together` asserts the batch's
  shape and "the batch is no card".

**The Discord side** (`crates/theseus-discord/src/`):
- `courier.rs` near :894 sends `"needs_you"` to `needs_you_post` (`courier/ask.rs` near :42), always to the owner's DM
  (`owner_dm`). A batch is one `Write` keyed `note:<first question>` with `Buttons::Keep` and `"settled": "a batch has
  no card"`; a single post is keyed `confirm:<question>` with `Buttons::Keep` unless it has choices (`Buttons::Ask`).
  Content: `render::ask::needs_you` (`render/ask.rs` near :25).
- `courier/ask.rs` near :21 `card_buttons` is the classic card's choice (`Ask`, `Accept`, `ConfirmTrust`,
  `ConfirmGrant`, else `Confirm`); the needs-you post does not call it. `Buttons` is `render.rs` near :53-88.
- Approve/Decline today: `confirm_buttons` (`runtime.rs` near :1772), ids `confirm:approve|trust|decline|<grant
  verb>:<correlation>`, parsed by `parse_confirm_id` (near :1603); a press (near :1254-1345) is `action.confirm` with
  the presser's `DiscordOrigin`; a refusal tells the presser alone and keeps the buttons; the outbox's settle edits the
  card. An ask's press: `ask:<n>:<question>` (`runtime/ask.rs` near :30, :61) is `work.answer { question_id, option }`.
  The V2 cards' `wa:<last6>:<option>` (`render/card_v2.rs` near :23) also go through `work.answer`
  (`runtime/live_board.rs` near :188, :311).
- The core's judgement: `rpc/answer.rs` near :283 `work_answer`, `may_answer` (near :149) via `places::owner_in_private`;
  option ids `approve`, `decline`, `approve_trust`, `accept`, `reset`, `ack`, `choice:<i>` (near :69-95); a question
  can be named by a unique end of 4 characters or more (near :25, `resolve_question` near :194).
- Components V2 helpers: `render/v2.rs` (`text`, `button`, `row`, `section(texts, accessory)` near :99, `container`,
  `separator`, `validate` near :179: 40 components, 4,000 characters, 100-character ids), `render/card_v2.rs`
  (`waiting`, `closed`, `summary`), `render/answer_v2.rs` (`controls`: buttons in rows of five or a select past five;
  `note_row`), `courier/v2.rs` (`create_v2`/`edit_v2`, `card_may_ping`, `v2_settled`; gated by `Shared::v2_cards()`,
  `runtime/live_board.rs` near :47, which needs `[discord] live_board`).
- **DM replies today:** `on_message` (`runtime.rs` near :1094) calls `tell_replies` (`runtime/reply.rs` near :14),
  which only feeds the core's reply watches. `Replies` (`runtime/route.rs` near :33) keeps message to turn for `turn_…`
  keys only. Every landed write passes `courier.rs` near :1642 (`self.shared.noted(m, w.channel, &w.key)`), and the
  lane keeps `msgs: key -> (channel, message)`: that is where a message-to-question map can be fed. `addressed_thread` /
  `thread_of_reply` (`runtime/thread_bind.rs` near :156-180) show how a reply is caught before it becomes a turn.
  `runtime/answer_cmd.rs` is the `/answer` slash command (near :271 calls `work.answer`).
- Tests: `tests_gateway/ask.rs` (159 lines; the rig sets `live_board = false` near :33, calls
  `core.escalate_questions(at + 61_000)` near :86, presses with `r.press(&post.id, "2. Ship it…", …)` near :101, checks
  the ledger's `action.confirm_answered` and the edit to "Answered" with no buttons). The stand-in:
  `theseus_sim::fake_discord` (`press` near :953 finds a button by label or id; `say_reply(&Typed{…}, to_msg_id)` near
  :834, used in `tests_gateway/threads.rs` near :145). The stand-in holds V2 messages to Discord's rules with its own
  validator. crates/theseus-discord/AGENTS.md names each feature's scenario tests through the stand-in: owner-ask is
  `tests_gateway/ask.rs`.

## What to build
1. **Every question in the DM is a card you can answer.**
   - A single question: its own card with **Approve**, **Decline** and **Details** (an ask: its choices, as today,
     plus Details). Approve and Decline are `work.answer` with `approve` / `decline` (or `action.confirm`, whichever
     the classic card's press already uses for that kind: reuse it, do not write a second path), as the presser, with
     their `DiscordOrigin`. **Details** answers the presser alone (ephemeral) with the full request: the tool, its
     target, the preview when there is one, the rule that held it, the asker, the expiry, and the question's id.
   - A batch (several due in one pacer window): one header line ("3 questions need you") and **one card per question**,
     each with its own buttons and its own settle; or one V2 message with a section and buttons per question when V2
     cards are on. Choose one, say why in the report, and keep it to one code path. The pacer's rule stays: one ping a
     window; the cards after the first post silently.
   - **Settle:** an answer from anywhere (a press, a reply, the CLI, the web UI) edits that question's card to say who
     answered and how ("Approved by the owner from the CLI, 08:41"), its buttons gone; an expiry says it expired. The
     batch's "a batch has no card" settle goes.
   - **Rollback:** `[notify] dm_cards` (default on): off, the DM gets today's posts byte for byte. A test shows it.
2. **Plain words from structured fields, never the raw reason.** Each card says, in at most three short lines:
   - **what** it wants: the tool in words and its target ("edit `crates/theseus-core/src/compiler/effort.rs`", "run
     `cargo test -p theseus-core`", "spend $4.00 more on task 7f3a2c");
   - **why** it asks: who asks (below) and the rule that held it, in words ("its file is under the state directory,
     which asks before a change");
   - **what yes does** ("Yes runs this one edit. Trust also allows edits under this tree for the task.").
   Build these from `ConfirmRequest`'s fields and the decision's structured parts. If a part you need is only inside
   `reason`'s text, add the structured field to the request (a protocol type in a module file of its own, a `pub mod`
   line in `lib.rs`), never parse the reason. If the request is stored, follow the store's version rule (contract 5).
   The CLI hint goes from the card (the CLI's own `theseus answer` lists stay as they are).
3. **A reply in the DM answers the question it replies to.** The owner replies (Discord's reply) to a question's card
   or to the batch header:
   - to a card: `yes`, `y`, `approve`, `ok` approve; `no`, `n`, `decline` decline; an ask's number or its choice's text
     picks that choice; `trust` is approve-and-trust where the card offers Trust;
   - to the batch header: `approve 2`, `no 1`, `yes all`, `no all`; a bare `yes` to a header with more than one open
     question answers nothing and says, to the owner, which numbers are open;
   - anything else is an ordinary message, exactly as today (a turn), so a sentence that starts with "yes, but…" stays a
     message: match the whole reply, after trimming and case-folding, against the short forms only.
   Each answer goes to the core as `work.answer` with the replier's `DiscordOrigin`, so the core judges it as a press.
   A refused answer tells the replier; an answered one is acknowledged by the card's settle (and a ✅ reaction on the
   reply). Keep the message-to-question map in memory (fed where every landed write is noted); after a restart a reply
   to an older card is an ordinary message, and the report says so.
4. **Human askers.** `asker_name` gives a self task as "Theseus's self-repair: <the backlog item's title>" ("Theseus's
   self-repair: Calls from harness fail"), from the task record's own `title` (walk the session's execution to its task);
   a user task keeps its title; never the objective's "Fix the cause …" text. Add no line to `toolrun.rs`.

## Tests (each with a planted revert; the first is the key test)
- **`a_dm_question_is_answered_by_its_cards_approve`** (`tests_gateway/`, through the stand-in): an approval escalates
  to the DM; its card has Approve, Decline and Details; pressing Approve as the owner answers it (the ledger's
  `action.confirm_answered` with `approve`, the held call runs), and the card is edited to the answer with no buttons.
  Plant: post the single question with `Buttons::Keep` again.
- `a_batch_is_one_card_per_question`: three questions due in one window give one header and three answerable cards (or
  one V2 message with three sections); answering the second leaves the other two open; one ping in all. Plant: restore
  the one-post batch.
- `a_reply_answers_the_question_it_replies_to`: `yes` as a reply to a card approves it; `approve 2` and `no 1` as
  replies to the header answer those two; `yes` to a header with two open answers nothing and lists them; `yes, but
  wait` and a reply to a non-question message are ordinary turns. Plant: drop the reply hook.
- `a_reply_from_anyone_else_is_refused` (a collaborator's reply in a shared place, or a non-owner's DM): the core
  refuses, nothing is answered. Plant: answer without the replier's origin (as the binding).
- `the_card_speaks_plain_words` (render unit tests, Discord side, plus `body_of` tests in the core): an edit, a run, a
  spend question; no raw reason text, no `fa…` cut mid-word, the three parts present. Plant: put `reason` back as the
  prompt.
- `a_self_tasks_question_names_its_backlog_title`: a self task's question is asked by "Theseus's self-repair: Calls
  from harness fail", never "Fix the cause". Plant: the session's title again.
- `dm_cards_off_is_todays_post`: with `[notify] dm_cards = false`, the single and the batch posts equal today's bytes.
- Update `tests_m3/ask.rs`'s batch test to the new shape without weakening what it checks (one ping a window, every
  question named, none pinged twice).

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree, this task is right: `main` is at `5e0bd195` (2026-10-19 06:43 MST:
v1's tag `bbc65530` plus five merges) or later, with **store format 45** (a join that lands while you clone may move it
by one). The `file:line` references in this task are at `5e0bd195`; on your base a line may have moved a little: search
by name.

Line ceilings at `5e0bd195` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-discord/src/runtime.rs`
**3,497 / 3,500** (full: add at most one line there, and prefer none; another row of this batch may add one too),
`crates/theseus-discord/src/render.rs` 2,963 / 3,009, `crates/theseus-core/src/turn.rs` 3,384 / 3,525,
`crates/theseus-protocol/src/lib.rs` 2,706 / 2,733, `crates/theseus/src/render.rs` 3,108 / 3,128. Unlisted files must
stay at or under 2,500 lines (`scripts/shape.sh`): `crates/theseus-core/src/toolrun.rs` is 2,489 (**add no line to
it**), `crates/theseus-sim/src/fake_discord.rs` is 2,450 (put new stand-in code in a `fake_discord/` module). Never raise
a ceiling and never add a `long-files.txt` entry. Put new code in new modules; in a long file add only the `mod` line, the
field or the call.

## v1 patch1, the owner's Discord and cockpit rows (this batch)

Theseus's v1 is cut. The owner uses it every day through his Discord DM, a voice channel, a forum of task posts and the
cockpit. On 2026-10-19 he looked at what reached him overnight and asked for these four changes (Discord DM, 08:27,
agreeing to the maintainer's recommendation, and asking to tune the questions mechanism so Theseus operates better in
the channel, not only to word things more nicely). His standing rules that bind every row: features ship **on**, with a
named rollback (2026-10-06); right over compatible (one right name, no aliases; stored data stays readable); minimal
prompting: anything a model reads states facts, never instructions (2026-10-09); self-improvement acts first and reports
after, and its safety is the keel, the gates and one-click revert at the self-join, not approvals on its drafts
(2026-10-09).

| Row | Slug | Beads | Area |
|---|---|---|---|
| D1 | `needs-you-cards` | theseus-suvpe | the core's needs-you post (`theseus-core/src/needs_you.rs`, and the asker's name), its Discord card and buttons (`courier/ask.rs`, `render/ask.rs`, `runtime/ask.rs`, a new module for a reply that answers), the DM reply that answers a question |
| D2 | `self-tree-acts` | theseus-kq4z4 | `theseus-core/src/lsp/{mod,edits}.rs` (`judge_start`, the edit hook's `gate`), `theseus-core/src/rsi/own_tree.rs`, `theseus-core/src/toolrun/order.rs` (the `own` argument only), their tests |
| D3 | `task-home` | theseus-lxfz0, theseus-xlxp4, theseus-90qqy | `crates/theseus-discord/src/runtime/{threads,forum,default_forum,task_channels,live_board,board,voice/tasks}.rs`, a new `runtime/task_home.rs`, `courier.rs` (`report_post`, `card()`, `route()` only), `courier/board.rs`, `render/board_v2.rs` + its golden, `render.rs` (`report` only), the `[discord]` config (`config/discord.rs`, the template), the stand-in's channel routes |
| D4 | `fleet-fullscreen` | theseus-uu3b2 | `cockpit/` only: `src/views/Fleet.tsx`, `src/components/TaskGraph.tsx`, a new pure module and its test |

All four run in parallel now. Joins are plain merges, one at a time: D2, D4, D1, then D3.

**Other work in flight today** (stay out of their areas): joins queued on the owner's machine: WS B (the provider-origin
allowlist for private data, routed-turn checkpoints, Jev's warnings: `routing/`, `data_gate.rs`, `catalog.rs`,
`turn/route_step.rs`, `turn/checkpoint_step.rs`, `config/privacy.rs`), the speed rows (the census and first request,
the turn's path before the request leaves, idle memory and binary size), the Google Workspace core (`crates/theseus-google/`,
`theseus-core/src/google/`, one registration line in `toolrun.rs`), turn-end-facts (`toolrun/job.rs`, `turn/`), quick
recall; a cloud row still building job-truths (`theseus-kernel/src/job.rs`, `toolrun/job.rs`, `theseus-tools/src/fs.rs`,
`file_read.rs`); later, the Google mail, drive, sheets and calendar rows and an effort row.

**Contracts between these rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need, and do not reformat files you only read.
2. **`crates/theseus-discord/src/courier.rs` is shared by D1 and D3 at different places.** D1 edits only the
   `"needs_you"` dispatch arm (near :894) and, if it records which message holds which question, the landed-write hook
   (near :1642, `shared.noted(...)`) by one call. D3 edits only `report_post` (near :1354), `card()` (near :1053) and
   `route()` (near :1225). Neither touches the other's.
3. **The DM's needs-you ping is D1's; where a task's card and report land is D3's.** D3 moves a task's approval card
   to its forum post and its report and board to the task home; D1 makes the DM's needs-you post a set of answerable
   cards. Each test asserts only its own side.
4. **`runtime.rs`:** at most one added line per row (a `mod` line or one call); D1 and D3 may each take one.
5. **The store format:** none of these rows is expected to change a store layout. If a row finds it must store a field,
   it says why in the report and bumps by one from its base (the joiner renumbers).
6. **No new model-facing instruction.** Any text a model reads (a tool result) states facts: what exists, where a thing
   went, what happened. It never tells the model what to do, never says "must", and never gives examples.

## Out of scope for every row

The owner's config, his daemon and his machine (his config may get a key at the install; say which in the report).
`docs/status.md`, the README, `docs/design/` and the spec: the maintainer writes those; say what should change and
where. The maintainer's own rehearsal scripts (they are not in this repository). Any benchmark run.

## The checks for this row (these replace the preamble's gate line)

This VM has 4 cores and no sccache, and the maintainer runs the full gate on the owner's machine at the join. So scope
your runs to the crates you touch:
- From the preamble's setup, run `cargo --version` first and alone, then `cargo install cargo-nextest --locked`; skip
  `cargo-deny` and its fetch. Run `npm ci` in cockpit/ only if the row touches cockpit/.
- Before every commit: `cargo fmt --all --check`, `bash scripts/shape.sh`, `cargo clippy CRATES --all-targets -- -D
  warnings`, and `TZ=America/Phoenix cargo nextest run --workspace -E 'FILTER'` (crates/theseus-core/AGENTS.md: use the
  `-E` filter, never `cargo test -p` or `nextest -p`, which rebuild the dependencies with other features). CRATES and
  FILTER are named in "Rules for this row".
- Before your last code commit: `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`, and paste its
  output in the report. It must be empty.
- A full `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` at the end is welcome if your deadline allows,
  never required. Say in the report which checks you ran and their counts.

## Rules for every row of this batch

- **Model roles:** you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller, and make an old config key fail the load with
  "X was renamed to Y". Stored data stays readable.
- **On, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the rollback: a
  test shows that with it off the old behaviour is byte for byte back.
- **Planted reverts, with notes for the reviewer.** Every behaviour change gets a test and a planted revert: break it,
  see the named test fail, restore the file and `touch` it, then `git status`. The row's **key test** (named first under
  Tests) must be shown failing on its plant. In the report, give each plant as a note the reviewer can replay: the file,
  the exact edit (the line before and after), and the test that failed with its failure line. A pure move needs no plant:
  say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. **Never edit a
  keel-guarded file** (`scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`,
  `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`,
  `crates/theseus-core/src/rsi/sealed*`, `crates/theseus-exam/exam/`, and the first request's budgets in
  `crates/theseus-core/src/first_request.rs`): any change there needs a signed commit with a `Keel:` trailer quoting
  the owner's yes, which this session can't make. If your design needs one, stop at that point and put the exact change
  in the report.
- **Signing:** you can't sign here. Commit without `-S`; sign nothing. The maintainer re-signs at the join.
- **First-request budgets stay** (cli/task 8,000, dm 15,000 tokens; `tests_first_request.rs` must pass on the
  defaults). A plain turn stays 4 frames. Nothing new on the path before a request leaves.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner"
  and "collaborator". Fixture names, guild and channel ids, and paths are invented. Run anything that leaves through
  `core.tools.scrubber` as the code already does.
- **Docs in code.** Where your change makes a crate's or the cockpit's AGENTS.md wrong, fix those lines in the same
  commit, in plain words, in the file's existing style. Read the AGENTS.md/CLAUDE.md of every crate you touch first.
- **CLOUD_REPORT.md last.** It is the branch's last commit, as the preamble says, and your final message starts
  `CLOUD REPORT COMPLETE`. Keep it honest: what you ran, what passed, what you could not run here and the exact commands
  for the maintainer.

## Rules for this row
- CRATES: `-p theseus-core -p theseus-discord -p theseus-protocol` (add `-p theseus-sim` if you touch the stand-in).
  FILTER: `package(theseus-core) | package(theseus-discord) | package(theseus-protocol)`, plus `package(theseus-sim)`
  if touched. If you add a protocol type, regenerate cockpit/src/protocol.gen/ as AGENTS.md says (theseus-protocol's
  test writes it) and commit it.
- Config: `[notify] dm_cards` with a default, in `theseusd example-config`'s template and its template tests (the
  template test requires every key to be mentioned).
- The Discord crate's AGENTS.md: update the "A question with options" paragraph to what the DM does now.
