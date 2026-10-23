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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-p2-card-drafting`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
integration lanes named in each task (`integ/p2-route`, `integ/p2-caps`, and others). Other rows land in the same
lanes from other branches at the same time, some built on another host. Your task names them and the files you share
with them: stay additive (one helper, not two), and leave their files alone.

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
- `crates/theseus-core/src/config.rs` 2,149 (unlisted, cap 2,500): a new config section goes in `config/<name>.rs`.

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
3. The data floor is never loosened: nothing private reaches a model of China origin, and nothing marked private or
   sensitive reaches a model the floor does not clear for it. A row near data verdicts ends its report with a
   "Data-routing review" section that a Claude model reviews.
4. The repository is public: no person's name, no real organisation, address or id anywhere. Fixtures use invented
   names and ids under `example.test`. Write "the owner".
5. Red first: each fix comes with a test that is red at the base and green after, and a planted revert named in the
   report. Real-world behaviour, never a benchmark special case: no task name from any benchmark in code or tests.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: p2-card-drafting, the owner card's proposals: stop reading agent-setup files as facts about the owner, keep one-task instructions scoped, say things fully and the right way round, never offer two contradictory proposals together, and let the owner set a kept record's scope (theseus-ly75u)

Branch: `cloud/20261023-p2-card-drafting`. Every commit's subject carries `theseus-ly75u`. Deadline for the report: 4
hours after you start.

**Why.** The owner card job (`crates/theseus-core/src/card/`, theseus-slvl.6) reads the imported history and proposes
short facts and preferences about the owner, one question each ("Add to your card: '...' (from 14 sources)? [Keep]
[Edit] [Not this]"). The owner reviewed the 30 proposals that were waiting on 2026-10-22: 12 kept (11 of them
redrafted with the owner's help, plus one he wrote himself), 18 declined. The findings:
1. **Agent-setup files read as facts about the owner.** 6 proposals restated another agent harness's workspace
   scaffolding (its soul, bootstrap, user, memory and daily-log files: instructions to an assistant about how to run
   itself) as if they were facts or preferences of the owner.
2. **One-task instructions applied everywhere.** 6 proposals were one repository's merge procedures, and one was a
   single task's "no builds in this task", each scoped `everywhere`.
3. **A meaning turned round.** "Don't speak as you in group chats" inverted its source, which said: do not speak ON the
   owner's behalf.
4. **Thin, undirected wording.** "Have a personality": nothing a turn could act on.
5. **Two contradictory proposals offered together** in one batch (one said ask first, the other said act).
6. **No way to set a kept record's scope.** A kept record about merge conflicts wanted a coding scope, and there was no
   way to say so: the owner asked for `card edit --scope`.

**This is a design row (Opus):** the drafting fixes are judgment about what evidence about a person is. Before you
code, write "Design" in your report (committed first): for 1, 2 and 5, the options (code rule before the call, an
instruction to the model, a check after it), the pick and why; for 6, the scope words and the surfaces they change.

**Risk: medium.** Nothing reaches the card without the owner's answer, and the job is off every turn, so a mistake
costs a bad question, not a bad turn. But a proposal is an owner-facing message: the locked-flow rule below applies.

**Prerequisite: base `origin/main` at `23328545`.** Your branch joins the lane `integ/p2-caps`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `p2-card-drafting` | theseus-ly75u | 1-5 in the evidence, the clustering, the ask's instructions and tool, and a batch check; 6 the scope on keep/edit and for a kept record | core `card/evidence.rs` (which episodes and lines are evidence), `card/cluster.rs` or `card/run.rs` (the batch check, the scope pick), `card/ask.rs` (ONLY `INSTRUCTIONS`, `tool()` and `parse`: see "Shared"), `card/answer.rs` (the scope on keep/edit), the place a kept record's scope changes (`rpc/standing.rs` or a new small method), tests in `card/tests_pure.rs`, `card/tests_job.rs` and `card/fixture.rs` (add cases; do not rewrite); protocol `card.rs` (`CardAnswerParams.scope`, serde-default) and `standing.rs` only if a new scope kind is needed; the CLI `crates/theseus/src/card_cmd.rs` (`--scope`); `cockpit/src/protocol.gen/` (regenerated only); core `AGENTS.md` (the card section) | M-L |

## What exists (at `23328545`; search by name)
- **The job.** `crates/theseus-core/src/card/mod.rs`: read its module doc first (which evidence, labels respected
  before any call, budgeted, each proposal a question, idempotent). `run.rs` runs it; `evidence.rs` picks episodes by
  label (a USER-style file, a feedback note, an SOP, a `decision_or_preference` triage) and splits them into lines
  (`LINE_CHARS`, `PER_EPISODE`, `MIN_WORDS`; whose words: the owner's own messages, or a note's operator and agent
  text); `cluster.rs` groups near-duplicates; `ask.rs` asks `[card] profile` (Haiku) through one forced tool
  (`TOOL = "state_cards"`, `INSTRUCTIONS`, `tool()`, `request()`, `parse()`, `BATCH = 8`); a statement citing none of
  its cluster's episodes, copying a line, or repeating a live record or an answered proposal is dropped.
- **Scope.** Protocol `crates/theseus-protocol/src/standing.rs`: `StandingScope { Session, Place, Surface, Persona,
  Everywhere }` (words via `words()`: `everywhere`, `place:<target>`, `surface:<name>`, `session:<id>`,
  `persona:<key>`), `SURFACES`. There is no scope for a kind of work (coding). `CardProposal.scope` exists;
  `card_question(text, scope, support)` (protocol `card.rs` near :241) already words a non-everywhere scope.
- **Answers.** `CardAnswerParams { id, option, text, author, discord }` (protocol `card.rs` near :195); `card.answer`
  in core `card/answer.rs`; Keep and Edit write the standing record through `standing.add` with `source`
  `card:<id>`. Standing methods: `standing.list/add/revoke/proposals/keep/dismiss/answer` (protocol `lib.rs` near
  :279): there is no edit of a kept record's scope. The CLI: `crates/theseus/src/card_cmd.rs` (`Keep { id }`,
  `Edit { id, text }`, `Decline`, `Done`).
- **Turns' modes.** The route pack's `mode` answer (`deep_coding`, `routine_coding`, `review`, ...) and the place
  rule decide what a turn is; read how standing records are chosen for a turn (`rpc/standing.rs`, the compiler's
  standing block) before you pick how a coding scope is matched.

## What to build (after the Design section is committed)
1. **Agent-setup files are not facts about the owner.** An episode whose file is an assistant's own instructions (by
   its kind and its text: the second person addressed to an assistant, how to run a session, read this file first,
   memory upkeep, bootstrapping) gives no owner facts. Where such a file also holds lines about the owner (a user file
   often does), keep only those lines, judged by their subject. The default: a code rule in `evidence.rs` before any
   call (cheap, testable), plus one instruction line. Count what it drops in the run's result (as `withheld` is
   counted) so the owner can see it.
2. **One-task instructions keep a narrow scope.** A line that speaks of one task, one repository, one branch or one
   run ("in this task", "for this repo", a merge or release procedure of one project) is either dropped or proposed
   with a narrow scope, never `everywhere`. Pick in Design; the default: the model marks a statement `task_scoped`
   (or names the scope it holds in) through the tool, and code maps it to the narrowest scope that exists, or drops it
   when none fits.
3. **Meaning kept and wording full.** The instructions say: keep who acts and on whose behalf exactly as the source
   says (an instruction NOT to speak on the owner's behalf must not become an instruction not to speak as oneself);
   each statement says what to do or not do and when, in words a turn could act on; a statement that cannot be made
   specific is not proposed. Add a cheap check where one helps (for example a statement whose negation flips against
   its cited lines is dropped), but do not add a model call.
4. **No contradictory pair in one batch.** Two statements of one run that give opposite instructions for the same
   situation are not both offered: the default is a check after parsing (the tool returns, per statement, what it
   conflicts with, or a code check by shared subject words), keeping the one with more support and noting the other in
   the run's result. Decide in Design; no extra model call unless the Design shows it is needed and costs under the
   job's day budget.
5. **The owner sets the scope (6).**
   - `card keep <id> --scope <words>` and `card edit <id> --scope <words> <text>`: `CardAnswerParams.scope:
     Option<String>` (serde-default, skipped when `None`), parsed by one function from `StandingScope::words()`'s
     forms; an unknown form is refused with the list of forms.
   - A kept record's scope can be changed: the smallest path (a `card scope <id> <words>` that revokes and re-adds with
     the same text and source, or a small `standing` method). Pick in Design; authority as `card.answer`'s (the
     owner's act).
   - **A coding scope.** If the Design shows a new scope kind is needed (for example `mode:<route mode>` or
     `work:coding`, matched by the turn's route mode), add it to `StandingScope` (a stored record's enum: store format
     49 to 50, with the old layout's literal sample in `tests_layouts.rs` as AGENTS.md says), with `words()` and the
     match where standing records are chosen. If an existing kind fits, use it and say why.
6. **Docs.** Core `AGENTS.md` (the card section): the evidence rule, the scope words, the batch check. The CLI's help
   text for `--scope`.

## Tests (red first: write each, see it fail on the base code, then fix; each behaviour change gets a planted revert)
Fixtures are invented (an assistant's instruction file under invented names, an invented repository's merge steps);
no real file of any harness and no person's name.
- **Evidence:** an assistant-instruction file gives no lines; a user file mixing owner facts and assistant
  instructions gives only the owner lines; the run's result counts what was dropped.
- **Scope of one task:** "no builds in this task" is never proposed `everywhere`; an invented repository's merge
  procedure is dropped or scoped narrow; a general preference is still `everywhere`.
- **Meaning:** the instructions carry the who-and-on-whose-behalf rule (a pure test on `INSTRUCTIONS`); a fake model
  answer that flips a "do not speak on the owner's behalf" source is dropped if you add the check.
- **Batch:** two contradictory statements in one fake answer yield one proposal and one noted conflict.
- **Scope answers:** keep with `--scope surface:terminal` writes the record with that scope; an unknown form is refused
  and nothing is written; changing a kept record's scope keeps its text and source; the coding scope (if added) applies
  to a coding turn and not to a chat turn; the old layout still reads (if the format bumped).
- **No Discord change:** the Discord golden and any card question test stay green.
- Run `cargo nextest run -p theseus-core card`, `-p theseus-core standing`, `-p theseus-protocol`, `-p theseus`
  (the CLI), `-p theseus-discord`, then the gate.

## Shared with other rows (stay additive)
- **Lane `integ/p2-caps` also gets two rows built on another host:** `p2-caps-stop` (theseus-2c6iz, theseus-1iqg0,
  theseus-kf4pf: one shared helper for "this answer hit its cap") and `p2-caps-forced` (theseus-opypg, theseus-j06y6:
  the forced-tool small-cap calls, the **card ask** among them). In `card/ask.rs` they change ONLY the request's
  `thinking`, `max_tokens` (`MAX_TOKENS`), `tool_choice`, and the failure text when the answer holds no tool call (to
  name a `max_tokens` stop). You change ONLY `INSTRUCTIONS`, `tool()`'s schema and `parse()`. Keep your edits off
  `request()` and off the "held no call" failure path, so the join merges cleanly; if your tool schema grows, say by
  how much in the report (the other row sizes the cap).
- **Another row** (`p2-public-names`, theseus-cuv2o) replaces old organisation names in fixture and test lines across
  the tree: if a fixture line you need is one it changes, add a new line rather than edit that one.
- Thread cards, task cards and the persona card are other features (other rows): do not touch them.
- Core `AGENTS.md`: the card section only.

## Not in this row
- The card ask's thinking, cap, tool choice and stop handling (above). The job's budget, schedule or `max_open`.
- Any Discord message or button, the card question's wording on any surface, `golden/dm_flow.txt`.

## Rules for this row
- Model roles: you design and write the code; keep it minimal and elegant. Every line you add should earn its place.
- **Locked owner-facing flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical
  (`git diff --exit-code origin/main -- crates/theseus-discord/src/golden/dm_flow.txt`). If the scope setting (or any
  fix) needs a new owner-facing message, a new button, or a change to how a card question is shown on Discord, stop
  that part and write "OWNER CALL: <what and why>, before/after text" in your report. The CLI flag and the protocol
  field are in scope.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`,
  then `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Protocol `lib.rs`:** at most 3 added lines (a method constant if you add one). **Dependencies:** none new.
  **Store format** 49; a new scope kind bumps it to 50 (say so in the report's first section).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Names.** No person's name anywhere: write "the owner". No real organisation, project or harness name in fixtures.
- Read the root `AGENTS.md`, `crates/theseus-core/AGENTS.md` (the card, standing and the place rule) and
  `crates/theseus-protocol/AGENTS.md` first. Run `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- "Design" first (for 1, 2, 4 and 6: options, the pick and why);
- what you built, item by item (1-6), with the file and function for each; the new `INSTRUCTIONS` text in full; the
  tool schema's growth in tokens (for the cap row);
- the scope words accepted and where a coding scope is matched;
- each test, and its plant with the failing line, the red-first runs on the base included;
- the keel guard's output, the store format, the Discord golden's `git diff --exit-code` result, any OWNER CALL;
- a `docs/status.md` sentence for the integrator; the join note for `card/ask.rs` (which lines are yours);
- follow-ups (for example a re-draft run of the declined proposals' sources after install, owner-approved).

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
