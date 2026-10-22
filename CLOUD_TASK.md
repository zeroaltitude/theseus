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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail-herdr-cancel`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `6918e3c1` (2026-10-21, after the patch1 stage-1 landing), unless your task names another base (an integration branch). Where the
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

---
## Your task: tail-herdr-cancel, herdr's stop key cancels the task it names, and the walkthrough's small rough edges go (theseus-dtvqa, theseus-xgfps)

Branch: `cloud/20261021-tail-herdr-cancel`. Every commit's subject carries `theseus-dtvqa` or `theseus-xgfps` (the
one it fixes). Deadline for the report: 3 hours after you start.

**Why.** A walkthrough rehearsal on the installed build of `6918e3c1` (a scratch daemon, herdr 0.9.1, tmux at 180x50)
found these. What a person hits today:
- **prefix+shift+s on a task's tab cannot stop the task (P1, theseus-dtvqa).** The popup asks the right words
  (`cancel task [walkthrough] drift and its jobs? [y/N]`), but `y` Enter prints
  `` theseus: `[walkthrough] drift` names no task and no pending wake `` and the task keeps running. The pane's tokens
  as herdr reported them live: `{"kind":"task","session":"ses_01a1…772ee8","task":"[walkthrough] drift",...}`. The
  `task` token is the pane's *title*; `stop_for` cancels it as an *id*. Its unit test feeds an id (`"a1b2c3"`), so it
  never met a real pane's tokens.
- **Popups ignore Esc (theseus-xgfps 1, 2).** In the prefix+i inspect popup, Esc leaves the task's view and opens a
  fresh `theseus chat` whose directory is the plugin's folder (header `theseus · model unknown ·
  …/contrib/herdr/theseus (main)`): a line typed there starts a conversation in the repository's plugin folder. Two
  Ctrl-C do not close it; only Ctrl-D does. In the prefix+a answer card, Esc is typed into the line (the terminal
  shows `^[`), and Enter after it would *decline* the question with `^[` as the note. Ctrl-C closes the card.
- **The pane still prints `approve? [y/N/t/note]` twice** for one question (xgfps 3), after the fix that was meant to
  end it (`9d2e9060`, theseus-dowi0).
- **TUI overdraw under tmux** (xgfps 4): a stray `s` before a task row (`s  └ ◐ [walkthrough] tide`), and the notice
  `theseus: no powershell.exe on PATH, so no Windows toasts; ringing the terminal's bell instead …` drawn across the
  sidebar.
- **A joined task's history shows `● Run `null`` for its `job.wait` call** after a steer line, and returning to main
  prints `resumed · 0 turns · untitled` (xgfps 5).
- **`theseus herdr plugin` prints a path that does not exist** (xgfps 6): the installed binary was built in another
  tree, so it prints `<that build tree>/crates/theseus/../../contrib/herdr/theseus`.
- **A daemon with `[web] enabled = false` still prints `inspect <a localhost link on port 7433>/…`** under each reply (xgfps 7):
  a link to a web UI that is not running (on that host, another daemon's port).

**Risk: low.** Client-side commands, the answer card's line reading, two TUI draw paths, and one link the daemon
builds. No store layout, no wire type, no model-facing text.

**Prerequisite: `main`.**

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail-herdr-cancel` | theseus-dtvqa, theseus-xgfps | stop by the pane's session; Esc closes both popups; one prompt per question; the two overdraws; `Run null`; the plugin path; no inspect link without a web UI | `crates/theseus/src/{herdr_keys,herdr_keys_tests,herdr_answer,interactive}.rs`, `crates/theseus/tests/herdr.rs` (new tests only), `crates/theseus-tui/src/chat/{events,task_view,keys,state}.rs`, `crates/theseus-tui/src/{run,ui}.rs`, `crates/theseus-protocol/src/term/words.rs`, `crates/theseus-core/src/config/cockpit.rs`, one line each in `crates/theseus-core/src/config.rs` and `crates/theseus-core/src/turn.rs` | S-M |

## What exists (lines at `6918e3c1`; search by name)
- `crates/theseus/src/herdr_slots.rs` `slots` (near :194-270) builds the pane's tokens: `("task", Some(name.clone()))`
  where `name` is `f.name` (the title) or, without one, `f.session_id`; `("session", Some(f.session_id.clone()))`;
  `("kind", …)` is `task` or `conversation` (`kind_token`).
- `crates/theseus/src/herdr_keys.rs` `stop_for` (near :188) returns `Stop::Cancel(tokens["task"])` for a task (the
  session only when `task` is empty) and `Stop::Halt(session)` for a conversation. `stop` (near :203) reads the tokens
  through herdr's `pane.get`, words the question with `name = tokens["task"]`, then calls `crate::cmd::cancel(conn,
  json, id)`.
- `crate::cmd::cancel` (`crates/theseus/src/cmd.rs` near :1559) tries `wake.cancel` then `task.cancel` with the name;
  the daemon resolves it with `crate::task::resolve` (`crates/theseus-core/src/task.rs` near :1317), which accepts a
  task's **session id exactly**, its execution id, or the end of either. So the session token is a valid cancel name.
  `hold_for` (prefix+p, near :258) already uses the session for `task.pause` (through `inspect_for`).
- The test `stop_is_cancel_for_a_task_and_halt_for_a_conversation` (`herdr_keys_tests.rs` near :172) feeds
  `{"kind":"task","session":"ses_a","task":"a1b2c3"}`.
- The manifest `contrib/herdr/theseus/herdr-plugin.toml`: the inspect popup runs
  `theseus chat --task "$THESEUS_SESSION_ARG" --inspect; sleep 1` (pane id `inspect`, near :114-119); the answer card
  runs `theseus herdr answer …; sleep 2` (near :82-87).
- The answer card's line: `herdr_answer.rs` `decide` (near :178). A line of only Tab characters is `Decision::Next`
  (theseus-dowi0.1's fix); the words `q`, `quit`, `esc` are `Decision::Quit`; any other words decline with themselves
  as the note. A raw ESC byte (`\x1b`) falls to the decline.
- `theseus chat --task ID --inspect`: `crates/theseus/src/chat_cmd.rs` `chat` execs `theseus-tui chat … --task ID
  --inspect` (parsed in `crates/theseus-tui/src/main.rs` near :173). Esc in a task view returns to main
  (`chat/task_view.rs`, the `── back in main · … ──` line near :373-396; keys in `chat/keys.rs` near :139-141, Ctrl-C in
  `chat/state.rs` near :657).
- The pane's question prompt: `interactive.rs` near :720-760 prints the card and `eprintln!("  {prompt}")`. The tests
  that count it: `crates/theseus/tests/herdr.rs` near :280-284 (`PROMPT`) and :1736 (`count(...) == 1`). They pass, yet
  the live pane showed two: the case they miss is yours to find (a re-told question, a second notification of the same
  question, a resize redraw).
- `Run `null``: `crates/theseus-protocol/src/term/words.rs` `summarize` (near :92) words `proc.run` as
  ``Run `{arg}` `` and `job.wait` as `Wait for job {arg}`; the joined view calls it through
  `crates/theseus-tui/src/chat/render_seam.rs` `tool_line`. A `job.wait` that renders as ``Run `null` `` means the tool
  name or the input reached the summarizer wrong in the joined task view's replay (find which: a history node whose
  tool is named by its parent `proc.run`, or an input that is JSON `null`). `resumed · {n} turns · {title}` is
  `chat/events.rs` `history` (near :298-310), whose `task_head` fallback is the line seen.
- The TUI's notifier: `crates/theseus-tui/src/run.rs` near :115 builds `Notifier::new(…)`, whose `say` sink is
  `eprintln!` (`crates/theseus/src/notify/mod.rs` near :126-131; the toast fallback's words are
  `notify/toast.rs` `give_up` near :213). Under the TUI's alternate screen, stderr text lands on the drawn frame.
- The plugin path: `herdr_keys.rs` `plugin_dir` (near :109) returns a checkout beside the binary if one has
  `herdr-plugin.toml`, else the **build tree's** path from `CARGO_MANIFEST_DIR`, without checking it exists.
- The inspect link: `crates/theseus-core/src/turn.rs` near :3297 sets
  `inspect_url: Some(self.cfg.cockpit.turn_url(…))` on every turn; `config/cockpit.rs` `turn_url` uses `public_url`
  else the local base that `resolve(web_port)` set (`config.rs` near :1780). Nothing checks `[web] enabled`.

## What to build
1. **Stop cancels the pane's task by its session (theseus-dtvqa).** `stop_for` returns `Stop::Cancel(session)` for a
   task pane: the `session` token is the one id the pane carries. The question's words keep the title (`tokens["task"]`,
   else the session). Do not change what `herdr_slots` writes (other readers use `task` as the title). Check every
   other reader of `tokens["task"]` in `crates/theseus/src/` (`git grep -n 'tokens\["task"\]'`) and fix any that
   treats it as an id the same way.
2. **Esc closes the answer card (xgfps 2).** In `decide`, a line that is only ESC bytes (with the line's own
   whitespace) is `Decision::Quit`; a line that *contains* an ESC byte is never sent as a decline note (it is
   `Decision::Unknown`, so the card says it read no answer and asks again). Tab's rule stays.
3. **Esc closes the inspect popup (xgfps 1).** A chat started with `--task ID --inspect` is a read-only look: Esc
   there ends the program (exit 0), and so do two Ctrl-C on an empty line, as in a plain chat. It never opens a chat in
   its directory. A plain `--task ID` (join) keeps today's Esc back to main. Keep it in the TUI (a flag on the chat's
   state set from `--inspect`), not in the manifest.
4. **One prompt per question in a herdr pane (xgfps 3).** Reproduce the double print in a test against the real
   `theseus watch --interactive` path that `tests/herdr.rs` drives, then fix the cause. If a second rig is needed, add
   it to `tests/herdr.rs` as a new test; do not change the existing ones.
5. **No overdraw (xgfps 4).** The TUI's notifier `say` goes to the TUI's own notice line (the same place its other
   notices go), never to stderr while the alternate screen is up. For the stray `s`: reproduce it with the TUI's
   buffer tests at 180x50 (the board rig in `crates/theseus-tui/src/tests_board_edges.rs`, theseus-ikw4p's), with a
   header and a sidebar task row; fix it if you can show it. If no buffer test shows it, say so in the report with
   what you tried, and leave it.
6. **`Run null` (xgfps 5).** Find why a joined task's `job.wait` renders as ``Run `null` `` and fix it where the
   wrong name or input comes from. In `history`, a task's session that has no turns of its own yet says what it is (its
   task head), never `0 turns · untitled`: find why `task_head` returned none there and fix that.
7. **The plugin path (xgfps 6).** `plugin_dir` returns only a directory that has `herdr-plugin.toml`: beside the
   binary's checkout, then the build tree, then the current directory's ancestors. When none has it, `plugin_lines`
   says plainly that it found no plugin directory and names where it looked, instead of printing a path that is not
   there.
8. **No inspect link without a web UI (xgfps 7).** With `[web] enabled = false` and no `[cockpit] public_url`, a turn
   has no `inspect_url`. Make the cockpit config know whether it has a base (`resolve` takes the web section's enabled
   flag as well as its port) and add `CockpitConfig::turn_link(…) -> Option<String>`; the one line in `turn.rs`
   becomes `inspect_url: self.cfg.cockpit.turn_link(…)`. `rpc/inspect.rs`'s use stays as it is unless it would print
   the same dead link (then say so in the report; do not widen the row).

## Tests (each with a planted revert; the first is the key test)
- **`stop_cancels_a_task_pane_by_its_session`** (`herdr_keys_tests.rs`): tokens exactly as `herdr_slots::slots`
  writes them for a task with a title (build them by calling `slots` with a `Facts` whose `name` is
  `"[walkthrough] drift"` and `kind` is the task kind, and read its `tokens` into the JSON object `pane.get` returns),
  then `stop_for` on that object gives `Stop::Cancel(<the session id>)`. Plant: return `tokens["task"]` again; the test
  fails. Update the old test's fixture to real tokens (the title in `task`), keeping its other cases.
- `a_stop_cancels_the_task_a_pane_shows` (`tests/herdr.rs`, if its fake herdr serves `pane.get`; else in
  `herdr_keys_tests.rs` against a scripted connection): `theseus herdr stop --pane P --yes` on a task pane with a
  titled task ends that task. Plant: the same as above.
- `esc_closes_the_answer_card`: `decide("\x1b\n", …)` is `Quit`; `decide("\x1by\n", …)` is not an answer and not a
  decline. Plant: drop the ESC rule.
- `esc_ends_an_inspect_popup` (TUI chat rig): `--task ID --inspect`, Esc ends the run with exit 0 and no
  `session.open` was sent; a join (`--task ID`) still goes back to main. Plant: drop the flag's check.
- `one_question_prints_its_prompt_once` (the case you reproduce in item 4). Plant: the old path.
- `a_notice_never_writes_to_stderr_under_the_tui` (the notifier's `say` reaches the notice line). Plant: `eprintln!`.
- `a_joined_tasks_job_wait_reads_as_a_wait` (the joined view's replay of a `job.wait` call shows `Wait for job …`).
  Plant: the old source of the name or input.
- `plugin_dir_names_only_a_directory_that_holds_the_plugin`: a build path that does not exist is never printed. Plant:
  return the build path unchecked.
- `no_inspect_link_without_a_web_ui` (`config/cockpit.rs` tests): web off and no public URL gives `None`; web on gives
  the local link; a public URL gives it whatever web says. Plant: always `Some`.
- Run `cargo nextest run -p theseus -p theseus-tui` and the cockpit tests in `theseus-core`
  (`cargo nextest run -p theseus-core cockpit`) before your last commit.

## Not in this row
- Changing which pane tokens herdr gets (`herdr_slots`), or the manifest's commands.
- herdr's own key handling, its plugin linking, and the owner's herdr config (the plugin is read fresh on every key
  press; nothing to relink).
- Item 5's footer for tasks a chat turn starts (theseus-umb8a, its own issue).
- Load-flaky tests in `tests/herdr.rs`: another row (tail-flakes) changes that file now. Add new tests at its end
  only, and change no existing test there.
- `interactive.rs`: another row (tail-hold) adds one comment line in `on_note`'s re-told question branch (near
  :618-621). Keep your change out of that hunk if you can; if the fix belongs there, keep it small and say so.
- If a fix needs the daemon's turn path beyond the one `turn.rs` line, stop and write it in the report.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** These are fixes, shipped on with no switch; the rollback is reverting the
  commit. Each fix is its own commit, so one can be reverted alone.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). This row adds no model tool and no
  model-facing words. FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`), nothing on the path before a request leaves.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. The old `stop` test's
  fixture changes on purpose (an id in `task` was never what a pane holds): say so in the report's keel note. Before
  your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in
  the report; explain each finding.
- Line ceilings: `scripts/long-files.txt`. `crates/theseus-core/src/turn.rs` is 3,405 / 3,525 and many rows touch it:
  change exactly the one `inspect_url` line, add none. Never raise a ceiling.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. No store format change
  is expected (the store format on your base is 48); if you find you must store something, bump to 49, add the old
  layout's sample to `crates/theseus-core/src/tests_layouts.rs`, and say why in the report.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
  Fixture paths and names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first (`crates/theseus/AGENTS.md`, `crates/theseus-tui/`,
  `crates/theseus-core/AGENTS.md`). Keep CLOUD_REPORT.md honest: what you ran, what passed, and what you could not run
  in the cloud (a real herdr is not on the VM: the herdr tests use the repository's fake; give the maintainer the
  live check below).

## Your report
Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc; check its size after the commit, it
must not be empty) with: what changed per item 1-8 (or why an item was left, with what you tried); each test and its
plant, and that the key test failed on its plant; what could not run on the VM; the keel guard's output; the store
format (unchanged, or the bump and why); follow-ups. Give the maintainer this live check: on a scratch daemon and a
scratch herdr, start a task, focus its tab, prefix+shift+s, `y` Enter: the task is cancelled; prefix+a then Esc closes
the card; prefix+i then Esc closes the popup; `theseus herdr plugin` from an installed binary prints a path that
exists. Push, and end with a short final message that starts `CLOUD REPORT COMPLETE`.
