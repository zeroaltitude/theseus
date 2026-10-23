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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261023-p2-public-names`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
integration lanes named in each task (`integ/p2-threads`, `integ/p2-misc`, and others). Other rows land in the same
lanes from other branches at the same time. Your task names them and the files you share with them: stay additive (one
helper, not two), and leave their files alone.

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
3. The repository is public: no person's name, no real organisation, address or id anywhere. Fixtures use invented
   names and ids under `example.test`. Write "the owner".
4. Red first: each fix comes with a test that is red at the base and green after, and a planted revert named in the
   report.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: p2-public-names, the old organisation names leave the public repository's fixtures and tests, a tree-wide digest check keeps them out, and `policy.explain` knows a hold already told the owner (theseus-cuv2o)

Branch: `cloud/20261023-p2-public-names`. Every commit's subject carries `theseus-cuv2o`. Deadline for the report: 3
hours after you start.

**Why.** The repository is public. An earlier row (theseus-a9zg9, on `main`) replaced four real organisation names in
the template's `[privacy] sensitive_names` example with placeholders (`examplecorp`, `examplecorpinc`,
`example-repo`), and the judge test `the_pack_text_names_no_organisation` (`crates/theseus-judge/src/pack.rs` near
:1759) now holds the four old names **only as SHA-256 digests** (`OLD`) and fails if the template says any of them.
Its review found the same four names still in 37 lines of fixtures and tests across 9 files: real hits, not chance
word matches (one is a fixture thread title; the matcher's own tests list all four). The same review left two smaller
follow-ups, filed on the same issue:
- `policy.explain` (`crates/theseus-core/src/rpc/explain.rs` near :137 and :163) builds its scene's hold from
  `external::held`, not from `external::held_untold`. After a notify hold has told the owner once, explain still
  predicts that the hold raises the call to notify, while the real call runs quietly: a small explain/real divergence
  in an order that is meant to be shared.
- Two spec lines still say `ask` is `external_text`'s default: `docs/spec/03-part1-s3.md:297` and
  `docs/spec/11-part3-a4.md:803`. Since theseus-nsqqo, an unset `external_text` follows `[policy] enforcement`.

**Never write the old names.** Not in code, tests, comments, commit messages or your report. Find them by digest: the
four digests in `OLD` are SHA-256 of the lowercased words. The hits at the base (file: lines):
- `crates/theseus-judge/src/names.rs`: 80, 111-115 (the matcher's own tests: the `OWNER` list and its cases);
- `crates/theseus-tui/src/threads.rs`: 69; `crates/theseus-tui/src/tests_threads.rs`: 38, 73, 84, 101, 120, 172, 176;
- `crates/theseus/src/current.rs`: 155; `crates/theseus/src/interactive/thread.rs`: 282;
- `crates/theseus/src/render/thread.rs`: 59, 171, 184, 189, 191-193, 198, 208, 210;
- `crates/theseus/tests/thread_interactive.rs`: 71, 75, 192, 198; `crates/theseus/tests/thread_switch.rs`: 41, 44, 85,
  89, 161, 175;
- `crates/theseus-discord/src/render/reply.rs`: 48 (a doc comment).
None is in a golden file. Re-find them yourself with a digest scan before and after (step 1).

**Risk: low.** Fixture strings, doc comments, a test, and one explain call.

**Prerequisite: base `origin/main` at `23328545`.** Your branch joins lane `integ/p2-misc`. Run the keel guard against
`origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `p2-public-names` | theseus-cuv2o | placeholders in every hit; one digest list; a tree-wide test; `policy.explain` uses `held_untold`; two dated spec notes | the 9 files above; `crates/theseus-judge/src/pack.rs` (the test's `OLD` moved to one shared place); a new test (the tree-wide scan); `crates/theseus-core/src/rpc/explain.rs` and `tests_explain.rs`; the two spec files (one note line each) | S-M |

## What to build
1. **A digest scan first (red).** A test that walks the repository's text files from the workspace root (find it from
   `CARGO_MANIFEST_DIR`), skipping `vendor/`, `target/`, `node_modules/`, `.git/` and binary files, splits each file
   into words the way `the_pack_text_names_no_organisation` does (alphanumerics and `-`, each hyphen part too,
   lowercased), and fails on any word whose SHA-256 is in `OLD`, naming the file and line (never the word). It is red
   at the base (the 37 lines). Move `OLD` to one place both tests read (for example a `pub(crate) const` in
   `names.rs` or a small module of the judge crate), not two copies. Keep it fast (well under a few seconds); if the
   walk is slow, say what it reads.
2. **Placeholders.** Replace every hit with an invented name of the same shape, so each test still tests what it
   tested:
   - the template's three placeholders where they fit (`examplecorp`, `examplecorpinc`, `example-repo`);
   - a fourth hyphenated placeholder of the same family for the fourth name (for example `example-utils`), keeping
     names.rs's case that a hyphenated name matches as a unit and its first word alone does not (pick a first word
     that is an everyday word, as the old one was, and change the case's sentence to match);
   - the email case keeps an email shape under `example.test`-style invented domains that the matcher still finds;
   - the fixture thread titles (`<name> pr`, `<name> pr round`, `/t <name> pr`) become one invented title used
     consistently across the TUI and CLI tests, so render and parse cases still pair up.
   Every assert stays as strict as before. The scan of step 1 is then green.
3. **`policy.explain` (red first).** A test in `tests_explain.rs`: a session whose notify hold has already told the
   owner (`told` is set, as the gate's path sets it); `policy.explain` for an acting call must report what the real
   call does now (no raise from the hold), not the old prediction. Then use `external::held_untold(…,
   rt.external.mode)` (or the gate's exact call) for the scene in both places in `explain.rs`, so explain and the gate
   share one order.
4. **The spec lines.** The specs are records: do not rewrite them. Add one dated note line under each of the two lines
   saying what holds now (an unset `external_text` follows `[policy] enforcement`, theseus-nsqqo; the template shows
   `ask`).

## Tests (each red at the base, with a planted revert; the first is the key test)
- **The tree-wide digest scan** (step 1). Plant: put one old name back in one fixture (in your working tree only,
  never committed); the scan fails naming that file and line.
- `the_pack_text_names_no_organisation` still passes, reading the shared `OLD`.
- The names.rs matcher cases, the TUI thread tests and the CLI thread tests all pass with the placeholders.
- **`explain_after_a_told_hold_predicts_no_raise`** (step 3). Plant: back to `external::held`.
- Run: `cargo nextest run -p theseus-judge`, `-p theseus-tui threads`, `-p theseus thread`, `-p theseus-core explain`,
  `-p theseus-discord`, then the workspace's tests for the crates you touched.

## Not in this row (other rows in lane `integ/p2-misc`, landing at the same time)
- **p2-recall-price** (theseus-zeyr3): `bench/recall/*`. Not yours.
- Lane `integ/p2-threads` rows touch the TUI chat (`chat/*`) and the thread router and recall: your TUI and CLI edits
  are fixture strings and doc comments in `threads.rs`, `tests_threads.rs`, `current.rs`, `interactive/thread.rs`,
  `render/thread.rs` and the two CLI tests only. Change no logic there.
- `crates/theseus-core/src/toolrun.rs` is at its cap (2,500 lines): add nothing to it. The gate's own use of
  `held_untold` stays as it is.
- Do not touch the keel guard or its tests.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed (for the digest scan's plant, name the file,
  not the word).
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Replacing a fixture
  name with a placeholder of the same shape keeps the assert; say in the report, per changed test, that its assert is
  unchanged. A cloud row cannot carry a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py
  --base origin/main --head HEAD --quiet` and paste its output in the report. It must be empty.
- **Dependencies.** None new (`sha2` is already the judge crate's). **Store format** 49; nothing here is stored.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name and no real organisation name anywhere: write "the owner". The old names
  never appear in anything you write, including the report.
- Read `crates/theseus-core/AGENTS.md` (policy, external text) and the judge crate's docs on `names.rs` first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the digest scan's count at the base (files and lines, no words) and after (0); where `OLD` lives;
the placeholders you chose per file; per changed test, that its assert is unchanged; the explain fix and its test; the
two spec notes; each test and its plant with the failing line; the keel guard's output; the store format; a
`docs/status.md` sentence for the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
