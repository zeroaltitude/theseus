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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261013-work-digest-r2`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: work-digest round 2, the re-integration the review asked for (theseus-qkoi3)

Branch: `cloud/20261013-work-digest-r2`. Every commit's subject carries `theseus-qkoi3`. Deadline for the report: 3 hours after you start.

**What happened.** Round 1 built this row on branch `cloud/20261013-work-digest` (head 841adf43). The joiner's reviewer held it; the
review is quoted verbatim below. **Start from that work on today's main:** `git fetch origin cloud/20261013-work-digest` and
merge it into your branch with a plain merge (never a rebase), resolve against main, then do exactly what the review's
re-integration section says, keeping everything it found sound. The product bug: 'told once' does not hold across a daemon restart (the row's own end-to-end test fails 1 run in 3). Find the cause (the digest's cursor is not durable before the restart, or is read before replay), fix it, and prove it: the test green 10 runs in a row with nextest --retries 0, plus a test that stops the daemon between telling and the next client's arrival.

**Main today:** store format 43 (if you must bump, take the next free number at your commit and say so). On main: chat,
chat-ask, progress (plan items), deferred-tools (families, `tools_find`), token-budget (the turn bench fails above
8,000 tokens as sent in cli/task and 15,000 in the owner's DM; the DM has about 67 tokens of room, so add nothing to
any always-loaded set; a new tool goes into a deferred family). Never loosen a test, budget or ceiling. Run the row's
own tests 5 times (a flake is a finding, not a retry). The report says, per review point, what you did.

## The review that held round 1 (verbatim)

Verdict: **HELD** (its own end-to-end test fails 1 run in 3 on its branch and 2-3 in 3 on reviewer A's stack: "told once" does not hold across a restart); the join fixes below are ready for when it is fixed

## Why held (reviewer A, 2026-10-14 15:3x)
`theseusd::digest what_happened_while_a_client_was_away_is_told_once_from_where_it_left` (the row's live check as a
test: a real daemon, three tasks, a cancel, an answer, a timer, then a restart):
- **On the row's own branch** (dc42b602, this machine): passed, failed, passed (3 runs). **On reviewer A's stack**
  without job-follow (9495c9ae): failed 3 of 3; with job-follow: failed 2 of 3 locally, and in EC2 gate rva-l4.
- **Two races, both in the product:** (1) right after the restart the ledger path's digest lacks the timer's `woke`
  line although its `wake.fired` row is in the ledger (`ledger.tail` shows it at position 549): the kind's page lags
  the rows written just before the stop, so a digest asked right after a restart is silently incomplete
  (`from_ledger: true`, a line missing). (2) The ring's `woke` line and the ledger's come from different positions:
  the ring told the timer at a notice at or before its `to` (543), the ledger's `wake.fired` row is after it (549), so a
  read from the ring's `to` after a restart tells the timer again (`{"from":543,"from_ledger":true,"lines":[{"kind":
  "woke",...}],"to":570}`). A test wait would hide (1) and cannot fix (2).
- What would fix it (for the row's author or a follow-up row): word a wake from the same fact on both paths (the
  ring's woke at its `wake.fired` row's position), and have the ledger path wait for, or read past, the kind index's
  catch-up (or say it is partial). Everything else in the row reviewed clean (below).

Draft verdict before the hold: ACCEPT WITH A JOIN FIX (protocol lib.rs at its ceiling; the herdr helper and run()'s length on current main), with a resolve script

# work-digest (theseus-qkoi3): review, reviewer A, 2026-10-14

- Branch `cloud/20261013-work-digest`, code head **dc42b602** (one commit; CLOUD_REPORT 3,894 bytes, read whole).
  Base work-board-era main. 46 files: `work.digest` (the board's notice ring when it reaches back, else the ledger by
  kind, newest 2,000 rows on the blocking pool; `from_ledger` says which; folded 3+ of a kind under one root in 10
  minutes), the place rule (`work.watch`'s: a guarded caller names roots, a shared place sees its own alone, scrubbed,
  no answers), `theseus digest` and `status`'s one line, the seen file's `Looked`, the TUI's digest at connect and on
  refocus after 60 s, the cockpit's `sincedigest.ts`, Discord's `/digest` and the welcome-back on typing. No frame,
  no store change, no unprompted post (so no pacer needed: decision 4 untouched).
- **Join fix** (in the review merge 5c1df5c4 + b35679e5; patch `joinfix-1-lib.patch`): protocol lib.rs went to 2,738
  (ceiling 2,733) with this row's re-exports: `pub use watches::*;` (watches' seven types, nothing else public there)
  and the row's re-export split in two lines. On current main the row's `own()` socket helper goes with its only use
  (main computes `herdr_socket` before the dispatch), and run()'s "returned above" arm takes one line so the row's
  `digest` arm keeps run() within clippy's 100.
- **Merge** (`resolve-work-digest.py`): Discord runtime.rs thread-discord's arm with the row's `typed`; voice.rs: the
  binding's own commands are sixteen with thread-route's `/move` (the count test: all[16..], 18); protocol's method
  list; TUI app.rs both rows' fields and arms; CLI main.rs the row's arm. runtime.rs stays at 3,500.
- Noted (the row's): the welcome's last sight is in memory (none after a restart until a second typing); a failing task
  is covered by the ledger/ring mapping only.

## The original task (round 1), for reference

---
## Your task: work-digest, "since you last looked" on every surface: what finished, failed or was answered while you were away, in a few lines (theseus-qkoi3)

Branch: `cloud/20261013-work-digest`. Every commit's subject carries `theseus-qkoi3`. Deadline for the report: 3
hours after you start.

**Why this work exists.** The owner's target (2026-10-08): "you can have lots of tasks going and you'll always be kept
in the loop on status". The plan's acceptance test 5: **any surface can tell you in a few lines what finished, failed
or was answered while you were away.** Today the TUI knows "done until seen" for itself (`crates/theseus/src/seen.rs`,
shared by the terminal clients since wave A), the cockpit computes its own plate from a copy of the ledger, and Discord
has nothing.

**Your base: main after `work-board` has joined.** Read its merge first: `core/push/work.rs` (the work board, its
notice ring of 4,096 and `WorkBoard::notices_since(position) -> (Vec<Notice>, complete)`), the `work.watch` /
`work.changed` / `work.notice` shapes, and `theseus work`. Also `seen.rs` and `theseus-protocol/src/notices.rs`.

**The seam you add (others read it; keep it additive):** `work.digest {since: position | since_ms, roots?:
[session_id]}` → `{from, to, lines: [DigestLine], now: Rollup, questions: [Question]}`. A `DigestLine` is `{at_ms, kind
(reported | failed | answered | cancelled | woke | reminder), work: WorkRef, words}`, worded once in the protocol
(`notices.rs`'s style: the caller passes `hm`). `roots: [task]` is that task's own timeline.

**What to build:**
1. `work.digest` in a new `core/rpc/digest.rs`: from the ring when it reaches back far enough, else from the ledger by
   kind (`task.ended`, `action.confirm_answered`, `execution.failed`, `wake.fired`, `tool.late_result`), bounded to
   2,000 rows, on the blocking pool. Bursts fold ("3 tasks reported to harbour survey"). The place rule: a shared
   place's caller sees only its roots, scrubbed.
2. **Seen stays the client's** (the seen file, the cockpit's storage, Discord's last read): each surface passes its own
   position. Add to `seen.rs` a machine-wide "last looked" position the terminal clients write when they show a
   board.
3. **Readers on the same commit:**
   - CLI: `theseus digest [--since 2h | --since-seen]` (new file `crates/theseus/src/digest.rs`), and `theseus status`
     prints one line `◆ 2 reported, 1 answered by you since 18:14 · theseus digest` when there is something.
   - TUI: on start and after the terminal regains focus, one notice line from the digest since its seen position.
   - Cockpit: the watch's sixth plate, "since you last looked" (`cockpit/src/ship/since.ts`, drawn by `watch.ts`),
     reads `work.digest` in live mode instead of its copy of the ledger; the time machine keeps reading its fold at
     its moment (a row after the moment is never read). Its tests.
   - Discord: `/digest` (ephemeral), in a new module `runtime/digest.rs` (`runtime.rs` is at 3,476 of 3,500 lines),
     and a welcome-back line in the DM after 4 h with nothing read, only when the digest is non-empty. `discord-board`
     builds beside you and draws the board's "since you looked" header from its own seen position; don't edit its
     board files.

**FAST:** no frame; the ring push is O(1) per notice (`work-board`'s); the digest read is off the turn's path; `theseus
digest` one read, under 10 ms on the bench's store.

**Tests:** the ring and the ledger fallback agree on the same window; a window past the ring's start says it read
the ledger; `roots` scopes to one task; bursts fold; a shared place sees only its roots; the CLI, TUI and Discord
renders as goldens. Plant a revert per behaviour (the fallback skipped; the place filter skipped) and record what
caught it.

**Live check on this VM:** a scratch daemon on the stand-in, run five tasks (one fails, one asks and is answered),
disconnect, reconnect: `theseus digest --since-seen` lists every report, the failure and the answer, and the cockpit's
plate (headless, `npm run test` with a recorded stream) shows the same lines.

**Rules.** No store change. Overlaps: `cli-tasks` edits `theseus status`'s neighbours and `interactive.rs`, so keep your
status change to one line in `status.rs`; `discord-board` owns the board. No person's name. Every new key has a
default and is in the template.

Run the gate before every commit; commit CLOUD_REPORT.md as the branch's last commit, and end with a short final
message starting `CLOUD REPORT COMPLETE`.

## The owner's decisions (2026-10-13 23:24 MST, verbatim: "i'm happy for yes to all until we're at testing time to eval")
They hold until the evaluation pass; build so each is easy to revisit.
1. **One conversation surface.** Bare `theseus` opens the chat (row `chat`, theseus-cyha, owns it); `theseus chat` is the same thing; the task footer and one-key answers (row `cli-tasks`) live inside that chat, not in a second CLI.
2. **One plan.** Plan mode's steps are the work board's plan items (row `progress`), never a second list of steps.
3. **One owner for Discord threads.** Activity threads (row `thread-discord`, T6) decide which Discord thread a task or message lands in; task rows (`discord-guild-tasks` and others) use its helper and never create threads on their own.
4. **One pacing rule for unprompted posts** (the digest, the morning/horizon view, nudges, the always-on DM ping, proactive voice). Every unprompted post goes through one shared pacer: at most one unprompted DM post per 30 minutes, later items batched into it; the first such row to join builds the pacer (a small module beside the push), later rows use it.
5. The size check joins the gate (a build fails if the binary grows more than 1 MB; the 60 MB miss warns). 6. Programs at once 8 -> 16 (reads 32). 7. Scoped grants are built on Opus with a security review.
