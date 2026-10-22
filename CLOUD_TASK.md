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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail4-route`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `8260dbab` (2026-10-22: release stage 2, which landed the threads lane rounds 1-3b and
the China-origin chain rounds 1-4 on top of install #69's `29f5dcad`); that is your base. Line numbers quoted below are
at `8260dbab` but may still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `29f5dcad`** (`git log --first-parent 29f5dcad..HEAD` lists them):
- `bb265d6c` + `8260dbab`: the threads lane (a place's home splits into threads, thread routing with the directory rule
  and the which-thread ask, the place digest, thread moves and growth, the protocol nodes split).
- `38743708`: the China-origin chain (the data rule's open view, the open request, the tool screen, the draft review),
  with join fixes `c0d35275` (a session's data state is store format 49) through `30d6472e`.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room: add nothing)
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,556 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail3` (`e29be1ed`) and `integ/threads` round 4 (`a52cac7b`) are being joined into the next release stage.
  Your task's "Rules for this row" names the files of theirs that sit near yours, and how to read their diff
  (`git diff origin/main origin/integ/tail3 -- <file>`). Read that diff before you edit those files.
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail4-route, a message naming two threads by title asks which, and the place digest never rides an open request (theseus-4qdh8, theseus-rcb2u)

Branch: `cloud/20261021-tail4-route`. Every commit's subject carries the Beads id of what it changes
(`theseus-4qdh8`, `theseus-rcb2u`). Deadline for the report: 3 hours after you start.

**Why.**
- **theseus-4qdh8.** On install #69, a rehearsal of the owner's TUI walkthrough had two young threads,
  `docs-census` "Docs word census" and `harbour-notes` "Harbour log notes" (one turn each). The message "For the word
  census of the sample docs and for the harbour log notes: add the census total as a line to the harbour notes, and add
  the ferry time to the census table, so both stay in step with each other." ran at home with **no** which-thread ask.
  Ledger `thread.routed`: `how=home`, `ask=null`, `movable=true`, Jev p=0.29 pick docs-census, pre scores
  harbour-notes 0.0 and docs-census 0.0 (no hits). On #68 the same shape asked "Is this about the Docs word census or
  the Harbour log notes?". The cause: the walk1 fix (theseus-e31an) asks only when two threads reach `CONTEND` (0.25)
  fused, and the pre-router scores a message that names both threads **by their titles** 0.0 for each (it scores
  working-set entities only), so nothing contends. The e31an fix itself is right (a plainly new request now runs at home,
  verified on the same rehearsal: Jev 0.41 docs-census -> home, no ask) and must stay so. The gap is the scorer: a
  message that names a thread by its title should give that thread evidence. The walkthrough's step 4b ("expect the
  which-thread question at once") depends on it.
- **theseus-rcb2u.** `turn/compile_step.rs` `tail_blocks` gates the task view and the project slice with `(!open)` (the
  owner's graph and project are not open, theseus-a1wvq.7), but `Self::digest_line` (the place digest, theseus-98bz.9)
  attaches the other threads' card lines whatever `t.tc.open` says. Not reachable today (the digest runs only in a
  thread's own session, and an open session is never one), but the rule must hold if one can ever be opened open.

**Risk: low-medium.** Routing scores move only for messages that name a live thread's title.

**Prerequisite: base `origin/main` at `8260dbab`.** Your branch joins the new lane `integ/tail4`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail4-route` | theseus-4qdh8, theseus-rcb2u | title evidence in the pre-router; the digest's open gate | a new `crates/theseus-core/src/thread_title.rs` (the title match, pure) + one `mod` line in `lib.rs`, `crates/theseus-core/src/thread_route.rs` (`pre_route`: one call and the sum, plus its module-doc paragraph 2), `crates/theseus-core/src/rpc/tests_thread_route.rs` (or a new `tests_thread_title.rs`), `crates/theseus-core/src/turn/compile_step.rs` (`tail_blocks` / `digest_line`), `crates/theseus-core/src/rpc/tests_place_digest.rs`, `crates/theseus-core/AGENTS.md` (the routing paragraph: one sentence) | S-M |

## What exists (at `8260dbab`; search by name)
- **The pre-router.** `crates/theseus-core/src/thread_route.rs` (2,152 lines): the module doc (paragraphs 1-4 are the
  order a message meets); `CONTEND` 0.25, `ACT` 0.90, `CONFIRM` 0.60, `CLOSE` 0.15; `norm`, `named(word, threads)`
  (slug or whole title as one word, for explicit addressing), `explicit` ("in the <title> thread:"), `has_token`,
  `entity_hits(text, &WorkingSet)`, `entity_of(hits, hitting)` (noisy-or of strengths, `SHARED` discount when more than
  one thread is hit), `pre_route(text, threads, last, now_ms, half_life_ms) -> Vec<Pre>` (near :499: entity, sticky,
  chain, `score = noisy_or([entity, sticky, chain])`, `other_named` halves stickiness), `apart_by_dir`, `fuse`,
  `decide` (near :967: `contenders` = threads fused at `CONTEND` or more; fewer than two and nothing raised -> home).
  `ThreadInfo` has `slug`, `title`, `working_set` (`crates/theseus-protocol/src/threads.rs`). `fold`/`normal` come from
  `toolrun::misroute`.
- **The orchestration** (`rpc/thread_flow.rs`) calls `pre_route`; tests in `rpc/tests_thread_route.rs` (search the two
  e31an tests: the plainly new request runs at home; `asked_home_claim` in `d1da1475` makes two threads contend).
- **The digest.** `crates/theseus-core/src/turn/compile_step.rs` (337 lines): `tail_blocks` (near :42), `let open =
  t.tc.open.is_some();` (near :53), `(!open)` for tasks and project, `Self::digest_line(t, compiled)` (near :77) with no
  gate. The block's text starts `[Place digest` (`compiler/place_digest.rs`). Tests: `rpc/tests_place_digest.rs`, and
  the open-request tests (`tests_open_request.rs`) show how an open turn is driven.

## What to build
1. **theseus-4qdh8.** `thread_title::title_hit(text, t) -> Option<f64>`: a thread whose title the message names gets
   title evidence. Names = the whole title as a phrase, or its content words (stop words out: "the", "of", "and",
   "notes"? decide and list them) appearing together as a phrase in the message ("word census", "harbour log notes"),
   folded like `norm` (case, hyphens, plurals as `fold` does). A single shared common word is never a hit. The evidence
   joins `pre_route`'s noisy-or beside `entity` with a strength between `CONTEND` and `CONFIRM` (say 0.35; pick and
   say why) and is **not** discounted by `SHARED`, so a message naming two threads by title makes both contend and the
   router asks. A message naming one thread's title alone gets that evidence and nothing else changes. Add a `title`
   flag or field to `Pre` only if the ledger's `thread.routed` row needs to show it (it is a wire/ledger shape: prefer
   putting the matched title in `hits`, which is already logged).
2. **theseus-rcb2u.** `digest_line` runs only when `!open`, with the same comment style as the two gates beside it.

## Tests (each with a planted revert; the first is the key test)
- **`a_message_naming_two_threads_by_title_asks_which`** (the rehearsal's two threads and message, verbatim, through
  the router as `tests_thread_route.rs` drives it, Jev answering p=0.29 docs-census): the decision is an ask naming
  both. Plant: return `None` from `title_hit`: it runs at home.
- `a_plainly_new_request_still_runs_at_home` (the e31an case: an unrelated request beside the same two threads, Jev
  0.41): home, no ask. Plant: make any one shared word a hit.
- `title_hit` unit tests: phrase, content words, plural and hyphen folding, a lone common word is no hit.
- **`an_open_request_carries_no_place_digest`**: an open turn in a thread's session (build it the way the open-request
  tests do; if no open thread session can be built, test `tail_blocks` with an open `tc` directly) has no
  `[Place digest` block. Plant: drop the `!open` gate.
- The e31an tests and the place-digest tests pass unchanged. `cargo nextest run -p theseus-core thread` and
  `cargo nextest run -p theseus-core place_digest` must pass, then the whole crate.

## Not in this row
- Jev's pack (`classify.v4`'s thread question) and the decision table's bars: unchanged.
- The walkthrough's live check: the owner's, after install.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Near you, not yet on main:** `integ/threads` round 4 (`a52cac7b`) changed `thread_route.rs` (`pre_route`'s
  `other_named` and `apart_by_dir`, theseus-ax4dp), `thread_grow.rs`, `rpc/thread_grow.rs`, `rpc/thread_move.rs` and
  the end of `rpc/tests_thread_route.rs`. Read `git diff origin/main origin/integ/threads -- crates/theseus-core/src/thread_route.rs crates/theseus-core/src/rpc/tests_thread_route.rs`
  first. Keep your `thread_route.rs` edit to the one call in `pre_route`, the `score` line, and the module doc; put your
  tests in a new file (`rpc/tests_thread_title.rs`, one `mod` line) rather than at the end of `tests_thread_route.rs`.
- **Rows beside you on `main` 8260dbab, joining `integ/tail4` with you:** tail4-voice, tail4-discord, tail4-tui,
  tail4-tests (core test helpers: `rpc/tests.rs`' `test_core`; do not edit it), tail4-onto. Stay out of their files.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; this row adds no stored field.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus-core/AGENTS.md` (threads and routing) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per Beads id; the title rule (what counts as naming a title, the stop words, the
strength and why); the rehearsal message's scores before and after; each test and its plant with the failing line; the
keel guard's output; the store format (unchanged); a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
