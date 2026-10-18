# CLOUD_REPORT: trim-binary (theseus-rwpbg.5)

Branch `cloud/20261017-trim-binary`, built from `432bbbeb` (the base of CLOUD_TASK.md's commit). `origin/main` in this
clone is stale (13 Oct), so the gate and the keel guard were run with `THESEUS_KEEL_BASE=432bbbeb`.

## Numbers (release-thin, bytes)

| binary | before (c36f0d98) | after | `.eh_frame`+hdr before → after |
|---|---|---|---|
| theseusd | 100,370,888 | 86,394,584 | 5,817,744 → 0 |
| theseus | 13,876,712 | 13,005,424 | 871,260 → 0 |
| theseus-tui | 5,648,400 | 5,242,408 | 405,724 → 0 |
| theseus-sim | 42,413,216 | 39,557,872 | 2,700,600 → 0 |
| theseus-index | 8,921,472 | 8,308,456 | 612,800 → 0 |

theseusd lands at 86.39 MB, not about 85. Embedded cockpit: 13,936,232 B raw (443 files) → 5,798,389 B embedded
(2,433,692 `.br` + 3,021,525 `.gz` + 343,172 `.woff2`).

Steps of the theseusd saving, measured: cockpit compressed + one brotli decoder + `-C force-unwind-tables=no`:
100.37 → 92.17 MB; adding the linker script: → 86.39 MB. "Before" was built from a worktree of c36f0d98 with the same
dist copied in raw. `size -A` was not run; `ls` sizes and `readelf -S` section sizes are above.

## Step 1: the cockpit embedded compressed (c65d9e0e, a8313ca7)

Found: `web.rs` embedded `cockpit/dist` raw. Changed:
- `cockpit/compress.ts` (new, a Vite plugin, `closeBundle` post): Node's `zlib` only, brotli q11 and gzip 9, `.br` and
  `.gz` beside every built file but `.woff2` (and already-packed image formats). No package added; `package-lock.json`
  untouched. Output is deterministic: two `npm run build`s give the same sha256 over the whole dist.
- `crates/theseusd/src/web.rs`: `#[include = "*.br"]`, `"*.gz"`, `"*.woff2"` on the embed; `serve_embedded` moved to the new
  `web/encoded.rs` (`respond`, `exists`). A request gets `x.br` if `Accept-Encoding` takes `br` (q=0 refuses, `*` allows),
  else `x.gz`, else the brotli copy decoded in memory (`brotli-decompressor` 5, now a direct dependency of theseusd, and
  `flate2` a dev-dependency for the test; both packages were already in the lock). `Content-Type` is from `x`, never
  `.br`; `Vary: Accept-Encoding` on every compressible file (not on fonts, which are sent as they are); cache headers
  unchanged. A direct request for `x.br` / `x.gz` falls back to the app shell. **Range:** `web.rs` has never answered
  `Range` (no handler), so there is nothing to break; **ETag:** none was ever sent, so none is added.
- A dist from before this change: `every_embedded_asset_decodes_to_its_dist_file` fails naming its first missing
  file (`"<name>.br is not embedded: rebuild the cockpit"`). No `build.rs` check.
- Debug and release both work: debug reads the folder with the same include filters; release was checked below.

Proof:
- `cargo test -p theseusd --bin theseusd web::` 7 passed: `every_embedded_asset_decodes_to_its_dist_file` (all 443
  originals: fonts equal, every other file's `.br` and `.gz` decode to its exact bytes),
  `the_cockpit_serves_br_then_gzip_then_identity`, `vary_and_content_type_are_set`, and the four existing web tests.
- `tests/web_ui.rs::the_cockpit_is_sent_in_the_coding_the_client_takes` (new, a real daemon over TCP): br, gzip, plain
  with the right headers. The 4 web_ui tests pass against the **release-thin** theseusd (copied over
  `target/debug/theseusd` and the compiled test run directly), so the release embed serves it too.
- Planted reverts: (a) `accepts` forced false → `the_cockpit_serves_br_then_gzip_then_identity` FAILED
  (`left: None, right: Some("br")`); (b) one byte of an `assets/*.js.br` corrupted → `every_embedded_asset_decodes_to_its_dist_file`
  FAILED (`Invalid Data`). Both restored (touched); all 7 pass again.

Live check for the maintainer (scratch daemon, own state dir):
`curl -s -D- -o /dev/null -H 'Accept-Encoding: br' http://127.0.0.1:<port>/assets/<some>.js` shows `content-encoding: br`,
`content-type: text/javascript`, `vary: accept-encoding`; the same with `gzip`, and with no header no
`content-encoding`. Open `/` in a browser: the page loads (browsers take br).

## Step 2: no unwind tables (478648d3, 60ab36ba)

**Found: `-C force-unwind-tables=no` alone does almost nothing.** theseusd's `.eh_frame` went 5,817,744 → 5,772,728
(0.05 MB; the other four about the same): std's precompiled code and the C code (ring, libopus) keep theirs, and a
test program shows the same (`.eh_frame` 0x51a8 → 0x5120). So the ~5 MB did not come from the flag as specified.
**Design choice for the owner:** I added `scripts/discard-eh-frame.ld`, an implicit linker script
(`SECTIONS { /DISCARD/ : { *(.eh_frame) *(.eh_frame_hdr) } } INSERT AFTER .text;`) passed by build.sh as
`-C link-arg=…`, so the link drops both sections (all five binaries: 0 bytes, theseusd −5.78 MB). The flag stays too
(`478648d3` has the flag, `60ab36ba` the link script). What is lost: `perf --call-graph dwarf`, gdb `bt`,
and eBPF stack walks of a live release daemon, and `Backtrace::capture`/`_Unwind_Backtrace` find no frames; with
`panic = "abort"` and stripped binaries nothing in the daemon unwinds, but anything that forced-unwinds (glibc
`pthread_cancel`/`pthread_exit`) would abort — none is used. Tested with GNU ld here; a build on lld is expected to take
the same script (INSERT is supported) but I did not run it. The comment in build.sh says all this. If the owner wants
only the flag, drop the one `link-arg` line in build.sh (and 5.7 MB of the saving).

Crash path (the owner's condition), on the build with the script: built with
`CARGO_PROFILE_RELEASE_THIN_DEBUG_ASSERTIONS=true CARGO_TARGET_DIR=/tmp/t-crash scripts/build.sh --profile release-thin -p theseusd`
(97,723,024 B, `.eh_frame` 0), copied over `target/debug/theseusd`, and the compiled `tests/crash.rs` run directly:
`a_panic_leaves_a_crash_file_and_the_next_start_reports_it ... ok`. That test asserts the planted panic ends the daemon
with SIGABRT (`status.signal() == SIGABRT`), `crash-socket.json` holds the message "a planted panic at after_serving",
a `crash.rs` location and a non-empty thread, the log prints the panic, the next start reports it in health and a
`server.crashed` row, and a third start still reports it as not its own. (I did not paste a separate manual run; the
test is the run.)

`scripts/repro.sh` was **not run**: it needs two full fresh release builds (about 20 min each here) beside the gate. What
I can say: the compress step is byte-deterministic (two cockpit builds, same sha256), `.cargo` flags are unchanged, and the
linker script path is passed by absolute path but never reaches the binary.

## Step 3: one brotli decoder (db06e7b1)

`theseus-aws-catalog` → `brotli-decompressor = { version = "5", default-features = false, features = ["std"] }`.
`cargo tree -p theseusd -i brotli-decompressor` and `-p theseus-sim -i …` show only v5.0.3 (lopdf, the catalog, theseusd).
The generator's `brotli = "9"` still brings 6 (not in the runtime binaries). `Cargo.lock`: no package added (6.0.1
stays for the generator; theseusd gains edges to brotli-decompressor 5 and, as a dev-dependency, flate2). Catalog
tests: 19 + 10 + 3 passed.

## Step 4: theseus-sim built, not installed (478648d3, 038f87b2, 47242d44)

- `build.sh --installed` prints `theseusd theseus theseus-tui theseus-index`; `--shipped` is unchanged (five). Header
  comment says theseus-sim is built for the size check and benches.
- `scripts/setup.sh`: new `INSTALLED` (four) drives the copy loop and the "match the build" count; `SHIPPED` stays the
  five and still drives the "is it built" check (the reader rule's `every_tool_is_installed_or_run_from_the_tree` holds
  setup.sh's `SHIPPED` equal to build.sh's, which the first gate run caught; fixed in 47242d44).
- Docs: `docs/setup.md` (steps list and the manual `for b in` loop), root `AGENTS.md` "Installing" (kept under 20 KiB:
  20,418 B), `scripts/AGENTS.md`, `crates/theseusd/AGENTS.md`, `cockpit/AGENTS.md`.
- Every user of an installed theseus-sim, as found (`rg theseus-sim` over scripts, bench, docs, install, contrib):
  `bench/h2h/run.py` (`--bin-dir`, default `target/release-thin`), `bench/personality/exam.py` (`--bin-dir` default
  `target/release`), `bench/async/*`, `bench/recall/*` (`THESEUS_RECALL_BIN_DIR`, else the workspace), `bench/self`
  (searches `target/release-thin`, `release`, `debug`), `scripts/chat-bench.py`, `scripts/f10/*` (`--bin-dir`, checks
  the dir), `scripts/conn-watchers.py` and `scripts/gate.sh` (`target/debug/…`): all take a bin dir or default to the
  target copy, so nothing needed the installed one and none was changed. `scripts/user-service.sh` and theseusd's
  install plans never reference theseus-sim. `scripts/repro.sh` builds and compares all five (it reproduces, it does not
  install): unchanged. An already-installed `~/.local/bin/theseus-sim` is left in place by setup.sh, not removed.
- Docs I did not edit (spec, status, design): the spec's install list (Part III items that name `theseus-sim` among the
  installed binaries) and `docs/status.md` should say four are installed; `docs/design/` trim notes should record the
  `.eh_frame` finding above (the flag is not enough; the linker script is what removes it).

## Gate

`THESEUS_KEEL_BASE=432bbbeb CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on head
`47242d44` (the first run stopped at the registry test, the next two when the disk filled with stale incremental
artifacts, which I cleared): door/keel ok (0 findings), fmt, shape, features, clippy, cockpit, test build, reader rule
all passed. **Suite: 6337 run, 6301 passed, 33 failed, 3 timed out, 47 skipped; failed in the suite phase.**
- The 33 failures are the known root-cluster: theseus-sandbox contract (clauses 01–12, egress_18b ×3, exit status,
  scratch, sigterm, "cannot start"), `bench spawn_100`, and theseusd `sandbox` tests (13). No others.
- 3 timeouts, theseus-discord, none touched by this branch (`git diff 432bbbeb -- crates/theseus-discord crates/theseus-core`
  is empty): `tests_task_threads::a_shared_forum_shows_a_shared_places_tasks_and_never_a_private_ones`,
  `…::a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end`,
  `tests_war_room::a_shared_place_refuses_a_share_and_keeps_its_guests_rule`. Rerun alone, the two task_threads pass;
  **`a_shared_place_refuses_a_share_and_keeps_its_guests_rule` timed out again alone at 120 s.** I did not run it on the
  base, so I cannot say it is a pre-existing failure; nothing in this branch reaches it.
- The phases after the suite, run by hand: protocol types unchanged; `theseus-sim bench turn --check --runs 5 --burst 0`
  ok (first-request tokens dm 13,983/15,000, task 6,788/8,000, headless 8,049/8,600); size step by `build.sh` (above):
  growth −5.78 MB, ok. lifecycle/jobs benches skipped by `THESEUS_GATE_NO_BENCH`.
- `cargo deny --offline check`: bans, licenses, sources ok; **advisories FAILED**: RUSTSEC-2026-0367 and a second
  advisory on the `zip` crate (`ZipArchive::extract`), from the local advisory database. `zip` is theseus-files'; this
  branch does not touch it. The gate's own deny phase did not run (the suite failed first).
- Keel: `python3 scripts/keel-guard.py --base 432bbbeb --head HEAD --quiet` printed nothing (rc 0). (Against the clone's
  stale `origin/main` it reports other commits' findings, not this branch's.)

## Proof nothing was lost

| change | evidence |
|---|---|
| cockpit files byte-identical | `every_embedded_asset_decodes_to_its_dist_file`, all 443 files, br and gz |
| every page still loads | `the_cockpit_serves_its_shell_at_the_root…`, `web_ui` ×4 incl. the new one, debug and release-thin binaries |
| identity clients | `the_cockpit_serves_br_then_gzip_then_identity`: plain bytes equal the dist file |
| crash file still written | `tests/crash.rs` against the release-thin + debug-assertions binary with no `.eh_frame` |
| catalog decodes with v5 | `cargo test -p theseus-aws-catalog` 32 passed |
| nothing needs an installed theseus-sim | the `rg` survey above; `setup.sh` registry test passes |
| `Cargo.lock` | no package added or removed; package-lock.json unchanged |

## Left, or uncertain

- The discard link script is beyond the brief; it works with GNU ld here and is the owner's call (see step 2).
- Not run: `scripts/repro.sh`; the release `release` (fat LTO) profile (same flags, not built); the lifecycle and jobs
  benches (owner's machine); musl target.
- The cockpit tsconfig.node.json now includes `compress.ts` so `tsc -b` type-checks it.
- Left alone per the contracts: Discord's copies of `clip` and the date math, herdr.
