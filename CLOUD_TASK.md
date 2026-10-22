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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail3-google`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `29f5dcad` (2026-10-22: install #69, which landed the patch1 tail lane `integ/tail` and the walk1 fixes); that is your base. Where the
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
- `crates/theseus-protocol/src/lib.rs` 2,731 / 2,733 (two lines of room)
- `crates/theseus/src/render.rs` 3,122 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections below. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail3-google, the Google tools' small gaps, a learning report that degrades instead of failing, and the docs three merged rows left (theseus-xe77s, theseus-efb8i, theseus-94782)

Branch: `cloud/20261021-tail3-google`. Every commit's subject carries the Beads id of what it changes
(`theseus-xe77s`, `theseus-efb8i`, `theseus-94782`). Deadline for the report: 3 hours after you start.

**Why.** Follow-ups of three rows now on `main`: tail2-google, tail-onto and tail-effort-400.
- **theseus-xe77s (Google).** (1) `drive.rs` `balanced()` treats only single quotes as string delimiters, so an
  unquoted `"` in a raw `q` passes the balance check that keeps a wrapped query one group. (2) `sheets.rs` `refused()`
  with no rows sets `meta.google_called = null` and the result records a `GoogleCalled { row: null }`; latent (no path
  reaches it today). (3) `sheets.rs` create accepts `spreadsheetId: ""` as an id. (4) Mail writes one merged
  `google.called` row per tool call (`mail.rs` `merged`), where Drive and Sheets keep every REST call's row; make mail
  one row per call like the others (`tests_mail.rs` near :846 marks the spot).
- **theseus-efb8i (learning report).** (1) `rpc/learning.rs` `r.undo = self.onto_undo_to(now_ms)?;` fails the whole
  `learning.report` on a store error reading the undo rows; the line should be absent instead. (2) "7 days" is a
  literal in the CLI's `render/learning.rs` and the cockpit's `ontostats.ts`; derive it from the window. (3)
  `judge/onto/judged.rs`'s module doc has one line past the wrap width. And the docs the tail-onto author named.
- **theseus-94782 (effort docs).** The status page and AGENTS.md should say the 2026-10-11 Sonnet 5.5 400 was a request
  sent without the effort beta; one AGENTS.md line in the effort paragraph is 210 characters in a 120-wrapped
  paragraph; and the docs should note the server-side fallback case (`BETA_FALLBACKS` to a model without per-message
  effort).

**Risk: low.** Three input checks, one ledger-row change in mail (rows, not behaviour), one `?` made a warning, two
derived strings, docs.

**Prerequisite: base `origin/main` at `29f5dcad`.** Your branch joins the new lane `integ/tail3`. Run the keel guard
against `origin/main`. **This row owns `docs/status.md`, `crates/theseus-core/AGENTS.md` and `cockpit/AGENTS.md` in
this wave**: the other two `integ/tail3` rows put their sentences in their reports, and the integrator adds them.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail3-google` | theseus-xe77s, theseus-efb8i, theseus-94782 (docs part) | `"` in `balanced`; `refused` with no rows; empty spreadsheet id; mail rows per call; the undo line degrades; "7 days" derived; doc wrap; status/AGENTS docs | `crates/theseus-core/src/google/{drive,sheets,mail}.rs`, `google/{tests_drive,tests_sheets_slides,tests_mail}.rs`, `crates/theseus-core/src/rpc/learning.rs`, `crates/theseus/src/render/learning.rs`, `cockpit/src/lib/ontostats.ts` (+ its test), `crates/theseus-core/src/judge/onto/judged.rs` (doc only), `crates/theseus-core/AGENTS.md`, `cockpit/AGENTS.md`, `docs/status.md` | M |

## What exists (lines at `29f5dcad`; search by name, a line may have moved)
- **`balanced`** `crates/theseus-core/src/google/drive.rs` :175 (doc: "Whether `q`'s parentheses and single quotes (with
  `\'` escapes) pair up, so wrapping it in parentheses keeps it one group"); used by `compile_q` just below.
- **`refused`** `crates/theseus-core/src/google/sheets.rs` :78: `let (mut meta, last) = with_rows(json!({"private":
  true}), rows); meta["google_called"] = last;` where `with_rows` pops the last row or `Value::Null`.
- **Create** `sheets.rs` near :490: `let Some(id) = j["spreadsheetId"].as_str().map(String::from) else { return
  Err(refused("the create answer named no spreadsheet id", rows)); };`.
- **Mail rows** `crates/theseus-core/src/google/mail.rs` :90 `fn merged(rows)` (lead row: authority, then write, else
  first; attempts and quota summed; a failure makes the outcome the failure's) and its callers; Drive's per-call rows
  are `drive.rs` `call(g, rows, m, req)` (pushes `done.row`) and Sheets' `with_rows` (`google_called` the last,
  `google_called_also` the rest). The ledger row kind is `google.called`; check how the ledger and `fact/google.rs`
  read `google_called_also` so mail's rows land the same way.
- **The undo line** `crates/theseus-core/src/rpc/learning.rs` :312 `r.undo = self.onto_undo_to(now_ms)?;` and
  `fn onto_undo_to` :345 (`learning::onto_undo::WINDOW_MS`, `crates/theseus-core/src/learning/onto_undo.rs` :25, seven
  days). The renderers: `crates/theseus/src/render/learning.rs` :109 `"undo rate by kind (Jev's writes, 7 days to
  {date}): {}"` and its test :211; `cockpit/src/lib/ontostats.ts` :72 (and its doc :6, :68). Check whether the
  protocol's learning result already carries the window (search `window` in `crates/theseus-protocol/src/learning.rs`).
- **judged.rs** `crates/theseus-core/src/judge/onto/judged.rs` module doc: one line over the crate's wrap width.
- **Docs.** `crates/theseus-core/AGENTS.md` (3,008 lines): the effort paragraph near :1600-1625, with a 210-character
  line near :1616; the nightly paragraph (search `dream_guidance` / `learning_hour`). `cockpit/AGENTS.md`: the Ontology
  bullet. `docs/status.md`: dated entries, newest first; find the newest and add one above it.

## What to build
1. **theseus-xe77s.**
   - (1) `balanced` treats `"` as a delimiter exactly as it treats `'` (with `\"` escapes), and its doc says so. A
     query with an unpaired `"` is refused as unbalanced.
   - (2) `refused` sets `google_called` only when there is a last row; with none, the key is absent and no
     `GoogleCalled { row: null }` is recorded.
   - (3) `spreadsheetId` must be non-empty: `.filter(|s| !s.is_empty())` before the `else`.
   - (4) Mail keeps one `google.called` row per REST call, the same shape as Drive and Sheets (the result's own row
     last, the rest in `google_called_also`), and `merged` goes if nothing else needs it. If the ledger's readers
     (`fact/google.rs`, the cockpit) or a test depend on mail's single merged row, change them to the shared shape and
     name each in the report. Item (5) of the issue (`drive.write` sits exactly at `MAX_TOKENS` 220) is a note: do not
     add a word to that tool's definition.
2. **theseus-efb8i.**
   - (1) `onto_undo_to`'s error degrades: `unwrap_or_else` with `tracing::warn!` naming the error, and the report's
     undo line is absent. Test: a store that fails that read still returns the rest of the report.
   - (2) The "7 days" in both renderers comes from the window: if the result carries the window, read it; otherwise
     add it to the result as an optional field with `#[serde(default)]` (protocol `learning.rs`, regenerate the
     cockpit's `protocol.gen` with the protocol's ts test) and say so. Prefer what exists.
   - (3) Rewrap judged.rs's module doc line.
   - Docs (the tail-onto author's): `docs/status.md` (one undo-rate definition, hidden memberships, `--onto`
     refusals); `crates/theseus-core/AGENTS.md`'s nightly paragraph (`dream_guidance`, the per-question mark, a session
     it cannot read is skipped); `cockpit/AGENTS.md`'s Ontology bullet (`pack.undo`, `groupfold.ts`). Read the code for
     each before you write it; write only what the code does.
3. **theseus-94782 (docs part).** In AGENTS.md's effort paragraph: rewrap the 210-character line; add that the
   2026-10-11 Sonnet 5.5 400 (`output_config: Extra inputs are not permitted`) was a request sent without the effort
   beta, and the server-side fallback case (a `BETA_FALLBACKS` model without per-message effort: read
   `compiler/effort.rs` for what the code does there). Do not change any other sentence of that paragraph.
4. **`docs/status.md`: one dated entry**, "**2026-10-22: v1 patch1 tail, part 3**", above the newest, in that page's
   plain prose: the Google checks (a `"` in a raw Drive query, mail's ledger rows per call), the learning report's
   undo line, and the effort note. Leave room for the integrator to add the other two rows' sentences.

## Tests (each with a planted revert; the first is the key test)
- **`a_double_quote_delimits_a_drive_query`** (Drive tests): `name = "a(b"` is balanced; `name = "a` is refused;
  `"it\"s"` balanced. Plant: restore the single-quote-only rule: the first or second case fails.
- `an_empty_spreadsheet_id_is_refused` (Sheets, fake Google). Plant: drop the filter.
- `refused` with no rows carries no `google_called` (pure). Plant: restore the unconditional set.
- Mail: a send that drafts then sends records two `google.called` rows (one per REST call), the send's last. Plant:
  merge them again. Existing mail tests that asserted one merged row change to the per-call shape: list each in the
  report with the old and the new assertion (this is a change of shape, not a weakened test).
- `a_failed_undo_read_leaves_the_rest_of_the_learning_report` (rpc). Plant: put back the `?`.
- The renderers' tests still print "7 days" for the 7-day window (derived). Plant: a window of 3 days prints "3 days".
- `cargo nextest run -p theseus-core google learning`, `cargo nextest run -p theseus render::learning`, the protocol ts
  test if you touched the wire, and the cockpit's `npm test` for `ontostats` (or say it could not run).

## Not in this row
- theseus-4jz82 (Google live checks: a real fromGmail event, Slides deleteText on an empty placeholder, writes into a
  nested shared folder) and theseus-94782's live check and spec Part III item: the owner's and the maintainer's.
- Any change to a tool definition's text (the 220-token tool-size test).
- `rpc/mod.rs`, `config/cockpit.rs`, test helpers (tail3-tests); the Discord crate and `theseus-voice` (tail3-discord).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- No new always-loaded model text and no new tool.
- **Line ceilings** (`scripts/long-files.txt`): none of your files is listed; each stays at or under 2,500 lines
  (`drive.rs` 1,549, `mail.rs` 1,456). Never raise a ceiling. `crates/theseus-protocol/src/lib.rs` is listed (tight):
  do not add to it; a learning field goes in `learning.rs`.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new, in `Cargo.lock` or any `package-lock.json`.
- **Store format** on your base is 48 (`MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). Ledger rows are not a
  stored layout change; if you find one is, bump to 49 with the old layout's sample in
  `crates/theseus-core/src/tests_layouts.rs` and say so.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture addresses use invented hosts under
  `example.test`.
- Read `crates/theseus-core/AGENTS.md` (Google, ontology, effort) and `cockpit/AGENTS.md` first. Run
  `cargo nextest run -p theseus-core`, `cargo nextest run -p theseus`, `cargo clippy --workspace --all-targets --
  -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id; every reader of mail's rows you changed; whether the window was already on
the wire; each test and its plant with the failing line; the AGENTS.md and status.md hunks by line range; what could
not run on the VM, with the exact commands; the keel guard's output; the store format; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
