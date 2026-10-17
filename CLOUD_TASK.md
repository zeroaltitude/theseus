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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-trim-security-v1`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: trim-security-v1, retire security.v1 from the gate: the notice brake's rules and the notified call's score move to security.v3, then the gate stops asking v1 (theseus-rwpbg.6)

Branch: `cloud/20261017-trim-security-v1`. Every commit's subject carries `theseus-rwpbg.6`. Deadline for the report: 3 hours after you start.

**The owner's approval (Discord DM, 2026-10-17 16:19:39 MST), verbatim:** "Agree", to the question: "retire it, after
moving the brake and the score over to v3". This is a security change, so it is **keel**: tests that assert v1's
judgments at the gate go or change, and the joiner must land them in a commit **signed by the owner's key** with a
trailer quoting that approval. You can't sign here: build it, and put the exact trailer the guard asks for in the report.

**Source:** the trim review's part 3, M6 (and part 1 §3's note that only v1's TOML is retired here, not the security.v2
builder, which v3 is built by). Today the gate asks `security.v1` (shadow) and `security.v3` (live) on **every acting
call**; their states differ, so the batcher can't merge them. Retiring v1 halves the gate's Jev calls and its latency
tail on the acting path.

**Prerequisite:** `main`. Builds now; joins 3rd of the eight trim rows.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Size |
|---|---|---|---|---|---|---|
| 6 | `trim-security-v1` | theseus-rwpbg.6 | (1) the brake's rules (`notice.rs` `rules()`, reading v1's `[[rollback]]`) come from security.v3; (2) the `JudgeScored` score of a notified call (`gate.rs:~398-420`, filtering `j.pack == SECURITY_PACK`) comes from v3's `risky`; (3) the gate stops asking v1: `GATE_PACKS` is v3 alone, `WIRED` no longer lists v1; (4) v1's stored judgments stay readable; v1's pack TOML may stay embedded for replay | `crates/theseus-core/src/judge/{gate,notice,mod}.rs`, `crates/theseus-judge/packs/security.v3.toml` (the rollback rules), `crates/theseus-core/src/tests_security.rs`, the gate's own tests, `crates/theseus-core/AGENTS.md` lines that describe v1 at the gate | `the_brake_reads_v3s_rollback_rules` (revert: v1's), `a_notified_call_is_scored_by_v3s_risky` (revert: v1), `the_gate_asks_v3_alone` (one Jev ask per acting call; revert: both packs), `v1_judgments_already_stored_still_read` (a ledger/store with v1 judgments lists and replays them), the brake's existing tests unchanged in behaviour | M |

## What exists (at `432bbbeb`)
- `crates/theseus-core/src/judge/gate.rs:48-53`: `SECURITY_PACK = "security.v1"` ("the incumbent: the owner's `risky`,
  whose score the notice shows"), `SECURITY_CANDIDATE = "security.v3"`, `GATE_PACKS = [SECURITY_PACK,
  SECURITY_CANDIDATE]`, read at `:147`, `:167`, `:200`. `RISKY = "risky"` (`:56`). `LAST_CALLS = 20` ("v1 shows the newest
  five, v3 computes its facts over the tail"). `:423` on: v1's input builder ("`security.v1`'s input for the call, from
  its session's nodes").
- The score, `gate.rs:398-420`: when `call.notified`, the first answered judgment with `j.pack == SECURITY_PACK` gives
  `risky` → `JudgeScored { pack, judgment, mode: "shadow", risky, percent }` to the turn's clients.
- The brake, `crates/theseus-core/src/judge/notice.rs:172-177`: `rules()` = `theseus_judge::pack::by_name(SECURITY_PACK)`'s
  `rollback` rules. v1's TOML (`crates/theseus-judge/packs/security.v1.toml:84-92`) has two: `notices_per_day` max 30,
  and `labels_per_day` label `noise` count 3 ("§2.7: the quiet-notices lesson (theseus-w4f)"). v3's TOML has none.
- `crates/theseus-core/src/judge/mod.rs:89-106`: `WIRED` lists `(gate::SECURITY_PACK, PackMode::Shadow)` and
  `(gate::SECURITY_CANDIDATE, PackMode::Live)`. `notice.rs:40` imports both names.
- `tests_security.rs` asserts v1 judgments at `:339`, `:386`, `:400`, `:596-605`, `:718` (both packs zipped).
- v3 (`security.v3.toml`) has a `risky` question (`[questions.risky]`, `:36`) and `steered` deciding beside it.

## What to build
1. **The brake on v3.** Copy v1's two `[[rollback]]` blocks into `security.v3.toml` (same rules, same numbers, with the
   same comment; write "the owner" where a comment names a person: v1's header comment names one, don't copy that) and
   make `notice::rules()` read `SECURITY_CANDIDATE`. First read `crates/theseus-judge/src/pack.rs` (its doc at `:1-10`
   says what makes a new pack version) and the crate's AGENTS.md: if adding `[[rollback]]` would count as a new version
   of v3, stop and explain in the report instead of making a v4 (the owner's approval names v3). The pack's recorded
   `sha256` changes for v3 judgments made after the change: say so; stored judgments keep theirs.
2. **The score on v3.** The notified call's `JudgeScored` takes v3's answered `risky` (`j.pack == SECURITY_CANDIDATE`).
   `mode` should say what v3 is (it is live at the gate; use the pack's actual mode, not the literal "shadow"). The
   cockpit and Discord read `judge.scored`: check how they use `pack` and `mode` (`rg -n "judge.scored|JudgeScored"
   cockpit/src crates`), and make sure a v3 score displays as a v1 score did (the percent and the call). No wire type
   change is needed; if one is, put it in the module that holds `JudgeScored`, never in protocol `lib.rs`.
3. **Stop asking v1.** `GATE_PACKS` becomes v3 alone (rename the consts so the next reader isn't misled: e.g.
   `SECURITY_PACK` = "security.v3" and drop `SECURITY_CANDIDATE`, or keep both names with v1 moved to a
   `RETIRED_SECURITY_PACK` used only by readers of old judgments; pick the smaller diff and say why). `WIRED` drops the v1
   line. v1's input builder (`gate.rs:423` on) and `LAST_CALLS`'s v1 note go if nothing else reads them (check replay and
   audit first). Remove the "decision point's order" notion if only one pack remains.
4. **Old judgments stay readable.** The ledger and store keep v1 judgments forever: `theseus judge list/get`, replay,
   audit and backfill (`judge.replay --candidate`, `learning/replay.rs:300-310`, which finds a candidate in
   `pack::EMBEDDED`) must still read and show them. Keep `security.v1.toml` in `pack::EMBEDDED` (the owner allowed it for
   replay). A test reads a store holding v1 judgments and lists them.
5. **Docs in code:** `gate.rs`'s module doc (`:1-35`), `judge/mod.rs:25-29`, `notice.rs:1-20` and
   `crates/theseus-core/AGENTS.md`'s gate lines describe v1 and v3 at the gate: make them describe v3 alone (keep the
   history in one sentence).
6. **Config:** a `[judge.packs."security.v1"]` table in a config must still load (ignored, or a warning, as other retired
   packs are treated: find the existing rule). The template: drop v1 from any gate comment.

## Keel (the joiner's part)
Run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` on your last commit and paste its output. Expect
`test-removed` / `assert-removed` lines for the v1 gate tests. Write in the report the exact trailer line(s) for the
joiner's signed commit, in the guard's form, quoting the approval, e.g.:

`Keel: assert-removed crates/theseus-core/src/tests_security.rs — the owner, Discord DM 2026-10-17 16:19:39 MST: "Agree" to "retire it, after moving the brake and the score over to v3"`

(one per rule the guard names, with the paths it names). **Keep the keel findings minimal:** change a v1 test to assert
the v3 behaviour where it is the same check (an equality stays an equality), and delete only tests whose whole subject
was asking v1. List every test you removed or changed, with why.

## Proof nothing is lost (report section)
The brake fires on the same numbers (its tests unchanged); a notified call still shows a score (from v3); the gate's
decision is unchanged (v3 already decided); v1's stored judgments list and replay; one Jev ask per acting call instead
of two (show the count in a test).

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `432bbbeb` (2026-10-17
15:53 MST) or later, with **store format 45**. The trim review that this row comes from read `c6f62abf`, so every
`file:line` below is at `c6f62abf`; on your base a line may have moved a little, so search by name.

Line ceilings at `432bbbeb` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-protocol/src/lib.rs` **2,732 /
2,733**, `crates/theseus-kernel/src/kernel.rs` **3,026 / 3,030**, `crates/theseus-discord/src/runtime.rs` 3,495 / 3,500,
`crates/theseus/src/render.rs` 3,116 / 3,128, `crates/theseus-core/src/turn.rs` 3,351 / 3,525,
`crates/theseus-core/src/tests_m3.rs` 7,996 / 8,050, `crates/theseus-discord/src/render.rs` 2,926 / 3,009,
`crates/theseus-index/src/vectors.rs` 2,564 / 2,650. A trim should shrink these files, never grow them; never raise a
ceiling in this row.

## The trim rows (eight siblings, built in parallel today from main)

The owner asked for a trim pass before the v1 feature review: "an efficiency and simplicity and elegance check ... what
we can trim without losing anything, both on total code, total binary size, etc." The review found the items; the owner
settled the ones that needed him. Eight cloud rows build them at once, each from `main`, and one joiner merges them in
this order:

| Join | Slug | Beads | Area |
|---|---|---|---|
| 1 | `trim-binary` | theseus-rwpbg.5 | scripts/build.sh, theseusd's web.rs, the cockpit's build, theseus-aws-catalog's Cargo.toml |
| 2 | `trim-docs` | theseus-rwpbg.8 | docs/glossary.md, docs/cli.md (new), the CLI's clap `about` lines, the CLI help goldens |
| 3 | `trim-security-v1` | theseus-rwpbg.6 | core judge/gate.rs, judge/notice.rs, judge/mod.rs, the security packs |
| 4 | `trim-config-inherit` | theseus-rwpbg.7 | core config.rs, config/cache.rs (`inherit`), config/sparse.rs, the template |
| 5 | `trim-effort` | theseus-rwpbg.4 | core turn/route_step.rs, compiler/effort.rs, keep_warm.rs (effort), catalog.rs (one field) |
| 6 | `trim-tokens` | theseus-rwpbg.3 | core tools_defer/, toolrun.rs's tools note, keep_warm (the window cap), turn/compile_step.rs, theseus-index tender |
| 7 | `trim-utils` | theseus-rwpbg.2 | many small sites: clip, dates, now_ms, dead pub items, protocol jsonrpc, core<->protocol mirrors |
| 8 | `trim-tests-dedup` | theseus-rwpbg.1 | theseusd/tests/ and theseus-core test helpers |

**Contracts between the rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need to touch, and do not reformat files you only read.
2. **`config/cache.rs`'s `inherit` belongs to trim-config-inherit.** trim-tokens' keep-warm window cap and trim-effort's
   keep-warm effort live in `keep_warm.rs` / the catalog, never in `inherit` or `ProfileConfig`.
3. **`keep_warm.rs`:** trim-effort adds the last request's effort to what keep-warm replays; trim-tokens adds the
   break-even cap on the window. Each adds its own small function and one call; neither restructures the file.
4. **`catalog.rs`:** trim-effort adds one field (`per_message_effort`) to `CatalogEntry` and `CatalogRow`; trim-tokens
   reads the existing cache prices for its cap and adds no field. trim-utils deletes no catalog item.
5. **`compiler/effort.rs`:** trim-utils deletes the dead `without_messages` (`effort.rs:80`); trim-effort must not
   start calling it.
6. **Test helpers:** rows 1-7 add tests in their own `tests_<subject>.rs` files (or a crate's existing test module for
   their subject) and may use today's helpers; trim-tests-dedup moves helpers only in files that exist on `main`.
7. **Leave `crates/theseus-discord/` and `crates/theseus/src/herdr*` alone** (the Discord rows theseus-bh3t.1-.6 and
   theseus-ss36w are joining to main right now, and a herdr lane is open). Discord's copies of `clip` and the date
   math wait for a later pass; say in the report which copies you left.
8. **The China-origin rows** (theseus-a1wvq.3-.13) build later on `catalog.rs`, `config.rs`, routing and the turn path:
   keep your edits there small and additive.

## Out of scope for every trim row (owner questions still open)

CLI command hiding, renames or folds; vocabulary renames (thread, mode, place, surface, section names); the sealed
holdout set; any deletion of a test (the review's part 1 §2b test-only APIs, §3 the old Jev packs, §4 jev-probe, §7 the
serving-daemon chain, §9 the startup migration and retired keys, §19 RPC removals); `PLAN_NOTE` and the plan tools; the
LTO profile. If your work shows one of these is needed, write it in the report and stop there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. This is a trim: the diff should remove more than it
  adds, and every line you add should earn its place.
- **Nothing lost.** Behaviour, wire JSON, stored records and user-visible output stay the same unless this task says
  otherwise. Where it says otherwise, pin the old output in a test first, then change it, and show the diff.
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore); list them in
  the report. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names a keel finding (only trim-security-v1 may have one), and then the report gives
  the exact `Keel:` trailer line the guard asks for, for the joiner's signed commit.
- No store format change. No new dependency: Cargo.lock and the package-lock.json files gain no package (a version
  that drops a duplicate is fine; say so). Never loosen a test, budget or ceiling.
- No person's name anywhere in the repo, in commit messages, or in the report: write "the owner". If you edit a file
  that already carries a person's name, replace it with "the owner" in the lines you touch.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud. Add a section **"Proof nothing was lost"** listing, per change, the test or the
  before/after evidence.
