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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261018-v1-fixes`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: v1-fixes, the v1 cut's fix row: an owner_ask's real choices in the answer hints, a pane's question printed once, four flaky tests made race-free, the turn bench's judge arm, docs/privacy.md, and the docs owed for the rows joined 2026-10-17 and 10-18 (theseus-pngho.1)

Branch: `cloud/20261018-v1-fixes`. Every commit's subject carries `theseus-pngho.1`. Deadline for the report: 5 hours
after you start.

**Why.** Theseus's v1 is cut today. These are the small defects the last two days' joins found and left, plus the
docs that fell behind. None is large; each must be right, proven by a test that a planted revert breaks (items 1 to
4), and none may change anything else.

**Risk: low.** Small, separate fixes; one docs pass. Commit each item on its own (items 1 to 6 below), so the joiner
can take or drop one alone.

**Prerequisite:** `main` (`70937771` or later).

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `v1-fixes` | theseus-pngho.1 | 1 the answer hints' choices; 2 a pane's question block once; 3 four flakes; 4 the bench's judge arm; 5 docs/privacy.md; 6 Part III items and status | `crates/theseus/src/render/task_tree.rs`, `crates/theseus-core/src/push/work/views.rs`, `crates/theseus-discord/src/render/board_v2.rs`, `crates/theseus/src/interactive.rs` (+ `interactive/ask.rs`), the four tests and what they test, `crates/theseus-sim/src/perf/judge.rs`, `docs/privacy.md` (new), `docs/spec/31-part3-item-244.md` (or a new chapter), `docs/spec/README.md`, `docs/spec/01-front.md`, `docs/status.md` | M |

## 1. An owner_ask's answer hints show its real choices

**Seen** (the morning lane's live herdr run, 2026-10-18): for a task's `owner_ask` (a multiple-choice question,
`crates/theseus-protocol/src/work/ask.rs:1`, `OWNER_ASK`), the Discord board's needs-you row and `theseus tasks`'
answer hint read "approve · decline", although the options are numbered choices. The herdr pane and the Discord card
are right (they show the choices).

**Where to look.**
- The CLI's hint: `crates/theseus/src/render/task_tree.rs:321` `answer_commands`. For `QuestionKind::Ask` it lists the
  `choice:N` options; for any other kind it says `{first} · {last} --note …`, falling back to the literal words
  `approve` and `decline` when the options are empty (`unwrap_or("approve")`, `unwrap_or("decline")`).
- The question's kind and options: `crates/theseus-core/src/push/work/views.rs:277-302` sets `QuestionKind::Ask` only
  when `crate::ask::input_of(a)` reads the act's input, and builds the `Question` with `options: Vec::new()` (filled
  later by `theseus_protocol::work::answer_options`, `crates/theseus-protocol/src/work.rs:520`).
- Discord's needs-you item: `crates/theseus-discord/src/render/board_v2.rs:121` `ask_line` and its answer button; the
  needs-you post's words, `crates/theseus-core/src/needs_you.rs:168` (`answer with theseus answer {short} <number>`).

**Do.** First reproduce it in a test: a task whose `owner_ask` waits, read through the same path `theseus tasks` and
the board read (`task.list` / the work views as the daemon sends them, not a hand-built `Question`). Find the step
where the kind or the options are lost (an empty `options` list, or a kind that is not `Ask`) and fix it there, at
the source, so every surface that reads the view gets it. Then:
- `theseus tasks`' hint for an owner_ask lists its choices as commands (`theseus answer 2a329e 1 · … 2 · … 3`, or
  whatever form `answer_commands` already gives an `Ask` within its room; when even that does not fit, `answer
  2a329e 1–3` is fine). It never says approve or decline for a question with choices.
- The board's needs-you row says the same thing in its words: the choices, or `answer 1–N`.
- An approval's hint stays byte-identical: the goldens and the existing `approve · … decline --note …` tests do not
  change.

**Tests:** one at the view (kind `Ask`, options `choice:0..` plus `choice:other`), one at the CLI's `theseus tasks`
output, one at the board's row. Plant: put the `unwrap_or("approve")` path back for an `Ask` (or empty the options at
the source) and see each fail.

## 2. A herdr pane prints a question's block once

**Seen** (the same run): a pane (`theseus watch` in a herdr pane) prints an `owner_ask`'s block (the `? question`
line, the numbered options, the `choose? [1-N, or your own words]` line) two or three times.

**Where to look.** `crates/theseus/src/interactive.rs:676-681`: after any event that printed below the prompt, while a
question waits, `say_prompt` runs again "so the line to answer on is the one the terminal ends on". `say_prompt`
(`interactive.rs:700-735`) prints the whole block for an `Ask` (`ask::prompt_lines`, `interactive/ask.rs`) every time,
not just its last line. An approval's prompt is one line, so it never showed.

**Do.** Reproduce first, in `crates/theseus/tests/pane_answers.rs`'s rig (`Daemon::start`, `Pane::start`, `ask()`):
an owner_ask waits, then events print below it (a parked turn's footer, a progress line). Then make the block print
once per question: the re-say after output below prints only the line to answer on (`choose? [...]`), and the block
itself prints once, when the question is first asked. A question asked again after the pane's reconnect (an
`EventsLost` reread) may print its block again once; say what you chose. The test counts the block's `? Which quay
first?` line (once) and the `choose?` line (as before: the existing `wait_err(..., n)` counts in this file must still
hold, so read them before you change anything).

Plant: re-print the whole block on the re-say; see the new test fail.

## 3. Four flaky tests, each made race-free

Rules for all four: **no retries, no sleeps as fixes, no wider timeouts as fixes.** Find the race and remove it:
wait for the state the assertion reads, or make a clock explicit where a test asserts something that depends on time.
For each, the report says the race in one sentence and how the fix removes it.

- **theseus-q1vlq** (closed, fixed on main in `c323da54`): `crates/theseus-discord/src/tests_gateway/calendar.rs`,
  `a_reminder_becomes_an_event_that_moves_settles_and_goes`: it read the store right after the fake Discord showed the
  event settled, but the store's write follows the event's PATCH. Read the fix: if it waits for the store's state
  through the rig's wait helper (no sleep), it is right; add its planted revert to your report (put the old read
  back, run it under load, e.g. alongside `stress-ng` or 16 busy loops, and show it fail at least once in N runs, with
  N). If the fix is a sleep or a retry, replace it.
- **theseus-w68w5** (closed, fixed on main in `b95a2558`): `crates/theseus-core/src/rpc/tests_self_backlog.rs`,
  `a_non_owners_drop_or_move_is_refused_and_ledgered`: "nothing changed" compared scores that decay with the wall clock
  (`crates/theseus-core/src/rsi/backlog/rank.rs:31` `score(m, now_ms)`). The fix compares the items without their
  scores. Check whether the listing can take an explicit `now_ms` (or a test clock) cheaply; if it can, prefer that
  (the test then asserts the scores too, at one instant); if it cannot without a new parameter on the wire, keep the
  fix and say why. Either way, a plant (compare the scores at two instants again) and its failure (cross a scoring
  step on purpose, e.g. by a `last_ms` placed so one milli-point turns over between the reads) in the report.
- **theseus-s8jod** (open): `crates/theseus-core/src/rpc/tests_peek.rs:444`
  `last_line_goes_out_at_most_once_a_second_under_a_chatty_job`; the assertion at `:491` saw a sample `"ch"`, a line
  cut mid-write. The sampler reads the job's tail with `crates/theseus-core/src/toolrun/peek.rs:86` `last_line` /
  `:115` `last_line_from`. **This is a product bug, not only a test bug:** a peek must never show a half-written line.
  Fix `last_line_from` so an unterminated last line is not returned (return the last *complete* line; a file whose only
  content is unterminated returns what? decide, say it, and test it). Check every other caller of `last_line` /
  `last_line_from` reads the same rule correctly (one rule, one place). A unit test on `last_line_from` with a reader
  that ends mid-line makes the race deterministic; the existing chatty-job test then holds by construction.
- **theseus-glisu** (open): `crates/theseusd/tests/whole_replay.rs:158`, `a_whole_turn_replay...`: once, under a
  loaded gate, `proc.run` came back `refused` for a non-root daemon (the test expects `refused` only when root,
  `:151-155`). Read the refusal's path (L1 setup for a replay's program: the job's cgroup, its namespace, its view)
  and find the transient failure that reads as a refusal. Two things are wrong if the cause is a setup step that
  failed: the replay calls it `refused` (a refusal is a policy answer, not a setup error), and the test cannot tell.
  So: (a) make the replay result carry the refusal's reason (it may already: read it) and the test print it in its
  assertion message; (b) if a setup step can fail transiently (for example a cgroup create racing another sandbox's
  teardown), make that step correct under concurrency (the right wait, the right unique name), not retried. The
  cloud VM runs as root, where this test takes its `root` branch, so you may not reproduce the non-root case; reason
  from the code, say what you could and could not run, and leave the joiner a recipe to run it as an ordinary user
  (`systemd-run --user --scope -p Delegate=yes cargo test -p theseusd --test whole_replay`, 20 times beside a loaded
  machine).

## 4. theseus-n10cv: `theseus-sim bench turn --judge`'s packs arm passes

**Seen:** with the judge's arms naming `route.v6` (the trim rows fixed `route.v4`), the loop arm passes and the packs
arm fails: "a plain turn's trace counts Some(4) frames, and the WAL holds 5 of the turn's: ...
[ledger:standing.asked, meta:standing.capture]". The fake Jev's default answer to `directive.v1` asks a standing
capture (a standing order the owner may have given), which writes two frames beside the turn's four.

**Where:** `crates/theseus-sim/src/perf/judge.rs:300` (the packs the arm runs: `classify.v3`, `role.v1`, `route.v6`,
`directive.v1`) and `:312` `scripted(jev)`.

**Do.** The bench measures a *plain* turn: a plain message carries no standing order, so script `directive.v1` to
answer "none" (its kind for no directive) in `scripted()`, the way a real plain turn's verdict reads. Do not count the
standing frames as the turn's, and do not loosen the frame check. Run `cargo run -p theseus-sim -- bench turn
--judge` (or the binary) and quote both arms' lines in the report. Test: if the sim has a test for the judge arms,
extend it; else add one that runs the packs arm's scripted rig once and asserts the plain turn's frame count. Plant:
the old default directive answer; see it fail.

Check `theseus-sim`'s AGENTS.md/CLAUDE.md for whether `bench turn --judge` is in the gate (`scripts/gate.sh`); if it
is not, say so in "For the joiner" and do not add it (out of scope).

## 5. docs/privacy.md

A short page (about one screen) at `docs/privacy.md`, linked from `docs/README.md` (one line) and from the README's
footer only if it already has a links line (else leave the README alone). It is the privacy note the owner's Google
OAuth app names as its branding URL (`https://github.com/zeroaltitude/theseus/blob/main/docs/privacy.md`), so a
Google reviewer reads it. Plain words, no marketing. It says:

- **What Theseus is:** personal, self-hosted software. One owner runs it on their own machine; there is no Theseus
  service, server or company that receives anyone's data. The source is this repository.
- **What Google data it reads, and why:** only what the owner grants it through Google's consent screen (for example
  mail, calendar or files, when the owner turns that integration on), and only to do what the owner asks.
- **Where it stays:** on the owner's machine, in Theseus's own store. It is never sold, never shared, never used for
  advertising, and never used to train models.
- **Who else sees it:** the parts a turn needs are sent to the model providers the owner configures, for that turn
  only, under those providers' own terms. Google Workspace content is never sent to models of China origin. This is
  the owner's stated policy and the page states it. What enforces it on main today is the model-origin floor:
  `crates/theseus-core/src/config/privacy.rs` (`[privacy] sensitive_names`; a turn that names one routes off models
  whose origin is `cn` or `unknown`) and the catalog's `origin` per model (a config cannot loosen a built-in `cn`
  origin, `config/tests.rs:1202`). Read whether Google-sourced content (the calendar, mail reads in
  `crates/theseus-core/src/routine/grant.rs`, `config/calendar.rs`) is covered by that floor. Do not build a new
  enforcement in this row (a later row, theseus-a1wvq, owns the China-origin data rule); in "For the joiner", say
  exactly what on main enforces the sentence and what does not yet, file:line.
- **Google API Services User Data Policy:** one sentence that Theseus's use of data from Google APIs adheres to it,
  including the Limited Use requirements, with the link
  (`https://developers.google.com/terms/api-services-user-data-policy`).
- **How to revoke:** at `https://myaccount.google.com/permissions` (remove the app), and locally by deleting the
  stored token (name where Theseus keeps it, if a Google integration exists on main; if none does yet, say the
  token is held in the owner's secret store as Theseus's other credentials are, and point to `docs/setup.md`'s
  secrets section if it has one).
- **Contact:** the repository's issues page.
- An "Updated" date (today's, from `date`).

No person's name and no organisation's name (the rules below). Every claim must be true of `main`: where the code
does not yet do something the page would promise, either the page says less or you name the gap in "For the joiner".

## 6. The docs owed: Part III items and the status page for the rows joined 2026-10-17 and 10-18

`docs/spec/` is The Ship of Theseus in chapters (`docs/spec/README.md` says how: a new Item goes at the end of the
last chapter, which is `31-part3-item-244.md` with Items 244 to 246; update its title line and its row in the index;
a new version changes chapter 1's first heading, v0.87 to **v0.88**). The last docs commit was Item 246 on
2026-10-11; nothing since is recorded. Read Items 244 to 246 first and follow their format: a `### Item N.` heading
naming what it built and, in parentheses, its Beads ids, its row (cloud or lane), when it joined and its merge, and
whether it is installed; then **Why**, what it built, how it was proven, and a divergence table where the build
differed from its plan (`| Planned | Actual | Why | Disposition |`).

**The rows to record** (from `git log --merges --first-parent origin/main`; each merge's message and its branch's
commits are your source, with the join fixes after it; the branches' CLOUD_REPORT.md files were dropped at the
merges, so read the merge's second parent's history with `git log <merge>^2` if you need more):
- **The task-join rows** (joined 2026-10-17 01:13 to 01:36): task-join-core `756bcfeb`, chat-board-gaps `8ed50fc3`,
  cockpit-notices-policy `6ca11a40`, herdr-horizon-digest `45d384bf`, tui-task-return `3c72841e`, cockpit-task-page
  `88c61b6d`, discord-task-join `3f045da8`, chat-task-view `ba931256`, herdr-task-return `f37900de`.
- **The afternoon rows of 2026-10-17** (14:51 to 16:45): cn-catalog `45f86304`, cn-route-v6 `5b18838f`,
  discord-task-channels `fbd326e1`, discord-war-room `57d50bd6`, discord-reactions `03ab67d0`, task-offer-chip
  `f25bcc3c`, voice-tasks `6846c432`, discord-emoji-catchup `a5df3b4e`, herdr-wire-fix `0f93aa81` (a local lane),
  discord-default-forum `c3801e51`.
- **The trim rows** (theseus-rwpbg.*, 2026-10-17 20:18 to 22:21, and the 10-18 00:11 to 00:52 commits): trim-docs
  `bf696c9b`, trim-security-v1 `065f1bf5`, trim-config-inherit `91d0b889`, trim-effort `ecb6edbf`, trim-utils
  `6987413e`, trim-keel-deletions part C `000f1864` with parts A, B and D (`fd33868c`, `f6ade10d`, `b88b360c`),
  trim-binary `34629c73`, trim-tokens `e7573983`, trim-names `3d14103d`, trim-tests-dedup `40aa83cd`, and the answer
  rename `c579c49b` (rwpbg.10).
- **review-fixes** (theseus-oejh3) `0daac1fa`, with its join fixes.
- **The last-features rows** (theseus-ypnxt.1 to .8, 2026-10-18 01:27 to 03:26): pane-answers `2b1710be`,
  chat-start-pause `a7131e02`, cockpit-tasks `d8728305`, discord-tasks `373bacb7`, task-model-by-brief `a1354a72`,
  progress-words `1673ed9a`, task-history `cd4df28e`, acceptance-journey `fd88eb5b`.
- **RSI loop 1's rows** (theseus-pw1q.21.*): flow-runner `92c16e5c`, pattern-miner `e09a0bbd`, keel-watch-sealed
  `746cd908`, l1-self-build `e36570c8`, sealed-runner `6e3bee86`, with their join fixes.
- **The morning lane** (theseus-31pgz, theseus-h9tu1; 2026-10-18 09:42 to 10:21, `22c1540f..70937771`, a
  fast-forward, no merge): a routine-coding brief on Sonnet, the progress line's cost forecast, a Reminders-homed
  task's forum post and board row, the ping window never holding a question, `self.patterns` on the blocking pool,
  the Keel ack, herdr's startup hook closing the focused restored shell, the journey's two steps running.

**Shape.** One Item per family above (seven Items, 247 to 253, in that order), each with a short paragraph or a
small table per row: what it built (in the owner's words, the way Items 244 to 246 say it), its merge sha and join
fixes, its proof (the gate it passed, its planted reverts if its merge message or commits name them). Installed
state: installs #37 (`96c592c0`), #38 (`c323da54`) and #39 (`70937771`, 2026-10-18 11:29) are on the owner's daemon;
a row merged at or before an install's sha is installed by it. Keep each Item honest and short; do not invent proof
you cannot find in the history. Chapter 31 is about 22 KB: append there unless it would pass about 150 KB.

**`docs/status.md`:** the "Updated" line (from `date`, MST) and the version (v0.88); a "Recently landed" entry per
family with its Item number; "Working today" gains what the owner now has (tasks found later, a task's model from its
brief, one progress line everywhere, herdr pane answers, the forum post per task, the trims' one name per thing),
in the page's style. Do not rewrite the page's older sections; add and correct. Do not touch `README.md` beyond the
privacy link (item 5).

Scrub what you write: no person's or organisation's name, no internal host names, no secrets (the rules below). The
spec's master copy lives outside the repository; the joiner copies your chapters back, so change only the chapters
you must.

## Tests
Items 1 to 4: the tests named under each, each with its planted revert (break the behaviour, see the named test
fail, restore), listed in the report's table. Items 5 and 6 are docs: no plant, and say so. Run each crate's suite
you touched (`cargo test -p theseus`, `-p theseus-core`, `-p theseus-discord`, `-p theseusd`, `-p theseus-sim`) and
`scripts/gate.sh` with `THESEUS_GATE_NO_BENCH=1` once at the end; quote the totals.

## Overlaps
- **Landing beside you today:** RSI rows R7 mint-validate and R4 sealed-hold (`crates/theseus-core/src/rsi/`, their
  runners and relay), R8 mint-deploy-notice, and task-checkpoint-routing (theseus-ypnxt.9:
  `crates/theseus-core/src/routing/checkpoint.rs`, `turn/checkpoint_step.rs`, `judge/inbound.rs`,
  `config/routing.rs`, `fact/route.rs`, `crates/theseus/src/` task show, and
  `crates/theseus-protocol/src/work/progress_line.rs` with its golden). None should touch your files; if you must
  touch one of theirs (for example `progress_line.rs` or the task tree), keep it to the lines you need and list them.
  Their Part III Items are theirs: do not write them, and leave Item numbers 254 on free for them.
- **For the joiner:** merge order does not matter for items 1 to 5; item 6's chapter and status edits conflict with
  any other docs row only in the chapter's title line, the index row and the status page's Updated line (take both,
  renumber Items if needed).

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at **`70937771`**
(2026-10-18 10:21 MST, the morning lane, installed on the owner's daemon as install #39) or later, with **store format
45**. The `file:line` references in this task were read on `70937771`. A line may have moved a little, so search by
name.

`main` already holds the last-features rows (task history, the one progress line, pane answers, chat start and pause,
Discord tasks, the cockpit's Tasks page, a task's model from its brief, the acceptance journey), the trim rows, the
RSI loop-1 rows R1, R2, R3, R5 and R6 (all off in the owner's config), and the morning lane. Use their names, never
older ones:
- the config: `[routing.classes.<name>]` (not `modes`), `on_max_loops`, one `[cache]`. An old key fails the load with
  "`<old>` was renamed to `<new>`";
- the words: a task is a "task" everywhere a person reads; one verb for an answer, `answer` (`theseus answer ID
  OPTION`; `confirm` is gone);
- nobody calls `task.get`; read one task through `task.list`.

**Still landing beside you today. Build on `main` as it is; the joiner resolves.**
- **task-checkpoint-routing (theseus-ypnxt.9):** Jev re-judges a running task at checkpoints and moves its model.
  New files under `routing/` and `turn/`, plus `judge/inbound.rs`, `config/routing.rs`, `fact/route.rs`, `task show`
  and the progress line's `on <profile>` part.
- **RSI rows R7 mint-validate, R4 sealed-hold and R8 mint-deploy-notice:** self-improvement's mint and the sealed
  scorer, under `crates/theseus-core/src/rsi/`. No file of yours.

Line ceilings at `70937771` (`scripts/long-files.txt`, ceiling): `crates/theseus-core/src/turn.rs` 3,525;
`crates/theseus/src/render.rs` 3,128; `crates/theseus-discord/src/render.rs` 3,009; `crates/theseus-discord/src/runtime.rs`
3,500. The files this row is most likely to edit are under 1,100 lines (`crates/theseus/src/interactive.rs` 1,013,
`render/task_tree.rs` 812, `push/work/views.rs` 571, `board_v2.rs` 850, `perf/judge.rs` 862, `tests_peek.rs` 568).
Never raise a ceiling. A new file over 2,500 lines is a shape failure.

## Out of scope

- Any new feature, any config-key rename, any Jev pack's text, route.v6's prompt.
- The China-origin data rule's enforcement (theseus-a1wvq) and the Google Workspace integration itself (theseus-batvg):
  the privacy page describes, it does not build.
- Adding `bench turn --judge` to the gate.
- Part III Items for the rows landing beside you today (task-checkpoint-routing, R7, R4, R8).
- The docs gap from 2026-10-11 to 10-16 (rows joined in that span with no Item): list them in the report with their
  merge shas, and stop there.

If your work shows one of these is needed, write it in the report and stop there.

## Rules for this row
- **Model roles.** You write the code. Keep it minimal and elegant: reuse what exists (the reads, the renderers, the
  RPCs named above), and let every line you add earn its place.
- **Nothing lost.** Behaviour, wire JSON, stored records and user-visible output stay the same except where this task
  changes them. Where it changes them, pin the old output in a test first, then change it, and show the diff in the
  report. New wire fields are optional with a default, so an old client still reads a new answer.
- **The first-request budgets hold.** No new model tool, and no always-loaded text:
  - the first request as sent stays at or under **8,000 tokens in cli and task places** and **15,000 in the owner's
    DM**. The turn bench and the owner-config fixture test must pass, and the report quotes their numbers;
  - FAST: nothing new on a plain turn. A plain turn stays 5 frames. A new read runs only when a person asks for it
    (a command, a key, a page), never per turn.
- **Every behaviour change gets a test and a planted revert**: break the behaviour, see the named test fail, restore.
  The report lists each plant, the test that caught it, and its failure line. A pure move needs no plant: say so.
- **Keel.**
  - Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Converting a test to a new
    wording that this task orders is fine; show the old and new assertion.
  - Before your last commit, run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its
    output in the report. It must be empty. If it is not, give the finding and the exact `Keel:` trailer the guard asks
    for, and say why the change is the task's; the joiner decides and signs.
  - Do not edit `scripts/keel-guard.py` or its test.
- **No store format change. No new dependency.** Cargo.lock and the package-lock.json files gain no package. A new
  META key or a new ledger kind is not a format change; a new table or a changed record shape is (stop and report).
- **Right over compatible** (the owner, 2026-10-17): one right name for each thing, no aliases. When you rename a word a
  person reads, fix every caller (scripts, bench, `contrib/herdr`, docs, the cockpit) in the same row.
- **Live, not shadow.** What you build is on by default and works on the owner's daemon after install. No feature flag
  that defaults off.
- **Discord stays lively.** Do not silence anything that pings today. A new post or edit follows the existing ping
  policy and the live board's pace (at most one edit per 2 s).
- **The cockpit loses no datum.** Anything you move names its new home in the report.
- **No person's or organisation's name** anywhere in the repo, in commit messages, or in the report:
  - write "the owner";
  - use obviously made-up names in tests (Examplecorp, Acme, Widgetco; harbour and quay titles are fine);
  - if you edit a file that already carries a person's name, replace it with "the owner" in the lines you touch.
- **Read the AGENTS.md/CLAUDE.md of each crate you touch, first**, and keep each AGENTS.md true for what you change (a
  line or two).
- **Keep CLOUD_REPORT.md honest:** what you ran, what passed, what you could not run in the cloud. It has these
  sections: "What changed" (file:line), "Tests and plants" (a table), "Budgets" (the turn bench's and the
  owner-config fixture's numbers), "Overlaps" (every line you touched in another row's files, and every name you used
  that a landing lane renames), "Proof nothing was lost", and "For the joiner".
