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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-home-warn`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base (it wins over anything below)

Your branch starts from the integration branch `integ/tail` at `7f428787` (2026-10-21: `main` `6918e3c1` plus the
patch1 tail rows merged since). That branch, not `main`, is your base, and your branch joins back into it. Line numbers
below are at `7f428787`; search by name if one has moved.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row changes
no stored record's layout (the home-size marker is a META value read with serde; a new optional field with
`#[serde(default)]` is not a layout change). If you find you must change a layout, bump to 49 and add the old layout's
literal sample to `crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says.

**Line ceilings** (`scripts/long-files.txt`): `crates/theseus-core/src/turn.rs` 3,525 is listed; do not add to it. None
of your other files is listed; each must stay at or under 2,500 lines. Never raise a ceiling.

**Work in flight beside you:** the row `voice-knobs` joins `integ/tail` too (`crates/theseus-voice/`,
`theseus-core/src/voice.rs`, the template's `[voice]` block). Your only shared file is the template: touch only its
`[threads]` block. Rows joining `integ/threads` and `integ/cn` are coding in `thread_route.rs`, the thread recall files,
`turn/route_step.rs` and the China-origin data rule files: stay out of them.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: home-warn, a home-size warning that fires only for a truly large home, and says something runnable (theseus-d9je7)

Branch: `cloud/20261021-home-warn`. Every commit's subject carries `theseus-d9je7`. Deadline for the report: 4 hours
after you start.

**Why.** The home-size warning (theseus-jg1eg, `crates/theseus-core/src/turn/home_size_step.rs`) fired on the owner's
voice channel's home at 76,205 tokens. The owner, 2026-10-21 (verbatim): "This is too generally aggressive -- long
sessions can get quite large, and we're comfortable with those. I would tend to say that token limits on sessions should
be used sparingly". And of the message itself: it reads as having unfilled variables. Today:
- **The bound** is `[threads] working_tokens` (60,000), which means something else: what a *thread's* request stays
  within after a cold resume (`compiler/cold.rs`). A home past it is ordinary, not a problem.
- **The text** (`warning_text`, ~:69) prints the place raw (`discord:channel:<digits>`), and the command leaves `NAME`,
  `@A`, `@B` and `SPLIT` for the reader to fill in. It is also long.

The owner's rule for this row: warn only past **50% of the model's context window** (about 500k on a 1M model), make
that configurable, and decouple it from `working_tokens`. No other budget changes in this row (another piece of work is
auditing them for the owner).

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `home-warn` | theseus-d9je7 | the warning's bound as a share of the serving model's window (`[threads] home_warn_percent`, default 50); a short warning with the place's human name and a concrete runnable split | `crates/theseus-core/src/turn/home_size_step.rs`, `crates/theseus-core/src/tests_home_size.rs`, `crates/theseus-core/src/config/threads.rs` (+ its tests), `crates/theseus-core/config/theseus.example.toml` (the `[threads]` block only), `crates/theseus-core/src/turn/compile_step.rs` (the one call at ~:155, only if it must pass the window), `crates/theseus-core/AGENTS.md` (the home-size paragraph), `docs/status.md` (one dated entry) | M |

## What exists (at `7f428787`)
- **The step.** `home_size_step.rs` (193 lines): `home_size_checked` runs once a turn at its first loop with
  `compiled.est_tokens`; `home_size` skips tasks and non-home sessions, reads the META marker
  `home.size.warned.<session>`, and `step(tokens, threshold, warned)` gives `Warn`, `Rearm` or `Quiet`. The post goes to
  the owner's DM only (`stage_to_operator`, kind `home_size`). Its module comment explains every choice: keep the comment
  true as you change the bound.
- **The tests** are `crates/theseus-core/src/tests_home_size.rs` (212 lines); they set `c.threads.working_tokens` to
  move the bound (~:19, ~:126). Those lines change to the new key: say so in the report, the assertions keep their sense.
- **A model's window** is `context_window` on a catalog entry (`crates/theseus-core/src/catalog.rs` ~:178; Sonnet and
  Opus 5.x are 1,000,000). Use the window of the model that served this request (the turn knows its profile/model when it
  compiles); if no window is known, fall back to 1,000,000 and say where in the report.
- **A place's human name.** `TurnRunner::place_name(session_id)` in `crates/theseus-core/src/recall.rs` (~:1208) gives
  `#harbor` or `DM @name` for a bound place, via `places::Places::name_of`. Use it (or `name_of` on the target you
  already have); never print the raw `discord:channel:<digits>` target in the text.
- **The split command** is `theseus thread split home@<place> <name> --first @A --last @B [--dry-run]`, and `theseus
  thread unsplit <name>` undoes it. Read how `thread split` names and validates a thread and takes the place (`git grep -n
  "thread.split" crates` finds the RPC; `rpc/tests_thread_home.rs` shows it used) and what `@` positions are (`theseus
  history`'s positions: the session's rows). Use only forms that command accepts today.

## What to build
1. **`[threads] home_warn_percent`** (default **50**): the home-size warning fires when the home's request passes this
   share of the serving model's context window. Validate `1..=100` with a bail naming the key, the value, the range and
   the default, in the style of the file's other checks. The step's threshold is `window * percent / 100`;
   `working_tokens` is no longer read by this step (it stays what it is for cold resumes). The marker's `threshold` field
   records the computed bound. Rewrite the module comment's "The bound" paragraph to say this and why (the owner's rule:
   large sessions are fine; a warning is for a home near the window, where a request can fail or every turn is costly).
2. **The text.** Short and plain, nothing for the reader to substitute. It names the place as the owner sees it
   (`#test-voice-channel`, `DM @…`), the request's size and the share of the window, and one command that runs as
   written:
   - A real thread name: a slug built from the stretch it moves, e.g. `earlier-2026-10-19` (the date of the stretch's last
     turn), valid for `thread split`.
   - Real `@` positions for a sensible split: the home's **oldest settled stretch**, from its first turn up to the turn
     before the newest `[threads] resume_keep_turns` turns (so the live conversation stays in the home). Compute them in
     the step from the session's rows at the time of the warning. If the home has too few turns to leave a stretch, say
     only the size and `theseus history` and no command.
   - `--dry-run` first, then one short clause that dropping it splits and `theseus thread unsplit <that name>` undoes it.
   Target: three short sentences, under ~350 characters before the command. Example of the shape (yours will differ):
   "📏 #test-voice-channel's home is 512,300 tokens, 51% of the model's window. To see what moving its oldest turns into a
   thread would save: `theseus thread split home@#test-voice-channel earlier-2026-10-19 --first @1 --last @412 --dry-run`.
   Without `--dry-run` it splits; `theseus thread unsplit earlier-2026-10-19` undoes it." Use whatever place form the
   split command really accepts (if it needs the stored name, the command uses that while the sentence uses the human
   name).
3. **Template.** One line in the `[threads]` block: `# home_warn_percent = 50 ...` saying it is a share of the serving
   model's context window, that the owner's DM is told once per home, and that large sessions are otherwise fine.
4. **Docs.** The home-size paragraph of `crates/theseus-core/AGENTS.md` and one dated entry in `docs/status.md`.

## Tests (each with a planted revert; the first is the key test)
- **`a_home_under_half_the_window_is_not_warned`** (in `tests_home_size.rs`): with a 1,000,000 window and the default,
  a home whose request is 76,205 tokens (the owner's case) posts nothing and writes no marker; one past 500,000 posts once.
  Plant: compare against `working_tokens` again. Paste the failing assertion.
- **`the_warning_names_the_place_and_runs_as_written`**: the posted text contains the place's human name, contains no
  `discord:` target, none of `NAME`, `@A`, `@B`, `SPLIT`, and its command parses with the CLI's own argument parser for
  `thread split` (or, if that parser is not reachable from the core's tests, with the split RPC's own validation of name and
  positions) and its `--first`/`--last` are positions that exist in the session. Plant: restore the old `warning_text`.
- A home with fewer turns than `resume_keep_turns` + 1: the text has no command. Plant: always emit one.
- `config/threads.rs` tests: the default 50; a set value read back; 0 and 101 refused with the key named. Plant: remove
  the range check.
- The existing home-size tests (rearm, once per home, no DM bound, tasks and threads skipped) pass with the bound moved by
  the new key. Do not weaken an assertion.
- Run `cargo nextest run -p theseus-core tests_home_size`, `cargo nextest run -p theseus-core config::`, and
  `cargo nextest run -p theseus-core` once at the end.

## Not in this row
- Any other budget, cap or default: `working_tokens`, first-request budgets, recall/injection budgets, compaction. They
  are under audit for the owner and change only with the owner's approval.
- Splitting automatically: the daemon still never splits on its own.
- The Discord crate (the binding already posts kind `home_size` to the DM unchanged).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- No new always-loaded model text and no new tool. A plain turn stays within its frame budget
  (`tests_m3::a_plain_turn_stays_within_its_frame_budget`).
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. This row moves a
  *warning's* threshold by the owner's explicit decision (Discord DM 2026-10-21 23:36); it is not a budget. If the keel
  guard flags it anyway, do not work around it: leave the change in, and quote the guard's output and the line it flags in
  the report's first section so the maintainer can record the owner's ack at the join. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/integ/tail --head HEAD --quiet` and paste its output in the report.
- **Dependencies.** None new.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so in the report). Put `theseus-d9je7` in
  each subject. End each message with the same `Co-Authored-By: Tabitha/Claude` trailer the branch's recent commits
  carry (`git log -3 --format=%B origin/integ/tail` shows it).
- **Names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner". Test places
  use invented channel names.
- Read `crates/theseus-core/AGENTS.md` (its threads and home-size paragraphs) before you edit. Run `cargo clippy
  --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit; do not run `cargo fmt` in a
  way that moves lines in files you were not asked to edit. L1 tests need a non-root daemon with a job cgroup and fail on
  this VM; this row needs none of them, so say which tests you ran.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit: it must not be empty. It holds:
- what changed, by step;
- the new key: name, default, range, the template line as written, and where the window comes from (and its fallback);
- the warning's text exactly as a test renders it, for a home with a stretch and one without;
- how the split's name and `@` positions are chosen, and how the test proves the command runs as written;
- each test and its plant, with the failing output's line;
- what could not run on the VM, with the exact commands for the maintainer;
- the keel guard's output;
- the store format (unchanged);
- follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
