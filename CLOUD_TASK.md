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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-p2-effort-max`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: p2-effort-max, the effort step: evidence that can move Jev's effort answer to max on hard work (theseus-ujzuh), and a later verdict that ends a sticky effort hold early, with a lower bar for `sticky_unsure` (theseus-u5a37)

Branch: `cloud/20261023-p2-effort-max`. Every commit's subject carries the Beads id it serves (`theseus-ujzuh` or
`theseus-u5a37`). Deadline for the report: 3 hours after you start.

**Why.** Two follow-ups on the same code, the effort a turn runs at (route.v3 on: Jev, the routing judge, answers
`reply_effort` with each message, and `routing::effort::decide` turns that answer into the level the request carries).
They are one row so they do not collide in `effort.rs` and `effort_step.rs`.

1. **theseus-ujzuh: max almost never runs.** A benchmark retest on 2026-10-23 (27 hard programming tasks x 3, the MAX
   tier, effort bounds `low..max`) found: in 118 route decisions Jev asked for `max` **once** (confidence 0.39, so held
   at high as `held_high_unsure`); `xhigh` was asked 66 times (35 above 0.40 ran at xhigh, 8 at 0.35-0.40 held at high,
   23 under 0.35 kept the profile's own effort). Two days earlier, on the same tasks, Jev asked for max 5 times
   (0.35-0.41). So the gate is open (theseus-7pm8t, landed) but max is never asked. This is a real-world question, not
   a benchmark tweak: **what evidence should move the effort answer to max for a user's hard task** (a failed check,
   repeated tool failures, work that keeps failing after more effort was already given)? Today Jev judges effort from
   the message alone at the inbound route, and from a task's recent state at a checkpoint; neither text tells it how
   much effort the work already ran at, or how many checkpoints in a row found the work failing. The `max` option's
   own meaning (route.v7, `reply_effort`) names "the hardest or most novel problems, or high-stakes work where every
   step must be checked, or the person asks for the most thorough reply possible": it names no evidence from the work.
2. **theseus-u5a37: a sticky hold has no way out but a two-step change.** The owner, 2026-10-22 (verbatim): "That
   sounds fine. I suppose that we could later introduce a mechanism to revert a sticky, and, we'd want a lower bar to
   revert it for sticky_unsure". The sticky rule (theseus-rwpbg.4) holds a one-step change of effort at the previous
   request's level on a model without per-message effort (a change of the top-level effort rewrites the messages'
   cache); `sticky_unsure` is the same hold of an `applied_unsure` or `held_high_unsure` answer. This row adds the way
   out: during a hold, a later route verdict that disagrees with the held level ends the hold early when it is sure
   enough, with a lower bar when the hold is a `sticky_unsure` one. Ledger reason `sticky_reverted`.

**Two owner calls are WAITING and are not this row's:** theseus-7pm8t (the effort bar decoupled from the model switch;
landed, the owner may close it) and theseus-goq94 (xhigh now runs often and doubles cost where it runs, for +1 solve in
30: keep xhigh at 0.40, raise it, or make xhigh/max depend on evidence of difficulty). Do NOT change
`effort_confidence`, `high_effort_confidence`, `effort_bounds`, the gate, or when xhigh applies. This row gives Jev the
evidence and adds the sticky revert; it does not decide the cost line. Your report's section "How this meets the owner
calls" says, for each of 7pm8t and goq94, what your change does to it (for goq94: evidence-led max asks are one of its
options; say how your evidence lines would serve that option without choosing it, and what extra cost a max that now
runs could bring, by your reading of the code).

**Risk: medium.** The effort step is on every routed turn. No new config key (a threshold the owner has not approved
must not appear as a `[routing]` key); reuse existing bars as below.

**Prerequisite: base `origin/main` at `23328545`.** Your branch joins the lane `integ/p2-route`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `p2-effort-max` | theseus-ujzuh, theseus-u5a37 | the checkpoint text tells Jev the effort the work ran at and how long it has kept failing; a proposed `max` wording handed to the integrator for route.v8; the sticky revert with its two bars and the `sticky_reverted` reason | core `routing/checkpoint.rs` (`text`, `text_within`, the call that builds it), `turn/checkpoint_step.rs` (only the ask's text and what it passes), `routing/effort.rs` (a pure revert rule next to `sticky`, `EffortReason::StickyReverted`), `turn/effort_step.rs` (`stick`), the keep-warm record of the last sent level (where `keep_warm.sent`/`last_effort` live), protocol `route.rs` (the effort reason word's doc only), `cockpit/src/protocol.gen/` only if a generated file lists the reason words (regenerated, never hand-edited); tests in `tests_effort_sticky.rs`, `tests_checkpoint_effort.rs` and `routing/effort.rs`'s own tests (add, do not rewrite); core `AGENTS.md` (the effort section) | M |

## What exists (at `23328545`; search by name)
- **The effort rule.** `crates/theseus-core/src/routing/effort.rs`: read its module doc first. `decide(bounds, bars,
  answer, runs, acts, carried)` and `sticky(asked, previous, tokens, floor)`; `EffortReason` (`as_str` gives each
  word; `the_wires_doc_lists_every_effort_reason_word` pins the protocol doc); `Bars { effort, high }` from
  `[routing] effort_confidence` (0.35) and `high_effort_confidence` (0.40).
- **The sticky hold.** `crates/theseus-core/src/turn/effort_step.rs` `TurnRunner::stick`: a decision that `applied()`
  on a caching model without per-message effort is held at `keep_warm.last_effort(session, model)` when `sticky`
  says so; the reason becomes `Sticky`, or `StickyUnsure` for an `AppliedUnsure`/`HeldHighUnsure` answer. A
  checkpoint's raise is never held. `note_sent` records the level each request carried. Tests:
  `tests_effort_sticky.rs` (`a_sticky_hold_of_an_unsure_answer_keeps_its_bar`,
  `a_sticky_hold_of_a_held_high_answer_keeps_its_bar`, `sticky_is_one_step_at_most`).
- **The answer's sureness.** `EffortAnswer { level, confidence, sure }`: `sure` is the answer's band at or above the
  question's own `confirm` threshold in the pack (`crates/theseus-judge/packs/route.v7.toml`, `[questions.reply_effort]`
  `confirm = 0.60`, `act = 0.90`).
- **The checkpoint.** `crates/theseus-core/src/turn/checkpoint_step.rs` (read its module doc): a routed turn, or a
  task started with no profile, asks the route pack about its recent state when `routing::checkpoint::Tally` fires
  (`Trigger::Failing(n)`, `Quiet(n)`, `Phase`, `Report`); "Effort without a move" raises the effort by the same rule.
  The text Jev reads is `routing::checkpoint::text(brief, plan, loops, trigger)` (`crates/theseus-core/src/routing/
  checkpoint.rs`): the brief, the plan, the last `LOOP_LINES` loops with their first failed result, and `Now: <trigger
  line>`, within `MAX_TOKENS`. It says nothing about the effort the turn runs at, or how many checkpoints of this turn
  already fired on failure. Tests: `tests_checkpoint_effort.rs`, `tests_turn_checkpoint.rs`,
  `tests_task_checkpoint.rs`, and `checkpoint.rs`'s own tests.
- **The route pack.** `route.v7.toml` is live; "a live pack's options never change in place": a wording change is a new
  pack version. **This row does not create a pack version** (see "Shared with other rows").

## What to build
1. **The checkpoint text carries the work's effort evidence (ujzuh).** Before you code, read `decide`, the checkpoint's
   ask and `text`, and write in your report (3-6 lines) which evidence you add and why it is the evidence a person
   would use to ask for the most careful work. The default, unless the code shows a better one:
   - one line `Effort: <level the loops ran at>` (and `, raised from <level> at loop <n>` when a checkpoint raised it
     this turn), so a failure after more effort reads differently from a failure at the first level;
   - one line `Checkpoints this turn: <n>, failing <k> in a row` (or the task's, for a task's checkpoint), so repeated
     failure is visible, not only the last window of loops;
   - both lines kept when the text is cut to `MAX_TOKENS` (as the trigger's line is kept); no new model call, no new
     state in the store (the turn already holds what it needs; if it does not, keep the counter on the turn's
     checkpoint state, not on disk).
   Keep it general: no line names a tool, a language or a benchmark.
2. **A proposed `max` wording for route.v8, handed over, not applied (ujzuh).** Write in your report, under a heading
   "route.v8 join fix: reply_effort", the exact old and new `means` text of the `max` option (and of `xhigh` only if it
   must change for the two to stay distinct), naming the evidence in 1 ("the work has kept failing after more careful
   effort was already given, or a check it must pass still fails"). The integrator applies it to the route.v8 pack that
   the row `p2-route-data` creates. Do not edit any `packs/*.toml` file or any judge golden.
3. **The sticky revert (u5a37).** A pure rule in `routing/effort.rs` next to `sticky`, called from `stick`:
   - **The window.** A hold opens when `stick` holds a level (reason `Sticky` or `StickyUnsure`) and lasts while the
     session's requests on that model keep carrying the held level. Record, beside the level `note_sent` already
     keeps, which kind of hold carried it (`sticky` or `sticky_unsure`), in memory as the level is (no store format
     change). If keep-warm's record must grow a field, keep it `Option`/default so nothing else changes.
   - **The revert.** On a later turn inside the window, an answer that disagrees with the held level (its decided
     level differs from the held one) ends the hold: the request carries the decided level, and the reason is
     `sticky_reverted`, when its confidence clears the window's bar:
     - a `sticky` window: the answer is `sure` (the question's own `confirm` threshold, 0.60 in route.v7: read from
       the answer's band, not a literal);
     - a `sticky_unsure` window: confidence at or above `[routing] effort_confidence` (the lower bar the owner asked
       for).
     The turn that opens a hold never reverts it on the same answer. A checkpoint's raise stays never held (unchanged).
     The two-step rule of `sticky` stays as it is.
   - **Words.** `EffortReason::StickyReverted` (`"sticky_reverted"`), `applied()` true for it, the protocol
     `route.rs` doc list updated so `the_wires_doc_lists_every_effort_reason_word` stays green. Check whether any
     surface renders effort reason words (`git grep -n sticky_unsure -- crates cockpit`): a renderer that lists
     words gets the new one; the Discord DM task flow must not change (see Rules).
   - Write in your report the cache cost this adds (a revert rewrites the messages' cache once, as a two-step change
     does today) and when a revert would fire on a conversation that wobbles between two levels.
4. **Docs.** Core `AGENTS.md`, the effort section: one short paragraph for the checkpoint's effort lines and one for
   the revert (its two bars and the word). If `docs/` lists effort reason words, add `sticky_reverted` there.

## Tests (red first: write each, see it fail on the base code, then fix; each behaviour change gets a planted revert)
- **Checkpoint text:** `text` with a turn that ran at high and was raised to xhigh at loop 4, after two failing
  checkpoints, contains both lines; the lines survive the `MAX_TOKENS` cut (a long brief and many loops); a first
  checkpoint says `failing 1 in a row`; a task's checkpoint carries the same lines. Use the fake Jev of
  `tests_checkpoint_effort.rs` to assert the request Jev receives at the second failing checkpoint carries them.
- **Sticky revert (pure, in `effort.rs`):** a table over (window kind x answer sure/unsure x confidence 0.30, 0.34,
  0.35, 0.59, 0.60 x agrees/disagrees): `sticky` window reverts only on a sure disagreeing answer; `sticky_unsure`
  window reverts at `>= effort_confidence`; an agreeing answer never reverts; no window, no revert.
- **Sticky revert (turn, `tests_effort_sticky.rs`, its rig):** a long conversation on the no-per-message-effort
  model: turn 1 high; turn 2 Jev says medium, sure: held (`sticky`, sent high); turn 3 Jev says medium, sure: reverted
  (`sticky_reverted`, the request carries medium). The same with an `applied_unsure` medium at 0.40 opening a
  `sticky_unsure` window and a later unsure medium at 0.36 reverting it, while a later unsure medium at 0.30 does not
  (the answer stays `unsure`, the hold stays). A checkpoint raise inside a window is unchanged
  (`tests_checkpoint_effort.rs` stays green, add one case if the window touches it).
- **Words:** `the_wires_doc_lists_every_effort_reason_word` covers `sticky_reverted`.
- **Unchanged bars:** a test that `decide` gives the same decision as at the base for the existing table of
  `routing/effort.rs`'s tests (they stay untouched and green).
- Run `cargo nextest run -p theseus-core effort`, `-p theseus-core checkpoint`, `-p theseus-core route`,
  `-p theseus-protocol`, `-p theseus-discord`, then the gate.

## Shared with other rows (stay additive)
- **Lane `integ/p2-route` also gets:** `p2-route-data` (theseus-f9qa8: the data verdicts; it CREATES `route.v8` from
  v7, changing only the `data` question, and keeps `reply_effort` word for word so your "route.v8 join fix" is a
  one-line change at the join) and `p2-turn-budget` (theseus-2uy56, theseus-yct32: a turn's spend and time awareness,
  the CPU quota in the tool context). `p2-turn-budget` does not edit `routing/checkpoint.rs`'s text or the
  checkpoint's ask; you do not edit its new modules. Both may add a paragraph to core `AGENTS.md`: keep yours in the
  effort section.
- Off limits: `packs/*.toml`, `crates/theseus-judge/fixtures/golden/*`, `routing/data.rs`, `judge/inbound.rs`'s state
  (p2-route-data's), `bench/` (another row's tooling), `turn/data_step.rs`, `data_gate.rs`.
- **Other lanes, same release, for your awareness:** rows built on another host change how capped answers are read
  (a shared "this answer hit its cap" helper) and the forced-tool small calls; neither touches the effort step.

## Not in this row
- The bars, the gate, `effort_bounds`, when xhigh applies, the xhigh cost line (theseus-goq94, the owner's).
- A new pack version, a new config key, any benchmark run, any model spend.
- Any Discord message or `golden/dm_flow.txt`.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical
  (`git diff --exit-code origin/main -- crates/theseus-discord/src/golden/dm_flow.txt`). If a renderer of effort words
  is part of that flow and the new word would change any of its messages, stop that part and write "OWNER CALL: <what
  and why>, before/after text" in your report.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`,
  then `git status`. List each plant in the report with the test that failed and its failing line.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- `turn.rs`: add nothing. **Dependencies:** none new. **Store format** 49: no bump expected.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Names.** No person's name anywhere: write "the owner". No benchmark task name in code or tests.
- Read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` (the effort and checkpoint sections) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds:
- what you built, item by item (1-4), with the file and function for each;
- the evidence you chose for the checkpoint text and why (the 3-6 lines asked for in 1), with a sample text;
- "route.v8 join fix: reply_effort": the exact old and new option text;
- the sticky revert as built: where the window is kept, the two bars and where each is read, the cache cost;
- "How this meets the owner calls" (7pm8t, goq94);
- each test, and its plant with the failing line, the red-first runs on the base included;
- the keel guard's output, the store format, the Discord golden's `git diff --exit-code` result;
- a `docs/status.md` sentence for the integrator;
- follow-ups (what to measure on the next MAX rerun: max asks, `sticky_reverted` counts, the cost per trial).

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
