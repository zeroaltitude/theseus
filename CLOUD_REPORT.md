# CLOUD_REPORT: job-truths (theseus-53ujg.2), relaunch r2

Branch `cloud/20261021-job-truths-r2`. Code commits: `a55e5b6` (kernel: the command off the command line, test helpers),
`79afadb` (fs_read kinds, file_read's path miss, the unknown outcome's signal). Store format unchanged (no stored field
added: the signal rides the existing free-text `producer`). No new dependency.

## 1. The command off the command line

**Found.** A job's text sat on the wrapper's command line (`-- <argv>`) and its shell's (`bash -c '<line>'`), so
`pkill -f <pattern in the line>` matched the job's own wrapper and shell.

**Changed.**
- `spawn_detached` writes the argv as a JSON array to `<spool>/argv/<id>` (0600) and passes an empty `--`;
  `parse_wrapper_args` reads the file when `--` is empty (a non-empty `--` still works, as the kernel's own tests use it).
  `Spool::remove_pid` (the wrapper's own end-of-job cleanup) also removes the argv file; `Spool::open` makes `argv/`.
- New `theseus-kernel/src/job_line.rs`: an argv `[bash|sh, -c, line, args...]` is exec'd as `[shell, -c, RUNNER, args...]`
  with the line in `THESEUS_JOB_LINE`; RUNNER is `eval "unset THESEUS_JOB_LINE\n$THESEUS_JOB_LINE"`. Used by `spawn_l0`
  (both the wrapper's `clone3` path, through a new `Exec::with_env`, which builds the child's envp before the clone, and
  the test-thread `Command` path) and by L1's `Spec::new` (`job_l1.rs`). Every other argv runs as before. `shell_argv`,
  `fit.rs` and `--redact` are untouched.
- Test helpers: `gone_jobs.rs::job_of` reads the spool's argv file (found by the wrapper's `--spool`/`--correlation-id`);
  `job_handles.rs::holding` finds a running wrapper by its argv file and a job's shell by `THESEUS_JOB_LINE` in its
  environment, and the stop test now asserts it finds the running job (shell and wrapper) before the stop, so it is not
  vacuous. `tests_term.rs::lives_marked` needed no change: it finds the terminal's own `sleep N.<run>` process, started by
  a pty terminal, not through a job wrapper (the suite passes). `sandbox.rs::with_marker` matches a `sleep` argument, also
  unaffected.

**Accepted differences** (as the brief lists): an error reads `bash: eval: line N:`; the shell stays as the parent of a lone
command (no exec optimisation); `BASH_EXECUTION_STRING` differs; the one-argument limit is 128 KiB less the variable's name.
Also: the line is still in the shell's initial `/proc/<pid>/environ` (readable only by the same user; `unset` does not
rewrite that block), and `pgrep -f` only reads cmdline. The runner text itself is constant.

**Proof** (`TZ=America/Phoenix`, nextest, real `theseusd job-wrapper`).
- New: `theseusd/tests/job_handles.rs`: `a_jobs_own_pkill_f_does_not_kill_it` (via a real scratch daemon; `sleep 30.<nonce> &
  pkill -f ...; wait; echo survived` ends `[exit code 0` with `survived`), `no_theseus_process_shows_a_jobs_text` (no
  `/proc/*/cmdline` holds the token; the shell's cmdline is the runner; the job is found through the spool),
  `an_unknown_outcome_names_the_signal`. New `theseusd/tests/job_line.rs::a_line_keeps_its_semantics` (exit code, `set -e`,
  heredoc, multi-line, quotes and `$`, `$0`/`$1`, `&`+`wait`, empty stdin, the variable absent from the shell and its
  children, `sh -c`, a non-shell argv; every case also checked against plain `bash`).
- Planted reverts: **Plant B** (runner not applied): `a_jobs_own_pkill_f_does_not_kill_it` and
  `no_theseus_process_shows_a_jobs_text` FAIL (`a_line_keeps_its_semantics` passes: it is semantics, as it should).
  **Plant A** (argv back after `--`): the same two FAIL. Each plant alone fails the key test. Restored, `touch`ed, `git status` clean.
- `a_restarted_daemon_still_finds_a_running_wrapper`: no new test; reused as the brief allows. `theseus-kernel` full suite
  (329 tests, includes `tests/children.rs` relearn and `spool.rs`'s cold-restart pid tests) passes after the change, as does
  `theseusd::gone_jobs` (a restart finds live and gone wrappers; its `job_of` now reads the argv file; 6 s), `job_wrapper` (all).
- Totals run: theseus-kernel + theseus-tools 329 pass; theseusd `job_line, job_handles, gone_jobs, job_wrapper, cgroup, reaping`
  29 pass; theseus-core full suite 3582 pass, 24 skipped (includes `tests_term`, the registry test, `tests_m3` frame budget).

**L1: could not run here.** My L1 test, `theseusd/tests/sandbox.rs::an_l1_jobs_own_pkill_f_does_not_kill_it_and_a_line_keeps_its_semantics`
(key test + a short semantics table in one turn), fails here for the known reason (root daemon: "Linux exempts root from
RLIMIT_NPROC", theseus-pv6i), like the 13 other L1 tests in that file. The maintainer should run, as an ordinary user with a job
cgroup:
`cargo nextest run -p theseusd --test sandbox -E 'test(an_l1_jobs_own_pkill_f)'` (expect pass), then plant B (change `"bash" | "sh"`
in `job_line.rs::runner` to never match, `touch` it) and expect that test to fail. Plant A is not an L1 plant: the wrapper is outside
the job's pid namespace.

## 2. An unknown outcome says why
`rpc/driver.rs::wrapper_signalled` passes `wrapper_lost:<SIGNAME>` to `mark_unknown` (new `theseus-core/src/wrapper_lost.rs`, one
`mod` line in lib.rs; names HUP, INT, QUIT, ABRT, KILL, SEGV, PIPE, ALRM, TERM, USR1, USR2, else `signal N`). The `Unknown` arm
of `toolrun/job.rs` calls `wrapper_lost::words(producer)`: `[outcome unknown: the job's wrapper was ended by SIGTERM before it
reported]` for a lost wrapper, today's sentence for every other producer (`reconciler:wrapper_gone_at_start`, a bare
`wrapper_lost`, none). Proof: `wrapper_lost::tests` (the sentences, including the unchanged ones) and
`job_handles::an_unknown_outcome_names_the_signal` (a job's `kill -TERM "$PPID"` through the real daemon). Plant (drop the signal
from the header): both FAIL. The "deadline's unknown keeps today's sentence" is covered by the unit test's other producers; I did
not find a daemon path that produces an unknown from a deadline (a wrapper deadline completes `Failed`, timed out).

## 3. fs_read names a binary's kind
New `theseus-tools/src/magic.rs` (ELF with `PT_INTERP` read from the program headers: PIE executable vs shared object, ET_EXEC,
ET_REL; Mach-O and universal; PE; netpbm P4-P7 with size, P1-P3 stay text; BMP, TIFF, ICO with size; SQLite; gzip, xz, zstd, bzip2,
7z; wasm; Java class; Python 3 bytecode). `fs.rs`'s binary branch (`binary_said`) says e.g. `<path> is a PPM image, 720×400,
864,015 bytes (fs_read shows PNG, JPEG, GIF and WebP images)` / `... is an ELF 64-bit PIE executable, x86-64, 412,608 bytes`;
unknown data keeps today's sentence; meta gains `kind`. Tests: `magic::tests` (4) and `tests_binary::fs_read_names_a_binary_kind`
(P6 under a `.png` name, PIE, shared object, SQLite, unknown bytes). Plant (return today's sentence for every binary): FAIL.
The tool's own description still says "Other binary files are reported, not returned", which stays true.

## 4. file_read's path miss
`file_read.rs::find`: a name containing `/` adds ` (fs_read reads files on disk by path)` to both miss messages. Test
`file_read_miss_on_a_path_names_fs_read` (also a plain name's miss unchanged). Plant (drop the clause): FAIL.

## Live check for the maintainer
On a scratch daemon of this build (non-root user for L1): `theseus ask` a turn that runs `proc_run bash -c "sleep 31.4242 &
pkill -f 'sleep 31.4242'; wait; echo ok"` (expect exit 0 and `ok`); `ps -ef | grep job-wrapper` while a job runs shows the wrapper
with an empty `--` and no command text; `fs_read` on a `qemu` screendump or `/bin/ls` shows the kind line.

## Gate
The full `scripts/gate.sh` was **not run**: this VM's disk allowance (about 39 GB) cannot hold the all-targets test build of the
workspace (it ran out of space twice; I freed `target/debug/incremental` and the test binaries, and ran by package). Run by hand
instead: `cargo fmt --all --check` ok; `cargo clippy -p theseus-kernel -p theseus-tools -p theseus-core -p theseusd --all-targets`
clean after fixing two lints of mine (`is_multiple_of`, `spawn`'s length); `scripts/shape.sh` ok; the suites above. Not run:
`cargo deny` (its advisory db is not fetched: `cargo deny fetch` was not run offline-ready), the cockpit build/protocol regeneration
(no protocol type changed), and the benches. Failing here and not mine: 14 `theseusd::sandbox` tests (the L1-as-root refusal, my new
L1 test among them, same reason). `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing.

## Docs the maintainer should touch
Part III item for job-truths; `crates/theseus-kernel/AGENTS.md` (`job.rs`: the argv in `<spool>/argv/<id>`, `job_line.rs`'s
runner); `crates/theseus-tools/AGENTS.md` (`magic.rs`). A design point for the owner: the shell stays the parent of a lone
command now, so a `pkill`-by-name of the job's own program still sees one more process (its shell) in the tree.
