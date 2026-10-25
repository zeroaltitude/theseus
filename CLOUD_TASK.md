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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3a-mem-spike`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `ac298666` (release 1.0.2, signed tag `release-1.0.2`; the release the owner runs
now). Fetch it and branch from `ac298666`. Line numbers below are at `ac298666` and may be off by a few: search by
name. Where the release preamble above disagrees with this note about the base, ceilings or the work in flight, this
note wins.

**THIS ROW IS A SPIKE.** It explores and measures, and it reports numbers and a recommendation. It ships NO change to
the store format, the index's vector-file format, or a default. Code you write to measure lives behind a bench, a
test helper, a feature flag that is off, or a `spike/` module the integrator can drop; say which. Anything that would
change a stored format stays a PROPOSAL in the report.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`, :688).
Do not bump it. The index's vector file format (`crates/theseus-index/src/vectors.rs` module doc: per text its
SHA-256, the int8 256-d cut and its scale, the 768-d vector at f16, a CRC) is also unchanged by this row.

**Line ceilings.** Never raise one. `crates/theseus-index/src/vectors.rs` is at 2,561 of its 2,650 ceiling
(`scripts/long-files.txt`): put new code in new modules, not there. Every other file caps at 2,500.

**Your lane: `integ/p3-first`** (new: the first 1.0.3 rows). Its other rows (`p3a-recall-floor`, `p3a-recall-now` in
`crates/theseus-core/src/recall/` and `config/memory.rs`; `p3a-kimi` in `catalog.rs` and the example config) do not
touch `crates/theseus-index` or `crates/theseusd`'s memory code: expect no overlap. If you need a bench script, put it
under `bench/` in a new directory of its own. Do not edit `docs/status.md`.

## Your task: p3a-mem-spike, where the daemons' memory goes and how to shrink it (theseus-xxac7)

Branch: `cloud/20261024-p3a-mem-spike`.

Every commit subject carries `theseus-xxac7`. Deadline for the report: about 3 hours after you start.

**What was measured on the owner's machine** (a large real store: about 229k nodes, 360k chunks; the numbers are
`/proc/<pid>/status`):
- BUSY (during a 37k-text embedding backlog right after an import): `theseus-index` VmRSS 935 MB, of it RssAnon
  904 MB; `theseusd` VmRSS 271 MB, RssAnon 221 MB.
- IDLE (15 minutes later, after the index's idle unload, `[index] idle_unload_mins` = 10): `theseus-index` VmRSS
  34 MB (RssAnon 22 MB); `theseusd` VmRSS 147 MB with RssAnon only 30 MB (the rest file-backed store pages the
  kernel can reclaim).
- So the problem is the BUSY peak: the index loads about 0.9 GB whenever it embeds or searches by vector after an
  unload, and the first vector search after an unload pays a reload.

**The likely holders (verify each; do not assume):**
- The model's weights: `crates/theseus-index/src/mapped.rs` maps `model.safetensors` (547 MB, f32) read-only and
  COPIES each tensor into its own f32 heap buffer, a window at a time (theseus-agqn), so the whole model becomes
  anonymous memory (~0.5 GB). Compute stays f32: that is the owner's standing decision and this row keeps it.
- The stored vectors: per chunk the int8 256-d cut (~92 MB for 360k) and the 768-d f16 re-score vector (~553 MB for
  360k), if both are held in memory rather than read from the file. Find what is resident and when.
- `theseusd`'s busy anon (221 MB): unknown. Candidates named by the maintainer: the ontology snapshot, session
  compiles, recall packs, caches without caps.

**What the owner approved exploring (verbatim scope):** index weights mmapped + f16/int8 stored vectors with f32
compute, a daemon heap profile + cache caps, an owner-shape RSS bench, recall checked against the memory exam.

**What to do.**
1. **A synthetic owner-shape store.** No private data exists in your session and none may be made up to look real:
   build a SYNTHETIC store of about the owner's shape (about 229k nodes, 360k chunks, invented text under
   `example.test`; reuse the bench's or the exam's generators if they scale, say which), plus a smaller one (about a
   tenth) to show how memory scales. Write the generator as a re-runnable script or test helper, so the maintainer
   can rerun the same bench locally on a COPY of the owner's store later (the local half of this spike; it never
   runs here).
2. **An RSS bench** (`bench/memory/` or similar, new): start scratch daemons (`theseusd` + `theseus-index`) on the
   synthetic store and record, from `/proc/<pid>/status` and `/proc/<pid>/smaps_rollup`, VmRSS / RssAnon / RssFile at:
   start; after the index catches up; during an embedding backlog (if the model is available in your session; see
   item 5); during and after a burst of vector searches; after the idle unload; and the latency of the first vector
   search after an unload. It takes the daemons' binaries and a store directory as arguments (so the local run points
   it at a copied store), never a default path into a real home.
3. **Measure the levers, one at a time, against the base:**
   - (a) **Weights used straight from the map:** keep compute f32 but read the f32 tensors directly from the
     read-only map (file-backed, reclaimable pages) instead of copying them into heap buffers; check alignment and
     endianness (safetensors is little-endian; say how a misaligned tensor is handled). Measure busy RSS and RssAnon,
     embed latency per batch, and that the embeddings are BIT-IDENTICAL to the base (they must be: same f32 math).
   - (b) **Vectors not held whole in memory:** map the vector file (or read the 768-d f16 re-score rows from it on
     demand) so only the int8 scan stays resident; measure RSS and search latency (p50/p95) on the large store.
   - (c) **Lower-precision stored vectors:** the file already keeps f16 768-d and an int8 cut: measure recall at k of
     the fused/vector search for (base) vs (int8-only scan without the f16 re-score) vs (any other layout you find
     worth it, e.g. int8 768-d), each computed in f32, against an f32 reference on the same synthetic queries. Any
     layout that needs a new file format is a PROPOSAL only.
   - (d) **`theseusd`'s heap:** profile it on the synthetic store under a scripted load (sessions, recalls, a
     compile). Use whatever needs no new runtime dependency in the shipped binary (dhat or a jemalloc stats build
     behind a dev-only feature or a bench binary; `/proc` sampling at least). Name the top holders and propose caps
     (size, entry count, eviction) for each cache you find without one. Implement a cap only if it is a one-line
     constant on an existing cache AND a test proves nothing is lost by it; otherwise propose.
4. **Recall checked against the memory exam:** run the exam's model-free probe (`theseus-exam probe`, the gold-at-k
   per family; read its doc in `crates/theseus-exam/src/main.rs`) for the vector and fused arms at the base and under
   each of (a)-(c). (a) must give the same ranks exactly. Report gold@6 per family and overall for each.
5. **The embedding model.** If the model file is not in your session and cannot be fetched by the repo's own model
   setup (read how `theseus-index` finds or downloads it), measure what you can without it (vector store, search,
   theseusd heap with synthetic vectors), say plainly which numbers are missing, and put those measurements in the
   local half's list.
6. **A recommendation**: the levers in order of RSS saved per risk, with the numbers; which ones can ship in a patch
   with no format change (likely (a) and (b)); which need an owner call (any precision or format change); and the
   targets you would set (busy and idle RSS for each daemon on the owner-shape store).

## Tests

A spike still keeps the build green and adds tests for any code that would ship:
- If you implement (a) behind a flag: a test that the mapped path gives bit-identical embeddings to the copying path
  on a small fixture model or tensor set (red with a planted perturbation); a test for the misaligned case.
- The bench script: a smoke test on a tiny synthetic store (it runs, it writes the expected columns).
- Keep green: every `theseus-index` test (`tests_weights.rs`, `vtests.rs`, `ftests.rs` and the rest), the exam's
  tests, `theseusd`'s.

## Not in this row

No store-format or vector-file-format change, no default change (`idle_unload_mins` included), no precision change in
compute, no new runtime dependency in a shipped binary, no Discord change. No real data: the synthetic store only.

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result.
- **Keel:** remove or weaken no test; raise no budget, cap or ceiling; do not edit `scripts/keel-guard.py` or its
  tests; rename no test; edit no golden. Before the report run
  `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and paste its output (it must be empty).
- **Data floor:** synthetic data only, invented under `example.test`; the local half runs on a COPY of the owner's
  store on the owner's machine and its numbers, not its data, go into reports.
- **Public repo:** no person's name, no real organisation, address or email. Write "the owner".
- **Right over compatible;** red first for any code that would ship.
- **The gate** (read the root `AGENTS.md` and `crates/theseus-index/AGENTS.md` if present, "Building and checking"
  first): `cargo fmt --check`, `cargo clippy` on the touched crates with `-D warnings`, `cargo nextest run` (or
  `cargo test`) for `theseus-index`, `theseus-exam` and `theseusd`; then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format 51 and vector-file format untouched (or what a proposal
would change), what code is spike-only and how to drop it, any OWNER CALL. (2) **The synthetic stores:** shape,
generator, how to rerun. (3) **Where the memory goes:** each daemon, busy and idle, with the /proc numbers and the
holders found. (4) **Levers (a)-(d):** each one's RSS, latency and recall numbers against the base. (5) **The exam
probe:** gold@6 per family per lever. (6) **Recommendation and targets.** (7) **The local half:** the exact commands
to run the same bench on a COPY of the owner's store (never the live store), what to record, what is missing from
here (the model, if it was). (8) **Tests**, **Gate**, **Keel** output, **DM golden** diff result. (9) **Join notes:**
every file touched. (10) **Follow-ups.**
