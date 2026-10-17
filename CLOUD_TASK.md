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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-voice-tasks`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

---
## Your task: voice-tasks, a task's question and report spoken in the owner's voice call, and answered by voice (theseus-bh3t.3)

Branch: `cloud/20261017-voice-tasks`. Every commit's subject carries `theseus-bh3t.3`. Deadline for the report: 3 hours after you start.

**The owner's words (Discord, 2026-10-17 09:30, verbatim):** "In tasks: what are discord forums and threads -- I didn't
know about these! Does the discord app now have the power to make new channels? If so, I'd love to start with every
task that's multi step or expected at more than 10 minutes of work to get a channel automatically. I love your other
ideas and not yet implemented ideas so please start those ASAP for quick implementation and join to main today".
This row is one of those "other ideas": while the owner talks with Theseus in a voice call, the tasks keep them in the
loop by voice, and a spoken answer settles a question.

**Prerequisite:** `main` (c6f62abf or later; store format 45). Five sibling rows build in parallel today:
discord-task-channels (theseus-bh3t.1), discord-reactions (bh3t.2), discord-war-room (bh3t.4), task-offer-chip
(bh3t.5), discord-emoji-catchup (theseus-ss36w). None of them touches the voice code; you share nothing hot.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Surface | Depends on | Where | Size |
|---|---|---|---|---|---|---|---|---|---|
| 3 | `voice-tasks` | theseus-bh3t.3 | in a call in a private voice place: a task's new needs-you question and its report are spoken once, short (title + one or two sentences, never tool output), at the next pause; a spoken "approve" / "the second one" / "stop it" while a question is pending becomes `work.answer` / `task.cancel` as the owner | new `dc/runtime/voice/tasks.rs` (+ `voice/tests_tasks.rs`); hooks in `dc/runtime/voice.rs` (≤ 15 lines) and `dc/runtime/voice_status.rs` (the one work watch) | with the voice test fakes: a report during a call is spoken once (`Command::Report`); a question is spoken once; an answer by voice settles the question (core state), as the owner; "stop it" cancels; nothing spoken with no call; nothing spoken in a shared voice place; an unmatched utterance is an ordinary voice turn | Discord voice | none | cloud | M |

## What exists (read these first)
- **Speech out at the next pause already exists**: `Command::Report { text }` ("A task's report, spoken at the next
  pause", `crates/theseus-voice/src/engine.rs:149-157`); a reply or report begins only on the floor and a cut report
  comes back at the next pause (`engine.rs:16-31`; queues `reports`/`queue`, `:323-325`). The call's sender is
  `Call.commands` (`dc/runtime/voice.rs:126-151`), the call is `Voice.call` (`:119`), and `Voice::joined()` (`:281`) says
  whether one is up. The greeting uses it (`begin()`, `:828-830`). Synthesis is metered (`Metered::synthesize`,
  `:1248`): reuse it as is; no new provider.
- **Speech in**: the engine's `Event::Turn` → `pump` (`:901-916`) → `PlaceMsg::Voice` → `voice_turn` (`:586`) →
  `submit_voice` (`:626`, `turn.submit` with `via: voice`, `surface: "voice"`, `:680-701`). `submit_voice` passes no
  `DiscordOrigin` today.
- **Today's spoken reports**: `voice_heard` (`:766-793`) speaks the whole `output` of another turn that ends in the
  voice place's own session (`:775-780`). Your task reports must not be spoken twice when a task's report also wakes
  that session.
- **The work watch** already runs for the voice channel's status: `Bus::watch_work` (`voice_status.rs:454`, filtered on
  `WORK_CHANGED`, `:466`); `Tally` keeps `needs` and the open questions (`:70-73`, `apply` `:108`); `Tally::spoken`
  (`:158-175`) is already the rule "only for a private place's voice turn" (`:21-23`). Hang your feed on this watch: do
  not open a second one.
- **Answers**: `WorkAnswerParams { question_id, option, note, author, discord, watch }`
  (`theseus-protocol/src/answer.rs:15-37`); `approves(option)` lists the approving option ids (`:94-99`); Discord's
  press sends the option (`dc/runtime/ask.rs:69-80`); `task.cancel` (`theseus-protocol/src/lib.rs:270`). The core judges
  the owner from the place: `owner_in_private` (`crates/theseus-core/src/places.rs:453-470`) passes only a private
  place, and the voice channel's place is `discord:channel:<id>` (`:445`).
- **Heard words**: `heard.rs:193` `classify` sorts Words/Wordless/Echo/Backchannel/Resume; `BACKCHANNELS`
  (`crates/theseus-voice/src/heard.rs:40-68`) include "yes", "ok", "sure", and a bare "yes" over or just after a
  sentence is no turn (`:29-31`); "stop" is not an echo (`:246`).

## The design (new module `dc/runtime/voice/tasks.rs`)

**What is spoken, when.** While a call is up and the voice place is private (`VoicePlace`, `voice.rs:97-106`; the same
rule as `Tally::spoken`), from the work watch: (a) a task of the owner's (a root in a private place) that gets a new
question (needs you): `"<title> needs you: <the question, first sentence>. Say approve, deny, or the option's number."`;
(b) a task that ends (done, failed, cancelled): `"<title> is done: <the report's first one or two sentences>"` (or
`failed:`). Text from `WorkView` (`title`, `question`, `report`; `theseus-protocol/src/work.rs:140-260`), cleaned: no code
spans or fences, no URLs, no paths, no tool lines, at most 220 characters, cut at a sentence. Each `(task, question id)`
and each task end is spoken at most once (a bounded set, 256). Send it as `Command::Report`, so the engine waits for the
floor. Skip (b) when the task's report goes to the voice place's own session (it will be spoken by `voice_heard`
already). A call that starts says nothing about the past (no backlog).

**Answering by voice.** Keep the last spoken question as `pending` (question id, its options, the task, spoken at) for
5 minutes. In `voice_turn` (`voice.rs:586`), before `submit_voice`, when one is pending and the speaker is an owner,
match the transcript (lowercased, punctuation stripped; one small pure fn with a table, unit-tested):
approve | approved | go ahead | do it → the first option `approves()` accepts; deny | no | don't | reject → the option
whose id is `deny`/`reject` (or none: then no match); first | one | 1 | second | two | 2 | third | three | 3 (also "the
second one", "option two") → that option by position; stop it | cancel it | stop the task | cancel the task →
`task.cancel`. A match calls the RPC with `author` and `discord: DiscordOrigin { user_id: speaker, channel_id: the voice
channel, guild_id }`, posts the usual transcript line (`🎙️ **name**: …`, `:635`), speaks a three-word ack ("Approved."
/ "Option two." / "Stopped.") as a `Command::Report`, clears `pending`, and does **not** submit a turn. No match → the
utterance is an ordinary voice turn, unchanged. The core's refusal (not the owner, not private) is spoken as "I can't take
that here." and posted with the core's words.

**Words that vanish.** Because "yes"/"ok" are backchannels, the spoken question asks for "approve"; do not change
`heard.rs`'s rule. Say in the report whether a bare "yes" right after a question reached `voice_turn` in your tests.

**Config:** `[voice] speak_tasks = true` (on by default): a field of `crate::voice::VoiceConfig`
(`crates/theseus-core/src/config.rs:151-152`), its line in the template's `# [voice]` block
(`crates/theseus-core/config/theseus.example.toml:804`), and the template test passing.

**FAST:** a watch event costs a set lookup; speech goes through the engine's existing queue; the matcher runs only when a
question is pending; nothing on a turn when no call is up.

**Tests** (`dc/runtime/voice/tests_tasks.rs`; the fakes: `tests::joined(p, commands)` installs a `Call` whose commands
go to a test receiver, `voice.rs:1394`; `heard()` builds an utterance, `:1413`; `Lounge`, `voice/tests_heard.rs:22-76`;
the assertion style of `voice.rs:1555-1562`): a task's report during a call → exactly one `Command::Report` with its title
and no backticks; the same report seen twice → still one; a new question → one, and "the second one" → the question
settled with option 2 (read back from the core's question list, `CONFIRM_LIST`, or the kernel), and no `turn.submit`; "stop
it" → the task cancelled; no call → nothing sent; a shared voice place → nothing spoken; "what's the weather" while a
question is pending → an ordinary voice turn. Plant a revert for: speaking twice, speaking with no call, the owner/private
check, the matcher's ordinal.

**Live check for the maintainer:** `/join` the test voice channel from a private place, start a task that asks
(`approval`), hear the question, say "approve", see the card settle; let a task finish and hear its two sentences.

**Limits for this row.** No new model tool, no model-visible text (nothing added to `FRAMING`, `voice.rs:79`, or the
board line). `voice.rs` is 1,802 lines: put the logic in `voice/tasks.rs` and add only calls there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
