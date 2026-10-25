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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261025-p3f-clear-by-tokens`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about main)

**This is a second attempt.** A first session was given this same task on 2026-10-24 on another branch and pushed
nothing in four hours. Nobody knows yet whether it died at launch or mid-run, so this run carries one extra rule: the
**early push** below. Work only on your own branch, named below. Never fetch, read or push the first attempt's branch.

Your branch starts from `main` at `b69236c1`. That is install #82's gated tree: release 1.0.2 (signed tag
`release-1.0.2` at `ac298666`) plus twelve commits of joined patch3 work. Fetch it and branch from `b69236c1`. None of
those twelve commits touches `crates/theseus-core/src/compiler/`, `turn.rs`, `turn/`, `tests_one_turn.rs`,
`crates/theseus-sim`, `crates/theseus-tools` or the core `AGENTS.md`, so every line number below still holds; search
by name anyway. Where the release preamble above disagrees with this note about the base, ceilings or the work in
flight, this note wins.

**Early push (required, before any code).** Do setup first (`cargo --version`, nextest) and your first
`cargo build -p theseus-core`. Then make an EMPTY commit and push it at once:
- `git commit --allow-empty -m "theseus-5lzox: session started, setup and first build done"`
- `git push origin HEAD:refs/heads/cloud/20261025-p3f-clear-by-tokens`

After that, push after EVERY commit, not only at the end: the red tests, the rule, the stub, the replay, and the
report. A run that ends early must leave its work on the branch.

**The evidence.** "Why" below sums up two documents: the bug hunt's ranked fix list (row 2) and the mechanism
analysis behind it. Neither is in this repository. Take the numbers as given; the replay (item 3) is what checks them.
The analysis also proposed another rule for big cached windows: run a pass only near the window, at about 80-85 % of
the budget. Put that rule in the replay table as a third row, for comparison only. Ship the owner's token rule, not
that one. If the replay says the near-window rule is clearly cheaper, report it as a follow-up for the owner.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`). This
row should need no store change: the cleared results already live as drops of tier `cleared` in the compilation's
stored report, and the stub is rendered from stored nodes. Keep it that way (derive the stub's new parts at render
time from stored nodes, never store them). If a part truly needs a bump, name it and justify it in your report's
first section (the integrator renumbers it at the join; a format bump moves two goldens).

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines).
`crates/theseus-core/src/turn.rs` sits at its ceiling: do not add lines to it. At the base:
`crates/theseus-core/src/compiler/clearing.rs` 339, `compiler/tests_ring.rs` 389, `compiler.rs` 1,070,
`compiler/tests_cold.rs`, `compiler/cold.rs`, `tests_one_turn.rs` (read their sizes).

**Your lane: `integ/p3-turn`** (rows that shape what a long turn does). Its other rows are already written and
reviewed, on their own branches, and none is on `main` yet. You do not need to read their branches. What they touch:
- `p3c-steer-keep-going` (theseus-ikr75), the progress steer: `turn/progress_step.rs`, `tests_turn_progress.rs`,
  `turn/checkpoint_step.rs`, `fact/progress.rs`, 4 lines of `turn.rs`, and the core `AGENTS.md` near :3005-3020.
  **Do not touch `turn/progress_step.rs` or `turn/checkpoint_step.rs` at all.**
- `p3c-narrow-reads` (theseus-tdkcn), the narrow-read hint: `turn/tier.rs`, `turn/compile_step.rs` (2 lines),
  `toolrun/session_dir.rs` and a new `toolrun/session_dir/read_window.rs`, `theseus-tools` (`fs.rs`, `lib.rs`), 2
  lines of `turn.rs`, and the core `AGENTS.md` near :2192-2198.
  - **It changes what an `fs_read` with no offset and no limit returns.** On `deep_coding` turns the default window
    becomes 400 lines (`ToolCtx.default_read_lines`); the footer format is unchanged.
  - So your stub's line range must come from the result itself: its `[showing lines a-b of N; …]` footer, or its
    line count. Never assume "no offset, so the whole file".
- `p3e-fanout-kernel` (theseus-z9mze.1), the in-turn helpers, is not yours. It adds a field `helper` to the kernel's
  `Execution`, and `helper: None` to every `Execution { .. }` literal (for example `theseus-sim/src/synth.rs` ~:151).
  If your replay builds an `Execution` literal, list it in your join notes: it needs `helper: None` at the join.

**Files you share with them, so keep every row additive:**
- **The core `AGENTS.md`.** You edit only the "One long instruction" paragraph (near :119-130, theseus-3ki0), and the
  one at :160 if it names `KEEP_EXCHANGES`'s rule.
- **`turn.rs`** (both other rows changed a few lines). Add no line to it, and change none if you can help it.
- **`turn/compile_step.rs`.** If you must pass the next answer's text into the render from there, keep your hunk
  separate from narrow-reads' 2 lines, and name it in your join notes.

You own `compiler/clearing.rs`, `compiler/tests_ring.rs`, `compiler/tests_ring_sums.rs`, `compiler/cold.rs` and
`tests_one_turn.rs`; no other row touches them. Do not edit `crates/theseus-tools/src/fs.rs` or `toolrun/` (the
narrow-reads row owns them). Do not edit `docs/status.md`: the integrator writes it at the join.

## Your task: p3f-clear-by-tokens, clear the turn's context by tokens, not 20 exchanges, with stubs that remember (theseus-5lzox)

Branch: `cloud/20261025-p3f-clear-by-tokens`.

Every commit subject carries `theseus-5lzox`. Deadline for the report: about 3 hours after you start.

**Why (the evidence, from a benchmark the owner ran on 2026-10-24, three trials).** In a bug hunt (13 planted bugs
in a copy of this workspace, Sonnet 5.5 on a 1M window, high effort), Theseus fixed 4, 4 and 5 of 13 while two rival
harnesses fixed about 10 each at a similar or lower cost. The analysis found the main cause in this compiler:

- **The sawtooth.** At `b69236c1` (`compiler/clearing.rs`) a pass runs when a request's estimate passes `CLEAR_AT =
  50` % of the budget (about 430K tokens here), stubs results older than the newest `KEEP_EXCHANGES = 20`
  exchanges (over `MIN_CHARS = 2,000`), oldest first, toward `CLEAR_TO = 35` %. With 12-25K-token file reads, 20
  exchanges alone hold about 270K, so every pass left a ~270K context and fired again ~13 calls later. Each trial's
  `prompt_tokens` climbed to ~430K and dropped to ~270K in one step, 7-8 times (r1: at loops 45, 66, 84, 104, 126,
  152, 176, 198). Mean context per call was 318K.
- **The churn.** Clearing goes oldest first, so the first thing lost is the first thing read, and the stub says
  "Read it again if you need it." The model then re-swept its opening files. From minute 8 on it opened only 1-2
  NEW files per 4-minute window, against 45-62 in the first four minutes; 66 % of the lines it read were repeats
  (a pilot without the churn: 9 %); `kernel.rs`, `store.rs`, `wal.rs` and `index.rs` were each read 4 times over,
  one 600-line file in 18 windows; and one file holding a planted bug that both rivals fixed in 3 of 3 trials was
  **never opened in any trial**.
- **The cost.** Each pass changes the prefix: the cache drops to the ~7.5K system block and ~265K is written again
  at the cache-write price. The 7-8 rewrites cost $4.73-5.40 a trial, about a third of each trial's ~$16. Cache
  reads of a 270-430K context on every call were another ~$6. The rivals' calls averaged about half Theseus's cost.
- Prices fitted exactly from the trial's 207 per-call costs (Sonnet 5.5): cache read $0.10/M, cache write $2.50/M,
  input $2/M, output $10/M.

**The owner's spec (the ranked fix list, row 2):** keep by TOKENS, not 20 exchanges: for example clear when results
pass ~150K and keep the newest ~60K, so a pass is cheap and its rewrite small; and make each stub carry the
file:line range and a one-line note of what was checked (or keep a short running "checked" list in the turn), so a
cleared read is not simply forgotten. **Expected:** mean context 318K -> ~120K; cost per call about -60 %; re-read
share 66 % -> under 20 %; a $16 hunt does the same work for ~$6-8.

**What to build.**

1. **The token rule** in `clearing.rs` (replace the count-based keep; the `pass()` / `any_old()` / `clear()` gate):
   - A pass runs when the request's not-yet-cleared tool results (by the same per-byte token estimate `pass()` uses
     today) pass `CLEAR_AT_TOKENS` (~150,000), or when the estimate passes `CLEAR_AT` % of the budget, whichever
     comes first (so a small window still clears as it does today: never later than at the base).
   - It stubs results oldest first, keeping whole the newest results whose tokens sum to at most `KEEP_TOKENS`
     (~60,000; and never more than the small-window line allows: for a small budget keep at most what `CLEAR_TO`
     allows), walking back from the newest. The newest exchange's results always stay whole even if larger (a
     floor: the model must see what it just asked for). `MIN_CHARS` and `late` handling stay.
   - The "a pass that cannot save enough does not run" guard (today: halfway back from the line to 35 %) is
     re-derived for the token rule so passes stay rare: a pass runs only when it saves at least half the gap
     between the trigger and the keep (or the base's rule for small windows). Name the formula in the doc comment.
   - `KEEP_EXCHANGES` goes (or becomes only the small-window floor: your call, say which). The constants get doc
     comments with the evidence (one line each: the date, the sawtooth, the costs). No new config key unless the
     constants truly cannot do (then name the keys and their defaults in the report; the owner decides on config).
   - Cache stability is the point: a pass is one recompile (one rewrite), and every later append renders the same
     stubs byte for byte (the drops in the report, read by the render) so the prefix holds until the next pass.
     Keep that property and test it (below).
   **Ranges allowed, and the replay decides inside them:** trigger 120-200K, keep 40-80K. Use the model-free replay
   (item 3) to choose, and report the table. If the replay shows the token rule costs MORE than the base on the bug
   hunt's shape, say so plainly and do not ship a rule that costs more; report what you found instead.
2. **Stubs that remember** (`stub()`, and `block()` if needed). Keep the head `[Result cleared to save context:`
   unchanged (other code and `tests_cold.rs` read it). For `fs_read` the stub carries:
   - the path and the **actual line range returned, and the file's length when the result says it** (from the
     result's own `[showing lines a-b of N; …]` footer or its numbered lines: a pure function of the stored result,
     correct whether or not the call gave offset/limit), and the characters;
   - **a one-line note of what was checked**: the first sentence (at most ~120 characters, cut at a word) of the
     model's own text in the answer that followed that result, when that answer has text; none when it has only
     tool calls. Deterministic, from stored nodes only, no model call. (If the render cannot reach the next answer
     cheaply from where `stub()` runs, pass it in from the render; say how.)
   - in place of "Read it again if you need it.": words that say the read happened and the model's notes after it
     still stand, and to read again only a line range it needs (your words, short).
   - For `fs_grep` and other read tools, the same note when it fits; for `proc_run`, today's stub plus the note.
   Optional, your call, only if it fits in about 60 lines: a running "checked" note (files read whole / ranges read,
   per path, merged) rendered once per pass in the uncached tail. If you build it, it may also name the files under
   the work dir never opened. The analysis found one file holding a planted bug that no trial ever opened. Say
   whether you built it and why.
3. **The context bench (model-free replay), before and after.** Check first whether a bench already measures this
   (`crates/theseus-sim/src/perf/long.rs` and `synth::long_session`; `tests_one_turn.rs`; `tests_ring_sums.rs`). Then
   add a deterministic replay of the bug hunt's shape through the real compiler with the stand-in model, no key:
   ~207 calls; a ~7.5K system block; ~300 tool calls, about 270 of them `fs_read` results of 12-25K tokens (300-800
   lines; mix sizes with a fixed seed), ~65 distinct files read in a first sweep and later re-read; ~450 output
   tokens per call; a 1M window, Sonnet 5.5's output cap. Run it at `b69236c1`'s rule and at yours and report, per
   rule: clearing passes, mean and max request tokens, total cache-write tokens at passes (the rewrite), total
   cache-read tokens, and $ at the prices above. Put the replay where the repo's benches/long tests live (a test
   that prints its numbers and asserts the properties: for example mean context under ~150K and each pass's rewrite
   under ~100K on this shape), not a new crate. If `crates/theseus-sim` has a bench that exercises clearing, rerun
   it before/after too and report its numbers.
4. **Goldens.** Run every golden test in the workspace (grep `golden` under `crates/`). None should move unless it
   renders a cleared stub. If one moves, name it, show the old and new line, say why, and update it in its own
   commit (never regenerate a golden blindly). The DM golden (below) must not move at all.

## Tests (each red at the base, with a planted revert)

Keep every existing test's name. Rewrite setups and expectations that encoded "20 exchanges" to the token rule (a
rewritten assertion is fine; never fewer assertions in a test, never a removed test).
1. `tests_ring.rs` `clearing_stubs_only_old_large_results_and_each_says_how_to_get_it_back`: under the token rule,
   the kept results are the newest within `KEEP_TOKENS`, the cleared ones are older and over `MIN_CHARS`, and each
   `fs_read` stub has the exact new text (an exact equality stays exact): path, actual range and length, chars, and
   the note from the following answer. Plant P1: put back the 20-exchange keep: red.
2. The "under the line, or with fewer than 20 exchanges, no pass" test (near :352): rewrite to "under the token
   trigger, no pass; the newest exchange's results never cleared", keeping its assertions.
3. A new test: a long session of 12-25K results on a 1M window passes the trigger at ~150K (not ~430K), each pass
   leaves the request near ~60K of results plus the rest, and appends between passes render byte-identical
   prefixes (cache stability). Plant P2: trigger at `CLEAR_AT` % only: red.
4. A new test: a small-window session (for example a 200K-window model) clears no later than at the base. Plant P3:
   drop the `CLEAR_AT` % arm: red.
5. A new test: the stub of an `fs_read` with NO offset/limit whose result was a window (`[showing lines 1-400 of
   1,210; …]`) names `lines 1-400 of 1,210`, not the whole file; the note is cut at ~120 characters on a word. Plant
   P4: compute the range from the call's input only: red.
6. The replay (item 3) with its asserted properties. Plant P5: the base's rule makes it fail its mean-context bound.
- Keep green: `tests_cold.rs`, `tests_ring_sums.rs`, `tests_one_turn.rs` (its `context.cleared` rows), `cold.rs`
  and `ring.rs` users of `clearing::drops`, and the whole `theseus-core` suite; theseusd's tests that read
  `context.cleared` if any (grep).

## Not in this row

No change to the ring, compaction or the summary path beyond what the token rule needs (say if it needs any). No
change to `fs.rs` or the tools (the narrow-reads row). No change to the progress steer (the steer row). Not the
in-turn helpers (another row). No benchmark run and no spend: you have no key, and the verification rerun of the
bug hunt waits for the owner.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code b69236c1 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. A change
  that would alter any message of that flow STOPS that part: write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove no test, rename no test, lower no test's assertion count, weaken no assertion, raise no budget,
  cap or ceiling, do not edit `scripts/keel-guard.py` or its tests. Clearing earlier is not raising a cap; if you
  find a place where your rule would let a request grow LARGER than at the base, that is a raised limit: stop it.
  Before the report run `python3 scripts/keel-guard.py --base b69236c1 --head HEAD --quiet` and paste its whole
  output. If it flags anything, STOP that part, paste what it prints, and write it as an owner question in the report
  (no `Keel:` trailer of your own: an unsigned ack counts for nothing).
- **Data floor:** nothing new is sent to a model or routed anywhere: the stub's note is the model's own earlier
  words, in the same session's request.
- **Public repo:** no person's name, no real organisation, address or email. Fixtures use invented names and paths
  under `example.test`. Write "the owner".
- **Red first** (each test red at `b69236c1`, green after; prove each with its planted revert); right over
  compatible.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-core/AGENTS.md` "Building and checking" first):
  `cargo fmt --check`, `cargo clippy -p theseus-core --all-targets -- -D warnings` (and `-p theseus-sim` if you
  touch it), `cargo nextest run -p theseus-core` (or `cargo test`), every golden test, then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched, or a bump named), ceilings, any OWNER CALL
or STOP, the chosen trigger and keep in one line, and whether any config key was added. (2) **The rule:** the new
constants, the gate formula, the small-window behaviour, and why passes stay rare. (3) **The stub:** an `fs_read`
stub before and after, verbatim, and where the note comes from. (4) **The replay table:** base vs new (and any
other points tried in the ranges): passes, mean/max request tokens, rewrite tokens, cache-read tokens, $. Compare
with the expected numbers (318K -> ~120K mean, about -60 % per call) and say plainly where yours differ and why;
say what this replay cannot show (re-read share needs a model: name the metric the verify rerun should read). (5)
**Goldens:** which ran, which moved and why (or none). (6) **Tests** and plants P1-P5, red at the base and green
after. (7) **Gate** commands and results. (8) **Keel** output, whole. (9) **DM golden** diff result. (10) **Join
notes:** every file touched with line ranges and the AGENTS.md hunk, so the join with `p3c-steer-keep-going` and
`p3c-narrow-reads` stays additive. (11) **What the verify rerun should read** (`efficiency.json` mean context,
re-read share by line coverage, clearing passes and their rewrite $, $ per bug). (12) **Follow-ups.** (13) One line
on the data floor.
