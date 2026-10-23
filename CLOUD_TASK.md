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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-p2-threads-cards`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `23328545` (2026-10-23: release 1.0.1, install #76). That is your base. Line numbers
quoted below are at `23328545` but may be off by a few: search by name. The release preamble above is older than this
note; where they disagree about `main`, ceilings or the work in flight, this note wins.

**This wave is patch2 (release 1.0.2): the follow-ups beside the Operations page.** Rows start on `23328545` and join
integration lanes named in each task (`integ/p2-threads`, `integ/p2-misc`, and others). Other rows land in the same
lanes from other branches at the same time. Your task names them and the files you share with them: stay additive (one
helper, not two), and leave their files alone.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Prefer a META key
or a serde-default field. If your row adds a field or a variant to a stored record, bump it to 50, add the old layout's
literal sample to `crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says, and say so in your report's first
section. Several rows may bump; the maintainer renumbers at the join. Never write the number as a literal anywhere but
the constant and its tests.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-core/src/toolrun.rs` 2,500 / 2,500 (unlisted, at the cap): add nothing to it.
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733: a new type goes in a module file; `lib.rs` gets at most a field
  line or a method constant.
- `crates/theseus-core/src/turn.rs` 3,469 / 3,525: add nothing; put turn logic in a module beside it.
- `crates/theseus/src/render.rs` 3,126 / 3,128 and `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500: add nothing.

An unlisted file stays at 2,500 lines or fewer. Never raise a ceiling yourself (the keel guard flags it). If a line
you truly need will not fit, leave the file one line over and say so in your report's first section.

**The owner's rules for this wave** (each task repeats the ones it touches):
1. The Discord DM task flow is locked: `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical
   (`git diff --exit-code origin/main -- crates/theseus-discord/src/golden/dm_flow.txt` before your report). If your
   fix would change any message of that flow (its wording, layout, order or timing), stop that part and write
   "OWNER CALL: <what and why>, before/after text" in your report instead.
2. Keel: no removed or weakened test, no raised budget, cap or ceiling, no edit of `scripts/keel-guard.py` or its
   tests. Run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` before the report and paste its
   output (it must be empty).
3. The repository is public: no person's name, no real organisation, address or id anywhere. Fixtures use invented
   names and ids under `example.test`. Write "the owner".
4. Red first: each fix comes with a test that is red at the base and green after, and a planted revert named in the
   report.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: p2-threads-cards, a thread-placed question that resolves in `theseus chat`, and a misroute card that says why it waits (theseus-nde5n, theseus-zcn80)

Branch: `cloud/20261023-p2-threads-cards`. Every commit's subject carries its Beads id (`theseus-nde5n` or
`theseus-zcn80`). Deadline for the report: 4 hours after you start.

**Why.** A walkthrough dry-run of install #75 (a scratch daemon, `theseus chat` in a project folder) found two bugs on
one path: a message the router placed in a thread **on a guess**, whose first write the misroute guard held.

- **theseus-nde5n (the chat never sees the answer land).** The chat printed `→ thread Harbour log notes … a guess`,
  then the guard's question as a card. The owner pressed `y`. The ledger shows the answer landed at once
  (`action.confirm_answered {"approved": true, "by": "theseus chat"}`), the file was written, and the thread's
  continuation turn ended 6 s later with its reply. The chat showed none of it: the card stayed at `answering…` for
  over 2 minutes; Esc folded it to `? … wants to write … · tab: answer it`, as if still waiting; the thread's final
  reply never printed. The TUI's needs-you list was empty (correct). Likely cause: the chat follows the home session's
  events, and the confirm and the continuation belong to the thread's session. Expected: the card resolves
  (`approved`) and the continuation's reply prints under it (or, if the reply cannot be followed, a pointer line
  `go there: theseus chat ses_…`).
- **theseus-zcn80 (the card hides the guard's question).** The guard held the write as designed: ledger
  `thread.misroute_held {"path": "~/projects/walkthrough/out/harbour-sum.md", "thread": "Harbour log notes",
  "tool": "fs.write"}`, and the confirm row's `reason` carries the guard's question: "This edits
  ~/projects/walkthrough/out/harbour-sum.md, which isn't in the Harbour log notes thread. Go on, or move to the Docs
  word census?". But the plain-words card drops it:
  - `plain.held` is "`fs.write` is set to ask before it runs". False: the posture was enforcement = notify, and the
    guard raised it.
  - `plain.why` is empty, so the TUI/chat card shows no reason line and no word of either thread or of a move.
  - `plain.who` is "Say ok, nothing else.", the thread session's first message, not the thread's title.
  - Also seen: an approve-list write's TUI card (another task) has an empty why line, while Discord's card for the same
    kind says `why it asks: … is on the approve list`.
  Expected: the card says it waits because the write may be in the wrong thread, names both threads, and (if it fits
  the card's existing answer keys) offers the move.

**Risk: medium.** A TUI event-routing fix and a core wording fix, both near the locked Discord flow.

**Prerequisite: base `origin/main` at `23328545`.** Your branch joins lane `integ/p2-threads`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `p2-threads-cards` | theseus-nde5n, theseus-zcn80 | the chat follows a placed thread's confirm resolution and its continuation; the plain card names the raising rule, the guard's question and the thread | TUI `chat/events.rs`, `chat/thread_ask.rs`, `chat/tasks.rs` (`watch_with`), `chat/ask.rs` only if the card's state needs it, `chat/state.rs`, their tests; core `needs_you/plain.rs` (+ `plain/tests.rs`), `needs_you.rs` (`asker_name`, the `Gloss`), `toolrun/misroute.rs` (only if the guard's question must travel on the request), `policy.rs` (`Decision::at_least`) only if needed; the protocol's `ConfirmRequest` module (not `lib.rs`) only if a serde-default field is the honest carrier; core and TUI `AGENTS.md` (their own sections) | M |

## What exists (at `23328545`; search by name)
- **The chat's events** (`crates/theseus-tui/src/chat/events.rs`): `Event::ConfirmRequested(c) if
  self.ours_in(&c.session_id, c.turn_id…)` → `self.ask(c)`; `Event::ConfirmResolved(r)` clears `self.question` only
  when the correlation id matches and prints `approved`/`declined`; `Event::TurnEnded(r) if self.ours_in(…,
  Some(&r.turn_id))`. `chat/thread_ask.rs`: `ours_in(session_id, turn_id)` (the placed sessions in `placed.also`, and
  a named turn must be in `self.mine`), `thread_placed`, `adopt`, `adopt_result`. `chat/tasks.rs`: `watch_with(sid,
  also)`, the subscription the chat asks for. `chat/ask.rs`: the card (`answering…` near :460 and :484, when
  `chat.ask.sent`).
- **The card's words** (`crates/theseus-core/src/needs_you/plain.rs`): `Gloss { state, who, own_task, model_why }`;
  `held_of(req)` (floor, external text, else "`<tool>` is set to ask before it runs"); `plain(req, g)` (`why` is
  `g.model_why` or empty). `needs_you.rs` builds the `Gloss` (`who: self.asker_name(req)`, near :224). The same
  "set to ask" sentence also lives in `needs_you/words.rs` near :95 (a second copy: find out which one each surface
  reads).
- **The guard** (`crates/theseus-core/src/toolrun/misroute.rs`): `misroute_hold` records `MisrouteHeld` and calls
  `d.at_least(Posture::Approve, &held.why(), "the thread misroute guard", …)`; `Held::why()` near :50 is the question.
- **Discord's card** (`crates/theseus-discord/src/render/approval.rs`): `why_it_asks(req, p)` builds the
  `why it asks: …` text from the request (its tests in `render/approval/tests_approval.rs`), and the card's line uses
  `p.who`. The DM golden (`golden/dm_flow.txt` :43-44) shows one such card.
- The misroute tests (`toolrun/misroute.rs` tests; search the core tests for `misroute`) and the placed-thread chat
  tests (search the TUI tests for `ThreadPlaced` / `placed`).

## What to build
1. **nde5n: find the cause first, with a red test.** Stage, in the TUI chat's test rig, the walkthrough's sequence: a
   chat message placed in a thread on a guess (`ThreadPlaced` for this chat's turn), a `ConfirmRequested` from the
   thread's session, the answer sent, then the daemon's `ConfirmResolved` and the thread session's continuation turn
   (`TurnEnded` with a new turn id, the reply text). Find which link drops it: the subscription (`watch_with` does not
   carry the placed session's confirm/turn events), the filter (`ours_in` rejects the continuation's new turn id
   because it is not in `mine`), or the card state. Fix it at that cause: the chat follows the placed turn's own
   continuation (and only it: another turn of that session, one sent from Discord say, stays out, as `ours_in`'s doc
   says). Expected after: the card leaves `answering…` and says `approved`, and the continuation's reply prints. If
   the continuation's turn id cannot be tied to the placed turn from the events alone, say so with evidence and print
   the pointer line instead (`go there: theseus chat <session>`), with a test.
2. **zcn80: one source for "why it waits".** Make the plain card name the rule that raised the call. For the misroute
   guard: `held` says the write may be in the wrong thread (not "set to ask"), and `why` carries the guard's question
   (both thread names and the move). For an approve-list hold: `why` says what Discord's `why_it_asks` already says.
   One wording, used by every surface: prefer moving or sharing Discord's `why_it_asks` logic into core
   (`needs_you/plain.rs`) and having Discord call it, over writing a second copy. Carry what the request lacks the
   cheapest honest way (the request's `reason` already holds the guard's question; a serde-default field on
   `ConfirmRequest` in its own module only if parsing the reason would be fragile). Then fold the two "set to ask"
   copies into one.
3. **zcn80: who asks.** For a thread session, `who` is the thread's title, not the session's first message. Keep a
   task's title where a task asks (the DM golden's `*Reef-chart: chart the reef*` line).
4. **The locked flow.** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Steps 2-3 change the
   words the core gives every surface: if any line of that golden would change, stop that part, keep Discord's text as
   it is, and write "OWNER CALL: <the line, before and after>" in the report. Discord cards outside the golden
   (`tests_approval.rs`) may change only where the new text is the same sentence Discord already shows; list every
   changed expected string in the report.
5. **Docs.** One paragraph in the TUI's `AGENTS.md` (the chat and placed threads) and one line in core `AGENTS.md`
   (needs-you words) saying where the reason comes from.

## Tests (each red at the base, with a planted revert; the first is the key test)
- **`a_placed_threads_answered_question_resolves_and_its_reply_prints`** (TUI chat). Plant: revert the subscription or
  filter fix.
- **`a_misroute_hold_card_names_the_guard_and_both_threads`** (core `plain/tests.rs` or a needs-you test): `held` is
  not "set to ask", `why` holds both thread names. Plant: `held_of` back to its old fallback.
- **`an_approve_list_card_says_why`**: `why` is not empty for an approve-list hold, and equals Discord's sentence. Plant:
  drop the reason.
- **`a_thread_sessions_question_is_asked_by_the_thread`**: `who` is the thread's title. Plant: back to the first
  message.
- A test that another turn of the placed session (not this chat's) still does not print in the chat.
- Run: `cargo nextest run -p theseus-tui`, `-p theseus-core needs_you misroute plain`, `-p theseus-discord`, then the
  core golden: `crates/theseus-core/tests/golden/core_output.txt` holds `plain` objects; if your wording changes them,
  regenerate it the way the file's own test says and list every changed line in the report (that golden is not the
  locked DM golden, but the reviewer reads each change).

## Not in this row (other rows in lane `integ/p2-threads`, landing at the same time)
- **p2-threads-recall** (theseus-n6mkd, theseus-7nx13; built on another host, branch `lane/p2-threads-recall`): the
  thread lens's note admission and the router's title matching (core thread recall, `thread_title.rs`, the router).
  Do not touch the thread router, recall or titles beyond reading a thread's title for `who`.
- **p2-dispatch** (theseus-gg78p): `tools_defer/*` and the deferred tool families. Not yours.
- Lane `integ/ops` (rows `cloud/20261023-ops-*`): `ops-verbs` and `ops-cal` add to the protocol (`lib.rs`, `ts.rs`,
  `tests/wire.rs`, `protocol.gen`). If you add a `ConfirmRequest` field, keep it in its module, regenerate
  `cockpit/src/protocol.gen` once, and name the overlap in the report; `ops-shell`'s needs-you queue reads `plain`:
  keep every field's meaning.
- `crates/theseus-core/src/toolrun.rs` is at its cap (2,500): add nothing to it.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Changing an expected
  string to the new honest wording is not weakening if the assert stays as strict. A cloud row cannot carry a `Keel:`
  trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and
  paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; nothing here should be stored (a serde-default request field is not
  a stored layout change, but say so if you add one).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (threads, needs-you, the misroute guard) and `crates/theseus-tui/AGENTS.md`
  first. Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: nde5n's cause (which link dropped the events, with the evidence) and the fix; where the "why it
waits" wording lives now and which surfaces read it; any OWNER CALL with the before/after text; every changed expected
string (Discord tests, the core golden); each test and its plant with the failing line; `git diff --exit-code
origin/main -- crates/theseus-discord/src/golden/dm_flow.txt` (empty); the keel guard's output; the store format; a
`docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
