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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p2f-threads`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about `main`)

Your branch starts from the lane `integ/p2-threads` at `64309ecb` (2026-10-23). That is your base: **not** `main`. The
release preamble above is older than this note; where they disagree about `main`, the base, ceilings or the work in
flight, this note wins. Line numbers below are at `64309ecb` but may be off by a few: search by name. Run the keel guard
against `64309ecb`, not against `origin/main`.

**This wave is patch2 (release 1.0.2): the follow-ups.** The lane `integ/p2-threads` was cut from `main` `23328545`
(release 1.0.1). At `64309ecb` it holds three rows, merged with their join fixes:
- **p2-threads-recall** (theseus-n6mkd, theseus-7nx13): threads named by the turn's own words; a named title counts
  as naming in `thread_route::settle`; an apart thread keeps its title (`apart_by_dir`).
- **p2-threads-cards** (theseus-nde5n, theseus-zcn80): the TUI chat watches a placed thread's session and follows its
  answered question and continuation (`chat/thread_ask.rs`); the plain card's `held`/`why` come from the rule's own
  clause (`needs_you/plain.rs`).
- **p2-dispatch** (theseus-gg78p): a message that asks for a task loads the `tasks` family (`want::asks_for_task`,
  `turn/defer_step.rs`, `want::extra` with `By::Asked`); the `FAMILIES` "tasks" line gained text.
- Two join fixes (an AGENTS.md sentence, rustfmt).

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Nothing in this
row is stored; it needs no bump. Prefer a META key or a serde-default field if something must be. A part that truly
must bump the format STOPS and says so in the report's first section (personas take 50 in another lane).

**Line ceilings at the base** (`scripts/long-files.txt`): none of your files is listed, so each stays at 2,500 lines or
fewer. The tightest is `crates/theseus-core/src/thread_route.rs` at 2,368; every other file you touch is under 900.
Never raise a ceiling.

**Other rows of lane `integ/p2-follow`** land at the same time, each from its own base (listed under "Not in this
row"). None touches your code files. Many rows share `crates/theseus-core/AGENTS.md`: touch only the line this row
names. Do not edit `docs/status.md`: give its sentence in the report.

## Your task: p2f-threads, the threads lane's small follow-ups: placed-question bookkeeping, plain-card wording, the apart debug line, the families fixtures, the asked family's ceiling, and a metadata answer for a large Drive file (theseus-hk0dk, theseus-axno0, theseus-capcs, theseus-s48k6, theseus-izyws)

Branch: `cloud/20261024-p2f-threads`.

Every commit's subject carries its Beads id. One commit per issue is best. Deadline for the report: 4 hours after you
start. Risk: low. Each item is small; two touch words the owner reads, so the locked-flow rule below applies.

## Your row

### theseus-hk0dk (review findings L1-L4 of the threads-cards row). L1-L4 IN; M1 OUT.

**L1 (IN): `owed` is set too easily and cleared too late.** TUI `crates/theseus-tui/src/chat/thread_ask.rs`:
- `placed_asked(c)` (~:203) marks `placed.owed` for any `ConfirmRequested` of a placed session that passed `ours_in`.
  `ours_in` (~:154) lets a placed session's event through when `turn_id` is `None`. So a turn-less question of the
  placed session (one another surface raised) marks it owed. Then that surface's next continuation prints in this
  chat. Fix: mark owed only when `c.turn_id` is `Some(t)` and `self.mine.contains(t)`.
- `owed` is cleared only by `placed_continues` (~:212), on the next `turn.started` with `continuation`. A question that
  closes with no continuation leaves it set until some later continuation, which then joins `mine` wrongly. First
  read the core to see which answers start a continuation (`crates/theseus-core/src/harness.rs`, the continuation
  driver; `tests_approvals.rs` shows a declined call continuing). If an expired or closed question can end with no
  continuation, clear `owed` for that session on its `ConfirmResolved` (the chat sees it: `events.rs` ~:467). Keep the
  session's correlation id with the mark if you need it to know which question it was. If every resolution starts a
  continuation, say so with the evidence and change nothing here.
- Also drop a session from `owed` when `adopt` evicts it from `placed.also` (see L2).

**L2 (IN): an evicted placed session stays watched.** `adopt` (~:224) pushes onto `placed.also` and drops the oldest
past `ALSO` (8) with `also.remove(0)`. It never sends `session.unwatch` for the dropped one, so its events keep coming
(`ours_in` then drops them: extra traffic only). Fix: when a session is evicted, also emit
`call(method::SESSION_UNWATCH, json!({"session_id": evicted}), Purpose::Watch)` (the form `app.rs` `focus` uses
~:1076), beside the `watch_with` and `watch_session` effects. Put the effect builder beside `watch_session` in
`chat/tasks.rs` (~:88) as `unwatch_session`. Never unwatch the chat's own session.

**Writer's override for L3 and L4 (it wins over the two paragraphs below).** The owner's lock covers every message
the owner reads in the Discord DM task flow, not only the lines in `dm_flow.txt`, and the needs-you post is one of
them. So: **L3 is an OWNER CALL.** Do not change `needs_you` or any Discord render; write in the report
`OWNER CALL: the needs-you post says the approve-list clause twice (why + held because)`, with the post's exact text
before and after the fix you would make. **L4:** first find every place the `asks:` sentence is shown. If it reaches
any Discord message, L4 is an OWNER CALL too (before/after text, no code change). Fix it in code only if it shows
solely in the TUI, the CLI or the cockpit, and say which in the report.

**L3 (OWNER CALL, see the override above): `held` and `why` say the same sentence.** Core
`crates/theseus-core/src/needs_you/plain.rs` `plain()`: for a hold by a rule with a clause (an approve-list match) and
no model line, `why` is the clause and `held_of` returns the same clause. The test `an_approve_list_card_says_why`
(`plain/tests.rs` ~:196) pins both equal. The review said no surface shows both. That is wrong: the Discord needs-you
post, `crates/theseus-discord/src/render/ask.rs` `needs_you` (~:70-84), prints ` · why: {why}` and then
`-# held because {held}`. For an approve-list question from the TUI, the CLI or a self-repair run (those reach the DM
as a needs-you post), it says the clause twice. Fix it where the two meet: in `needs_you`, print the `held because`
line only when `held` differs from `why`. Keep the core fields' meaning (`held` is always why it waits; `why` is the
model's reason or the clause), so `an_approve_list_card_says_why` keeps its asserts.

**L4 (IN, see the locked-flow check): a clause ending in "." prints "..".** `held_of` returns the clause unclipped, and
`crates/theseus-core/src/needs_you/words.rs` ~:136 (the `QuestionKind::Extension | QuestionKind::Approval` arm, for a
request with no `plain`) formats `"{who} asks: {}."` with `super::plain::held(req)`. A clause like
"the folder is protected." prints "protected..". Fix: trim one trailing `.` (and trailing whitespace) from the held
sentence before that format appends its own. Change only the punctuation. Do not change `rule_clause`: Discord's
approval card reads it (`render/approval.rs` `rule_clause`, the golden's `why it asks:` line).

**The locked-flow check for L3 and L4 (do this first).** `crates/theseus-discord/src/golden/dm_flow.txt` at the base
has one approval card (lines ~43-44): `risk: writes a file · why it asks: <tmp>/outside/reef.txt is protected:
<tmp>/outside is on the approve list`. That line comes from `render/approval.rs` `why_it_asks` -> Discord's
`rule_clause` wrapper -> core `needs_you::rule_clause`. It does not read `p.why`, and reads `p.held` only for a floor
hold. The golden has no `🔔` needs-you post, no `held because` line and no `asks:` line. So neither fix should change the
golden. Confirm this by reading, and again after the fix with
`git diff --exit-code 64309ecb -- crates/theseus-discord/src/golden/dm_flow.txt` and the Discord golden test. If any
line of the golden would change, STOP that part: keep the text as it is and write `OWNER CALL: <what and why>,
before/after text` in the report. In any case, list in the report the needs-you post's text before and after L3 for an
approve-list question (the owner reads that post in the DM, outside the locked flow).

**M1 (OUT: owner call).** The needs-you DM post's `asks:` wording for floor, approve-list and misroute holds changed at
the threads-cards row. That is the owner's call, relayed by the maintainer. Do not reword that sentence: L4 removes
only a doubled full stop.

### theseus-axno0 (review findings 1-2 of the threads-recall row). Both IN.

**(1) The apart debug line names a thread that keeps its title.** `crates/theseus-core/src/thread_route.rs`
`apart_by_dir` (~:642). In the loop, `h = entity_hits(text, &t.working_set)` holds entities only. `h.retain(...)` then
drops file and root hits not named whole. The push condition (~:679) is
`p.sticky > 0.0 || p.chain > 0.0 || p.learned || h.len() < p.hits.len()`. But `p.hits` (from `pre_route`) includes the
thread's title when the message names it. So an apart thread named only by its title is counted as dropped. The debug
line in `crates/theseus-core/src/rpc/home_claim.rs` ~:151 ("the turn's directory lies under none of these threads'
roots") then names it. Fix: compare entity hits only. Take `h.len()` before the `retain` and compare after it
(`h.len() < before`). The debug trace is the only reader. Nothing else changes.

**(2) Rewrap `crates/theseus-core/AGENTS.md` line ~604.** The line that begins `theseus-4qdh8: two titles named contend
and are asked; ...` is about 371 characters wide. Rewrap it to the file's width (about 120), with no word changed.
Touch no other line of that file for this item.

### theseus-capcs (review follow-up 3 of the dispatch row). IN.

The `FAMILIES` table (`crates/theseus-core/src/tools_defer/family.rs` ~:38-41) now says `tasks` holds "tasks and
sub-sessions, and the task graph; background work with a budget is task_create". The judge's labelled set
`crates/theseus-judge/fixtures/eval/families.v1.json` still has `"holds": "tasks and sub-sessions, and the task graph"`
in all 7 cases (~:46, 129, 212, 295, 378, 461, 544). Refresh those to the table's text. Its other family lines (`aws`,
`terminals`) match the table today; the `mcp:tidebase` family is invented and not in the table.
`fixtures/golden/families.v1.request.json` holds no `tasks` line: check it, and leave it if so. Add a guard so the
fixture cannot drift again (see Tests). Do not change any case's `expect` or message.

### theseus-s48k6 (review LOW of the dispatch row; optional). IN: it is small.

`crates/theseus-core/src/turn/defer_step.rs` `deferral_in` (~:137). For a tool-search model
(`mechanism == Mechanism::Search`), a message that asks for a task inserts `family::TASKS` into `extra` (~:169-176)
before `fit` exists (~:182). Nothing holds it to the ceiling, and `Fit::demote` never demotes an extra family. Without
tool search, `want::extra` (`tools_defer/want.rs` ~:137) admits the same family as `By::Asked`, but only while
`fit.total(&now) <= fit.ceiling`. Fix: hold the asked family to the same ceiling on the search path. Build `fit`
first, then admit `TASKS` only if the loaded set plus `TASKS` stays at or under `fit.ceiling` (re-offer with the new
`extra`, as the Find branch does). Use one predicate for both paths, not two copies: if `want.rs` has the check, call
it. With tool search, a family that is not admitted stays deferred and searchable, so the model loses no tool. Keep
the plan-mode family (`PLAN_FAMILY`) as it is. If this grows past ~30 lines or needs a `Fit` change, STOP, revert it,
and report s48k6 as OUT with the reason.

### theseus-izyws (live check finding, no review). IN.

`crates/theseus-core/src/google/drive.rs` `Reading::run` (~:676). For a file that is not a Google type, the
`_ =>` arm (~:737-745) refuses when `i.size > INLINE_MAX` (1 MB) and `save_to` is not given:
`"{} is {} bytes, over 1 MB: `save_to` writes it to a workspace path"`. A live read of an 11.5 MB audio file got that
error, though the metadata read had succeeded. Fix: over the cap with no `save_to`, return an Ok answer from the
metadata alone: name, type (`type_word`), size in bytes, owner, modified time and id (all in `Info` ~:229; reuse
`Info::line` ~:272 or its pieces), then one line: `save_to` with a workspace path writes it whole. Download nothing. Keep `finish(g.account(), text, m, row,
i.source())`, so the same source rule as a full read applies (a file that is not the owner's own and unshared is
outside text). Put `"inline": false` (or similar) and `"bytes": i.size` in the result's meta. The two other refusals
in `shown` (~:787-800: text over 1 MB after an export, and "is not text") are the same case: the content cannot be
shown inline. Give them the same metadata answer through the same helper if that fits cleanly. Say in the report
which paths you changed. Leave the tool's description (~:383-386) as it is: it is still true that the content needs
`save_to`. Changing tool text can break the pinned tool-hash tests (`tests_open_view`).

## Tests (each red at the base, with a planted revert)

Write each test first, see it fail at the base, then fix. In the report, name each planted revert (undo the fix, name
the test that fails), then restore and `touch` the file.

- **`a_turnless_question_of_a_placed_session_is_not_owed`** (TUI `chat/tests_thread_ask.rs`, the placed-thread rig
  `placed_script`/`placed`). A placed session gets a `confirm.requested` with no `turn_id`, then a
  `turn.started{continuation}` and its reply. The reply does not print in the chat. Red at the base: `placed_asked`
  marks owed. Plant: drop the `turn_id` check.
- **`a_closed_question_leaves_nothing_owed`** (same file, only if the core shows an answer with no continuation). The
  placed turn's question expires, then a later continuation of that session (another surface's) starts. It does not
  print. Plant: drop the clear.
- **`an_evicted_placed_session_is_unwatched`** (same file). Place 9 turns in 9 thread sessions. The 9th adopt emits
  `session.unwatch` for the first, and never for the chat's own session. Red at the base: no unwatch. Plant: drop the
  effect.
- **`a_needs_you_post_says_the_rule_once`** (Discord `render/ask.rs` tests, beside
  `an_approval_card_names_who_what_why_risk_and_yes_no`). A card whose `why` and `held` are the same clause shows it
  once. A card whose `held` differs still shows `-# held because`. Red at the base: two copies. Plant: drop the
  comparison.
- **`a_clause_ending_in_a_stop_prints_one`** (core `needs_you/words.rs` tests). A no-`plain` approval whose reason's
  clause ends in "." gives `why` ending in exactly one ".". Red at the base: "..". Plant: drop the trim.
- **`a_title_only_apart_thread_is_not_dropped`** (core `thread_route.rs` tests, beside
  `an_apart_thread_keeps_its_named_title` ~:1578). An apart thread named only by its title, with no sticky, chain or
  learned weight, is not in `apart_by_dir`'s returned list. A thread with a dropped file hit still is. Red at the base.
  Plant: compare against `p.hits.len()` again.
- **`the_labelled_families_say_the_tables_lines`** (core `tests_families.rs`). For each case of
  `theseus_judge::eval_labelled::set("families.v1")`, each family whose key is in `tools_defer::family::FAMILIES` has
  `holds` equal to that table's line. Red at the base on `tasks`. Plant: restore one old line.
- **`an_asked_family_on_tool_search_stays_under_the_ceiling`** (core `tests_dispatch.rs`, its `rig(true)` with a
  config tweak that makes the ceiling too tight for `tasks`, e.g. a low `admit_ceiling` or place budget). `STEP_ONE`
  leaves `task_create` deferred (`defer_loading: true`). With the base config, `STEP_ONE` still loads it (the existing
  test stays green). Red at the base. Plant: insert `TASKS` unconditionally again.
- **`a_large_file_without_save_to_answers_with_its_metadata`** (core `google/tests_drive.rs`, the fake Google
  `drive_rig`, as `a_large_binary_needs_save_to` ~:294 does). A 1 MB + 1 binary read with no `save_to` is Ok. Its text
  has the name, the type word, the size, the owner and the modified time, and says `save_to`. Nothing is downloaded
  (no `alt` query in `r.fake.log()`). Red at the base: it errors. The existing `a_large_binary_needs_save_to` asserts
  `unwrap_err()` for that read: change that assert to the new Ok answer, at least as strict (keep the
  "nothing was downloaded" assert and the `save_to` path asserts). Do not rename it. If you change the "not text"
  path, update its assert there the same way. Plant: restore the refusal.

Run at the end: `cargo nextest run -p theseus-tui`, `-p theseus-core` (whole crate, with the core golden),
`-p theseus-discord` (the DM golden), `-p theseus-judge` (the refreshed fixture still parses);
`cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --check`; then, if the VM has the disk,
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. In a cloud VM the `theseusd::sandbox` set fails as root;
name any other red.

## Not in this row

- p2f-ops-act, p2f-ops-ui, p2f-ops-daemon (lane `integ/ops`): the cockpit, the protocol's new methods, `client.rs`, the
  rehome/session.profile handlers, `calendar_cache.rs` and the Google calendar tests. You touch `google/drive.rs` and
  `google/tests_drive.rs` only, not the calendar files.
- p2f-turn (`integ/p2-route`): the turn budget, the bench driver, provider timeouts. p2f-caps (`integ/p2-caps`): card
  drafting, guidance, forced-tool calls. p2f-onto, p2f-bench, p2f-fanout: the ontology, bench rigs, async fan-out.
- Shared file: `crates/theseus-core/AGENTS.md`. Others add sentences in their own sections. You rewrap one line (~604)
  only. In the TUI's `AGENTS.md` chat-thread paragraph (~:156-161) add at most one short sentence for L1/L2. Name each
  AGENTS.md line you touch in the report.
- Do not touch the thread router's scoring, recall, `rule_clause`, `misroute.rs`, the protocol (`crates/theseus-protocol`)
  or `cockpit/src/protocol.gen/`. No protocol type changes in this row, so no regeneration.

## Rules for this row

- **Locked Discord DM task flow.** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical:
  `git diff --exit-code 64309ecb -- crates/theseus-discord/src/golden/dm_flow.txt` must be empty before the report. A
  fix that would change any message of that flow (wording, layout, order or timing) STOPS that part: write
  `OWNER CALL: <what and why>, before/after text` in the report instead.
- **Keel.** No removed, ignored or weakened test, no raised budget, cap or ceiling, no edit of `scripts/keel-guard.py`
  or its tests, no renamed test. Changing an expected string or an err-to-ok assert to the new honest behaviour is not
  weakening if the assert stays as strict. A cloud row cannot carry a `Keel:` trailer: if the guard flags something,
  undo that change and report it. Run `python3 scripts/keel-guard.py --base 64309ecb --head HEAD --quiet` before the
  report and paste its output (it must be empty).
- **Data floor.** Never loosened. The Drive metadata answer keeps the file's source rule (`i.source()`). It sends no
  more of a private file anywhere than a full read would.
- **Public repo.** No person's name, no real organisation, address, calendar id or email anywhere. Fixtures use
  invented names and ids under `example.test`. Write "the owner".
- **Red first; real behaviour.** Each fix is real-world behaviour, never a benchmark special case (no benchmark task
  name in code or tests).
- **Right over compatible; one helper, not two.** Reuse `Info`, `watch_session`'s form, `want.rs`'s ceiling check.
  Keep the code minimal. No new dependencies.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B 64309ecb`; do not retype it).
- Read `crates/theseus-core/AGENTS.md` (threads, needs-you, tools_defer, Google), `crates/theseus-tui/AGENTS.md` (the
  chat and placed threads) and `crates/theseus-discord/AGENTS.md` first.

## Your report

Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check that it is non-empty
after the commit. Push the branch, and end with a short final message that starts `CLOUD REPORT COMPLETE`.

Sections, in order:
1. **For the maintainer first**: store format (49, unchanged), any file within a line of 2,500, every OWNER CALL with
   before/after text, and the L3 needs-you post's text before and after (owner FYI).
2. **Per issue and finding** (hk0dk L1-L4, axno0 1-2, capcs, s48k6, izyws): done or not and why, the cause as you
   found it, the test, the planted revert and the line that failed. Say what the core showed about continuations after
   an expired question (L1), and which `drive.read` paths now answer with metadata (izyws). OUT: M1 (owner call); s48k6
   too, if you stopped it, with the reason.
3. **Gate**: each command above and its result.
4. **Keel**: the guard's output (empty).
5. **DM golden**: the `git diff --exit-code 64309ecb -- crates/theseus-discord/src/golden/dm_flow.txt` result.
6. **Join notes for `integ/p2-follow`**: every AGENTS.md line touched, and the files this row shares with no one else.
7. A `docs/status.md` sentence for the maintainer, and any follow-ups.
