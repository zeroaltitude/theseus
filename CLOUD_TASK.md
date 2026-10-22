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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail6-onto`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `49352e8a` (2026-10-22: release stage 4, install #72, which landed `integ/tail4` round 1
and `integ/cn` round 5 on top of stage 3's `f1fc9b03`); that is your base. Line numbers quoted below are at `49352e8a` but
may still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `f1fc9b03`** (`git log --first-parent f1fc9b03..HEAD` lists them):
- `6854991f`: `integ/tail4` round 1 (the TUI work view, thread routing by title, voice warm tracks, the Discord approval
  card and forums, the test temp-dir leak, the ontology sweep's renames and kind names).
- `a95ec41d` + `b046ca16`: `integ/cn` round 5 (the tool screen's redirects, the China-origin config guidance, reply data,
  the draft's first pass).
- `49352e8a`: a join fix to the approval card's why line.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/runtime.rs` 3,494 / 3,500 (the lane beside you adds lines here: add nothing)
- `crates/theseus-discord/src/render.rs` 2,975 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail5` (`origin/integ/tail5`, `b1cd38e9` or later) lands as the next release stage. Its rows are the branches
  `origin/cloud/20261021-tail5-effort`, `-tail5-threads`, `-tail5-discord` and `-tail5-t4fix`. Your task's "Rules for this
  row" names the files of theirs that sit near yours. Read their diff before you edit those files:
  `git diff origin/main...origin/cloud/20261021-<row> -- <file>` (three dots).
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail6-onto, the ontology sweep's follow-ups: titles off the worker, the mint's pair and name limits, the docs, renames and Jev's names on view (theseus-e3pxa)

Branch: `cloud/20261021-tail6-onto`. Every commit's subject carries `theseus-e3pxa`. Deadline for the report: 3 hours
after you start.

**Why.** Release stage 4 (`6854991f`, now on `main`) landed tail4-onto (theseus-63tfk.1: the sweep renames a stale
category and Jev names a minted kind). Its review (`aed88ab6` on the old `integ/tail4`) left findings the owner wants in
before the release closes:
- **F1.** `judge/onto/renamed.rs` `Run::titles_of` reads `store.get_session` straight from the async task (the
  `core.titled` path). Wrap the `filter_map` in `theseus_store::blocking`, as `judged.rs` `held_by` does.
- **F2.** `judge/onto/mint.rs` `kind_pairs` asks a pair only when its union reaches 3 sessions, so singleton chains
  (`a~b`, `b~c`) never cluster, and two names that are each already a cluster are never asked, so two kinds can mint for
  one thing.
- **F3.** `renamed.rs` `kind_names`: a paid kind-name ask whose cluster does not mint that night is not asked again for
  30 days (its key is spent); and there is no `pressure::quiet` between asks (bounded only by `PER_RUN` = 3).
- **F4.** The template's `[ontology] sweep_usd_per_day` comment lost an "and" (read the review for the exact spot), and
  `config/ontology.rs`'s module doc (near :27) names only the two older packs (`same_thing.v1`, `two_things.v1`), not
  `name_it.v1` or `same_kind.v1`.
- **Visibility.** Renames and "named by Jev" show in the cockpit's Ontology view and in the morning note. Today the
  morning note has a `"rename"` line (`judge/onto/digest.rs` near :130) and nothing for a kind Jev named
  (`Minted.named`, `Swept.named_kinds` live in the info log and the struct only).

**Risk: low.** A background sweep and its view; no turn path changes.

**Prerequisite: base `origin/main` at `49352e8a`.** Your branch joins the new lane `integ/tail6`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail6-onto` | theseus-e3pxa | `titles_of` off the worker; the mint's pair rule; a kind-name ask's retry and pacing; the two doc fixes; renames and Jev's names in the cockpit and the morning note | core `judge/onto/renamed.rs`, `judge/onto/mint.rs`, `judge/onto/digest.rs`, `judge/onto/sweep.rs` (only to carry what the view needs), their test files (`tests_renamed.rs` and the mint tests: search by name), `config/ontology.rs` (module doc only), `crates/theseus-core/config/theseus.example.toml` (the one `[ontology] sweep_usd_per_day` comment line near :1398), protocol (only if the cockpit needs a new field: in the ontology module, not `lib.rs`), cockpit `src/components/OntologyGrowth.tsx` or the Ontology view's component (search `Shell.tsx` for it), `src/lib/ontology.ts`, regenerated `src/protocol.gen/*` if the wire changes; core `AGENTS.md` (the ontology section, one sentence) | M |

## What exists (at `49352e8a`; search by name)
- `crates/theseus-core/src/judge/onto/renamed.rs`: `titles_of` near :158, `kind_names` near :232 (key
  `kind-name <cluster key>`, a pick at the act band into `Hints::named`), `pressure::quiet` uses near :124 and :205.
- `crates/theseus-core/src/judge/onto/mint.rs`: `PER_RUN` near :49, `Minted.named` near :96, `Hints::named` near :128,
  `kind_pairs` near :167, `named_kinds` counted near :447. `judge/onto/sweep.rs` near :57 `Swept.named_kinds`.
- `crates/theseus-core/src/judge/onto/judged.rs` near :254 `held_by` (the `theseus_store::blocking` model to copy).
- `crates/theseus-core/src/judge/onto/digest.rs` near :130: the morning note's `"rename"` line.
- `crates/theseus-core/src/config/ontology.rs` near :20-32: the module doc's `sweep` and `sweep_usd_per_day` items.
- Cockpit: `cockpit/src/components/OntologyGrowth.tsx`, `OntologyParts.tsx`, `src/lib/ontology.ts`; `cockpit/AGENTS.md`
  says how `protocol.gen` is regenerated and how the cockpit's tests run.
- The review: `tail4-onto`'s findings are quoted above; the row's tests are in `tests_renamed.rs` (byte-for-byte undo).

## What to build
1. **F1.** `titles_of` runs its store reads under `theseus_store::blocking`, like `held_by`.
2. **F2.** `kind_pairs` asks a pair whose names would join existing clusters or chains, not only a union of 3 or more
   sessions: singleton chains can cluster, and two names that are each a cluster are asked whether they are one kind
   (`same_kind.v1`) before both mint. Keep the ask count bounded (`PER_RUN` or the pack's own cap: say which).
3. **F3.** A kind-name answer whose cluster does not mint that night is kept (in `Hints` or the ask's record) and used
   when the cluster mints later, so the paid ask is not wasted; and the asks are paced with `pressure::quiet` as the
   other sweep asks are. If keeping the answer needs a stored field, prefer the existing hint store; say what you used.
4. **F4.** Restore the template comment's "and"; the module doc names all four packs in one line.
5. **Visibility.** The morning note gains a line for a kind Jev named (beside `"rename"`, same voice). The cockpit's
   Ontology view shows a category's rename (old name → new, when) and marks a kind "named by Jev". Use the data the
   ontology list already carries if it is enough; else add the smallest serde-default field.
6. **Docs.** Core `AGENTS.md`, the ontology section: one sentence on F2's pair rule and F3's kept answer.

## Tests (each with a planted revert; the first is the key test)
- **`a_chain_of_singletons_clusters`** (`kind_pairs`: `a~b`, `b~c` gives one cluster or asks the pair). Plant: restore
  the union-of-3 rule.
- `two_clusters_are_asked_before_both_mint`. Plant: skip clusters in `kind_pairs`.
- `a_kind_name_answer_waits_for_its_mint` (the answer is used on a later night). Plant: drop the kept answer.
- `titles_of` under `blocking` (a test that fails if it runs on the async worker, if the store's test rig has such a
  check; else say so and rely on the code read). 
- The morning note's named-kind line. Plant: remove the line.
- The cockpit view: a component test (the cockpit's test runner) showing a rename and a "named by Jev" mark. Plant:
  drop the mark.
- `cargo nextest run -p theseus-core onto`, the cockpit's tests and type check, then `-p theseus-core` whole.

## Not in this row
- The release join's pack counts (`six.len()`, `INBOUND_ALONE`, health pack lists): stage 4 settled them. Do not touch.
- Owner calls: the `sensitive_names` template line, any token budget default.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Near you, not yet on main (integ/tail5):** tail5-t4fix edits core `scratch.rs` and `thread_title.rs`, the TUI,
  voice and herdr; tail5-effort edits the template's `[routing]` lines and core `AGENTS.md`. In the template you edit
  only the one `[ontology]` comment line; in `AGENTS.md` only the ontology section.
- **Rows beside you on `main` 49352e8a, joining `integ/tail6`:** tail6-cn (core `data*.rs`, `turn/data_step.rs`,
  `turn/route_step.rs`, `data_gate.rs`, compaction's gate, `consolidate/run.rs`, `keep_warm.rs`, `own_bench/replay.rs`,
  `judge/reply_data.rs`) and tail6-tests (`bench/recall/*`, core `scratch.rs`). Stay out of their files.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49; prefer no stored field (see the note above if you truly need one).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (ontology, the sweep, the morning note) and `cockpit/AGENTS.md` first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed per finding; the pair rule and its bound; where a kept kind-name answer lives; what the
cockpit and the morning note now show; each test and its plant with the failing line; the keel guard's output; the store
format; a `docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
