# CLOUD REPORT: output-kept (theseus-v73m)

Branch `cloud/20261010-output-kept`, from `main` at 2fd1f654 (store format 26, unchanged). One code commit,
**776f2e5**, then this report. Started 20:16 UTC, finished 21:35 UTC.

## What I found

- When the result cap (`[tools] result_max_chars`, 30,000) cut a job's result, `toolrun::cap` kept the head and tail
  and proc.run's `rest` said "its output is not kept: run it again printing less". The raw output file was deleted
  once the result was written (`remove_job_output`), and the result read only the file's last 4 MiB
  (`MAX_RESULT_READ`).
- `fs_read` stopped its window at `[tools] max_read_bytes` (256 KiB). The core then cut that text at 30,000
  characters, head and tail, so a 60 KB file came back with a hole in the middle.
- **A defect on the way, now fixed:** `fs_window::next_line` honoured the scan bound only where the reader's 64 KiB
  buffer split a line. A window could read past `MAX_SCAN_BYTES` by up to a buffer's worth of whole lines. The old
  test passed only because its bound was the same size as the buffer.

## What I changed (776f2e5)

1. **Kept output.** New `crates/theseus-core/src/outputs.rs` (`Outputs`: paths, keep, sweep, `own_read`) and
   `toolrun/kept.rs` (`ToolRuntime::capped`, the cut line). When the cap cuts a result from `proc.run` (a batch's
   steps included) or from `term.read`, the whole output is kept, scrubbed, at
   `<state>/outputs/<session>/<call>.out`:
   - A job's raw file is scrubbed again in full, in a streaming pass of about 1 MiB whole-line chunks, up to its job
     file's own cap (+64 KiB). It is never just the 4 MiB tail. A private-key block still open at the end of a chunk
     reads on to its END line first, so the scrubber sees it whole.
   - The file is 0600 in a 0700 directory, written to a temporary name and renamed. It is not synced.
   - The cut line reads, for example: `…[3,308 lines (144,975 characters) not shown: they are lines 421-3728 of the
     whole output, kept at <path>; fs_read with offset=421 and limit=2000 reads them]…`. Lines are counted in the
     whole output; `limit` is the gap, at most 2,000.
   - The node's `full_ref` is set to the kept file. That field already existed, so no layout change and no format
     bump.
   - The raw spool file is still deleted.
   - If keeping fails, the cut line says `its whole output was not kept (<why>)`, followed by the tool's own `rest`.
   - In toolrun.rs the additions are a `mod` line, a field, two constructor lines, the `capped` call, and
     `At.session`. The file is now 2,460 lines.
2. **`fs_read` can read it.** The gate's order (`order.rs`) has a new step: an `fs.read` whose every resolved path is
   under the session's own outputs folder is judged with no resources, so the tool's posture decides it. That step
   needs the session, so it is passed in as `At.session` (None for `policy.explain`'s probe). Another session's
   folder, the outputs folder itself, a `..` climb out of it, and the store all still wait for approval
   (tested).
3. **The sweep.** `Core::sweep_outputs` runs on the spool sweep's blocking thread, after serving and hourly, never
   on a turn's path. It deletes:
   - the copies of a session whose state reads `retired`;
   - every copy older than `[tools] outputs_keep_days` (default 7);
   - the oldest copies first while all of them together pass `[tools] outputs_max_bytes` (default 2 GiB);
   - folders left empty.

   Both keys are in the example template. Their defaults are in `config/outputs.rs`. config.rs is at its ceiling, so
   I moved `[tools.web]`'s types into `config/web.rs` and re-exported them. config.rs went from 2,924 to 2,896 lines;
   no ceiling was raised.
4. **`fs_read` never returns a hole.** In theseus-tools, `fs_read_cap.rs` adds `Room`, and the whole-file read and
   the window read now stop at the last whole row that fits both `max_read_bytes` and `FS_READ_MAX_CHARS` less 400
   characters of room. The existing footer then names the next offset.
5. **`fs_read`'s own cap (D6).** A new `Tool::result_max_chars` returns `Some(100_000)` for `fs.read`. The core cuts
   such a tool to a contiguous head at a whole line, ending with `…[N lines (M characters) not shown: lines a-b;
   fs_read with offset=a and limit=n returns them]…`. Every other tool keeps 30,000 characters, head and tail.
6. proc.run's `rest` no longer claims the output is not kept. It now covers only the bytes the job file's own cap
   dropped: "what was dropped is gone: run it again printing less, or …".
7. The theseus-core and theseus-tools AGENTS.md guides are updated: the tool-calls bullet, two invariants, and
   fs.read's cap.

## How I proved it

- **New tests.**
  - `tests_kept.rs` (6 tests):
    - A 4,000-line job with its failure in the middle and a planted secret on line 10: the result is cut, the cut
      line's offset is the first line not shown, and `full_ref` names the file. The file holds all 4,000 lines,
      scrubbed (`[redacted:demo_secret]`), with mode 0600. A second turn's `fs_read` at the middle returns the
      failure line with status ok and no approval. The secret is in no file under the state dir (WAL segments,
      redb, spool, outputs). The secret comes from a file in the work dir, so the call's own input never holds it.
    - The gate's postures for the six paths above.
    - The sweep by retirement, age and total (counts checked).
    - The core's sweep reading a by-hand retirement from the store.
    - `fs_read` of a 60 KB file comes back whole, and of a 300 KB file as a contiguous window from line 1 with its
      footer and no cut; `fs_grep` keeps head and tail at 30,000 with no file.
    - The config keys' defaults and parsing.
  - `tests_read_cap.rs` (3 tests) and `kept.rs`'s unit tests (2).
- **Rewritten, none deleted.**
  - tests_m3: two assertions on the old wording (the job past its output cap, and the late result held open).
  - tests_m3 `a_capped_result_says_what_was_cut_and_the_call_that_returns_it`: its meaning changes with the cap.
    `fs_read` no longer obeys `result_max_chars = 2000`, so it now proves the contiguous window and footer over a
    3,000-line file. It holds 4 assertions for the read, as before.
  - tests_window `a_window_that_runs_into_the_scan_bound_says_where_it_stopped`: the bound is now 32 KiB, so its rows
    stay under the new cap, and it asserts the bound holds within a line, not a buffer.
- **Planted reverts.** Each file was restored and `touch`ed, and `git status` was clean after each.

  | Planted revert | What caught it |
  |---|---|
  | The kept file deleted again | `a_capped_command…` (NotFound reading it) |
  | Scrub skipped for the copy | `a_capped_command…` (`line 10: token invented-secret-…`) |
  | fs_read back to head and tail (Room's character cap off, `capped`'s own-cap branch off) | `tests_read_cap` ×2, `tests_kept::fs_read_of_a_long_file…`, `tests_m3::a_capped_result…` ("no hole": rows 1-17 then 1991-2000) |
  | Sweep ignoring the total | `the_sweep_honours…` ("the oldest went first") |
  | `own_read` off | `only_the_sessions_own_outputs…` (approve) and `a_capped_command…` (the read waited) |
  | Scan-bound fix off | `tests_window` (scanned 55,548 against 32 KiB) |

- **Gate:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 THESEUS_KEEL_BASE=2fd1f65 scripts/gate.sh`.
  - Every phase through the reader rule passed: keel, keel tests, fmt, shape, features, clippy, cockpit, test build.
  - Suite: 3,693 tests, 3,660 passed, 33 failed, 44 skipped. The 33 are exactly the known L1-as-root failures: 20 in
    theseus-sandbox (contract and `spawn_100`) and 13 in theseusd `sandbox::*`, all saying "the daemon runs as root"
    (theseus-pv6i). Nothing else failed and no listed timing test tripped.
  - The later phases, run by hand: protocol types unchanged; `theseus-sim bench turn --check --runs 5 --burst 0` gave
    frames_plain 5/5 and frames_tool 9/9.
  - `cargo deny --offline check bans licenses sources` passed. **Advisories were not checked:** the setup's
    `cargo deny fetch` left no advisory database (only `db.lock`), the same skip the gate itself makes.
- **Keel:** `THESEUS_KEEL_BASE=2fd1f65 python3 scripts/keel-guard.py` gives `keel: ok (2fd1f65..the working tree; 25
  files changed, 0 findings)`. Run without the override it compares against this clone's stale local `main` (a9ad950,
  behind 2fd1f65) and lists 5 findings from main's own commits. I was not allowed to move the local `main`, so I used
  the override.
- **Benches** (debug build, 4-core VM, once):
  - `bench lifecycle --runs 10 --check`: LIFECYCLE OK. p95: cold start 27.7 ms, from the config copy 29.9, clean
    shutdown 22.4, SIGKILL and restart 24.9, binary swap 23.3, cancel 11.9.
  - `bench jobs --class l0`: p50 7.30 ms, p95 14.62 ms. L1 cannot run here as root.

## Live check (this VM)

`/tmp/claude-0/live/live.sh` runs a scratch daemon on a fresh state dir and `theseus-sim fake-model --rules`. The
command prints 4,000 numbered lines with `line 1000: FAILED tide_pools::the_middle_case`.
- Turn 1's result ended with `…[3,308 lines (144,975 characters) not shown: they are lines 421-3728 of the whole
  output, kept at …/st/outputs/ses_…/toolu_fake_…_0.out; fs_read with offset=421 and limit=2000 reads them]…`. The
  file is `-rw-------`.
- The stand-in was restarted with a rule for `fs_read` at that offset and limit. Turn 2's read was `open`, returned
  97.3 KB (under the 100,000 cap), and showed `  1000\tline 1000: FAILED tide_pools::the_middle_case`.

**For the maintainer**, on the install build: start a scratch daemon with `--config`, `--state-dir` and `--socket` of
its own, `[model] api_base` pointed at `theseus-sim fake-model --addr 127.0.0.1:<port> --rules r1.json`, where
r1.json is `[{"when":"run the roster","calls":[{"name":"proc_run","input":{"argv":["bash","<dir>/printer.sh"]}}]}]`
and printer.sh is the loop above. Then:
1. `theseus --socket S ask "run the roster"` and `theseus --socket S --json history <sid>`: the result ends with the
   cut line above, naming `<state>/outputs/<sid>/<call>.out`, a 0600 file of 4,000 lines.
2. Restart the stand-in with `[{"when":"what failed","calls":[{"name":"fs_read","input":{"path":"<that
   file>","offset":<offset>,"limit":<limit>}}]}]`, then run `theseus --socket S ask -s <sid> "what failed?"`. The
   read is not held for approval, and its result shows the FAILED line.
3. With a real model and the owner's config, the default `approve_paths` includes `~/.theseus`, which holds the
   state dir. Check that the read still runs without a card (`own_read` judges it as inside the roots), and that
   `fs_read ~/.theseus/store/…` still asks.

The repository's bench has no "capture rig S13" that I could find (`grep -r S13 bench docs/design` finds nothing), so
there is no S13 run to give.

## Left open and design choices for the owner

- **FAST, the copy's cost on the turn's path.** The copy is scrubbed and written inside the result's node
  (`theseus_store::blocking`), before the raw file is deleted. On the live check, a job that printed 60 MB (its job
  ran in 94 ms) took a turn of 8.7 s. Almost all of that is scrubbing 60 MB in a **debug** build, about 7 MB/s.
  - A 200 KB log costs milliseconds.
  - Release will be several times faster but will still cost about a second at 64 MiB. Please measure on the
    install build.
  - If that is too much, the fix is to write the copy after the node, on the blocking pool, and delete the raw file
    there. The cut line needs only a newline count of the unread head, which is cheap. I didn't do it because it
    reorders deletion against the spool's sweep and the wrapper-lives check.
- **Line numbers** are exact when the scrub keeps line breaks. A multi-line private-key block collapses to one
  `[redacted:private_key]` line in both the result and the copy, but for an output over 4 MiB the count before the
  read tail is taken from the raw file. A key block before that point would shift the numbers by its lines.
- **Visibility:** the sweep logs a `tracing` line but writes no ledger row and no metric. A `outputs.swept` row
  needs a new `LedgerKind` and fact; I left that out to keep the change small.
- **Scope:** only `proc.run`, its batches, and `term.read` keep output. Other tools (MCP, web) keep head and tail
  and no file.
- **`fs_read`'s cap** is fixed at 100,000 and ignores `result_max_chars`, so an operator who lowers
  `result_max_chars` does not lower `fs_read`.
- **Docs for the maintainer to write:**
  - The spec's Part III item.
  - In `docs/status.md`: fs_read reads by contiguous window to 100,000 characters, a cut command's output is kept
    and named, and the new `[tools]` keys.
  - The tools note (`system_note_for`) could tell the model that a cut proc_run names a file to `fs_read`. The cut
    line already says so.

## Keel findings expected

None. `keel-guard.py` against 2fd1f65 reports 0 findings. No ceiling, cap or budget moved, and no test or assertion
was removed.

## Gate result

Green, but for the 33 known L1-as-root failures listed above. The phases after the suite were run by hand and passed,
with deny's advisories skipped for lack of a database.
