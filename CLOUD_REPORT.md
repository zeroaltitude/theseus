<!-- CLOUD_REPORT.md: the report of the cloud session on branch cloud/20261019-speed-idle-size (theseus-nsp9l.3). The maintainer drops this file at the merge. -->

# S3 `speed-idle-size` (theseus-nsp9l.3): what a resting daemon holds, and what grew `theseusd`

Branch `cloud/20261019-speed-idle-size`, from `bbc65530`. One code commit, `a4f49fdc` (a size fix, below), and this report.

**The short answer.** The idle-memory growth is not heap and not a load a resting daemon could skip. It is the daemon's
own **code**, file-backed pages of `theseusd` that the kernel maps 64 KB at a time around every page that startup
touches. The same growth is the binary's: the week added about 11 MB of code, and a larger binary, running more startup
code, has more of it resident. Nothing a resting daemon reads from its store was found to be big. I fixed the one thing
that was plainly waste (one generic helper compiled 164 times) and did not build the "lazy load" the row guessed at,
because there was nothing under it to load lazily (below, with the numbers). What would reach 60 MB, and what it would
cost, is under "Step 5", for the owner to decide.

All measurements are on this VM (4 vCPU, kernel 6.18, no swap, a warm page cache), **relative only**: the baseline is
`026dce3f` built here, not the owner's numbers.

## Step 1: measured here, base against head

`scripts/build.sh --profile release-thin` at `026dce3f` (a worktree under /tmp) and at `bbc65530`. Sizes, bytes:

| binary | `026dce3f` | `bbc65530` | final head |
|---|---:|---:|---:|
| theseusd | 77,857,000 | 88,745,544 | 87,996,056 |
| theseus | 8,445,664 | 13,225,744 | 13,225,744 |
| theseus-tui | 3,067,424 | 5,425,424 | 5,425,424 |
| theseus-index | 8,919,392 | 8,307,448 | 8,307,448 |
| theseus-sim | 8,877,592 | 39,627,280 | 39,702,328 |

(`026dce3f`'s theseusd is the owner's 77.8 MB to the byte, so this build is the same bytes as his. The head here is 88.7
MB against his 82.4 at `351159fc`: he measured an earlier commit.) `theseus-sim` is 4.5 times bigger, and is built for the
benches, not installed; I did not look into it.

`python3 bench/h2h/run.py --theseus-only --rows T7 --runs 5 --bin-dir <dir>`, T7's idle RSS (the TUI and the daemon, after
the ready prompt and the bench's turns, 10 s idle, kB; each run's mean):

| | runs | median |
|---|---|---:|
| `026dce3f` | 40589 40390 39647 39511 39608 | **39,647** |
| `bbc65530` | 52037 52264 51940 50950 52083 | **52,037** |
| final head (the fix) | 52602 52340 52135 52443 50218 | **52,340** |

T1 (start to a ready prompt) medians 46.1 ms against 45.0 ms, and the CPU is 0.0 to 0.1 % in both: no start or idle-CPU
regression shows. The growth, +12.4 MB (+31 %), is the owner's growth (49.6 to 71.7, +22) in direction and in kind.

`/proc/<pid>/status` and `smaps_rollup`, the bench's run, last idle sample (kB):

| process | | RSS | anonymous | file-backed | threads |
|---|---|---:|---:|---:|---:|
| theseusd | `026dce3f` | 35,252 to 36,240 | 3,136 to 3,248 | 32,052 to 32,992 | 11 to 12 |
| theseusd | `bbc65530` | 44,224 to 45,344 | 3,800 to 4,004 | 40,376 to 41,412 | 11 to 20 |
| theseus-tui | `026dce3f` | 4,976 to 5,048 | 756 to 760 | 4,216 to 4,292 | 2 |
| theseus-tui | `bbc65530` | 7,284 to 7,468 | 912 to 952 | 6,352 to 6,548 | 2 |

**Of the +12.4 MB, about 10.6 is file-backed (theseusd +8.4, the TUI +2.2) and 0.9 is anonymous (+0.7, +0.2); the rest of
the 12.4 is sampling (the table is the last idle sample, the bench's figure the window's mean).** The daemon's threads settle at 7 (five tokio workers, the store writer, main); the higher counts are
blocking-pool threads the start's work spawns, which leave after their 10 s keep-alive.

## Step 2: attributing idle memory (base `bbc65530`)

**File-backed text, not heap.** At 10 s idle the daemon's `r-xp` mapping of its own binary is 29 to 31 MB resident of 68
MB, the `r--p` rodata 5 MB, the RELRO 1.8 MB (dirty, so it counts as anonymous). Then `echo 1 > /proc/<pid>/clear_refs`
and 20 s of idle: the pages **referenced** in the text are **1.1 MB** (0.7 MB at `026dce3f`), so a resting daemon uses
1 MB of code and keeps 29 MB mapped that startup touched once. The cause is the kernel's **fault-around**: a read fault
on a file page maps the 16 pages (64 KB) around it that are in the page cache. A small C probe that touches one byte
every MB of the binary: 64 touches add 4,624 kB of RssFile, 72 kB each; `MADV_RANDOM` does not change it. The text is
scattered by 16 codegen units and thin LTO, so startup's code spans about 450 distinct 64 KB windows.

**What startup touches, by phase** (a temporary probe printing `RssFile` at each step, not committed; the daemon on a
fresh state dir, the bench's config; kB of file-backed resident; `026dce3f` then `bbc65530`):

| at | `026dce3f` | `bbc65530` | |
|---|---:|---:|---|
| `main` (loader, libc, std's init) | 5,100 | 5,760 | |
| after clap and tracing | 7,560 | 7,552 | |
| before the store opens (config, secrets, runtime) | 15,464 | 16,776 | **+9 MB here**, config's serde visitors |
| after the store opens | 17,728 | 18,980 | redb: +2 MB |
| after `Core::new` | 24,596 | 26,596 | **+7.6 MB** (tools, catalog, providers) |
| serving | 25,284 | 27,468 | |
| 12 s later | 30,968 | 37,500 (38,164 in a second run) | **+10 MB after serving** |

So most of the growth is in what runs once after the socket answers: base +5.7 MB, head +10. A second probe skipped one
group of `after_serving` at a time (`THESEUS_SKIP`, also not committed; two runs each, so about ±0.7 MB): at +12 s the
daemon is 38.2 MB with everything, 36.1 without the `warm_*` projections (2.0 MB), 36.6 without the nightly tenders
(1.6), 36.2 without the actors (the MCP board, the cgroup find, the harness loop and driver, telemetry; 2.0), and
33.1 MB with all four skipped (5.1 MB); the remaining 4 MB is the first RPCs (the bench's readiness check calls `health`,
a very large function). Every one of these is "code run once", and none of them holds data: the heap is 3.8 MB.

**The heap is flat, and small.** Anonymous: 3.2 MB (`026dce3f`) and 3.9 MB (`bbc65530`) at idle; 0.9 MB for the TUI. For a
store the owner's size, `theseus-sim synth-store --sessions 20000 --ledger-rows 400000` (580,000 records, 443 MB), copied
into a scratch state dir: the daemon holds RssAnon 40.4 MB at 20 s (`bbc65530`) against 42.7 MB (`026dce3f`), so there is
no growth with history, and `heaptrack` (installed from apt; the VM allows it) on that daemon, killed at idle, says what is
live: **16.8 MB is redb's page cache** (5,694 allocations; bounded at `index::CACHE_BYTES`, 16 MiB, by design,
theseus-vjn7) and about 1 MB everything else. The rest of the anonymous bytes is the allocator holding freed pages after the
history check, which peaks at 67 MB in `wal::read_range` and 37 MB in a `Vec` growing in core, and which `resident.rs`'s
trim returns after `QUIET`; at 50 s the daemon was `RssAnon` 31 MB with 17 MiB of heap in use. `valgrind --tool=massif`
crashes on this binary (an internal valgrind error); heaptrack worked.

**Named causes, with their MB** (file-backed unless said; bench config, `bbc65530`, daemon unless said):

1. Fault-around over code run once at startup: 29 MB of the daemon's 40 and 6.4 of the TUI's 7.4. The 8.4 MB growth is
   the week's startup code: config (+1.3), Core (+2.0), after serving (+4.3: the warm projections, the nightly tenders,
   the actors) and the first RPCs, each a few dozen windows more than before.
2. redb's page cache, anonymous, 16.8 MB with a store the size of the owner's, bounded by design; none on a fresh store.
3. The heap held after the history check, anonymous, up to 25 MB transient, returned by the trim.
4. Everything else: threads 7 at rest (stacks are virtual), `RssAnon` 3.9 MB, the RELRO 1.8 MB (103,802 relative
   relocations at load, `.rela.dyn` 2.5 MB of the file).

**What startup loads eagerly that a resting daemon never reads: nothing big.** `warm_ontology`, `warm_standing`,
`warm_forecasts`, `warm_persona`, `warm_threads`, `warm_grants`, `warm_tools_use`, `warm_modes`, `warm_labels`,
`warm_retention`, `warm_activation`, `warm_ladder`, `warm_corrections`, `warm_judge`, `warm_leases`, the outbox's index
and `check_pack_text_after_serving` are each a META scan into a small in-memory board; on the 20,000-session store they
leave about 1 MB of heap in all. The nightly tenders are gated by their config already (`learn_after_serving`: nothing
with the judge off; `route_replay_after_serving`: nothing at a 0 cap; the self ones: only `[self] mode = "act"`). The index
tender, the LSP, voice, the web UI and Discord are off or not started in this config, and the LSP and the MCP servers
start at first use. The prefix library, the context files and the persona's files are read at the first compile. Making
these lazy would move their scans onto a turn (the first turn after a start pays them), which the row forbids, to save
about 2 MB of file-backed code that the kernel can drop whenever it wants.

**Rejected, with why: releasing the startup code after a quiet stretch.** `madvise(MADV_DONTNEED)` on the daemon's own
`r-xp` mapping (never the RELRO, which holds dirty relocated pages) would take the 28 MB out of the RSS, and the pages
stay in the page cache. I did not build it: (a) it frees no system memory, since the page cache is charged to the unit
and stays; (b) the bench's T7 starts the moment the last turn ends, so a release that waits for a quiet stretch
cannot show in it, and a release that does not wait refaults code on every turn; (c) unmapped pages lose the kernel's
protection for referenced mapped executable pages, so after a long idle under memory pressure the first turn could meet
major faults on the send path. It is the owner's to want; the function is about 30 lines in `resident.rs`.

## Step 3: attributing size

`scripts/size-breakdown.py` on unstripped builds of both (the same `build.sh` flags, `CARGO_PROFILE_RELEASE_THIN_STRIP=false`),
and `cargo-bloat` is installed but its build would have repeated the same 17 minutes, so I used `nm -S -C`, which is what
`cargo-bloat --crates` does. Head 88.95 MB stripped: `.text` 71.6 MB, `.rodata` 12.9 MB, `.rela.dyn` 2.5 MB, `.data.rel.ro`
1.75 MB. By crate, base to head (MB):

| crate | `026dce3f` | `bbc65530` | |
|---|---:|---:|---:|
| theseus_core | 10.43 | 15.74 | **+5.31** |
| std (generics instantiated by our types) | 11.58 | 16.31 | **+4.73** |
| serde_json (per-type glue) | 5.13 | 7.55 | **+2.43** |
| tokio (our futures) | 2.65 | 4.38 | **+1.73** |
| theseus_discord | 0.89 | 2.23 | +1.35 |
| theseus_protocol | 0.68 | 1.33 | +0.65 |
| theseus_kernel | 0.78 | 1.10 | +0.32 |
| toml, similar, hashbrown, theseus_store, twilight_* | | | +1.0 together |
| total (stripped) | 77.85 | 88.95 | **+11.1** |

It is the sum of 448,000 added lines across 2,487 files; no one crate, dependency or embedded blob is the week's growth,
and the shipped five gained no dependency that explains it (`theseus-tui` took `vt100` for its tests, the CLI an edge to `toml`).

**By function** (head, grouped by name without generic arguments): `serde_json`'s deserializer glue instantiated per
derived type is **7.2 MB**: `Map::deserialize_any::<T>` 1,007 times (3.0 MB), `deserialize_struct` 641 times (2.7 MB),
`visit_array` 1,108 times (1.6 MB). A type read from both a `Value` and a string pays twice. The slice sorts are 2.1 MB over
345 element types; `drop_glue` 5.0 MB over 11,948; `serialize` 1.1 MB over 2,106. The largest single functions are the
four `rpc_prefixed` (80 to 84 KB each) and `dispatch` closures (72 to 74 KB), `Glyph::from_name` in lopdf (441 KB, one
match), and brotli's dictionary and `ecp_nistz256_precomputed` (data).

**`theseus_store::blocking` was instantiated 164 times**, and `block_in_place` and `exit_runtime` after it 184 and 155
times: 0.69 MB of one function body copied for each caller's closure. That is the fix below.

**Embedded data** (`include_bytes!`, `include_str!`, rust-embed): there is no large uncompressed blob. The cockpit's
`dist/` is 24 MB, 20.7 MB of it in the build's 1,436 files; the binary embeds only the `.br` (2.56 MB, 474 files), the
`.gz` (3.18 MB, 474) and the fonts (0.34 MB, 14), 6.1 MB, already compressed. The AWS catalog is one 2.27 MB brotli blob.
Every other `include_str!` of any size in `crates/` is a test reading source files. Of the cockpit's two copies, the
`.gz` is the one a browser on `http://127.0.0.1` most often asks for (Chromium offers `br` to localhost as I remember it,
Firefox does not over plain http; I did not check), and one copy of the two would be 2.6 to 3.2 MB (3 to 3.6 % of the binary). I did **not** do it: the cockpit's
serving tests (`the_cockpit_serves_br_then_gzip_then_identity` and `every_embedded_asset_decodes_to_its_dist_file`) pin
both encodings, and changing them is a keel finding the owner would have to sign. It is the cheapest non-code line item.

## Step 4: what I changed

1. **`theseus-store`: `blocking` is one function over a `&mut dyn FnMut`, not generic in each caller's closure**
   (`crates/theseus-store/src/store.rs`; `a_section_runs_once_and_returns_its_value_in_every_context` in
   `store/tests_blocking.rs`). Behaviour is unchanged: the same `Handle::try_current` and flavour test, the same
   `block_in_place`, the same `spend_task_budget`; the closure runs once through a `&mut dyn FnMut` and its result comes
   back by an `Option`. The cost is a virtual call per disk wait. Measured: the unstripped head went from 119,690,016 to
   117,492,928 bytes (-2.2 MB) with the probes added, and the stripped `theseusd` is 87,996,056 against 88,745,544 (-749,488 bytes, -0.84 %; `build.sh`'s size check: "-0.75 MB, ok").
   A pure refactor, so no planted revert of the behaviour: the existing `tests_blocking` (a section outliving its
   runtime polls no timer after it; the task gives way once and runs on) hold what `blocking` does, and the new test holds
   the value and the single run on a plain thread, a current-thread runtime, a worker and the blocking pool, with a result
   that is not `Send`. Planted: the non-runtime arm of `run_blocking` made to call the closure twice; the new test failed
   (a panic at the `expect` in `blocking`, store.rs:464); restored, `touch`ed, `git status` clean.

Nothing else is committed. I considered and left: type-erasing `rpc::server::route`, `reply` and `parse` (136, 105, 113
instantiations, 0.31 MB together, but they are the RPC dispatch); `similar`'s `conquer` (32 copies, 0.37 MB); the one
cockpit copy (above); `opt-level = "s"` for the Discord and twilight crates (an estimated 0.9 MB, and their generics
instantiate in whatever crate calls them, not in the dependency, so a per-package override reaches few of them).

## Step 5: what reaching 60 MB would cost (the owner's call)

`release-thin` is thin LTO and 16 codegen units. `scripts/AGENTS.md` measured it 23 % bigger than `release` (fat LTO, one
unit) on 2026-10-02, and slower by 8 to 10 % on CPU-bound work, for installs that rebuild in minutes. I did not build `release` here: a fat link of this binary on 4 cores would not have ended beside the rest of the work, so
the 23 % is that earlier measurement, and the 71 MB below is its projection, not a measurement.
The levers, with what each costs:

- **`--profile release` for the install**: the size line's biggest lever (about -20 %, 88.7 to about 71 MB) and the
  resident set's, since startup's windows are spread over a smaller text; the cost is the build time of every install and
  the single-threaded fat link.
- **`-C link-arg=-Wl,-z,pack-relative-relocs`**: `.rela.dyn` is 2.5 MB (103,802 relocations) and RELR packs it to
  about 0.2 MB, -2.3 MB; it needs glibc 2.36 at run time, so Ubuntu 22.04 (2.35) and RHEL 9 (2.34) could not run the binary,
  which `scripts/AGENTS.md` names as the floor (2.34).
- **One cockpit copy** (-2.6 to -3.2 MB, above): a keel decision.
- **`opt-level = "s"` for the whole build**: roughly another -20 % of text and a few percent to ten of CPU; it is the
  hot-path trade the row forbids, so only for crates the owner calls cold.
- **Features**: voice (openmls, rustfft, songbird, the opus glue: about 2.5 MB), PDF reading (lopdf, zip: about 1.5 MB), the
  AWS catalog and tools (the 2.27 MB catalog and about 1 MB of code), Discord and twilight (about 3.5 MB). A `voice`
  feature on `theseus-discord` that the install builds without would be the largest single feature cut.

Fat LTO plus RELR plus one cockpit copy is about 66 MB; 60 needs a feature or `opt-level = "s"` on top. Short of those
the resident set follows the text size: this week RSS grew 0.77 MB (daemon) and 0.93 MB (TUI) for each MB the binary grew,
so a smaller text should take its resident set down in that proportion, if the ratio holds beyond this range.

## What to run in the morning (the owner's quiet machine)

```
scripts/build.sh --profile release-thin
python3 bench/h2h/run.py --theseus-only --rows T7 --runs 10 --bin-dir target/release-thin --out /tmp/h2h-s3
```
The binary should be about 0.75 MB smaller than the same tree without this change
(`ls -l target/release-thin/theseusd`: 87,996,056 bytes here against 88,745,544). T7 should not move: here it read 52.3 MB
against 52.0 for the head, inside the run's noise (this measurement ran beside the gate's compile load, so the page cache
was not comparable to the first two; 0.75 MB of text is about a dozen of the 450 windows startup touches). Nothing here moves
a speed. To see the residency, not the RSS: `grep -E 'RssAnon|RssFile' /proc/$(pgrep -x theseusd)/status` on a daemon
that has been up a minute, and `echo 1 > /proc/<pid>/clear_refs`, wait 20 s, `awk '/r-xp.*theseusd/{f=1} f&&/^Referenced/{print;exit}' /proc/<pid>/smaps`:
the code a resting daemon uses (1 MB here).

## Found, not fixed (other rows' ground)

- **The start path grows with today's ledger.** With the synthetic store (66,666 ledger rows "today", all written at once),
  `day_ceiling: today's spend read` took 3.59 s and the daemon began to serve at 4.2 to 5.8 s (`core 3.54 s`, `day_ceiling
  3.53 s`). Each row costs about 54 us, so a day of 5,000 rows is 0.27 s of the start. That is a FAST finding (the start
  path must not grow with history, and "today" is history), not an idle-memory one: it belongs to S1 or S2's area.
- `health`'s `connections:` line read `open files 20000 of 20000` on that daemon (a count of the daemon's own descriptors
  against a limit that the start had raised to 19,988); I did not look at what it counts.
- `theseus-sim` grew from 8.9 to 39.6 MB (it now links theseus-core). It is built for the benches, not installed.

## Verification

- `cargo build --workspace --all-targets`: clean. The gate, `CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1
  scripts/gate.sh` (`CARGO_INCREMENTAL=0` because the VM's disk allowance ran out twice on incremental caches; it cost one
  gate, the first, which died writing a cache): door, keel, keel tests, fmt, shape, features, clippy (`-D warnings`), the
  cockpit's lint, test and build, the test build and the reader rule **pass**. The suite ran 6,847 tests: 6,813 passed and
  **34 failed**: the 33 L1 cases the brief names (`theseus-sandbox::contract` x19, its bench's `spawn_100`, and `theseusd::sandbox`
  x13; the root-daemon refusal, theseus-pv6i) and **`theseus markdown::rendering_costs_little_per_hundred_lines`**, a
  timing test (0.317 s) that failed beside the release build and passes alone (`cargo nextest run -p theseus -E
  'test(=rendering_costs_little_per_hundred_lines)'`: 1 passed in 0.03 s); it is not on the flaky list and has nothing to do
  with this change. `theseus-store`'s `tests_blocking` (the two old tests and the new one) pass in the suite. Then I ran the
  phases after the suite by hand: **protocol types** ok (`cockpit/src/protocol.gen` unchanged after the suite's generating
  test), **turn** (`theseus-sim bench turn --runs 5 --burst 0 --check`) exit 0, plain turn 4 frames, the first-request
  budgets ok (dm 9,411 tokens, task 6,585, headless 7,846); the lifecycle and jobs benches are skipped by
  `THESEUS_GATE_NO_BENCH`; **size** was judged by `build.sh` (theseusd -0.75 MB, ok).
- **`cargo deny --offline check`: advisories FAILED, bans, licenses and sources ok.** It is **not this change** (no
  dependency moved): after a `cargo deny fetch` the database holds **RUSTSEC-2026-0367**, `zip 8.6.0` (a dependency of
  `theseus-files`; "Upgrade to >=9.0.0"; SUID and SGID bits kept when a zip is extracted). The owner's `deny-daily.sh` should
  see it; it is a finding for whoever owns `theseus-files`. Before the fetch the offline check could not run at all (no
  database; I had skipped the brief's `cargo deny fetch` step until the end).
- Keel guard, `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: empty, exit 0.
- Not run: the lifecycle and turn benches (the gate's `THESEUS_GATE_NO_BENCH=1`; the speed budgets are the owner's morning).
  Nothing in this change is on a start path or a turn's send path but the `blocking` call, which adds one virtual call to
  each disk wait.

## Doc changes for the maintainer

- `docs/status.md` and the spec's Part III item for S3: the finding that the idle RSS is file-backed code, with the
  startup table above; no lazy-loading step was needed.
- `crates/theseus-store/AGENTS.md`, the `blocking` invariant: "one function over a `&mut dyn FnMut`: do not make it
  generic again; it was 164 copies, 0.7 MB".
- `scripts/AGENTS.md`, "Releases": a line that `release-thin`'s idle RSS is mostly the kernel's 64 KB fault-around over
  startup code (the daemon's referenced text at idle is 1 MB), so RSS follows the text size and a "resident" budget
  should read `RssAnon` and `Referenced`, not `VmRSS`.
