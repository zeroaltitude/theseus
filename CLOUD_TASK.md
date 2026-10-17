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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-trim-binary`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: trim-binary, a smaller theseusd with nothing lost: the cockpit embedded compressed and served by Accept-Encoding, no unwind tables in release builds, one brotli decoder, and theseus-sim built but not installed (theseus-rwpbg.5)

Branch: `cloud/20261017-trim-binary`. Every commit's subject carries `theseus-rwpbg.5`. Deadline for the report: 4 hours after you start.

**Source:** the trim review's part 2: §1a (cockpit compression), §2b (unwind tables), §3a (brotli-decompressor), §5
(theseus-sim installed). **Owner-settled:** dropping the unwind tables (§2b) is approved; installs **stay on thin LTO**
(the owner's Q1: do not touch `release-thin`'s LTO or codegen-units, or the `release` profile). The installed theseusd is
100.86 MB today; this row should take it to about 85 MB. The size check (`theseus-sim bench size --check`) will show
shrinkage only.

**Prerequisite:** `main`. Builds now; joins 1st of the eight trim rows.

## Your row
| # | Slug | Beads | Scope | Files | Tests (planted revert) | Size |
|---|---|---|---|---|---|---|
| 5 | `trim-binary` | theseus-rwpbg.5 | (1) cockpit assets embedded as brotli and gzip, served by `Accept-Encoding`; (2) `-C force-unwind-tables=no` for release builds in build.sh; (3) theseus-aws-catalog on brotli-decompressor 5; (4) theseus-sim built, not installed | `cockpit/vite.config.*` (a post-build step), `crates/theseusd/src/web.rs` (+ a `web/encoded.rs` module), `crates/theseusd/Cargo.toml` (rust-embed include patterns), `scripts/build.sh`, `crates/theseus-aws-catalog/Cargo.toml`, docs that say where theseus-sim is installed | `every_embedded_asset_decodes_to_its_dist_file` (br and gz; revert: corrupt one), `the_cockpit_serves_br_then_gzip_then_identity` (revert: ignore Accept-Encoding), `vary_and_content_type_are_set`, the existing cockpit/web tests, the catalog's decode tests | M |

## (1) The cockpit, embedded compressed (§1a; ~10 MB)
`crates/theseusd/src/web.rs:63-64` embeds `cockpit/dist/` with rust-embed 8 (`include-exclude`,
`deterministic-timestamps`; no compression) and serves every byte raw. Today's dist is 443 files, 13.94 MB: report figures
(.svg, 246 files, 5.36 MB raw → 0.68 MB gzip), app JS (81 files, 5.51 MB), dated report pages (95 .js, 2.58 MB), fonts
(14 .woff2, 0.34 MB, already compressed), css (4, 0.14 MB). gzip -9 of the whole is ~3.4 MB; brotli is smaller again.
- **Compress at the cockpit's build**, with Node's built-in `node:zlib` (`brotliCompressSync` at quality 11,
  `gzipSync` at level 9) in a small Vite plugin or post-build script: **no new npm package** (package-lock.json must not
  change). For every file in dist except `.woff2` (and anything already compressed, such as images in a compressed
  format), write `<file>.br` and `<file>.gz` beside it. Deterministic output (no timestamps in the gzip header:
  `mtime` 0), so the binary stays reproducible.
- **Embed only the compressed copies** (plus the files left raw): rust-embed `#[include = "*.br"]`, `#[include =
  "*.gz"]`, `#[include = "*.woff2"]`, … on the existing `#[folder]`. The owner asked for both encodings, so both are
  embedded (~6-7 MB together against 13.94 MB raw). Measure and report the embedded total.
- **Serve:** for a request of `/x`, look up `x.br` when the request's `Accept-Encoding` accepts `br`, else `x.gz` when it
  accepts `gzip`, else decode the gzip copy (or the brotli copy) in memory and send identity bytes. Set
  `Content-Encoding`, `Vary: Accept-Encoding`, and the **original** file's `Content-Type` (from `x`, never from `.br`).
  Keep today's cache headers and ETag behaviour; an ETag must differ per encoding (append the encoding). If web.rs
  handles `Range`, a ranged request on an encoded body must not send a broken range: serve identity for a Range request
  or drop Range support for encoded bodies, and say which. The decoder for the identity fallback must already be in
  theseusd's tree: `brotli-decompressor` is (theseus-aws-catalog), and check `cargo tree -p theseusd -i flate2` before
  using flate2. Add no dependency.
- **A dist with no compressed files** (a stale `npm run build` from before this change): the gate builds the cockpit
  first, so this only bites a hand build. Make it fail loudly (a test or a build.rs check that every embedded name has
  both encodings, naming the first missing file), not serve 404s silently.
- In debug builds rust-embed reads the folder from disk at run time: the serving code must work in both modes.
- Tests: `every_embedded_asset_decodes_to_its_dist_file` (for every original dist file, the embedded `.br` and `.gz`
  decode to exactly its bytes); a request with `Accept-Encoding: br, gzip` gets brotli, one with `gzip` gets gzip, one
  with none gets the identity bytes; `Content-Type`, `Content-Encoding` and `Vary` asserted; the cockpit's existing e2e or
  smoke page load passes (`npm run build` then the theseusd web tests). The 1b report archive stays embedded (now
  compressed).

## (2) No unwind tables in release builds (§2b; ~5 MB theseusd; owner-settled)
theseusd carries 5.78 MB of `.eh_frame` + `.eh_frame_hdr` (sim 2.69, theseus 0.87, index 0.53, tui 0.35). With
`panic = "abort"` nothing unwinds, `crash.rs` records thread/location/message with no backtrace, and the binaries are
stripped. Add `-C force-unwind-tables=no` to `scripts/build.sh`'s release flags (the `flags=` lines at `:72-76`; build.sh
builds only `release` and `release-thin`), not to `.cargo/config.toml` (that would change the gate's debug builds).
- **Check the crash path still works** (the owner's condition): `THESEUS_TEST_PANIC` is a debug-assertions hook, so a
  plain release binary ignores it. Build once with build.sh's flags and the release-thin profile plus
  `CARGO_PROFILE_RELEASE_THIN_DEBUG_ASSERTIONS=true` (or the equivalent `--config` override), run theseusd with
  `THESEUS_TEST_PANIC` as `crates/theseusd/tests/crash.rs` does, and show that it aborts cleanly (signal, exit status)
  and that `crash.json` is written with its thread, location and message. Paste the commands and output.
- `readelf -S` before (main's build) and after: `.eh_frame` size per binary; `size -A` totals. Note in build.sh's comment
  what is lost (perf `--call-graph dwarf`, gdb `bt` and eBPF stack walks of a live release daemon) and why that is
  accepted (stripped + panic=abort already).
- `scripts/repro.sh` (if it rebuilds with build.sh's flags) still reproduces: run it or say why it can't run here.

## (3) One brotli decoder (§3a; ~0.2-0.3 MB each in theseusd and sim)
`crates/theseus-aws-catalog/Cargo.toml:13` uses `brotli-decompressor = "6"`; lopdf (theseus-files) brings version 5, so
theseusd and theseus-sim link both. Point the catalog at
`brotli-decompressor = { version = "5", default-features = false, features = ["std"] }`; its only call is
`Decompressor::new(input, 16 * 1024)` (`lib.rs:154`), which v5 has. The generator's `brotli = "9"` (an encoder, not in the
runtime binaries) still uses 6, so Cargo.lock keeps both versions: that is fine. Proof: the catalog's decode tests;
`cargo tree -p theseusd -i brotli-decompressor` (and `-p theseus-sim`) shows only 5.

## (4) theseus-sim: built, not installed (§5)
theseus-sim is a bench tool (42.92 MB installed; it links most of the daemon). `scripts/build.sh` builds the five
(`shipped=(theseusd theseus theseus-tui theseus-sim theseus-index)`, `:49`) and its size check runs the **target** copy
(`"$out/theseus-sim" bench size --check`, `:96-99`), so building it stays. The install itself is the maintainer's step
outside the repo (it links `target/release-thin/*` into `~/.local/bin`), so in the repo:
- Keep `--shipped` exactly as it is (the gate's features phase reads it: narrowing it would narrow the gate).
- Add `build.sh --installed`, printing the four binaries an install copies (`theseusd theseus theseus-tui
  theseus-index`), and say in build.sh's header that theseus-sim is built for the size check and the benches and is not
  installed.
- Find everything that expects an **installed** theseus-sim (on PATH or in `~/.local/bin`): `rg -n "theseus-sim"
  scripts bench docs crates/theseusd/src/install contrib` and check each: `bench/personality/exam_drive.py:226`,
  `bench/h2h/run.py:357-377`, `bench/async/*` take a bin dir; `scripts/setup.sh`, `scripts/user-service.sh` and
  theseusd's install plans. Where one defaults to the installed copy, make it default to the target copy (or say it
  already does). List each in the report with what you found. Docs that tell a user to run `theseus-sim` from PATH get
  a one-line fix (outside the spec, README, status and design docs, which you must not edit: name those instead).

## Proof nothing is lost (report section)
Byte-identical cockpit files after decoding (the test); every page still loads; the crash file is still written; the
catalog decodes; nothing that runs theseus-sim needs the installed copy. And the numbers: theseusd's size before/after
on release-thin (and the other four), `.eh_frame` before/after, the embedded cockpit total before/after.

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
