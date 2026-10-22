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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail2-chat`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: tail2-chat, the chat follows a turn the router placed in a thread, a held turn names no model in the TUI, unlink by the live slug, and the thread ask's select through the gateway (theseus-umb8a, theseus-7kjb7, theseus-fzt4v, theseus-t8fow, theseus-ma3g8)

Branch: `cloud/20261021-tail2-chat`. Every commit's subject carries the Beads id of what it changes (`theseus-umb8a`,
`theseus-7kjb7`, `theseus-fzt4v`, `theseus-t8fow`, `theseus-ma3g8`). Deadline for the report: 4 hours after you start.

**Why.** Five follow-ups that the reviews of the threads lane and the privacy-hold row left open. Each one is a place
where a person reading the chat or a thread sees the wrong thing, or a path that has no test:
- **A placed turn's tasks vanish from the chat (theseus-umb8a).** When the router runs a chat message in a thread's
  session, the tasks that turn starts belong to the thread's session. The chat's footer, ctrl-t, `/task ID` and the
  report lines all filter on the chat's own session, so they show nothing. The person asked for the work in this
  chat and cannot see it here.
- **The placement line comes at the turn's end (theseus-7kjb7).** The chat prints `→ thread <title>` and how to go
  there from `TurnSubmitResult.thread`, which only arrives when the placed turn ends. During a long turn the person
  sees nothing about where their message went. The daemon should tell the submitting connection at placement time.
  The issue also asks for a decision on whether `theseus chat SESSION` (a session the person named) should pin the
  way `ask -s` does. That decision is made for you below; carry it out and state it in your report.
- **A held turn's TUI line names a model (theseus-fzt4v).** In a shared place, a reach for the owner's personal
  details is held: no model call is made, and the reply is a fixed line. On the tail lane the Discord footer now
  says `held: no model ran` and the CLI line says `held, no model ran` when `stop_reason == "held"`. The TUI chat's
  end-of-turn line still prints the model's name. Your base does not have that row, so your test builds a
  `TurnSubmitResult` with `stop_reason: "held"` directly.
- **Unlinking by the live slug after a merge does nothing (theseus-t8fow).** Say A is linked to b, and b is merged
  into c. `linked_with(A)` already waives b and c. But `thread unlink a c` finds c in neither record, writes nothing,
  and returns A unchanged. So A stays linked through b, and `thread show c` lists no link.
- **The thread ask's select is untested end to end (theseus-ma3g8).** The Discord card's `other` select (a pick
  among more threads) is unit-tested through `parse` and the components only. No gateway interaction test sends a
  select pick and checks that it answers the ask the way a button press does.

**Risk: low to medium.** fzt4v, t8fow and ma3g8 are local. umb8a and 7kjb7 change what the chat counts as its own and
add one notification to the wire, with its generated TypeScript. That gives the change a wider surface but keeps
it additive: a client that does not know the notification ignores it.

**Prerequisite: `origin/integ/threads` at `8b641b6b`** (the threads lane's round 2, green; it contains `main`
`6918e3c1` plus the thread home claim, the rest of thread routing and the deferred-tools families). Your branch starts
there, and it joins the threads lane as its round 3, not `main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail2-chat` | theseus-umb8a, theseus-7kjb7, theseus-fzt4v, theseus-t8fow, theseus-ma3g8 | a `thread.placed` notification to the submitting connection at placement; the chat prints the placement line from it and adopts the placed session's tasks and the placed turn's events as its own; a held turn's TUI line says `held: no model ran`; unlink resolves a merged end; a select-pick gateway test | `crates/theseus-protocol/src/thread_placed.rs` (new), one `mod`/`pub use` line pair and one notify name in `crates/theseus-protocol/src/lib.rs`, one variant in `crates/theseus-protocol/src/events.rs`, one `include_str!` in `crates/theseus-protocol/src/ts.rs`, `cockpit/src/protocol.gen/` (regenerated), `crates/theseus-core/src/rpc/thread_flow.rs` (`route_to_thread` only), `crates/theseus-core/src/rpc/thread_link.rs` + `rpc/tests_thread_link.rs`, `crates/theseus-core/src/rpc/tests_thread_route.rs` (new tests at its end), `crates/theseus-tui/src/chat/{events,tasks,thread_ask}.rs`, one field in `chat/state.rs`'s `Chat`, `chat/tests_thread_ask.rs` (new tests), `crates/theseus-discord/src/tests_gateway/thread_ask.rs` (one new test) | M |

## What exists (lines at `8b641b6b`; search by name, a line may have moved)
- **Routing a chat message.** `crates/theseus-core/src/rpc/methods.rs` near :980 calls
  `self.route_to_thread(&mut p, conn, &mut session)` for `turn.submit`. `crates/theseus-core/src/rpc/thread_flow.rs`
  `route_to_thread` (near :154) decides the tag (`explicit_tag` or `place_unaddressed`), runs `session_for`
  (near :264; a paused thread sends the message home), then records `state.routed(..)` and
  `state.hand_off(&run_session.session_id, Handoff { turn_id, .. })`. It returns `Routing { slug, tag, home, session,
  .. }` and swaps `*session = run_session`. The turn's end writes `thread.routed` (`thread_routed_turn`, near :790;
  fact `crates/theseus-core/src/fact/thread_route.rs` `ThreadRouted`). That is the ledger row, and it stays as it is.
- **How a notification reaches only the submitting connection.** `ask_thread` (near :566) builds a `ThreadAsk`, then
  `EventSink::new(self.bus.clone(), home, Some((conn.client.to_string(), conn.tx.clone())))` and
  `rec.record(&ThreadAsked(&ask))` with `To::Sink(&sink)`. `EventSink::send(e: Event)` is in
  `crates/theseus-core/src/bus.rs` (near :379). The fact `ThreadAsked` (`fact/thread_route.rs` near :83) has both a
  `KIND` (ledger) and a `METHOD`.
- **May a message route at all.** `may_route` (near :288): `threads.enabled`, `via != "ask"` (the `ask` command
  always sends `via: "ask"`, `crates/theseus/src/cmd.rs` near :103), no `thread`, no `prompt`, no `opened_from`, a
  `session_id`, not a task's session, not an MCP or unnamed surface, and not a slash command. `route_to_thread` also
  returns `None` when the session is a thread's own session (`board.of_session(&home).is_some()`).
- **Protocol.** `crates/theseus-protocol/src/lib.rs` (2,552 lines / ceiling 2,733 at the base): the `notify` names
  block near :425-495 (`THREAD_CHANGED = "thread.changed", THREAD_ASKED = "thread.asked"` near :485), and
  `TurnStarted` near :2417. `crates/theseus-protocol/src/events.rs` near :577:
  `ThreadAsked(crate::thread_route::ThreadAsk) = notify::THREAD_ASKED`. `ThreadTag` is in
  `crates/theseus-protocol/src/thread_route.rs`. `crates/theseus-protocol/src/ts.rs` `declared()` lists every module
  with `include_str!("<module>.rs")` (near :38-63; `nodes.rs` was the last added). `cockpit/src/protocol.gen/` is
  generated from them (see the protocol crate's AGENTS.md for the regen command and the `ts` test).
- **The TUI chat.** `crates/theseus-tui/src/chat/events.rs`:
  - `ours(&self, session_id)` (near :394) is `self.session.as_deref() == Some(session_id)`.
  - `turn_is_ours(turn_id)` reads `self.mine`.
  - `notified_one` (near :409) filters `TurnStarted`, `ToolProposed`, `ToolStarted`, `ToolEnded`,
    `ConfirmRequested`, `SessionMode`, `TaskChanged`, `TurnFailed` and `TurnEnded` on `ours(..)`, and
    `LoopStarted`/`Model*` on `turn_is_ours`. `ThreadAsked` is not filtered.
  - `Purpose::Submit` (near :81) passes the result to `turn_ended`.
  - `turn_ended` (near :661) calls `self.placed_line(r)`, then sets `self.model` from `r.model`, then builds
    `parts = vec![took(r.elapsed_ms), model_words(&r.model)]`, and pushes `r.stop_reason` unless it is `""`, `done`,
    `end_turn` or `no_tool_calls`.

  `chat/thread_ask.rs` `placed_line` (near :130) prints `→ thread {title}`, `  go there: theseus chat {session}`, and
  a move hint when `movable`. `chat/tasks.rs` `watch_work(sid)` (near :83) sends
  `work.watch {"roots": [sid], "limit": 0, "interactive": true}`; `work_changed`, `work_notice` and `task_changed`
  keep the board, and the footer, ctrl-t and `/task` read it. `chat/events.rs` `opened(sid)` (near :246) sends
  `session.watch`, `watch_work(sid)` and `session.mode`. The test rig for a placed turn is
  `chat/tests_thread_ask.rs` (near :114, "the daemon's script, `turn.submit` answered as a turn the router placed").
- **The daemon's board watch.** `crates/theseus-core/src/rpc/work_board.rs` and `crates/theseus-protocol/src/work_watch.rs`
  (`WorkWatchParams.roots`). Before you send a second `work.watch` on one connection, read what the daemon does with
  it: does it replace the first watch's roots, or add a subscription? See `rpc/tests_thread_board.rs`.
- **Unlink.** `crates/theseus-core/src/rpc/thread_link.rs` `link_change` (near :61) gets both records with
  `board.get(&a)` / `board.get(&b)`, then calls `unlink_ends(ta, tb)` (near :186). That returns nothing when neither
  record holds the other. `link_ends` refuses a merged thread with `the thread {slug} was merged into {m}: link that
  one`. `crates/theseus-core/src/threads.rs` `live_of(slug)` (near :112) follows `merged_into` to the end, and
  `Board::linked_with` waives merged ends. Test `a_link_follows_a_merged_thread` in `rpc/tests_thread_link.rs`
  (near :352) builds exactly the a/b/c case: copy its setup.
- **The thread ask's select.** `crates/theseus-discord/src/runtime/thread_ask.rs`: the select is added when the ask
  has an `other` option and `others` is not empty. Its `custom_id` is `{PREFIX}:{ask_id}:{PICK}` and its placeholder
  is `another thread`. `chosen(i, key)` (near :138) reads the pick's first value; the press path calls it near :236.
  The gateway test `the_owners_press_wakes_the_message_and_the_card_becomes_the_answer` in
  `crates/theseus-discord/src/tests_gateway/thread_ask.rs` (104 lines) shows how to arm the ask (`route.ask(..,
  Pending{..})`, publish `Event::ThreadAsked`) and press. `theseus_sim::fake_discord::FakeDiscord::press_values(&Pressed
  { message, button, user, name }, &[values])` sends a select pick. `button` names the menu by its custom id or its
  placeholder. See its use in `crates/theseus-discord/src/tests_board.rs` near :618.
- **The held wording elsewhere** (on the tail lane, not your base; quoted so the words match): the Discord footer
  pushes `held: no model ran` in place of the model, and the CLI's turn line says `held, no model ran`, both when
  `r.stop_reason == "held"`.

## What to build
1. **theseus-7kjb7: `thread.placed` at placement.**
   - New module `crates/theseus-protocol/src/thread_placed.rs`, with `pub struct ThreadPlaced { session_id: String /*
     the chat's own (home) session */, placed: String /* the session the turn runs in */, turn_id: String, tag:
     ThreadTag }` (serde default and ts derive as the module's siblings do).
   - In `lib.rs`: one `mod thread_placed;` + `pub use thread_placed::*;` (as `history.rs`/`nodes.rs` are wired), and
     `THREAD_PLACED = "thread.placed"` on the `THREAD_CHANGED`/`THREAD_ASKED` line or the next one, with its doc
     sentence.
   - In `events.rs`: one variant `ThreadPlaced(crate::thread_placed::ThreadPlaced) = notify::THREAD_PLACED`.
   - In `ts.rs`: `include_str!("thread_placed.rs")`.
   - Regenerate `cockpit/src/protocol.gen` and commit it.
   - In `route_to_thread`, when the tag has a slug (the turn runs in a thread, not at home), send it to the submitting
     connection only, through an `EventSink` built as `ask_thread` builds it, with `EventSink::send` directly. It is
     a notification, not a ledger row: `thread.routed` at the turn's end stays the record.
   - `turn_id` must be the id the placed turn will run as. Check that `Handoff.turn_id` is the id the turn adopts
     (`crates/theseus-core/src/turn/inbound_step.rs` `take_handoff`, and `crates/theseus-core/src/thread_flow.rs`).
     If it is not, carry the session and tag only, and match the turn by its session in the TUI.
   - **The pin decision (made for you): no pin.** A chat on a thread's own session is never re-routed today
     (`board.of_session(&home)` returns `None` early), and a named home session is the place's home, where routing
     is the feature. Pinning every named session would turn routing off for every resumed chat. So `theseus chat
     SESSION` keeps sending no `via`, and nothing in `may_route` changes. Prove the first half with a test (below).
     State the decision in your report's first section as one the owner may override.
2. **The chat prints the placement at once (7kjb7, TUI).**
   - `notified_one` handles `Event::ThreadPlaced(p) if self.ours(&p.session_id)`: it calls the placement printing
     with the notification's tag and session, and records that this turn's placement was told.
   - `turn_ended`'s `placed_line(r)` then prints nothing for that turn. A daemon without the notification still gets
     the line at the end, as today, so keep `placed_line` working from `TurnSubmitResult`. Refactor it to take
     `(&ThreadTag, &str session)` so both paths share one body.
3. **theseus-umb8a: the placed turn and its tasks are the chat's.** On a `ThreadPlaced` for this chat (or, failing
   that, on a submit result whose `thread` has a slug and whose `session_id` is not the chat's):
   - Insert the placed turn's id into `mine`, so `LoopStarted`/`Model*` and the live line follow it.
   - Add the placed session to a small bounded set on `Chat` (one field in `state.rs`; for example `also:
     Vec<String>`, keeping the last 8 sessions, oldest dropped).
   - Extend the filters for the placed turn's own events so they are the chat's: `ToolProposed`, `ToolStarted`,
     `ToolEnded`, `ConfirmRequested`, `TurnFailed`, `TurnEnded` and `TaskChanged`. Key them on the turn id where the
     event carries one. Where an event carries only a session, key on `also`, but never treat another turn of that
     thread's session (one sent from Discord, say) as this chat's turn: `TurnStarted` for a session in `also` whose
     turn id is not in `mine` is ignored.
   - The board: re-send `work.watch` with `roots` = the chat's session plus `also`. Do this only if a second watch
     replaces the first one's roots; otherwise send what the daemon's semantics need, and say which in the report.
     The footer, ctrl-t, `/task ID` and the report lines then list the placed turn's tasks.
   - Keep each hunk in `events.rs` inside `ours`/`notified_one`/`turn_ended`, and the `tasks.rs` hunk inside
     `watch_work` or a new function beside it.
4. **theseus-fzt4v: a held turn names no model.** In `turn_ended`, when `r.stop_reason == "held"`:
   - push `held: no model ran` in place of `model_words(&r.model)`, and do not also push the reason;
   - do not set `self.model` from that result (no model ran, so the status line keeps the last real model).

   Every other turn's line stays byte for byte as today.
5. **theseus-t8fow: unlink by the live slug.** In `link_change`'s unlink branch, when neither record holds the other,
   look for the pair the link really sits on: a link of `a` whose `live_of(..)` is `b`, or a link of `b` whose
   `live_of(..)` is `a`. Unlink that pair (both records, both `ThreadUnlinked` rows and announcements, as today). If
   none is found, keep today's no-op. Do not change `link_ends`. If resolving inside the `write` closure is awkward
   (the board is borrowed there), resolve the slugs before the write and pass them in.
6. **theseus-ma3g8: the select pick through the gateway.** One new test in `tests_gateway/thread_ask.rs`, set up the
   way the press test is:
   - an ask with an `other` option and two `others` (`t-one`, `t-two`), the `Pending` listing
     `Choice::Thread("t-two")` (or however `chosen`'s value is mapped: read the press path near :236 first);
   - `press_values` on the card's `another thread` select with `["t-two"]`;
   - assert that the pending ask is answered with `Choice::Thread("t-two")` by `discord:ana`, and that the card
     settles as an update-message (type 7).

## Tests (each with a planted revert; the first is the key test)
- **`a_placed_turns_task_shows_in_the_chats_footer`** (`chat/tests_thread_ask.rs`). Set up the daemon's script so it
  places a submitted message in a thread's session. Send the `ThreadPlaced` notification, then a `work.changed`
  frame whose view is a task of the placed session. The footer (or ctrl-t's list) shows that task, and the chat
  sent a `work.watch` naming the placed session. Plant: leave `also` out of the board's roots (or out of
  `TaskChanged`'s filter); the key test must fail.
- `the_placement_is_told_when_it_happens` (`chat/tests_thread_ask.rs`): a `ThreadPlaced` notification prints
  `→ thread <title>` and the `go there` line before any turn end. The submit result for the same turn afterwards
  prints them no second time. A result for a turn with no notification still prints them at its end (today's test
  stays green). Plant: drop the told-once check, and the lines print twice.
- `another_turn_of_the_thread_is_not_the_chats` (`chat/tests_thread_ask.rs`): after a placement, a `TurnStarted` in the
  placed session with a different turn id starts no live line in the chat. Plant: filter `TurnStarted` on `also`.
- `a_placed_message_tells_the_submitting_connection` (`rpc/tests_thread_route.rs`, at its end): a routed
  `turn.submit` on a connection makes that connection receive one `thread.placed` with the placed session, the
  slug's tag and the turn id the turn ran as. A message the router keeps at home sends none. Plant: drop the send.
- `a_chat_on_a_threads_own_session_is_never_rerouted` (`rpc/tests_thread_route.rs`): a `turn.submit` with no `via` on
  a thread's own session runs there, and no `thread.placed` is sent. If an existing test already proves this, name it
  in the report instead of adding one. No plant needed for an existing behaviour test; say so.
- `a_held_turn_names_no_model_in_the_chat` (`chat/tests_thread_ask.rs` or the module that tests `turn_ended`'s line):
  a result with `stop_reason: "held"` and `model: "claude-sonnet-5-5"` prints a line containing `held: no model ran`
  and not the model's name, and the status line's model is unchanged. An `end_turn` result's line is byte for byte
  today's: compare it with an existing expected string. Plant: drop the `held` branch.
- `unlink_by_the_live_slug_after_a_merge` (`rpc/tests_thread_link.rs`): link a and b, merge b into c, then
  `thread_unlink { slug: "a", other: "c" }`. Afterwards `linked_with("a")` is empty, `linked_with("c")` has no `a`,
  and one `thread.unlinked` row per end was written. Plant: the old no-op.
- `the_others_select_answers_like_a_press` (`tests_gateway/thread_ask.rs`), as in step 6. Plant: make `chosen` return
  the key (`other`) instead of the pick, and the answer is not `t-two`.
- The protocol's `ts` test and the regen check must pass, with `protocol.gen` committed exactly as generated.

## Not in this row
- Thread routing's scoring and the router's misplacement of an unrelated message (theseus-vw8k7), the ask's `new`
  offer (theseus-q3ggn), the cross judge, families, the home claim (`rpc/home_claim.rs`), and every docs file:
  `AGENTS.md`, `docs/status.md`, `docs/design/`. A sibling row on this same base owns them. If your change needs
  a docs line, write the exact text in your report's follow-ups for the maintainer.
- `crates/theseus-core/src/thread_route.rs`, `turn/route_step.rs`, `why`/`context.explain`, `crates/theseusd/tests/judge.rs`
  (another sibling row), `crates/theseus-core/src/thread_flow.rs` (not the `rpc/` one), `may_route`'s rules.
- The privacy hold's own code (`turn/personal_step.rs`) and the Discord/CLI held wording: those are on the tail lane.
- Any store layout change (none is needed: the notification is never stored).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias. Stored data stays readable.
- **Shared TUI files with another lane.** A row on the tail lane (`tail-herdr-cancel`) edits
  `crates/theseus-tui/src/chat/events.rs` (`history`, near :298-310), `chat/task_view.rs`, `chat/keys.rs` and
  `chat/state.rs` (Ctrl-C, near :657). The two lanes meet at the release merge. Keep every TUI hunk small and inside
  the functions named in "What to build". Touch `state.rs` only to add the one field (and its default). Do not touch
  `task_view.rs` or `keys.rs`. Reformat nothing else, and do not run `cargo fmt` over the crate if it would move other
  lines.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`). List each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure refactor needs no plant: say so.
- No new always-loaded model text and no new tool: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). FAST: nothing new on a plain turn (a
  plain turn stays 4 frames, `tests_m3::a_plain_turn_stays_within_its_frame_budget`). The one notification is sent
  only for a turn the router placed in a thread.
- **Line ceilings** (`scripts/long-files.txt`, length / ceiling at `8b641b6b`): `crates/theseus-protocol/src/lib.rs`
  2,552 / 2,733 (you add about 5 lines), `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (you add nothing
  there). Every other file you touch is under 2,500 lines and must stay there (`rpc/tests_thread_route.rs` is at
  1,394). Never raise a ceiling.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/integ/threads --head HEAD --quiet` and paste its output in the
  report. It must be empty.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. **Store format 48** on
  your base (`MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row should not change it. If you find you
  must, bump to 49 and add the old layout's literal sample to `tests_layouts.rs`, never writing the number anywhere
  but the constant and its tests.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, commit unsigned and say so in the report).
  Put the Beads id in each subject, and end each message with the repo's usual `Co-Authored-By: Tabitha/Claude`
  trailer (copy the exact line from a recent commit: `git log -1 --format=%B origin/integ/threads`).
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the
  owner". Fixture paths, users and thread names are invented.
- Read the AGENTS.md of every crate you touch first (`theseus-protocol`, `theseus-core`, `theseus-tui`,
  `theseus-discord`, `theseus-sim` for the fake's select). Run each touched crate's tests
  (`cargo nextest run -p theseus-protocol -p theseus-core -p theseus-tui -p theseus-discord`; the core suite is
  long, so at least `-E 'test(thread)'` first and the whole crate before the last commit), then
  `cargo clippy --workspace --all-targets -- -D warnings`. About 33 L1 tests need a non-root daemon with a job
  cgroup and fail on this VM; this row needs none of them. Say which ones you ran.

## Your report
Commit `CLOUD_REPORT.md` with:
- what changed per Beads id;
- the pin decision (no pin) and its reason, marked as one the owner may override;
- what a second `work.watch` does on one connection, and what the chat sends because of it;
- whether `Handoff.turn_id` is the placed turn's id;
- each test and its plant, with the failing output's line;
- what could not run on the VM, and the exact commands for the maintainer;
- the keel guard's output;
- the store format (unchanged);
- the docs lines a sibling should write (core AGENTS.md's thread-router paragraph, the TUI chat's AGENTS.md if it has
  one);
- follow-ups.

Write `CLOUD_REPORT.md` with your editor tools, never a shell heredoc. After the commit, check its size: it must not
be empty. Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
