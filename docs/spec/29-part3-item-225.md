# The Ship of Theseus, chapter 29: Part III, A4's Items 225 to 233 ([index](README.md))
### Item 225. Flake causes: five tests that failed under load are fixed at their causes (a close's test looks only for its own run's sleeps; the lag prove overflows a cap the test sets; the pool-thread test starts its worker first; a relearn reads a child still in its exec again, bounded at 500 ms; the catalog's decode is timed by the thread's CPU time), with R36's join fix making `Push::backlog_cap` test-only (theseus-d006, theseus-0u6g, theseus-1g8j, theseus-r4hn and theseus-rnl3; the tenth cloud batch's flake-causes session, launched by the DM thread at 12:37 and fired 2026-10-06 12:44 from d279767f, Opus 5.5, its report at 14:29; 43603a76, d83019a6, 4b5fabc0, fb4045a2, c16cdbd0 and 4561f91e; reviewed 15:48 to 17:51 by local reviewer R36, the fs stack, and accepted at 18:07, its join fix written 18:09 to 19:03; joined 19:26 at fc612103, a signed merge onto e8120522, the first of the stack's two merges under one lock and one gate, by the batch-10 joiner; installed 2026-10-06 22:13 at 02de4b70, install #9)

**Why.** Five tests that failed under load, in gates or in reviewers' suites:
- **theseus-d006** (P2; R28, in a loaded whole-suite run): `term::tests::a_close_leaves_no_child_behind` failed once,
  "a child outlived its terminal": its marker scan is machine-wide, or a child survived the close.
- **theseus-0u6g** (P2): `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` passed nextest's
  120 s kill under the load recipe, on main as on queue-frames.
- **theseus-1g8j** (P3; two batch-9 sessions' gates): the learning tender's
  `a_pool_thread_started_from_the_idle_thread_keeps_its_policy` read policy 0 under load (3 of 15).
- **theseus-r4hn** (P2; a gate under load): theseus-kernel's
  `a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child` classed the stand-in tender as an orphan (tenders 0,
  orphans 2).
- **theseus-rnl3** (P2; R27, reviewing voice-turns): the catalog's `one_service_decodes_in_under_5_ms`, a wall-clock
  best of five, took 55 to 68 ms against its 50 ms debug bound.

**What landed** (the merge with its join fix 13 files, +327 −61, without the cloud files: theseus-core's
`term/tests.rs`, `push.rs`, `rpc/server.rs`, `outbound.rs`, `tests_push.rs`, `learning/tender.rs` and AGENTS.md;
theseus-kernel's `children.rs`, `tests/children.rs` and AGENTS.md; theseus-aws-catalog's `tests/catalog.rs` and
Cargo.toml; Cargo.lock's one `libc` line in theseus-aws-catalog's dependency list, no package; no protocol type, config
key or store format change (23 stays 23)).
- **d006, cross-talk** (43603a76, the test only). The sleeps' seconds carry the run's pid, seven digits wide (`sleep
  4343.0012345`; pid_max is 4,194,304, so no run's marker is a prefix of another's), and a failure prints each
  survivor's pid, ppid, session, pgrp, state, start, cgroup and command line (`described`). On the owner's machine,
  gates from several worktrees run at once, and another tree's run of this test, starved between starting its sleeps
  and closing them, matched `marked("4343")` while it lived (so did a sleep an earlier run leaked, which lives 72
  minutes). `Pty::close` is untouched: the session wrote a rescan for the fork window it suspected (a `setsid` child
  forked between the close's last scan and its SIGKILL), but no test failed without it, so it was left out (a fix
  starts from a failing test).
- **0u6g, the lag prove's work** (d83019a6). The time was the work, not a wait: idle, 400 opens took 0.38 to 0.46 s;
  under load 7.5 to 37 s (20 to 90 ms an open), so 5,000 opens came to minutes at nice 19. `Push::backlog_cap` is the
  cap a new connection takes (`BACKLOG_CAP`, 4,096, by default), read once per connection by `serve_connection`;
  `Shared.capped: bool` became `cap: usize` (a raw channel's `usize::MAX`, which never drops). The test sets 256 and
  opens 400 sessions, asserts the default first, and waits for the board on `Push::feed()` (a watch of the board's
  applied position) instead of a 10 s poll; it still proves the rule through a real connection (fill to the cap, drop
  and count, drain, one `events.lost` naming the stream, a re-snapshot equal to a fresh client's, health's count), and
  the rule at 4,096 itself is held by `outbound::`'s test and `tests_push_once`. **R36's join fix** made the field
  `#[cfg(test)] pub(crate)`, its default test-only too, and `serve_connection` reads it only in a test build,
  `BACKLOG_CAP` otherwise: a release build's connection takes the 4,096 of design stage2 §3.2 with no knob, as on main
  (+20 −13 in `push.rs` and `rpc/server.rs`).
- **1g8j, the pool thread** (4b5fabc0, the test module only). tokio 1.53.1's blocking pool pushes a task to the back
  of its queue and, with no pool thread idle, spawns a thread from the caller; a probe variant failed 10 of 40 under
  load, each "probe ran on <tid> policy 0; worker thread <other tid> policy 5": the SCHED_IDLE thread started from the
  idle thread took the worker's task. `started()` now spawns and awaits a task on each fresh runtime, so the worker
  holds its pool thread before anything else is queued, and `probe()` returns the thread id with the policy, each
  assertion naming the probe's, the worker's and the idle thread's thread. Both halves keep their meaning.
- **r4hn, a relearn mid-exec** (fb4045a2, the product path). A probe read `/proc/<pid>/cmdline` empty right after a
  spawn returned 333 times in 2,000 idle and 1,991 in 2,000 under load. After an exec restart, a tender read mid-exec
  stayed an orphan for the image's whole life, and the supervisor started a second tender, which exits 3 on the
  first's lock. `relearn` now classes each child with `learn(read, wait)`: an empty command line of a live child is read
  again every 1 ms (`in_exec`), up to `EXEC_WAIT` = 500 ms for the whole relearn (one deadline, taken at the first
  wait); a line still empty at the bound is an orphan as before, or a zombie if it became one; a gone process is
  `Other`. relearn runs in theseusd's `adopt_children` before the runtime is built, so the sleep holds no worker (it
  holds the registry lock, while nothing else runs). 500 ms is about six times the 84 ms window `job.rs` measured for a
  starved child, and only an empty command line waits. A by-order unit test with a command-line reader the test
  controls; `tests/children.rs` spawns the stand-in tender last; one line in the kernel's AGENTS.md.
- **rnl3, the decode's clock** (c16cdbd0). The decode is timed by `CLOCK_THREAD_CPUTIME_ID` in both builds, with the
  same bounds (50 ms debug, 5 ms release), both clocks printed; `libc = "0.2"` became a dev-dependency (as nine other
  crates declare it). CPU time proves the pure computation's cost; a decode that waited would pass, and the catalog
  neither locks nor reads.
- **4561f91e**, theseus-core's AGENTS.md: the run's marker and the test's backlog cap.

**How it is proven.**
- **The session** (a 4-core VM; each test's binary run alone in a loop, "under load" the AGENTS.md recipe): d006, 0
  of 30 under load after (main had not failed either; beside one stray `sleep 4343` main's test failed 3 of 3 and the
  new one passed 3 of 3); 0u6g, 0 of 30 under load (8.2 to 41.5 s; idle 0.51 to 0.57 s); 1g8j, main 5 of 30 failed,
  the branch 0 of 30; r4hn, 0 of 30; rnl3, main 2 of 30 failed ("ec2 took 680.7ms, over 50ms"), the branch 0 of 30
  (ec2 9.5 to 11.7 ms on the CPU, 557 to 820 ms on the wall). Its five plants failed their tests: `holders` out of both
  of `Pty::close`'s scans (the test names the survivor, a `sleep 4344.<pid>` with ppid 1 in a session of its own), a
  dropped notification not counted, the fault's shape in 1g8j's second half (SCHED_IDLE, `left: 5, right: 0`), the
  re-read removed (the unit test fails; the integration test still passed, so only the unit test guards it), and each
  decode done nine times. theseus-core's `term::`, `tests_push::` and `learning::` tests 34 of 34 five times under load;
  theseus-kernel and theseus-aws-catalog 191 of 191; the gate red only on the 33 known L1 cases (theseus-pv6i).
- **The review** (R36, the tree cloud-b10fs; review commit 33112cc2 on ea34457e): fmt and clippy `-D warnings` clean;
  targeted on the stack, **237 of 237**; **the whole workspace suite on the stack, 3,020 of 3,020**, its first three
  minutes beside 16 busy loops. The report's five plants caught; R36's R1 (a connection given `BACKLOG_CAP` instead of
  the field) caught at once ("the cap dropped some"); R36's R3 (`in_exec`'s deadline planted at `now`) **not caught**
  (theseus-46qs). **Under the report's recipe on the owner's 16-core machine** (nice 19 beside 16 busy loops): 1g8j,
  main **7 of 179 failed**, the branch 0 of 200; rnl3, main **2 of 50 failed** (`sagemaker took 72.3ms`), the branch 0
  of 18 (CPU at most 9.1 ms, the wall up to 227 ms, so a wall bound would fail there); 0u6g, main **1 of 3 hit the
  125 s kill** (the others 78.9 and 104.6 s), the branch 0 of 30 (4.4 to 39.7 s); r4hn, 0 of 200 on both (its fix
  argued, as in the cloud); d006 beside one stray `sleep 4343` and `sleep 4344`, main 3 of 3 failed, the branch 3 of 3
  passed. d006's 200 loaded runs were not run: main's marker matches any process holding those digits, so each run could
  fail another tree's run of main's test, d006's own cross-talk.
- **The close's fork window is real** (R36's probe R4, no code change): a terminal running `trap '' HUP TERM; while :;
  do setsid sleep 4345.<pid> & sleep 0.005; done`, closed by cancel: 23 alive at the close, **2 left** 3 s later, each
  in a session of its own, adopted by the subreaper (pid 444, not 1 as the report's hint said). Filed theseus-z0kk (P2).
- **The join fix** (R36's addendum): clippy `-D warnings` clean in both of theseus-core's builds (the lib without the
  field, its tests with it); tests_push 10 of 10 (the lag prove 1.2 s), tests_push_once 3, `outbound::` 2 and the
  reader rule's tests_registry 15, all passing; plant F1 (a test build's connection given `BACKLOG_CAP`) fails the lag
  prove, so the test still proves the rule through a real connection at the cap it set.
- **FAST.** r4hn's `EXEC_WAIT` is on the start path. R36's lifecycle A/B (debug daemons frozen to /tmp, `--phases
  cold,kill,swap --runs 10`, A B1 B1 A A B1 in one hold, CPU pressure under 1 %): medians of p50, cold start 28.4 ms
  (main) against 26.7, SIGKILL then restart 30.1 against 31.9, binary swap 59.4 against 60.5: no change, as only a
  child whose command line reads empty waits, and no bench child is mid-exec at a swap.

**What the review found.** theseus-z0kk (P2): ship the report's rescan (descendants and holders after the kill, until a
look finds nothing, bounded at 2 s), with R36's probe as its test; it costs one more `/proc` scan per close, off the
turn path. theseus-46qs (P3): relearn's own wait and its bound are held by no test, and `children::tender_of` (pub) lost
its last caller. For the owner: make `Push::backlog_cap` `#[cfg(test)]`, not a public field (an ambient, mutable knob
on `Core.push` that would change the 4,096 cap for every later connection with nothing at the connection site to show
it); the DM thread had it written as this join fix (18:08).

**The join** (the fs stack's first merge, under the stack's one lock; the queue, the dry run, the warm, the tests and
the one gate are told in Item 226). The merge (19:12:28 onto the discord stack's e8120522): no
conflict (Cargo.lock and theseus-core's AGENTS.md auto-merged); the CLOUD files removed; `flake-causes/joinfix.py`
printed four "applied" lines (the `use` line without `AtomicUsize`, the field test-only, its default test-only, the
connection's cap only in a test build); `git diff --cached --quiet 73c179b6 -- push.rs rpc/server.rs` exit 0 (R36's
own commit of the fix); staged 13 files, +327 −61, the tree ad9319e8, the dry run's R1; the scrub's names family 0. The
signed merge **fc612103** (e8120522 and 923f2338). theseus-d006, 0u6g, 1g8j, r4hn and rnl3 closed with fc612103. In
the stack's gate all five former flakes passed at gate speed (rnl3's decode test 0.10 s, 1g8j's 0.012 s, d006's close
0.84 s, 0u6g's lag prove 0.75 s, where main timed out at 120 s under load, r4hn's sweep 0.27 s). The store stays at
format 23.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). On the owner's daemon only r4hn's change acts: a
relearn after an exec restart reads a child still in its exec again, up to 500 ms for the whole relearn, only while a
child is mid-exec, so a tender caught mid-exec is no longer an orphan for the image's life (R36's A/B: nothing
measurable on the start path). `Push::backlog_cap` is test-only, so the daemon's connections take the 4,096 cap with
no knob, as before. The rest is tests. No config key. Health after the restart (22:13:05): `theseusd check` exit 0, 9
secrets ready 1.12 s after the start, startup serving at 61.6 ms (config 4.5, store 8.9, kernel 13.0 ms; at load 19 to
21, over the recipe's 60 ms), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`,
`rerank.v1`), memory live on the `baseline` arm, voice ready (`0 resumed`, rejoins 0, `deaf_failed` false), Discord
ready, the unit active with NRestarts 0, and no error or warning in the journal; no config change, the store at format
23, sessions 21,785 with the lists in low milliseconds, and durability caught up at position 143,096 from 22:13:18
(12 s). The old daemon's stop took 11.87 s (install #8's: 298 ms), with nothing logged between (theseus-vjn7, P2).

**Divergences.** d006's cross-talk is shown by its mechanism, not by a failure of main's test, and its rescan was not
shipped (now theseus-z0kk). r4hn was fixed in the product path, not only in the test, since the test is shielded by
its own order. rnl3 times release builds by CPU time too. `Push::backlog_cap` joined test-only by R36's join fix,
where the branch made it public.

**Known gaps.** theseus-z0kk (P2: ship `Pty::close`'s rescan with R36's probe as its test); theseus-46qs (P3:
relearn's bound untested; `children::tender_of` without a caller). Owed by the review and not written at the join: the
status page's line for the five tests, and the known-flakes list in the cloud preambles, which loses them.

### Item 226. Scrub encodings: the scrubber withholds a value JSON-escaped twice, in YAML's and Python repr's escapes, and as base64 wrapped by escaped line breaks; with R36's join fix, `base64_runs` skips only the letters of `\n`, `\r`, `\/` and `\u`, so a value's base64 right after a lone backslash is withheld again (theseus-nlvx and theseus-cjyt, with theseus-g88t by the join fix; the forms Item 204 left open; the tenth cloud batch's scrub-encodings session, launched by the DM thread at 12:37 and fired 2026-10-06 12:44 from d279767f, Opus 5.5, its report at 14:22; 6ef9243e, 196c8656, 28a1e11f and 297ea919; reviewed 15:48 to 18:01 by local reviewer R36, the fs stack, on flake-causes' review merge, and accepted at 18:07 with one security regression to fix at the join, its join fix written 18:09 to 19:03; joined 19:26 at 05b36e06, a signed merge onto fc612103, the second of the stack's two merges under one lock and one gate, by the batch-10 joiner; installed 2026-10-06 22:13 at 02de4b70, install #9)

**Why.** scrub-escaped (Item 204) decoded a tool output's JSON escapes once and matched every board value in the
decoded text. It left three forms open:
- **theseus-nlvx** (P2): a value escaped twice, as JSON inside a JSON string, keeps `\"` and `\\` after one decode, so
  a value holding a quote, a backslash, a control character or (under an ASCII-only encoder) any non-ASCII character
  got through; so did YAML's and Python repr's escapes (`\xNN`, `\U`, `\'`).
- **theseus-cjyt** (P2): `base64_runs` found a value's base64 in the raw text only and ended a run at the backslash of
  `\n` (or `\/`), the `n` beginning the next run, so base64 wrapped by escaped line breaks inside a JSON string passed
  (GitHub's contents API prints this; so do `encodebytes` in JSON and a PEM in JSON); and a match on a line after an
  escaped break left a stray backslash before its marker, which broke the JSON.

**What landed** (theseus-core only; the merge with its join fix 6 files, +633 −43, without the cloud files: `scrub.rs`
(765 lines), `scrub/escaped.rs` (254), `scrub/tests_escaped.rs` (`scrub_cost` only), the new `scrub/tests_nested.rs`
(364), `tests_outside_text.rs` and the AGENTS.md Secrets line; `Scrubber::scrub`'s signature unchanged, so no caller
changed, and the judge's `ScrubWith` and theseus-discord wrap the same `Scrubber`; no package, protocol type, config
key or store format change (23 stays 23)).
- **A second decode** (6ef9243e, nlvx). `escaped::spans` decodes up to `LEVELS = 2` times, the second only when the
  first decoded text still holds a backslash, stopping when a decode finds no escape; every value is matched at every
  level (so a value holding a literal backslash-n still matches at level 1, where level 2 would read a line break), and
  a match at level k maps back through each level's escapes, innermost first (`start` and `end` composed; R36 checked
  that a level-2 match's ends are character boundaries of the level-1 text). The second buffer is made only for
  outputs whose first decode leaves a backslash. A third level is a constant and a test, if ever needed (JSON inside
  JSON inside a JSON string).
- **YAML's and repr's escapes** (196c8656, nlvx). `escape_at` gains YAML's `\0 \a \e \v \N \_ \L \P`, `\ ` and
  `\<tab>`, the eight-digit `\U` (YAML and repr), repr's `\'`, and `\xNN`. `hex4` became `hex(b, at, n)`, every read a
  bounds-checked `get`, so a cut escape decodes nothing and cannot panic. **How `\xNN` reads:** a run of `\xNN` that
  forms one valid UTF-8 character is that character (a bytes repr's `\xc3\xa9` is `é`); any other `\xNN` is U+00NN, as
  YAML and a str repr mean it (`\xE9`). Its cost: a YAML or str-repr value holding the mojibake pair `Ã©`, written
  `\xC3\xA9`, reads as `é` and is missed; reading both ways would need a third buffer on every output with a `\x` run.
- **Base64 in the decoded text** (28a1e11f, cjyt). The needle search moved into `base64_spans(text, needles, keep)`,
  run over the raw text and over each level's decoded text (there only runs with one of that level's escapes inside,
  the level before having read the rest), each match mapped back through the same composed maps, so the span covers
  the lines it touches with the escapes between them. An escape's letter (after an odd count of backslashes, where
  `escape_at` takes it) no longer starts a raw run, so a match wholly on a line after `\n` has the same span in both
  passes and no stray backslash is left.
- **Its cost** (297ea919). The decoded base64 pass runs only where an escape joins base64 (a line break, or a base64
  character between base64 characters: `escaped::joins`), since ungated it cost +360 to +500 µs on escaped outputs;
  `scrub_cost` gained three outputs (a few escapes, twice-escaped JSON, base64 with `\n` every 60 characters in JSON).
- **R36's join fix** (theseus-g88t, P2, security): `base64_runs`' skip narrowed to `n`, `r`, `/` and `u`
  (`matches!(b[i], b'n' | b'r' | b'/' | b'u') && escaped::is_escape_letter(b, i)`), its doc saying why: no value's
  base64 can begin with one, since its first character comes from a UTF-8 lead byte's top six bits (`A`-`Z`, `a`-`f`,
  `w`-`z`, `0`-`9`); every other letter starts a run, as on main. And R36's probe as a test,
  `tests_nested::a_values_base64_right_after_a_lone_backslash_is_withheld`: four invented values whose base64 begins
  with `b`, `e`, `a` and `N`, after `C:\` and after `key\`, each withheld with the backslash kept and a count of 1, and
  after a space as the control.

**How it is proven.**
- **The session's tests** (`scrub/tests_nested.rs`): tests_escaped's six values in a serialized object inside a JSON
  object, compact, pretty and ASCII-only at both levels; a Secret's `stringData` in kubectl's
  `last-applied-configuration` annotation beside its base64 `data` (a count of 2); a backslash-holding value matched at
  its own level; YAML's escapes inside a mapping; repr's forms (`repr(v)`, `repr(v.encode())` in both hex cases, the
  astral value's four bytes, a `\xNN` run that is no character's UTF-8 read as `Ã(`); base64 wrapped by `\n`, `\r\n`
  and `\n` with PHP's `\/`, in the first line, across the first wrap and wholly in the third line, at three offsets
  each, the expected output exact JSON; `encodebytes` (76) and a PEM in a JSON string (64); and three new ways in
  `tests_outside_text::a_value_never_comes_through` (twice escaped, `\xNN`, wrapped base64). Its seven plants failed
  their tests (`LEVELS = 1`; YAML's arms off; repr's arms off; the UTF-8 run alone off; the decoded pass off, the
  property test's minimal failing input `how = 7`; the escape-letter skip off, one test failing on the stray backslash
  as "invalid escape" re-parsing the output; the `joins` gate forced off). Every scrub, redact or secret test in the
  workspace, 68 of 68; the property tests 20 times with fresh seeds after steps 2, 3 and 4, 20 of 20 each. Each gate red
  only on the 33 known L1 cases.
- **The review** (R36, review commit 5e913169 on flake-causes' 33112cc2): the build clean; the report's selection and
  the judge's (`judge::`, `tests_judge*`), **114 of 114**; **the whole workspace suite on the stack, 3,020 of 3,020**.
  **R36's fixture** (invented values, in the tree only while it ran), on a value with both quotes, a backslash and an
  accent: every form the report names withheld exactly with a count of 1, the text around it intact and the JSON still
  parsing (plain; JSON once and ASCII-only; JSON twice compact, pretty and ASCII-only at both levels; PyYAML's double
  quotes; `repr(v)`; `repr(v.encode())` in both hex cases; `encodebytes` in JSON; GitHub's contents form at all three
  offsets, with `\r\n` and with `\/`); each of the 17 new arms withholds its character, 17 of 17; **no new false
  positive**, 12 texts holding no value unchanged (a Windows path, every arm with no value, nested escapes, wrapped
  base64 of other bytes, a near miss of the value in each form). **Seven plants, seven caught**: the report's five and
  R36's two (a match mapped back through its own level only, four tests; `hex()` reading its digits unchecked, a panic
  on a cut escape caught by four tests, `any_output_scrubs_without_a_panic` among them, where a release build would
  abort).
- **The regression R36 found** (theseus-g88t): the escape-letter skip also skipped `\b`, `\f`, `\a`, `\e`, `\N` and the
  rest, so a value's base64 right after a single literal backslash, starting with one of those letters, was no longer
  withheld where main withheld it: four such values after `C:\` and `key\`, **0 of 8 withheld** (after a space, 4 of
  4); with the skip planted off, the probe passes.
- **Live** (scratch daemons of the frozen debug builds, the stack and main, each on a fresh state dir under a transient
  user unit, with two `file:` secrets written by the file tool, the stand-in model running `proc.run`): on the stack,
  plain, JSON twice, `yaml.dump` (PyYAML 6.0.3), `repr(v)` and `repr(v.encode())`, and `encodebytes` in JSON each came
  back `[redacted:probe]` with no form of the value in the history or the WAL; on main all four new forms showed the
  value (`Pr\xE9be`, both repr forms, its base64). R36's lone-backslash case showed the base64 in the stack's history
  and WAL segment, where main withheld it.
- **The join fix** (R36's addendum): the selection 115 of 115; the probe 4 of 4, the lone backslash withheld 8 of 8
  (the branch 0 of 8), every named form still withheld and its JSON parsing, the 17 arms and the 12 texts as before;
  plant G1, the branch's skip put back, fails exactly the new test (28 of 29 pass); plant G2, no skip at all (main's),
  passes the new test and fails the two wrapped-base64 tests with the stray backslash (`…\[redacted:plain]` where
  `…\n[redacted:plain]` is right), so the four letters still do the branch's job.
- **FAST.** Every tool output passes the scrub, on the turn's path. R36's release-thin `scrub_cost` A/B (64 KB
  outputs, ten values, one hold, the machine settled), µs a call, medians of three blocks: plain text 477 to 479 and
  pretty JSON 1,080 to 1,072 (no change: one scan for a backslash, 2.8 µs); about 7,800 escapes 567 to 588 (+4 %); a few
  escapes 502 to 535 (+7 %); twice-escaped JSON 529 to 595 (+12 %); 67.7 KB of `\n`-wrapped base64 in JSON 3,008 to
  3,248 (+8 %). A 1 MB output costs about 8 ms (plain) to 17 ms (pretty JSON) on main, and the branch adds 0.3 to 1 ms
  to an escaped one, paid once per output. The turn bench (debug, A B2 B2 A A B2) ran under a neighbour's load spike:
  frames 5 and 9 on both arms, nothing pointing at the scrub. R36's cost probe: `base64_runs`' per-word allocation is
  189 µs of a 64 KB plain text's 468 µs scrub (40 %), and one reused buffer brings it to 80 µs; `base64_spans` is 822
  of a pretty-JSON scrub's 1,023 µs (theseus-rlgv).

**What the review found.** g88t above, which the DM thread put in as this branch's join fix rather than after the
join, as a security regression against main (18:07). theseus-d80h (P3): PyYAML folds a long double-quoted value at a
space with an escaped line break that decodes to nothing, so a long passphrase forced into double quotes passes (a
zero-width escape in the map is needed); generated tokens never fold. theseus-rlgv (P3): the allocations above. R36's
calls, each adopted: keep the `\xNN` reading; leave `LEVELS` at 2; fix the folds next. R36's further question (skip
`t` and `v` too, for a clean cut after a JSON `\t`) was declined by the DM thread at 19:04: the value is withheld
either way, the stray backslash before the marker is main's behaviour there, and a wider skip invites g88t's class
back.

**The join** (the fs stack: flake-causes, Item 225, then this branch, each with R36's join fix; one
lock, one gate). The join was the one job the gaming policy released at 19:08 (the owner's game ran from about
18:26; gaming mode on since 18:27:34: one build tree, cargo at 4 jobs). The two branches are siblings, both cut from
d279767f. The queue was clear at 19:08 (the discord stack's done line 18:25:19; main = origin/main = e8120522). The
joiner's dry run before the lock (19:10:17) and inside the take (19:12:22): flake-causes' merge-tree clean, its
`joinfix.py` run by path on the unpacked tree, 4 of 4 applied, R1 ad9319e8 with `push.rs` and `rpc/server.rs` byte
for byte R36's 73c179b6; scrub-encodings' merge-tree onto R1 clean, 2 of 2 applied, R2 d25cab24 with `scrub.rs` and
`tests_nested.rs` R36's aeb69b8f's; no marker; format 23 throughout; the scrub's names family 0 on both merges' added
lines; both branches' files, 18 of 18, R36's. The guarded take (19:12:21, lock `cloud-b10-fs-join`): merge 1 committed
as fc612103 at 19:12:28; merge 2 (no conflict; the CLOUD files removed; `scrub-encodings/joinfix.py` two "applied"
lines; `git diff --cached --quiet aeb69b8f -- scrub.rs scrub/tests_nested.rs` exit 0; staged 6 files, +633 −43, the
tree R2) committed as **05b36e06** at 19:12:30 (fc612103 and 024312d9), both signed, each message naming its fix in
R36's words. The warm (19:12:47 to 19:17:50, the test build 3 m 18 s at 4 jobs, clippy clean); the stack's suites,
**192 of 192** in 41.2 s (theseus-core 146: scrub 21 with g88t's test, secrets 13, tests_outside_text 8, tests_push
10 with 0u6g's in 0.77 s, term 14, learning::tender 4, the judge's 28, outbound 2, the reader rule's 15, tests_output
3; theseus-kernel 5, theseus-aws-catalog 10, theseus-protocol 31, protocol.gen unchanged). The one gate, on 05b36e06
(19:19:09, minute 19, to 19:25:38, ok; no lock wait): **3,041 of 3,041** (1 slow, 24 skipped; the discord stack's
3,032 plus the stack's 9) in 305.8 s, no hour crossed, none of the five former flakes red and no known flake red;
lifecycle ok in one run (cold start p50 31.2 / p95 42.0 ms; from the config copy 34.8 / 45.8; clean shutdown 38.6 /
51.0; a post in flight 92.9 / 96.7; SIGKILL then restart 34.5 / 35.4; binary swap 52.8 / 55.0; restore 281.4 / 300.6;
a cancel's round trip 108.1 / 113.9 against 250); L1 start 11.69 / 14.59 ms; turn frames 5 and 9, plain p50 93.1 ms,
tool call 192.0 ms. The rows stayed at the discord gate's raised level across paths the stack does not touch (cold
start 31.2 against the quiet afternoon's 22.1 to 24.2 ms, L1 start 11.7 against 6.0 to 7.0, restore 281 against 234 to
237): the owner's game on the shared host the likeliest common cause, the joiner's inference, tracked by theseus-4284.
Pushed 19:25:57 (both merges), both branches deleted on origin, done line 19:26:11; theseus-nlvx, cjyt and g88t closed
with 05b36e06; R36's logs copied into the reports tree, then its tree and 33 GB target removed. 14 min from the take to
the done line. The store stays at format 23.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). On the owner's daemon the scrubber withholds a
board value JSON-escaped twice, in YAML's and Python repr's escapes, and as base64 wrapped by escaped line breaks
(GitHub's contents API, `encodebytes` in JSON, a PEM in JSON), and, with g88t's fix, a value's base64 right after a lone
backslash, as before the branch. Its cost: nothing on plain text (one scan), +4 to 12 % on escaped outputs (R36's
release-thin numbers). No config key. Health after the restart (22:13:05): `theseusd check` exit 0, 9 secrets ready
1.12 s after the start, startup serving at 61.6 ms (config 4.5, store 8.9, kernel 13.0 ms; at load 19 to 21, over the
recipe's 60 ms), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`),
memory live on the `baseline` arm, voice ready (`0 resumed`, rejoins 0, `deaf_failed` false), Discord ready, the unit
active with NRestarts 0, and no error or warning in the journal; no config change, the store at format 23, sessions
21,785 with the lists in low milliseconds, and durability caught up at position 143,096 from 22:13:18 (12 s). The old
daemon's stop took 11.87 s (install #8's: 298 ms), with nothing logged between (theseus-vjn7, P2).

**Divergences.** The `joins` gate on the decoded base64 pass was the session's own, after measuring the ungated pass.
The `\xNN` reading is a choice (a valid UTF-8 run is one character). The branch's escape-letter skip was wider than its
job and opened g88t; the join fix narrowed it to the four letters, with the probe as its test; `\t` and `\v`, which
cannot begin a value's base64 either, were left out on purpose. g88t's description also asked for an arm in the
property test; not added, since that test's value's base64 begins with `S`, which no escape takes, and the new test
holds the case exactly.

**Known gaps.** theseus-d80h (P3: PyYAML's folded strings); theseus-rlgv (P3: `base64_runs`' per-word allocation,
about 110 µs of every plain output's scrub, and `base64_spans`' per-run string). No third decode level. Owed by the
review: the spec's list of scrubbed forms (§3.19), written in this version's Part I amendments; the status page's
line for the new forms.

### Item 227. Bench bounds: the recall driver refuses a daemon more than 50 tokens off its plan either way and plans the default at 13,640; the retraction rule's misreadings are fixed, with R38's join fix that the last naming decides; and the sampler's cost bound holds on the owner's host, its namespace bound improved but still open (theseus-qryz and theseus-tqa3, with theseus-ufe5 improved and left open; R25's findings on bench-recall-plan (Item 200) and the sampler bounds Item 199 re-derived; the tenth cloud batch's bench-bounds session, launched by the DM thread at 12:37 and fired 2026-10-06 at about 12:44 from d279767f, Sonnet 5.5, its report at 13:57; 67ca9021, 946deb1c, fdc26302 and 4d3b0770; reviewed 17:42 to 19:17 by local reviewer R38, and accepted at 19:33 with a join fix; joined 20:00 at fc59e7f6, a signed merge onto 05b36e06, by the batch-10 joiner; installed 2026-10-06 22:13 at 02de4b70, install #9, bench/ only)

**Why.**
- **theseus-tqa3** (P3; R25, reviewing bench-recall-plan, Item 200): the recall driver refused a daemon more than 50
  tokens *over* its progression's planned overhead, but a daemon under it was unchecked (smoke seed 12 stops crossing
  its mark 150 under), and `plan_misses`' cushion check was untested.
- **theseus-qryz** (P3; R25, the same review): `--stale retracted` still scored four reply shapes right that give the
  old value (`the old X`, a quoted move), scored two-old-value replies wrongly stale, and its new-value half
  (`new_free`) was untested.
- **theseus-ufe5** (P3): two of `bench/harbor/test_sampler.py`'s bounds, re-derived on the cloud's 4-core VM by
  theseus-99by (Item 199), SamplerCost's 2.1 times a bare `stat` read and InANamespace's 6.5, failed on the owner's
  16-core host beside other trees' Rust builds; a rerun alone passed, so the tests were at fault, not the sampler.

**What landed** (bench/ only, Python and Markdown; the merge with its join fix 9 files, +310 −88: harbor's
`sampler.py` and `test_sampler.py`; recall's `README.md`, `drive.py`, `generate.py`, `score.py` and their tests; no
Rust, so no store format, protocol type, config key or package change (23 stays 23); nothing outside bench/;
`bench/theseus-bench.toml` untouched).
- **A daemon under its plan** (946deb1c and fdc26302, tqa3). `plan_misses` checks each plan at its overhead and at 50
  either side (`OVERHEAD_CUSHION`); the driver refuses `|measured − planned| > 50`, naming the direction, both numbers,
  the gap and `--overhead <measured>`; `--allow-overhead` still runs on. Today's daemon measured 13,599 in the cloud,
  101 under the old default, so the live stand-in tests refused the default plan: `OVERHEAD_TOKENS` went **13,700 to
  13,640** (the session's call, for the owner), and every default progression's digest moved (the recorded overhead is
  in the digest; smoke seed 7's pin `SMOKE_7` repinned); a file written earlier keeps its recorded overhead. Two tests
  (a plan holds at the cushion over and under its overhead; a daemon 51 under refused, 50 not), and the live test bounds
  the daemon from below too.
- **The retraction rule** (67ca9021, qryz; `score.py`, strict untouched). `the old` and `the former` govern only with
  a retraction elsewhere in the clause; a quote after a citing verb cancels the prefix (a quoted move is no
  retraction); `no longer wrong` is no suffix retraction; a coordinated list (`or`, `and`, `then`, `and later`, a comma)
  shares its phrase, a prefix flowing to later members and a suffix to earlier ones; `then`, `later` or `next` "from X"
  is a prefix; "earlier it was X" and "before the move it was X" bracket like "was X before"; "X are both retired".
  Two tests (every reply in the brief under both rules; the new value must stand free). **R38's join fix:** the
  branch's "same value named twice" arm retracted the old value where any naming in the clause was governed, so four
  of R25's own re-assertion replies, wrong on main, scored right ("It moved from 27340 to 38013, then back to 27340.");
  now the old value is retracted where its **last** naming is governed, an earlier naming must be governed too unless
  it is a move's destination (so "from 11111 to 27340, then from 27340 to 38013" holds), `WAS_EARLIER`'s lead word
  reaches four words as every phrase does (`REACH`), eight test replies join the careful-reader test, the README states
  the rule as fixed with three false-right shapes nothing narrows yet, and bench-pi's README line "13,700 for the
  smoke" became 13,640 (the one semantic conflict).
- **The sampler's bounds** (4d3b0770, ufe5). SamplerCost holds the sampler under `SAMPLER_UNDER_F5` = 0.85 of F5 (the
  sampler plus every `cmdline` read), timed in the same interleaved loop, where it was 2.1 times a bare `stat` read
  (the cloud VM: 0.70 to 0.73, a 17 % margin); the teeth test times a second F5 pass (`plant`) and F4 against the same
  bound. InANamespace's floor is the mean of `stat` passes taken for as long as the sampler runs (it was the least of
  40 early passes), and `NAMESPACE_RATIO` went 6.5 to 3.5 (the VM: 2.48 to 3.23). `sampler.py`'s head now says a child
  read alive before its parent reaps it has its whole time counted twice (160 for a true 80), with a `Classes` test.

**How it is proven.**
- **The session:** SamplerCost and InANamespace ten times quiet and ten at nice 19 beside four busy loops, no failure;
  the plant (`wants_cmdline` always true) failed each 3 of 3. The retraction rule's plants each failed their tests
  (`new_free` dropped, the `the old` gate off, the coordinated arm off, the negated-suffix guard off, the quote cancel
  off, then-from off, was-earlier off, `both` off, the same-value share off); the cushion's four plants failed theirs.
  Suites under Python 3.11 and 3.12: harbor 70, recall 73, report 9 (the cloud VM has no Harbor). The gate red only on
  the known L1 cases.
- **The review** (R38; the tree cloud-b10bounds, no Rust built; review commit 4ecdfdb9, the join fix on the merge
  98c71a1c, on 938a8dc8; spend $0, every drive on bench/recall's counting stand-in): the suites before (main) and
  after, under host Python 3.14.4 and Harbor's venv 3.13.12, harbor 89 to 90, report 36, recall 71 to 75, async 44, all
  passing, recall and async running their live parts on frozen copies of main's debug binaries.
  - **tqa3:** every bound `plan_misses` checks is monotone in the overhead, so checking planned ± 50 covers the driver's
    closed interval. **Today's daemon: 13,611 tokens** (install #8's release binaries and main's debug build alike;
    the README's recipe, refused after one turn), 29 under the new default, 89 under the old, which the new check would
    refuse. At 13,611 the under check re-plans one progression of 51 (full seed 8, whose first mark main's plan crosses
    by 42 tokens and misses by 8 at 13,561). Explicit-overhead digests are unchanged, so R25's published plan is
    `--overhead 13700`. Live, offline (a scratch daemon of install #8's build): −150 and −51 refused after one turn,
    +51 and +150 refused, each naming both numbers and "generate it again with --overhead 13611"; +50 ran all 30 turns,
    compacting after the mark. Six plants, all caught. The three published smokes rescore byte-identically.
  - **qryz:** 66 hard replies scored under both rules (R25's 39, the brief's 3, R38's 24): false rights 9 on main, 11
    on the branch, 4 with the join fix; on R25's 39, 4, 4 and **0**; false stales 17, 6 and 6. Strict never scores one
    right. Fifteen plants, twelve caught; the three missed (`the old`'s own arm, `later`/`next`/`after that` from, the
    unreachable curly quotes) and three remaining false-right shapes are theseus-hau2.
  - **ufe5** (188 sampler runs, each started by what the machine was doing): **SamplerCost is fixed**: 119 of 120
    unplanted runs passed, where main's 2.1 failed even quiet (1.83 to 2.18 against 2.1). **InANamespace at 3.5 does
    not hold here**: 11 of 40 runs with no compiler running failed (to 4.41), 5 of 40 beside builds; the new teeth
    test, two identical F5 passes held within 15 %, failed 6 of 40 beside builds; the report's plant escaped SamplerCost
    once beside builds. Like for like: with no compiler running 11 of 40 runs fail against main's 4 of 15, and beside
    builds 11 of 40 against main's 14 of 20. Why: each pass's least is the luckiest round, and two passes get different
    luck; InANamespace's sampler runs in another process, with nothing to pair; and SamplerCost's ratio cannot see cost
    added in code the sampler shares with F5 (a `status` read, a second `stat`, classifying twice). R38's proposals for
    ufe5: a deterministic read count in the fixture, SamplerCost on the median of per-round paired ratios, the teeth
    test without its duplicate pass, and InANamespace against an F5-variant sampler beside the real one; change no
    constant now (4.5 would end the quiet flake here and stop the cloud VM catching its plant).
- **FAST.** Not touched: Python and Markdown under bench/, nothing on the start or turn path.

**The join** (batch 10's bench-bounds, alone; the batch-10 joiner's seventh job, released at 19:33 under the gaming
policy). The queue was clear at 19:34 (the fs stack's done line 19:26:11). The dry run before the lock (19:34:48): the
merge-tree clean (recall's README.md, drive.py and test_drive.py, changed on both sides by bench-pi, auto-merged),
R38's `joinfix.py` "fixed" seven edits (score.py 2, README.md 3, test_score.py 2), the resolved tree 9333c17b whose
bench/ tree cca3fa50 is R38's review commit's, byte for byte; no file outside bench/; format 23; the scrub's names
family 0. The guarded take (19:35:57, lock `cloud-bench-bounds-join`, which batch 11's recall-fair waited on): the
merge, the CLOUD files removed, the join fix's seven edits, the staged bench/ tree R38's; staged 9 files, +310 −88.
The signed merge **fc59e7f6** (05b36e06 and 20cc1c47), 19:36:01. The warm (19:36:14 to 19:36:46: theseusd relinked
for the commit) and main's debug binaries frozen for the suites; bench's suites (19:37:02 to 19:45:27, load 5.7 to
9.4): under the host's Python, harbor 90 (15 skipped), report 36, recall 75 (its live test, which wants the daemon at
13,590 to 13,690, passing on the merged main's build), async 44 (7 skipped); under the venv the same, but harbor failed
once on InANamespace (3.508 times its floor against 3.5, theseus-ufe5's known rate) and passed 90 of 90 rerun alone,
as the brief said to (noted on ufe5). The gate waited 96 s for a build slot (gaming mode: one tree) and started at
19:48:03 (minute 48), then 284 s for the gate lock behind a review step, to 19:59:40, ok: **3,041 of 3,041** (1 slow,
24 skipped; no Rust changed, so the fs gate's count) in 311.6 s, no hour crossed, no known flake red; lifecycle ok in
one run (cold start p50 31.6 / p95 40.9 ms; from the config copy 32.9 / 38.8; clean shutdown 55.3 / 79.3; a post in
flight 112.2 / 126.8; SIGKILL then restart 49.6 / 60.8; binary swap 78.5 / 96.1; restore 275.7 / 287.6; a cancel's
round trip 110.5 / 123.0 against 250); L1 start 9.16 / 11.66 ms; turn frames 5 and 9, plain p50 94.2 ms, tool call
195.3 ms. The stop, kill and swap rows rose up to about 50 % over the fs gate's with **no Rust change between them**,
which bounds what the machine alone makes of a gate-to-gate difference while the owner games (theseus-4284). Pushed
19:59:53, the branch deleted on origin, done line 20:00:06; theseus-qryz and tqa3 closed; ufe5 (with this join's note)
and hau2 open; R38's tree removed. 24 min from the take to the done line, most of it the suites and the two waits.
The store stays at format 23.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). bench/ only, so nothing on the owner's daemon:
no installed binary, config key, package or store format change. What it changes is how a recall run is planned: a
published run plans at the build's measured overhead (`generate.py --overhead 13611` on that day's build, R38's and
the DM thread's call), with the default 13,640 accepted within the cushion. Health after the restart (22:13:05):
`theseusd check` exit 0, 9 secrets ready 1.12 s after the start, startup serving at 61.6 ms (config 4.5, store 8.9,
kernel 13.0 ms; at load 19 to 21, over the recipe's 60 ms), `cgroup: delegated`, the judge's live packs as before
(`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready (`0 resumed`, rejoins 0,
`deaf_failed` false), Discord ready, the unit active with NRestarts 0, and no error or warning in the journal; no
config change, the store at format 23, sessions 21,785 with the lists in low milliseconds, and durability caught up at
position 143,096 from 22:13:18 (12 s). The old daemon's stop took 11.87 s (install #8's: 298 ms), with nothing logged
between (theseus-vjn7, P2).

**Divergences.** `OVERHEAD_TOKENS` moved to 13,640, a design call the session made for the owner, so every default
digest moved. The retraction rule as joined is the join fix's (the last naming decides), not the branch's (any
naming). ufe5's bounds are an improvement on this host, not a fix: the issue stays open, and a harbor failure in
test_sampler at a join is the known flake (rerun harbor alone, once).

**Known gaps.** theseus-ufe5 (P3, open, with R38's four proposals in its notes); theseus-hau2 (P3: the three uncaught
plants and three false-right shapes, "Use the old port 27340 instead; 38013 isn't up yet.", "It's no longer wrong to
use 27340; …" and "Ignore 11111, 27340 is the live port, and 38013 is only planned."). Owed by the review and not
written at the join: `docs/benchmarks/README.md`'s "more than 50 tokens over it" (now "off it, over or under") and the
recall-smokes report's plan line (`--overhead 13700`), both under docs/benchmarks/ and left to that tree's owner; the
status page's bench line.

### Item 228. Core gaps: a failed routed turn is counted under the model it ran on, and four tests hold what the core already does: the route's kept compile, a call's time by order, the audit's thread by name, the turn's entry box by size (theseus-udzb, theseus-y9p4, theseus-b38m, theseus-fner and theseus-2kyc; the gaps route-tests' session and R29 found (Item 213), core-waits' session (Item 212), R22 on learning-fixes (Item 203) and R20 on turn-stack (Item 192); the tenth cloud batch's core-gaps session, launched by the DM thread at 12:37 and fired 2026-10-06 at about 12:44 from d279767f, Sonnet 5.5, its report at 13:37; ed456e6f, c6853c52, 8c6680d8, 72b8b156 and 9114dcff; reviewed 17:42 to 19:06 by local reviewer R34c, on main plus learned-shadow, and accepted at 19:08 with no join fix; joined 20:19 at 8300824c, a signed merge onto fc59e7f6, the first of the stack's two merges under one lock and one gate, by the batch-10 joiner; installed 2026-10-06 22:13 at 02de4b70, install #9)

**Why.**
- **theseus-udzb** (P3, found by route-tests' session, Item 213, and confirmed by R29): a routed turn that failed was
  counted under the model it was moved from: `count_failed_turn` took the request's pre-routing target, so a failing
  switch or detour was counted under the base model.
- **theseus-y9p4** (P3, the same review): route-tests' `keep_first` clear of the recall drops (theseus-3urn) was held
  by no test; removing it passed every route and recall test.
- **theseus-b38m** (P3, core-waits' session, Item 212):
  `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn` held an instant read to a 20 ms wall bound
  (84 ms once under load).
- **theseus-fner** (P3, R22 reviewing learning-fixes' theseus-bgg5, Item 203): the audit's nice test passed on either
  build under `nice -n 19`.
- **theseus-2kyc** (P2, R20 on turn-stack, Item 192): tests_stack did not catch the removal of `TurnRunner::run`'s
  entry box, though its doc said unboxing a turn overflows there.

**What landed** (theseus-core only; the merge 7 files, +358 −15, without the cloud files: `rpc/methods.rs`, `lib.rs`
(one module line), `tests_audit.rs`, `tests_m3.rs`, the new `tests_route_keep.rs`, `tests_route_model.rs` and
`tests_stack.rs`; no package, protocol type, config key or store format change (23 stays 23)).
- **The count** (ed456e6f, udzb). `count_failed_turn` takes `profile`, `provider` and `model` from the failed turn's
  trace root (`te.trace.attrs`), and the given target only when the turn failed before its trace began. The driver's
  continuation (`ran_on`, the session's `last_target`) goes through the same function. Only the values of the turn's
  own points change, from one label set (`theseus.turns`, `theseus.turn.duration_ms`, the turn's tokens and cost, its
  tool calls; `theseus.provider.errors` from its own set): the trace root's, as a routed success's already were. The
  keys are the same, so the dashboards and the cockpit read what they read.
- **The kept compile** (c6853c52, y9p4). The plant the issue names (`keep_first`'s two lines removed) is an
  equivalent mutant today: `recall_compiled` reads `t.recall.drops` only while `t.recall.pending` is Some, and
  `turn.rs`'s dispatch sets it to None before `plan_and_dispatch`, so no second loop meets the drops; a second compile
  before the call happens only on a switch, which does not go through `keep_first`. So `tests_route_keep.rs` holds the
  pair: two loops (the first answer billed at 34,500 input tokens against a 40,000-token catalog window, a `fs_read` as
  the call), the second loop's compile a compaction on the routed model's window, for `chat` (the first compile kept)
  and `sophisticated` (a switch): loop 0's stored budget names `["recall"]`, loop 1's `["compaction"]`, all three
  requests on the routed model.
- **A call's time by order** (8c6680d8, b38m). The test spawns the turn and, beside it on the test's one thread, a
  task that blocks the thread 1.2 s once a read has run, so every ready result waits that long in the turn's task (each
  span about 1,216 ms, its own time 0 ms): each `duration_ms` must stay under half the stall, and some tools span must
  hold it. The 20 ms wall bound is gone. tests_m3.rs 7,870 lines (ceiling 8,050).
- **The audit's thread by name** (72b8b156, fner): `tests_audit` records each polling thread's name beside its nice
  value, and none may be `learning`; the nice check stays where `base < 19`.
- **The entry box by size** (9114dcff, 2kyc): `tests_stack::the_turns_entry_is_a_box_not_the_turns_future` holds
  `size_of_val(&runner.run(req))` equal to `size_of::<Pin<Box<dyn Future<…> + Send>>>()`, a fat pointer (16 bytes on
  a 64-bit target), computed from the type, so not tied to the toolchain; the stack test's doc says what it holds.

**How it is proven.**
- **The session:** each test's plant failed it: the three fields back to the request's target (`left: "sonnet", right:
  "opus"`); y9p4's pair (each line alone passes, as an equivalent mutant must; both removed fail the chat half, loop 1
  naming `["compaction", "recall"]`); the result's time from the turn's receipt (`[1211, 1212, …]`); `block_on` on the
  low thread (fails at nice 0 and under `nice -n 19`, "a request polled on the low thread", `left: "learning"`); `run`
  an `async fn` again (9,640 bytes against 16; the 1.5 MiB stack test still passed, as the issue said). b38m did not
  reproduce on the cloud VM (40 runs of the old test under the load recipe, 0 failures); after the change, 30 runs under
  load, 0 failures. The gate red only on the known L1 cases.
- **The review** (R34c, R34's tree; review commit c098441e on b177d98a, whose tree is learned-shadow's 57f265f2):
  the build clean; **the whole workspace suite on the stack, 3,032 of 3,032** (tests_route 27, tests_route_keep 1,
  tests_route_model 2, tests_m3 111, tests_audit 5, tests_stack 2, telemetry 62, the golden 3). Nine plants: the
  report's all caught, y9p4's lines alone passing as the report says, fner's at nice 19 too (the name check holds where
  the nice check cannot); R34c's plant of udzb's fallback (a turn failed before its trace began, counted under empty
  names) **not caught** (theseus-knql). **Live, main against the stack** (scratch daemons, route.v1 the one pack on at a
  stand-in Jev answering `sophisticated`, the base profile on a stand-in model, opus behind a provider answering 500,
  an OTLP sink keeping every body): the hard question switched to opus and failed, and the driver's retry failed too.
  On main `theseus.turns` (outcome failed), `theseus.turn.duration_ms` and `theseus.provider.errors` split
  {sonnet, anthropic, claude-sonnet-5-5} = 1 and {opus, broken, claude-opus-5-5} = 1 (the inbound failure is the bug;
  the retry was already right); on the stack all name {opus, broken, claude-opus-5-5} = 2. Health shows a count
  ("provider errors 2" on both), not names, unlike the report's expectation. b38m under light load (beside four busy
  loops, before gaming mode): 20 of 20, the reads' own times 0 ms against the 600 ms bound and all seven spans holding
  the stall (1,273 to 1,306 ms); the planned heavy-load runs (a busy loop per core) were not run once the owner's game
  began at 18:27.
- **FAST.** No code on the turn's or the start's path: `count_failed_turn` runs on a failed turn only, and the rest is
  tests. The stack's lifecycle and turn checks are in Item 229.

**What the review found.** The driver's continuation counts at the session's `last_target`, so it was right already,
except where `route_base` moves a continuation back to the session's own profile (route.v1 no longer acting,
theseus-9yyr, or the owner's live switch), which udzb now makes right too; its test needs a ladder rollback between
the failed turn and its retry, against the driver's backoff, and is filed with the untested fallback as theseus-knql
(P3). For the owner: keep `keep_first`'s clear as a guard (it is dead only because of a line two files away, costs
nothing, and the new test holds the pair); adopted by the DM thread at 19:08.

**The join** (the cb stack's first merge, under the stack's one lock; the queue, the dry run, the warm, the tests and
the one gate are told in Item 229). The merge (20:05:47 onto bench-bounds' fc59e7f6): no conflict, no
join fix; the CLOUD files removed; staged 7 files, +358 −15, the tree b7ced6a3, the dry run's R1; tests_m3.rs 7,874
of 8,050, the core's test modules (92) in order; every file equal to R34c's review commit's; the scrub's names family 0.
The signed merge **8300824c** (fc59e7f6 and ee01ce31). theseus-udzb, y9p4, b38m, fner and 2kyc closed with 8300824c. The
store stays at format 23.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). On the owner's daemon a failed routed turn's
metrics (`theseus.turns`, `theseus.turn.duration_ms`, `theseus.provider.errors`) name the model it ran on, with the
label keys unchanged; nothing else changes for the owner. No config key. Health after the restart (22:13:05):
`theseusd check` exit 0, 9 secrets ready 1.12 s after the start, startup serving at 61.6 ms (config 4.5, store 8.9,
kernel 13.0 ms; at load 19 to 21, over the recipe's 60 ms), `cgroup: delegated`, the judge's live packs as before
(`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready (`0 resumed`, rejoins 0,
`deaf_failed` false), Discord ready, the unit active with NRestarts 0, and no error or warning in the journal; no config
change, the store at format 23, sessions 21,785 with the lists in low milliseconds, and durability caught up at
position 143,096 from 22:13:18 (12 s). The old daemon's stop took 11.87 s (install #8's: 298 ms), with nothing logged
between (theseus-vjn7, P2).

**Divergences.** y9p4's own plant is an equivalent mutant, so the test holds the clear together with the dispatch's
`pending = None`, and the clear stays as a guard rather than being deleted. b38m's flake did not reproduce; the test
was rebuilt on order, not on a wall bound.

**Known gaps.** theseus-knql (P3: udzb's two untested paths, the driver's continuation moved back by `route_base` and
a turn failed before its trace began). Owed by the review: the status page's line; and, optionally, the note that
`keep_first`'s clear is an equivalent mutant today, held with the dispatch's line by tests_route_keep (written here).

### Item 229. Budget upper: a turn's own provider call is reserved on the input estimate's upper bound, as a summary's is; `Core::build` hands memory the pipeline's exporter; and the warm build's stop test holds its waits, not the machine's speed (theseus-ps9i, theseus-xd6l and theseus-9o2o; memory-tests' session and R30 (Item 210), the owner's call that a turn's call take `upper`, and telemetry-tests' answer (Item 211); the tenth cloud batch's budget-upper session, launched by the DM thread at 12:37 and fired 2026-10-06 at about 12:44 from d279767f, Sonnet 5.5, its report at 14:39; 988d524e, 8012730c and b6f68a35; reviewed 17:42 to 19:07 by local reviewer R34c, on core-gaps' review merge, and accepted at 19:08 with no join fix; joined 20:19 at c0ed7ada, a signed merge onto 8300824c, the second of the stack's two merges under one lock and one gate, by the batch-10 joiner; installed 2026-10-06 22:13 at 02de4b70, install #9)

**Why.**
- **theseus-ps9i** (P3): a turn's own provider call was reserved on the input estimate's `tokens`, not its `upper`,
  while a session summary's call had taken `upper` since memory-tests (Item 210). memory-tests' session raised it and
  R30 recommended `upper` at both lines; the owner's call, recorded in §2 at v0.83, was that a turn's own call take it
  too. A provider that counts its input above the byte estimate settled past the reservation.
- **theseus-xd6l** (P3; the batch-9 writer's note, answered by telemetry-tests' session, Item 211, and confirmed by
  R30): `Core::build`'s pipeline branch handed the judge and the stops their telemetry but not memory, where the
  daemon's path (`build_telemetry`) hands all three.
- **theseus-9o2o** (P3; memory-tests' session): `tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits` failed
  under load on main, 5 of 5 (`took` 5.47 to 6.65 s against a 4.5 s bound): its bound counted the fold's own time.

**What landed** (theseus-core only; the merge 10 files, +251 −43, without the cloud files: `turn.rs`, `fact/turn.rs`,
`rpc/mod.rs`, `recall/adjacency.rs`, `lib.rs`, the tests `tests_turn_reserve.rs` (new),
`telemetry/tests_daemon_path.rs`, `tests_activation_pace.rs` and `tests_m3.rs`, and the core golden
`tests/golden/core_output.txt`; no package, protocol type, config key or store format change (23 stays 23)).
- **The turn's reservation on `upper`** (988d524e, ps9i). `turn.rs`'s two expressions, the `reserve` and the narrated
  row's `input_micros`, take `compiled.estimate.upper` (`counted + estimated + ⌈estimated × 40 / 100⌉`, the compiler's
  margin on the estimated part only); `est_tokens` stays the estimate. The narrated line now reads "… for about N input
  tokens and the estimate's margin", since the dollars are on `upper` and the count is not; the core golden's
  `Calling …` lines move by those words alone (it masks digits). tests_m3's by-hand money test, which pinned `est * 2`,
  now reads `estimate.upper`. **The cost, from the code** (Sonnet 5.5, $2 in and $10 out a million, a 128,000-token
  output cap): an append turn of 60k counted and 10k estimated reserves $0.008 more, 0.56 % of $1.42; a cold turn of
  100k estimated $0.08 more, 5.41 % of $1.48; at a 16k cap, 2.67 % and 22.2 %.
- **Memory's exporter** (8012730c, xd6l): `runner.memory.export_to(t.clone())` beside the judge's and the stops' in
  `rpc/mod.rs`'s pipeline branch, one line. The daemon never takes that branch: its `Parts` pass `telemetry: None`,
  since `install_telemetry` builds its pipeline after serving, where `build_telemetry` already hands memory its
  exporter; only `Parts::for_tests` and theseus-discord's test runtime pass a pipeline.
- **The stop test by its waits** (b6f68a35, 9o2o). The unpaced fold's wall time swings from 80 ms to 6 s under
  starvation, and under load the build had often not reached its first pace when the 1.5 s sleep ended, so the old
  test proved nothing about waits. It now waits until `Adjacent::paces()` is 1, holds 1.5 s, begins the stop, and
  asserts that the sum of the build's pace waits (`recall::adjacency::WAITED`, a `#[cfg(test)]` thread-local beside
  `PACES`, three lines in `pace()`, read on the build's thread) is at least 0.9 of the hold and under the hold plus
  three looks: the stop ends the waits whatever the fold's speed. A shipped build computes and returns the same `waited`
  as before (the frozen non-test theseusd carries none of the test symbols).

**How it is proven.**
- **The session:** `tests_turn_reserve.rs`: a new session's first turn reserves `reserve_micros(max_tokens, upper)` with
  `upper > tokens`, and a stand-in billing input 11 % over the estimate with its whole `max_tokens`
  (`Scripted::BilledBy`) settles within the reservation; planted back on `est_tokens`, both fail (reserved 1,307,258
  micros, settled 1,310,258: past it; with the fix 1,318,162). xd6l's
  `telemetry::tests_daemon_path::a_core_built_with_a_pipeline_records_the_retention_gauge` (a pipeline to the receiver,
  `+retention`, the projection built with 2 nodes, the gauge read "2", grown to 3, read "3"); the line removed, it fails
  after 10 s. 9o2o, reproduced on main's test under the load recipe 5 of 5; after the change, 30 of 30 under load
  (`waited` 2.005 to 2.008 s, `took` about 7.2 to 7.6 s); `quiet_blocking(BOUND)` planted, it fails with 20.0 s of waits
  against a 1.5 s hold. The core suite whole, 1,379 passed; `bench turn --check` frames 5 and 9; the gate red only on
  the known L1 cases.
- **The review** (R34c, review commit c06f4968 on core-gaps' c098441e): **the whole workspace suite on the stack, 3,032
  of 3,032** (tests_budgets 3, tests_budget_loop 1, tests_compaction 11, tests_activation_* 16, tests_turn_reserve 2,
  telemetry 62, the golden 3, tests_m3 111). Five plants: the report's three caught (both reserve tests and tests_m3's
  by-hand test, $1.307254 against $1.318156; the gauge test; the stop test), R34c's narrated words caught by the
  golden; R34c's narrated `input_micros` left on `est_tokens` **not caught** (narration only: the line's three dollar
  figures would no longer add up, and the golden masks digits). The cost table recomputes exactly from `compiler.rs`
  and `catalog.rs`.
- **ps9i live** (scratch daemons of main and the stack, a stand-in model counting input as the request's bytes over 4
  and able to bill chosen usage, Sonnet 5.5's prices): the cold turn's estimate 13,645 (upper 19,103) reserved
  **$1.318206** on the stack against main's **$1.307288** (+$0.010918, 0.84 %); an append turn +$0.000018; billed 1.11
  times the cold estimate with the whole cap, main settled **$0.003 past** its reservation and the stack **$0.008
  within** its own; with `[kernel] spend_limit_usd` at $1.312747, halfway between, main answered and the stack asked a
  budget question ("This session is waiting on the call to claude-sonnet-5-5, which alone reserves $1.32: more than its
  whole $1.31 limit, so resetting its spend to $0 cannot make it …"). That earlier question is the one thing the owner
  would notice; with the default $100 limit it shows only at a session's end.
- **FAST.** ps9i is on the turn's path only as arithmetic (one field read instead of another). xd6l: a lifecycle A/B
  (debug, stack, main, main, stack in one hold, CPU pressure 0 to 0.2) met every budget in all four runs, `bench turn
  --check` frames 5 and 9 on both (plain p50 75.7 ms against 76.2); a ~3 ms debug cold-start gap sat in the daemon's
  config parsing, which no change touches, and a four-build rotation (main, main plus the line, the stack less the line,
  the stack; 20 cold starts each in palindrome order) put the line at +5.5 ms added to main and −2.9 ms removed from the
  stack: the debug builds' own spread, no start-path cost attributable. The changed tests under light load, 20 of 20;
  the heavy-load runs were not run once the owner's game began.

**What the review found.** theseus-6je6 (P3): `learning/audit.rs`'s `audit_reserve` prices an audit request on
`est.tokens`, all of it estimated, and checks `spent + need > limit` before each request, so a request billed over its
byte estimate can carry the run past `[judge] audit_limit_usd` by its overage; `est.upper` closes it (one token, with
ps9i's money test's shape). R34c's calls, adopted by the DM thread at 19:08: keep the reservation on `upper`; take it
in audit.rs too; keep the narrated line's new words (or name the count the dollars are for: "… for up to U input
tokens (about E and the estimate's margin)").

**The join** (the cb stack: core-gaps, Item 228, then this branch; one lock, one gate). The joiner's
eighth job, run right after bench-bounds' done line while the owner gamed. The two branches are siblings, both cut
from d279767f; both edit `tests_m3.rs` and `lib.rs`. The queue was clear at 20:03 (bench-bounds' done line 20:00:06;
main = origin/main = fc59e7f6). The dry run before the lock (20:03:50) and inside the take (20:05:40), with no join
fix: core-gaps clean, R1 b7ced6a3; budget-upper onto R1 clean, merge base d279767f, R2 35f8c83d; no marker; format 23;
tests_m3.rs 7,874 after merge 1 and **7,876 after merge 2, against its 8,050 ceiling**; turn.rs 3,496 of 3,523; the
core's test modules 92, then 93, in order; the scrub's names family 0 on both merges' added lines; every branch file,
15 of 15, R34c's. The guarded take (20:05:38, lock `cloud-b10-cb-join`): merge 1 committed as 8300824c at 20:05:47;
merge 2 (no conflict, `lib.rs` and `tests_m3.rs` auto-merged with core-gaps'; staged 10 files, +251 −43, the tree R2)
committed as **c0ed7ada** at 20:05:51 (8300824c and 1364bcbd), both signed. The warm waited for a build slot behind
R34's voice review (gaming mode: one tree) and ran 20:08:26 to 20:10:42 (the test build 1 m 29 s at 4 jobs, clippy
clean); the stack's suites, **177 of 177** in 43.3 s (theseus-core 146, with the core golden passing on the moved
"Calling …" lines; theseus-protocol 31). The one gate, on c0ed7ada (20:12:01, minute 12, to 20:19:11, ok; 21 s
waiting for the gate lock behind a review step): **3,047 of 3,047** (1 slow, 24 skipped; bench-bounds' 3,041 plus
the stack's 6) in 308.7 s, no hour crossed, no known flake red; lifecycle ok in one run, most rows back toward the quiet
afternoon's (cold start p50 25.6 / p95 34.3 ms; from the config copy 27.6 / 32.9; clean shutdown 33.0 / 55.7; a post
in flight 84.9 / 88.8; SIGKILL then restart 31.0 / 108.1, one slow restart of ten inside its 175 ms budget; binary
swap 49.8 / 64.1; restore 263.5 / 289.8; a cancel's round trip 107.6 / 115.2 against 250); L1 start 9.67 / 11.75 ms;
turn frames 5 and 9, plain p50 87.2 ms, tool call p50 178.7 ms with **one turn of ten at 605.7 ms** (its p95). ps9i is
on the turn's path only as arithmetic, nothing in it waits or writes, and the p50 sits with that evening's gates: the
joiner read the outlier as the machine, and the DM thread noted it without filing, since bench-rows (theseus-w7dk)
adds per-run wall times to the turn bench (chain log 20:30). Pushed 20:19:27 (both merges), both branches deleted on
origin, done line 20:19:43; theseus-ps9i, xd6l and 9o2o closed with c0ed7ada; knql and 6je6 open; R34's tree kept for
the voice review. 14 min from the take to the done line. The store stays at format 23.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). On the owner's daemon a turn's reservation grows
by the estimate's margin, up to 5.4 % of a cold turn's at the 128k cap and almost nothing on an append turn's, so a
session near its spend limit asks one call sooner (with the default $100 limit, only at a session's end), and a
provider counting above the byte estimate now settles within the reservation. The narrated "Calling …" line adds "and
the estimate's margin". Memory's exporter in `Core::build`'s pipeline branch is never on the daemon's path. No config
key. Health after the restart (22:13:05): `theseusd check` exit 0, 9 secrets ready 1.12 s after the start, startup
serving at 61.6 ms (config 4.5, store 8.9, kernel 13.0 ms; at load 19 to 21, over the recipe's 60 ms), `cgroup:
delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline`
arm, voice ready (`0 resumed`, rejoins 0, `deaf_failed` false), Discord ready, the unit active with NRestarts 0, and no
error or warning in the journal; no config change, the store at format 23, sessions 21,785 with the lists in low
milliseconds, and durability caught up at position 143,096 from 22:13:18 (12 s). The old daemon's stop took 11.87 s
(install #8's: 298 ms), with nothing logged between (theseus-vjn7, P2).

**Divergences.** The narrated line's words changed, so the core golden moved by them. audit.rs's reservation, the same
shape, was left alone (theseus-6je6). The report's live check 3 (the retention gauge on the daemon's path) was not run:
the branch does not touch that path.

**Known gaps.** theseus-6je6 (P3: the audit's reservation on `est.upper`). The narrated `input_micros` is held by no
test (R34c's plant 4, not filed). Owed by the review: the spec's money section (both reservations on `upper`, written
in this version's Part I amendments) and the status page.

### Item 230. Voice deaf call: a voice call that joins but never hears is noticed, rejoined once and told, with R34d's join fix that the rejoin never outlives its call; a voice turn waiting on the operator says so, a stopped one says nothing, a turn queued behind a failed one goes after the rebind, and the waiting notes are capped (theseus-d93y, theseus-ved2, theseus-b6vz and theseus-nthu; the owner's deaf call of 2026-10-05 20:32 and R33's findings on voice-heard (Item 216); the tenth cloud batch's voice-dave session, launched by the DM thread at 15:39 once voice-heard had joined, fired 2026-10-06 15:42 from ea34457e, Opus 5.5, its report at 17:22; 2d6f7979, df14ec29, 491d4f38 and 5907aa76; reviewed 19:36 to 21:01 by local reviewer R34d, the voice stack, and accepted at 21:05 with a join fix; joined 21:37 at 76f20059, a signed merge onto c0ed7ada, the first of the stack's two merges under one lock and one gate, by the batch-10 voice-stack joiner; installed 2026-10-06 22:13 at 02de4b70, install #9)

**Why.**
- **theseus-d93y** (P2): on 2026-10-05 at 20:32 the owner's daemon (install #5) joined a voice call and heard nothing,
  0 utterances ("in voice chat but not responding"), the first live use of the receive path; a `/join` at 21:02 worked
  (DAVE's group welcomed, a conversation of about 4.6 minutes). DAVE's MLS welcome sometimes never arrives, and
  songbird then drops every packet silently. Nothing noticed a deaf call, and the person learned only from silence.
  It was v1-beta's last open item.
- **theseus-ved2** (P3; R33, reviewing voice-heard): `Place::handle`'s rebind branch after a failed turn submitted the
  queued typed messages and returned before `voice_next()`, so a voice turn waiting in the call's queue was never
  submitted, and the call took no more turns until someone typed (pre-existing since step 44b).
- **theseus-b6vz** (P3; R33): a voice turn held for the operator (an approval, a budget question) said nothing aloud;
  and a `/stop` that a step refused ended the submit in an error whose class was not "stopped", so it would be said as
  a failure.
- **theseus-nthu** (P3; R33): three runtime rules voice-heard's suite lacked (an old call's events reach no new
  call's notes, a stopped voice turn is not said as a failure, the 32-turn first-words cap), and `Notes.waiting` was
  uncapped.

**What landed** (five crates; the merge with R34d's join fix 18 files, +1,466 −29, without the cloud files; no package
and no store format change (23 stays 23: two ledger kinds whose detail is JSON, and health fields, never stored); one
additive protocol change; one config key; INSTRUMENTS 42 to 43).
- **The deaf watch** (5907aa76, d93y; one commit, since the rule, the watch, the rows and the key have no reader
  without each other). Read from songbird 0.6.0's and davey 0.1.4's sources: songbird keeps the `DaveSession` private
  (no readiness, status, epoch or welcome event; the journal's `dave_session … INACTIVE` is a span recorded at the
  connect, not diagnostic), and `udp_rx` drops a DAVE packet it cannot decrypt before any event sees it. What songbird
  does show is `CoreEvent::RtcpPacket` after transport decryption (DAVE never encrypts RTCP): a sender report from an
  SSRC means it is sending media. So theseus-voice's `songbird_io.rs` counts sender reports per SSRC
  (`SsrcCount.reports`), and `Link` gives `counts()` and `rejoin(gap, within)`: a leave, a wait, the counts zeroed (the
  users kept), a join of the same `Call`, connected within the bound, its requested disconnect not passed to the engine
  as a drop (a fresh connection sends a fresh key package; no API asks for a DAVE reset). theseus-discord's new
  `runtime/voice/deaf.rs`: `Hearing` (ready, elapsed, the listed speakers present by the gateway's voice states, the
  per-SSRC counts with the speaking map's user), the pure rule `deaf()`, the seam `Line`, and `watch`, spawned at
  `start_call`, every `deaf_after` off the audio path, until a listed speaker decodes or the call ends. **The rule:** a
  listed speaker in the channel whose sender reports (or frames) arrive with nothing decoded is deaf; alone, or a
  listed speaker present and silent, is not. **Deaf:** a `voice.deaf` row (why, elapsed_ms, attempt, place) on the
  place's session, `theseus.voice.deaf` by `theseus.voice.why`, health's `deaf_since_ms`, the notice "I can't hear the
  call (…). Rejoining…", one rejoin (a 2 s gap, a 20 s connect), `voice.rejoined` (ok, error, elapsed_ms, place),
  `rejoins` 1; then hearing gives "Rejoined: I can hear you now" and clears `deaf_since_ms`, while deaf again, or a
  rejoin error, gives `voice.deaf` attempt 2, `deaf_failed`, "Still can't hear: try /leave and /join", and the watch
  returns. No path rejoins twice, and nothing is spoken. `[voice] deaf_after_secs` (default 10, 0 refused at start,
  with its template line); `VoiceStatus` gains `deaf_since_ms` (optional), `rejoins` and `deaf_failed` (serde
  defaults; the cockpit's `VoiceStatus.ts` regenerated); health's line `· deaf since 22:41:07 UTC, rejoined once`,
  `· deaf (rejoin failed)`, `· rejoined once`; `record_voice_deaf` and `VOICE_DEAF`. `examples/join.rs` prints the
  sender reports. The rule's "not ready" clause is built and tested, but songbird gives it no input (`ready` is
  `Some(true)` once anything decodes, else `None`).
- **R34d's join fix** (`voice-dave/joinfix.py`, 3 files, +248 −10, nine edits, each checking `new in s` first): the
  rejoin outlived its call. Theseus's `/leave` is songbird's manager `leave`, which keeps the `Call` registered, so a
  `/leave` during the rejoin's 2 s gap was followed by the rejoin's join: the bot came back into the channel with no
  call, health said "ready", and `/leave` answered "I'm not in a voice channel."; and a call left while its rejoin
  connected was told "Still can't hear", with health saying `deaf (rejoin failed)` on the left call, or on the next
  call when one had been joined. Now `Link::rejoin(gap, within, wanted)` asks `wanted()` after the gap, under the call's
  lock and just before its join ("the call ended during the rejoin" without joining), and the watch passes
  `|| current(&shared, serial).is_some()`; a call that ended writes no row, no count and no notice. `/leave` takes the
  call's slot before songbird's leave takes that lock, so the answer is never stale.
- **The held turn and the stop** (df14ec29, b6vz). `HELD_TURN` = "I need your answer in the text channel.", spoken
  after the reply's text (or alone) for a turn that waits on the operator (`awaiting_confirm`, or a stop reason of
  `awaiting_confirm` or `budget`), once per held turn; a continuation after the answer is not a voice turn's submit.
  `Call.stopped`, set by `Place::voice_stop` (first in `Control::Stop`, one line) and by the call's own turn's
  `turn.failed` of class `stopped`, cleared at each voice submit; `spoken_end` says nothing for a stopped turn (an `Ok`
  whose stop reason is `stopped` included), the reply plus `HELD_TURN` for a held one, and `FAILED_TURN` for any other
  failure.
- **The rebind** (2d6f7979, ved2): `self.voice_next();` before that `return`, one line.
- **The notes** (491d4f38, nthu): `Notes::wait` keeps at most `KEPT_NOTES` = 8 waiting notes, the oldest dropped.

**How it is proven.**
- **The session:** the rule's unit tests (`deaf::tests`: past the bound with a listed speaker, deaf, and not before;
  alone, never; silent but ready or unknown, not deaf; sender reports with nothing decrypted, deaf, and one decoded tick
  is hearing); `songbird_io::tests::a_sender_report_is_counted_and_a_restart_zeroes_the_counts`; the runtime through a
  fake `Line` at a 30 ms period (`tests_deaf.rs`: a deaf join whose rejoin hears, one `voice.deaf` and one
  `voice.rejoined`, the notices exactly [DEAF, REJOINED]; a rejoin deaf too, attempts [1, 2] and one rejoin even ten
  periods later, [DEAF, STILL_DEAF]; a rejoin that errors; alone, and present but silent: no row, no notice, no
  rejoin); `the_health_line_says_a_deaf_call`; the metric in `telemetry::tests_voice`; tests_reply.rs for ved2 (the
  queued voice turn submitted on the new session), b6vz (exactly one reply "Writing it now.\n\nI need your answer in the
  text channel."; a `/stop` before the answer, and a `turn.failed` of class `stopped`, each an empty reply, the next
  failure spoken again) and nthu (the 32-turn cap, the 8-note cap, an old call's late `Cut` reaching no new call). Nine
  planted reverts, each failing its test. Under the load recipe the 25 new and touched voice tests ran three times, 25
  of 25 each. Each commit's gate red only on the known L1 cases.
- **The review** (R34d, R34's tree; review merge a0bd176d on 05b36e06, the join fix's commit bee0cb22): the build
  clean, theseus-protocol 32 of 32, protocol.gen matching; **the whole workspace suite on the merge 3,059 of 3,059**,
  and on the stack with voice-holds 3,090 of 3,090; the voice families 112 of 112 (the slowest 1.24 s). The report's
  nine plants caught; of R34d's four, M4 (the stopped mark not cleared) caught, M2 (the count reset) and M3 (a healthy
  call saying "Rejoined") passing until the join fix's tests held them, and M1 (the requested-disconnect guard)
  passing: songbird's `DisconnectData` is `#[non_exhaustive]`, so its test needs a pure function (theseus-1oun).
  **R34d's probes** on the merge: a songbird `Call` over a shard recording each voice state update (the control
  rejoin sends join, leave, join; with a `/leave` 100 ms into a 300 ms gap it sent join, leave, leave, then a **join**,
  and `current_channel()` was the channel again); and a call left while its rejoin connected (the notices [DEAF,
  STILL_DEAF], health `deaf (rejoin failed)`, `rejoins` 1, and with a next call joined meanwhile that call marked).
  **The join fix:** four tests (songbird_io's `a_rejoin_joins_again_unless_the_call_ended_in_its_gap` on a paused clock
  with the recording shard, which also holds the count reset; tests_deaf's
  `a_call_that_hears_ends_its_watch_in_silence`, `a_call_left_while_it_rejoins_is_not_joined_again_or_told_more` and
  `the_next_call_is_not_marked_by_the_old_calls_rejoin`); its five plants (the ask off, the check after the rejoin off,
  `wanted` always true, the count reset off, "Rejoined" said on a call that heard at once) all caught; the voice
  families 118 of 118; a second run of the script reports "already applied" nine times.
- **Protocol and config** (R34d): main's `VoiceStatus`, copied, reads the new block; the new one reads an old block as
  `None`, 0 and false; nothing in theseus-protocol denies unknown fields, and the cockpit reads no `VoiceStatus` field.
  On the rig (`theseus-sim discord rig`, the merge's binaries; the fake Discord has no voice side, so no call joins):
  the key loads, health's voice binding reads `rejoins: 0`, `deaf_failed: false`, no `deaf_since_ms`; with
  `deaf_after_secs = 0`, theseusd exits 1 with "voice.deaf_after_secs = 0: a call needs time to hear (10 is the
  default)". `VoiceConfig` denies unknown keys, so a daemon from before this join refuses a config naming the key.
- **FAST.** The start path gains one `u64` in `[voice]`'s parse and three fields in health's block; the watch starts
  only at a join and reads a small map under the SSRC mutex every 10 s (the 20 ms tick takes the same mutex for
  microseconds); b6vz reads one mutex at a voice turn's end. R34d's A/B (frozen debug builds, palindrome): turn frames 5
  and 9 on both; the lifecycle A/B was inconclusive, both arms missing budgets beside three other trees' builds.

**What the review found.** The deaf rule's misses: a client that sends no sender reports (no one has seen a Discord
client's yet, so the live check's first step is to see them); a listed speaker present and silent, by design; a call
that heard and later goes deaf, since the watch ends at the first hearing. Its needless rejoins, once a call: a listed
speaker who arrives after the join and speaks inside their own DAVE transition, and perhaps a quiet join if silent
clients send sender reports. Filed: theseus-kcng (P3, the clock per speaker), theseus-1oun (P3: the requested-
disconnect guard and `deaf_after_secs = 0`'s refusal untested, and the watch's silence about what it saw).

**The join** (the voice stack's first merge, under the stack's one lock; the queue, the dry run, the warm, the tests and
the one gate are told in Item 231). The merge (21:16:30 onto the cb stack's c0ed7ada): no conflict
(`runtime.rs` auto-merged); the CLOUD files removed; `python3 -I …/voice-dave/joinfix.py` exit 0 with nine "applied"
lines, three each in `songbird_io.rs`, `deaf.rs` and `tests_deaf.rs`, only those three files then added; `git diff
--cached --quiet bee0cb22 --` over the branch's 18 files exit 0; staged 18 files, +1,466 −29, the tree 4d3f2713, the
dry run's R1; `--check`'s three hits ts-rs's trailing spaces in `VoiceStatus.ts`; INSTRUMENTS 43 entries; runtime.rs
3,465 of 3,500; the scrub's names family 0. The signed merge **76f20059** (c0ed7ada and 29c9662c). theseus-ved2, b6vz
and nthu closed with 76f20059; **theseus-d93y stays open** until the owner's live voice call shows clients' RTCP
sender reports arrive, the rule's only live input. The store stays at format 23. V1-beta was declared at 22:04:46 on
02de4b70 by the 22:00 safety-net wake (chain log 22:07): Theseus feature complete, its two conditions install #8 and
this deaf-call fix.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). On the owner's daemon a voice call that hears no
listed speaker 10 s after it joins says so in its text place and rejoins once; health's voice line shows `deaf since
…`, `rejoined once` or `deaf (rejoin failed)`; a `/leave` during the rejoin stays left; a voice turn waiting on an
approval says "I need your answer in the text channel.", and a `/stop` silences the voice turn. **One new config key,
`[voice] deaf_after_secs`, default 10:** the operator's config names it nowhere (`[voice]` holds only `enabled`), so
the default holds and no config changed; an older daemon would refuse it, so it is never set before an install.
INSTRUMENTS 43; `VoiceStatus` additive. Health after the restart (22:13:05): `theseusd check` exit 0, 9 secrets ready
1.12 s after the start, startup serving at 61.6 ms (config 4.5, store 8.9, kernel 13.0 ms; at load 19 to 21, over the
recipe's 60 ms), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`),
memory live on the `baseline` arm, **voice ready, `0 resumed`, with no `deaf since`, `rejoined once` or `deaf (rejoin
failed)`, and the voice binding's JSON holding the new `rejoins: 0` and `deaf_failed: false`**, Discord ready, the unit
active with NRestarts 0, and no error or warning in the journal; the store at format 23, sessions 21,785 with the lists
in low milliseconds, and durability caught up at position 143,096 from 22:13:18 (12 s). The old daemon's stop took
11.87 s (install #8's: 298 ms), with nothing logged between (theseus-vjn7, P2). The owner was sent the result and the
live-call steps at 22:26.

**Divergences.** The rule reads deafness from RTCP sender reports with nothing decoded, since songbird exposes no DAVE
readiness; its "not ready" clause has no live input. The four d93y parts landed as one commit. The rejoin outlived its
call on the branch; R34d's join fix closed it. b6vz's `spoken_end` also silences an `Ok` turn whose stop reason is
`stopped`, which the task did not ask (R34d: right). No forced-deaf recipe exists: the deaf path cannot be provoked
from outside.

**Known gaps.** theseus-d93y stays open for the owner's live voice call (R34d's seven steps: see sender reports in the
join example, a usual call, a quiet join, a late arrival, the deaf path if it comes, a `/leave` during a rejoin, b6vz's
held and stopped turns). theseus-kcng (P3): the owner's answer of 23:31 (F2 in the evening's question walk) is per
speaker: each listed speaker's deaf clock starts at the later of the join and their own arrival, the 10 s default
kept, a small change with a test riding with the fix behind the live check. theseus-1oun (P3). Reading DAVE's readiness
directly would need songbird to expose its session, or a tracing layer on davey's welcome line. Owed by the reviews:
m7-surface's voice section (the deaf watch, its one rejoin, and a call ended during its rejoin not joined again),
written in this version's Part I amendments; the status page.

### Item 231. Voice holds: a held reply decides at most 3 s after the last utterance over it closed, a floor held by a sound with no words frees at 8 s, and the echo (now over the sentences joined as they played, and from 2 words at a playing sentence's head), the call's end, a closing question's "yes" with another reply queued, and six speech shapes are fixed and pinned (theseus-aq4t, theseus-zcxx, theseus-e6mj, theseus-qrwx, theseus-j2ut and theseus-q4pc; R27's findings on voice-turns (Item 202), R31's on voice-echo (Item 215) and R33's on voice-heard (Item 216); the tenth cloud batch's voice-holds session, launched by the DM thread at 15:39 once voice-heard had joined, fired 2026-10-06 15:42 from ea34457e, Opus 5.5, its report at 17:19; c4f3147e, ec411738, 62159cf0, 229d46d5, 6af519b7, a1a0ed9e, 452faec2, 222a389b and c7099112; reviewed 19:36 to 21:02 by local reviewer R34d, the voice stack, on voice-dave's review merge and join fix, and accepted at 21:05 with no join fix; joined 21:37 at 02de4b70, a signed merge onto 76f20059, the second of the stack's two merges under one lock and one gate, by the batch-10 voice-stack joiner; installed 2026-10-06 22:13 at 02de4b70, install #9)

**Why.**
- **theseus-aq4t** (P2; R27, reviewing voice-turns, Item 202): the hold and the floor had no bound. `resume` waited
  while any VAD was open or a speech-overlap utterance's transcript was still due, and nothing counted time: a
  transcript that never came held the reply for the whole call, and a provider failing at 20 s committed the cut 20 s
  after the laugh closed. `play_next` waited while any VAD was open, and a steady sound closes at the VAD's 30 s
  maximum and reopens on the next frame, so the floor was never free. R27 recommended about 3 s and about 8 s; the DM
  thread adopted them at 09:06.
- **theseus-e6mj** (P2; R27): turns.rs pinned neither another speaker's words superseding a waiting reply nor `Leave`
  while held; two plants passed.
- **theseus-qrwx** (P3; R27): a cut report waiting at the call's end got no `Cut { CallEnded }`.
- **theseus-j2ut** (P2) and **theseus-q4pc** (P3; R31, reviewing voice-echo, Item 215): the one-sentence echo rule
  made three real echoes words (an echo-prone speaker's echo of a whole multi-sentence reply, a first echo cut short
  by its own 300 ms stop, an echo across a sentence boundary), and a "yes" on a closing question's last word was still
  no turn when another speaker's reply was queued behind the question. R31's fix sketches were adopted (12:33): the
  run measured also over the sentences joined in play order, and a 2-word head-run rule.
- **theseus-zcxx** (P3; R33, reviewing voice-heard's `speakable`): a few things replies hold were misread for speech
  (a prose line opening with `|`, a `---` rule, task checkboxes, a year opening a line, `>` before a number, prose with
  a pipe after a table).

**What landed** (`crates/theseus-voice` only: `engine.rs`, `heard.rs`, `sentences.rs`, `vad.rs`, `tests/turns.rs` and
the crate's AGENTS.md; the merge 6 files, +1,259 −77, without the cloud files; no protocol, config-file, store,
discord or core change; the types the binding destructures unchanged; `Config` gains two pub fields, built by
`Config::new`; no store format change (23 stays 23)). theseus-voice's suite went from 85 tests to 103 (turns.rs 23 to
40).
- **The hold's bound** (c4f3147e, aq4t). `Config::transcript_bound`, 3 s by default. `Engine::overdue` runs first in
  `advance`: under a hold, if the speech-overlap transcripts are still due 3 s after the **last** of those utterances
  closed (call time, by ticks), each is marked `Dropped` with a `Failed { Transcribe(speaker), "no transcript 3000 ms
  after the utterance closed" }`, and the cut is committed (`Cut { Words }`, `BargeIn`), as a failed transcription
  is: a "stop" that was not heard must not be talked over, and a wrong commit costs only a cut reply. `heard` now acts
  only on a pending that is still `Waiting`, so a transcript after the bound emits nothing. 3 s is far past a real
  round trip (well under a second) and far short of the 20 s request bound.
- **The floor's bound** (62159cf0, aq4t). `Config::floor_bound`, 8 s by default. `Engine::probe` fires when a reply's
  front item `opens`, nothing plays, and it has waited 8 s since it was asked, or when a hold has waited 8 s since it
  began, and only when no transcript that would decide first is still due. It transcribes each open, unprobed
  utterance's audio so far once (`Vad::so_far`, to its last speech frame), as `Done::Probed`. Heard as no words, the
  utterance's sound becomes `Wordless`: `floor_held()` (read by `play_next`, `resume` and the pause test) ignores it, so
  the reply plays, and that speaker's 300 ms stop is not counted while the mark stands, as for an echo-prone speaker;
  a `Wordless` utterance that the VAD's maximum closes and reopens on the next tick keeps the mark (`Engine::steady`),
  so the 30 s reopen does not stop a playing reply; its words still decide when it closes. Heard as words, or a failed
  probe, it holds the floor as before, so a person's long sentence is never talked over. The cost: one STT request per
  open utterance per wait that reaches the bound, of up to about 8 s of audio, and one copy of it.
- **e6mj** (229d46d5, 222a389b): another listed speaker's words supersede a waiting reply (`Cut { Superseded }`, the
  words a turn); `Leave` while held gives one `Cut { CallEnded }` and no `Resumed`; the echo count is each speaker's.
- **qrwx** (6af519b7): `call_ended` cuts each report still waiting: `Cut { what: Report, why: CallEnded, sentences,
  heard: first, into: 0, last_heard, cut }`, `said` keeping a report's last whole sentence when it is sent back; the
  binding's pump writes a `voice.cut` row for it, and its notes skip `CallEnded`.
- **j2ut** (a1a0ed9e): `is_echo` measures the in-order run over the candidate sentences joined in the order they
  played (which holds each one's run too), and a run from the head of the sentence playing when the utterance began,
  from an utterance begun within 0.5 s of that sentence's start (`ECHO_HEAD`), is an echo from 2 words, still at 80 %
  of the utterance; elsewhere the floor stays 3. `is_echo` and `classify` take `head`, kept on `Opening.head`.
- **q4pc** (452faec2): in `what_is_over`, `last` is the item's own (`item.index + 1 == item.count`), and the close's
  flip to the tail applies while the queue's front has not begun.
- **zcxx** (ec411738, c7099112): `speakable()` says a prose line opening with `|` (a row needs a separator next, a
  table's shape, or both outer pipes) and prose with a pipe after a table (rows go on while they keep the separator's
  shape); drops a `---`, `***` or `___` rule and a task checkbox with its bullet; keeps a year opening a line (list
  numbers have at most 3 digits); says `>` directly before a digit as "more than" ("> 5" with a space is still a quote
  mark, as Discord renders it); and the `+` and `•` bullets are tested. Three regression tests hold the echo against
  what was played (`speakable`'s sentences): a table's sentence heard back, a sentence heard back without its link or
  emphasis, and a cut quoting the sentences as played.
- The crate's AGENTS.md: the bounds, the probe's fixture trap (the stand-in hands fixtures out in order), the echo's run
  and head rule, and q4pc's tail.

**How it is proven.**
- **The session's tests** (turns.rs, virtual time, a `Stalled` transcriber beside `Deaf`): a held reply whose
  transcript never comes is cut at the bound (a laugh from 1.5 s stops the reply at 1.8 s and closes at 2.6 s; one
  `Failed` and the commit at 5.6 s, `into: 600 ms`; another speaker's words at 7.0 s are the next turn and its reply
  plays); the provider's own failure at 22.6 s no longer sets the hold's wait; a reply waits 8 s for a floor held by a
  steady 40 s fan and then plays whole at 9.48 s, with 4 transcriptions (before the change, after 41.3 s); a hold under
  a steady sound resumes at the floor's bound (`Resumed { Wordless, held: 8 s }` at 9.8 s); a speaker talking 10 s in
  one breath is still waited for (the probe at 9.48 s hears words); e6mj's three; qrwx's two (a report cut by words and
  waiting at the call's end; one never begun); j2ut's E1 (an echo-prone speaker's echo of a whole reply), E2 (a first
  echo cut short by its own stop), E3 (across a sentence boundary), and "Monthly view." (2 words, not at the head) a
  turn; q4pc's two ("Yes." from 3.4 s on the question's last word is turn 2, the queued reply superseded); and zcxx's
  seven and the echo's three. Every plant the report names failed its tests (22, the seven `speakable` plants each
  failing its one test and no other), but one: `heard`'s `Waiting` guard removed passes, a defensive guard no order
  reaches. theseus-voice under the load recipe 102 of 103, 102 of 103 and 103 of 103: the failure,
  `deepgram::a_call_with_no_answer_ends_at_its_timeout`, is main's (a 300 ms client timeout under starvation), filed by
  R34d as theseus-ndg9. theseus-discord and theseus-voice 240 of 240; each gate red only on the known L1 cases.
- **The review** (R34d; the merge 4604ef33 on voice-dave's join-fix commit bee0cb22, the two branches sharing no
  file): the build clean; theseus-voice 105 (with voice-dave's sender-report test and the join fix's rejoin test) and
  the voice families 139 of 139; **the whole workspace suite on the stack, 3,090 of 3,090**, beside a joiner's build.
  The report's plants as it says; R34d's three (the hold's bound from the first overlapping close; the wordless mark
  not carried across the 30 s reopen; a probe's answer applied to whatever the speaker has open) pass: no test has two
  overlapping utterances closing apart, a reply playing across a fan's reopen, or a probe in flight across a close
  (theseus-id2h). On R34d's 29 `speakable` inputs (R33's seven findings and 22 shapes beside the fixes), R33's findings
  are fixed but `> 5 GB` with a space (a quote by choice), prose right after a table without outer pipes holding the
  separator's pipe count is still swallowed, and nothing is said worse than before.
- **R34d's probe of the floor's bound** (`finding_` tests that pass on the stack): a fan from 0.6 s, the answer ready
  at 1.5 s, and a 6 s question begun at 9.40 s: at 9.48 s both probes hear no words, and the answer plays from 9.48 s
  over the question; a hold the fan began at 1.8 s, a question begun at 9.75 s, and the cut sentence resumed whole over
  it at 9.8 s; the control, a question from 7.0 s, heard as words and waited for. The probe judges an utterance however
  young, on its 200 ms pre-roll and a few frames (theseus-qhzs).
- **FAST.** `overdue` and `probe` run in `advance`, after the tick's VAD work: a comparison per event, then a walk of
  the open VADs once past the bound; `floor_held` adds a map lookup per open VAD. The stop still comes on the tick of
  the 300th ms (`tick` to `hold` unchanged; the exemption only skips the count for a speaker marked wordless). A resume
  replays the held audio with no new synthesis (`resume` touches no audio; the old test still counts 3 syntheses). The
  engine runs only in a call: turn frames 5 and 9 on the stack, as on main; the lifecycle A/B was inconclusive under
  neighbour load, and the branch touches no start code.

**What the review found.** theseus-qhzs (P2): the floor's 8 s probe talks over a question begun just before it; the
fix is to probe only an utterance with about 1 s of speech so far, or open since before the wait began. theseus-kb6e
(P2): "The monthly view." said over "Do you want the daily or the monthly view?" is still dropped as an echo, a 3-word,
100 % run of its question; the brief expected the head-run rule to fix it, and the session found, and reported, that
it cannot (the rule only adds echoes): fix it by timing (an echo begins sooner than an answer can). theseus-l1pe (P3):
the probe's spend, and a transcript the hold's bound dropped, are billed but not booked (booking needs a new
`Event::Probed`, which the binding's exhaustive match would have to take). theseus-id2h (P3) and theseus-ndg9 (P3).
R34d's calls for the owner: keep the 3 s hold bound; keep 8 s for the first live check, fix qhzs, then try 4 to 5 s;
keep dropping a late transcript and book its spend; keep "more than"; fix the last option by timing. The DM thread
adopted them at 21:05, keeping both bounds for the first live check.

**The join** (the voice stack: voice-dave with R34d's join fix, Item 230, then this branch; one lock,
one gate; a fresh joiner, since the long-lived one's context was near its limit). The two branches are siblings, both
cut from ea34457e, sharing no file. The queue was clear at 21:08 (the cb stack's done line 20:19:43; main =
origin/main = c0ed7ada). The joiner's dry run before the lock (21:11:22) and inside the take (21:16:13), in linuxbrew
git: voice-dave's merge-tree clean, the join fix applied by path (nine "applied", then nine "already applied"), R1
4d3f2713 with its three fixed files R34d's bee0cb22's byte for byte; voice-holds onto R1 clean, R2 8e92b3a9; no marker;
format 23; INSTRUMENTS main 42, R1 and R2 43 with 43 entries; every changed `.rs` file under its ceiling; the scrub's
names family 0 on both merges (merge 2's one hit, in the URL family, a test fixture's link to a reserved example host);
every branch file equal to R34d's commits (18 of 18, then 24 of 24), and R1 and R2 differing from them only in main's
own 24 files since 05b36e06. The guarded take (21:16:09, lock `cloud-b10-voice2-join`): merge 1 committed as 76f20059
at 21:16:30; merge 2 (no conflict; `git diff --cached --quiet 4604ef33 --` exit 0; staged 6 files, +1,259 −77, the
tree R2) committed as **02de4b70** at 21:16:39 (76f20059 and bc477399), both signed. The warm (21:16:56 to 21:28:13:
the test build 8 m 56 s, since the stack touches theseus-protocol and three crates above it, beside three other trees'
builds and suites at load 20 to 28; clippy clean); the stack's suites, **305 of 305** in 32.1 s (theseus-voice 105 with
turns.rs's 40 and the join fix's rejoin test; theseus-discord's voice 34 with tests_deaf's 7; theseus-core 131 with
the core golden; theseus-protocol 32; theseusd's `default_config` 3, holding the template's new line). The one gate, on
02de4b70 (21:29:38, minute 29, to 21:37:00, ok; 37 s waiting for the gate lock behind two review steps): **3,096 of
3,096** (1 slow, 24 skipped; the cb stack's 3,047 plus the voice stack's 49) in 303.7 s, no hour crossed, no known flake
red; lifecycle ok in one run (cold start p50 29.1 / p95 44.5 ms; from the config copy 33.2 / 45.4; clean shutdown 34.8
/ 47.5; a post in flight 88.9 / 95.0; SIGKILL then restart 30.6 / 32.6; binary swap 81.0 / 113.6, within that day's
range; restore 252.6 / 316.7; a cancel's round trip 108.0 / 129.0 against 250); L1 start 7.99 / 8.97 ms; turn frames 5
and 9, plain p50 81.5 ms, tool call 177.8 ms (its p95 187.1: the cb gate's 605.7 ms outlier did not recur). Pushed
21:37:15 (both merges), both branches deleted on origin, done line 21:37:41; theseus-aq4t, zcxx, e6mj, qrwx, j2ut and
q4pc closed with 02de4b70 (ved2, b6vz and nthu with 76f20059; d93y open); R34d's seven findings open; R34's tree kept
for R41's cancel-fast review. 21 min from the take to the done line, nine of them the warm's build. The store stays at
format 23.

**The install** (installed 2026-10-06 22:13 at 02de4b70, install #9). In a voice call on the owner's daemon a reply
waits at most 8 s for a floor held by a sound with no words, and a held reply at most 3 s past the last sound over it;
an echo of a whole reply, or one cut short by its own stop, makes no turn and no stutter on loudspeakers; a "yes" on a
closing question's last word is answered with another reply queued; a report still waiting at the call's end is cut
there; a few markdown shapes are spoken right. The bounds are `Config` fields, not config-file keys: nothing to set.
To watch live (R34d): with a fan at a listed speaker's microphone, stay quiet in the second before an answer plays,
until theseus-qhzs is fixed. Health after the restart (22:13:05): `theseusd check` exit 0, 9 secrets ready 1.12 s after
the start, startup serving at 61.6 ms (config 4.5, store 8.9, kernel 13.0 ms; at load 19 to 21, over the recipe's
60 ms), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on
the `baseline` arm, voice ready (`0 resumed`, rejoins 0, `deaf_failed` false), Discord ready, the unit active with
NRestarts 0, and no error or warning in the journal; no config change, the store at format 23, sessions 21,785 with
the lists in low milliseconds, and durability caught up at position 143,096 from 22:13:18 (12 s). The old daemon's stop
took 11.87 s (install #8's: 298 ms), with nothing logged between (theseus-vjn7, P2).

**Divergences.** "The monthly view." stays an echo: the brief expected j2ut's head-run rule to make it a turn, and the
session showed it cannot (the test that would hold it failed with `Echo`, and was kept out of the suite). `>` before a
number is said "more than", the session's choice for the owner. The hold's bound commits rather than resumes, and drops
a late transcript, whose words then go unanswered (rare). The bounds cover only the hold and the floor: a floor held by
words still waits for the VAD's 30 s close, and a non-speech utterance whose transcript never comes still blocks the
next turn until the provider's 20 s bound, both by design.

**Known gaps.** theseus-qhzs (P2: the probe on a young utterance); theseus-kb6e (P2: the last option repeated, to fix by
timing); theseus-l1pe (P3: the probe's and a dropped transcript's spend unbooked; health's voice line cannot count the
bounds until a probe event exists); theseus-id2h (P3: three rules of the bounds untested); theseus-ndg9 (P3: the
deepgram test under load); zcxx's residual (prose after a table without outer pipes) noted on it. The live checks
(R34d's six: a fan with a question, a long question, a laugh then `/leave`, a cut report then `/leave`, loudspeakers, a
"yes" with two people) wait for the owner's next voice call. Owed by the reviews: m7-surface's voice section (both
waits bounded), written in this version's Part I amendments; the status page.

### Item 232. AWS mints: every credential mint and stored secret in the catalog's models is held to a reviewed decision by an output-shape rule, the walk finds a secret's name in any case, three mints the catalog missed (two STS, one EKS) are secret-bearing writes, and, with R39's join fix, Lightsail's SSH private key is held by name (theseus-ye7o, with theseus-xscd by the join fix; found by the tool-tree design lane at 16:33 that day; the eleventh cloud batch's aws-mints session, fired 2026-10-06 18:10 from 57f265f2, Opus 5.5, its report at 18:53; 8b3e38cc, 0b65b5a1 and 420c4a5e; reviewed 20:14 to 21:49 by local reviewer R39, and accepted with a join fix; joined 22:50 at c500c3c1, a signed merge onto 02de4b70, the first of the amhb stack's three merges under one lock and one gate, by the batch-11 amhb joiner; not installed yet: on install #10's list)

**Why.** The catalog's tables (`crates/theseus-aws-catalog/src/tables.rs`) made the STS mints `AssumeRole*`,
`AssumeRoot`, `GetSessionToken` and `GetFederationToken` secret-bearing writes (Item 97's handles: the value goes to
the broker's board, the model gets a handle). The tool-tree design lane (theseus-0nnk), generating leaves from the
catalog at ea34457e, found three newer operations in AWS's models that return credentials and were in neither table:
sts `GetDelegatedAccessToken` (temporary AWS credentials; classed Read by its `Get` prefix), sts `GetWebIdentityToken`
(a signed token for outside OIDC services; Read) and eks-auth `AssumeRoleForPodIdentity` (a Write, not secret-bearing).
aws-toolset.md §3.1 says every credential mint is secret-bearing; as built, `aws.call` would have put these values in
the model's context. theseus-ye7o (P1, security) asked for the three rows, and a rule so the next one cannot slip in
unseen.

**What landed** (theseus-aws-catalog's tables, tests and generator doc; theseus-core's `aws/secret.rs` and two new test
modules; the merge 7 files, +1,086 −15, with R39's join fix and without the cloud files; no package, config key,
protocol type or catalog data change, and no store format change (23 stays 23)).
- **The three rows** (8b3e38cc). `CLASS` rows (Write, with the MINT note, "mints a credential, whatever its name
  says") for the two STS mints, and one for eks-auth's (already a write by its name; the row is there for the note, as
  `AssumeRole*`'s is); `SECRET` and `RETRY` (`SafeToRepeat`) rows for all three. The session found what the brief did
  not expect: all three were already held whole by the walk once secret-bearing, since their models mark the shapes
  sensitive (`Credentials` is in `NAMED`; `WebIdentityToken` and eks-auth's `credentials` are marked). The catalog's new
  `tests/mints.rs` holds each mint's label `W 🔑`, class, `SecretBearing::Always`, `SafeToRepeat` and note.
- **Through the core** (0b65b5a1). `aws/tests_mints.rs` (new) on tests_handles' rig: the stand-in answers STS's two
  in query-protocol XML and eks-auth's in REST-JSON; each call is planned and checked as a Write and run with a binding;
  `meta.secrets` equals the handles exactly, no value is in the text, the meta or the request rows, and the board holds
  each value. Step 3 added ECR's `GetAuthorizationToken` (the JSON protocol, by its `x-amz-target`).
- **The output-shape rule, and every hit decided** (420c4a5e). `aws/tests_secret_shapes.rs` (new) walks every
  operation's output shape in the compiled catalog (17,928 operations) with a seen-set. A member is credential-shaped
  when its name, in any case, is one of `NAMED`'s (`Credentials`, `SecretAccessKey`, `SessionToken`, …), or when the
  model marks it sensitive and its name ends in `Token`, `Password`, `Secret`, `Key` or `Credentials`; a paginator's
  output token, any `NextToken`, and a tag's key are never credential-shaped. An operation with a hit must be
  secret-bearing, or each of its hits must be on an `ALLOWED` row that names a member, not a whole operation
  (`(service, operation globs, "Shape.member" glob, reason)`); a row is stale, and fails, when it matches no
  credential-shaped member or matches one in a secret-bearing operation. The rule reads `secret::NAMED` itself (now
  `pub(super)`), so the rule and the walk cannot drift apart, and it needed no generator change. The 263 hits the
  report decided (R39's count: 76 newly secret-bearing operations plus 187 on allowed rows; the rule hits 305 with the
  42 already secret-bearing on main):
  - **38 mints** (a `CLASS` row, Write with the MINT note, `SECRET`, and `RETRY SafeToRepeat`): sts's and eks-auth's
    three, deadline's five fleet and queue role mints, s3 `CreateSession` (S3 Express session keys), s3control
    `GetDataAccess`, ssm `GetAccessToken`, lakeformation's two, gamelift's three, finspace-data's two, emr's and
    emr-containers' session credentials, datazone `GetEnvironmentCredentials`, signin `CreateOAuth2Token`,
    amplifyuibuilder's two, bedrock-agentcore's four token reads, connect `GetFederationToken`, ivs-realtime's and
    ivschat's tokens, mwaa's two, license-manager `GetAccessToken`, redshift's and redshift-serverless'
    `GetIdentityCenterAuthToken`, workmail `AssumeImpersonationRole`, and kinesis-video-signaling's TURN credentials;
  - **36 stored or made secrets read back** (`SECRET` alone, the class unchanged): stream keys, challenge passwords,
    tunnel tokens, client secrets, a cluster's shared secret, a domain transfer password, payment plaintext, join
    tokens and artifact keys, with datazone `GetConnection` held only when `withSecret` is true (`secret_when`);
  - **24 plain `ALLOWED` rows**, each with its reason (a role ARN, a Secrets Manager ARN, a public key, a revision or
    confirmation token usable only with the caller's own credentials, message text, an empty grant type);
  - **24 `ECHO` rows**: real secrets configured on a resource and echoed as one optional member of its description or
    of a write's result (amplify's basic-auth credentials, dms's endpoint passwords, rds's 29 and redshift's 17
    operations that echo `PendingModifiedValues.MasterUserPassword`, ec2's VPN pre-shared keys, …). `SECRET` would
    fail the whole call closed whenever the member is absent, the usual case, and for a write after it ran, so they
    stay as on main: if AWS echoes the value, the model sees it. The report proposed `SecretBearing::WhenPresent` (hold
    what the walk finds, do not fail closed when nothing is found) and left it to the owner.
- **The walk finds a name in any case.** `walk`'s `named()` matches `NAMED` with `eq_ignore_ascii_case`. Not for the
  three mints, but because a check of every secret-bearing operation found ecr's and ecr-public's
  `GetAuthorizationToken` (`authorizationToken`) and lightsail `GetInstanceAccessDetails` (`password`) failing closed
  on every call on main ("no member of its output was found to hold it"). Seven older `SECRET` rows still hold nothing
  and fail closed on every call (apigateway's `value`, appsync's `id`, lightsail's key pairs): left, as below.
- **R39's join fix** (theseus-xscd, the join's): `PrivateKey` in `NAMED`, the two rows the rule then asks for (App
  Mesh's `*TlsFileCertificate.privateKey`, a file path on the proxy, allowed; Amplify Backend's Apple provider
  `PrivateKey`, an echoed secret, ECHO), and `aws::tests_mints::lightsails_ssh_access_holds_its_private_key`.
- The generator's doc (`examples/theseus-aws-catalog-gen.rs`) names the rule and its decisions.

**How it is proven.**
- **The session:** the catalog's suite (18 lib tests, `every_table_row_names_real_operations` holding every new row to
  a real operation; `tests/catalog.rs` 10 with `GOLDEN` unchanged; `tests/mints.rs` 1) and the core's `aws::secret`,
  `tests_mints`, `tests_handles` and `tests_secret_shapes`; the rule 0.58 to 0.78 s in a debug build. Five plants, each
  failing: each step-1 `SECRET` row removed (`left: "W"`, `right: "W 🔑"`, and `meta.secrets` with the value in the
  text); `GetDelegatedAccessToken`'s `CLASS` row removed (`R 🔑`); s3 `CreateSession`'s `SECRET` row removed (three
  findings naming it); kms `GetParametersForImport` made secret-bearing (its allowed row stale); `named()` exact again
  (ECR's token unheld). Its gate on a root VM: 3,030 run, 2,997 passed, the 33 known L1 failures of that VM
  (theseus-pv6i).
- **The review** (R39, review commit 3cb39487 on fc59e7f6, the join fix 7e73ab24 on top): fmt, the test build (26.5
  min, cold), clippy `-D warnings`, theseus-protocol 31 of 31, shape; the report's selection **216 of 216**; **the
  whole workspace suite 3,045 of 3,045** (`--retries 0`). **Row by row against the CLI's own botocore models**
  (aws-cli 2.34.15, the catalog's source; every operation's classification dumped on main and on the branch): 76
  operations changed, secret-bearing operations 55 to 131, 27 reads became writes, 10 writes went from `NonRepeatable`
  to `SafeToRepeat`, none became IaC-only or cost-bearing, and no earlier row shadows a new one. Every MINT is a fresh
  credential per its doc; every `SECRET` member holds a secret; every plain `ALLOWED` reason holds (the two the report
  took from memory, lightsail `GetBucketAccessKeys` and eks `License.token`, are settled by the models' own docs).
  **Plants:** the report's seven all caught; of R39's four, the stale-row check (the rule's endings cut to `Token`:
  16 rows stale) was caught, and three passed unnoticed: a wrong allowed row (s3 `CreateSession`'s members allowed with
  a reason), a step-3 mint's `CLASS` row removed, and a `RETRY` row removed (theseus-b586). **FAST:** `classify` cost
  755 ns an operation on the branch against 778 on main (release, every operation, settled, CPU pressure 0.1 and 0.7
  %); nothing on the start or turn path (static tables read when a call is planned or `aws_describe` answers; `named()`
  runs only inside `hold`).
- **Live** (R39): a scratch daemon of the review commit's frozen debug build, a fresh state dir under a transient user
  unit, the stand-in model, and one bound AWS account with an invented id and key whose endpoint was a loopback
  stand-in for AWS: no request and no signature left the machine. `aws_describe` showed the three mints `"class":
  "write"`, `"label": "W 🔑"`, `"retry": "safe_to_repeat"` and the MINT note (on main the two STS ones read `R` and
  eks-auth's `W`, none with 🔑); s3 `CreateSession` and ecr `GetAuthorizationToken` the same; datazone `GetConnection`
  `R 🔑` with `"secret_when": "withSecret"`. `aws_call ecr GetAuthorizationToken` answered with the handle
  `aws-secret:GetAuthorizationToken#authorizationData[0].authorizationToken`, the token nowhere in the history or the
  state dir (main fails closed).

**What the review found.**
- **theseus-xscd (P2, security): one leak main does not have.** lightsail `GetInstanceAccessDetails`' output holds, for
  SSH, `privateKey` (the temporary SSH private key) and `certKey`, unmarked and not in `NAMED`, beside RDP's
  `password`. Main held nothing there, so every call failed closed. The any-case walk holds `password`, so an SSH
  answer carrying an empty `password` (the model's doc: empty "if the password for your new instance is not ready
  yet") no longer failed closed, and the private key and the certificate reached the model, the history and the WAL.
  Shown statically, by a probe test through `aws.call` (the invented key in the tool's text; main fails closed both
  ways), and live (the key in the history and in WAL segment `000000001.seg`). The join fix closed it, proven on the
  review commit: the selection 217 of 217 with the new test, both SSH cases held, and its plant (the `NAMED` line
  removed) failing the new test and the rule (both new rows stale). With it, SSH access works through `aws.call`,
  where main failed closed.
- **theseus-37ds (P2, security, older than this branch):** the rule finds a credential by its name, so real
  credentials named otherwise pass it and reach the model through `aws.call` today: grafana's API keys and service
  account tokens, cognito-idp's added client secrets and TOTP seed, ivs's stream keys and SRT passphrase, ec2's VPN
  configurations, storagegateway's CHAP credentials, ecs `ExecuteCommand`'s token, connect's participant tokens, and
  more. The issue proposes a broader rule and a check that a secret-bearing operation's every credential-shaped member
  is one the walk holds.
- **theseus-qan5 (P2, the owner's decision):** `WhenPresent` for the ECHO rows and the `SECRET` rows whose secret can
  be absent (codepipeline `PollForJobs` with no jobs; gamelift `CreateBuild` with a `StorageLocation`, which now fails
  closed after the build is made; a chat `CreateParticipantConnection`, likewise), with datazone `GetConnection`'s
  `withSecret` gate, which R39 could not settle from the catalog. R39: build it, never on a mint, and only on rows
  whose member the walk recognizes; meanwhile mediapackage's two credential rotations can be `SECRET` (mints), cognito
  `CreateUserPoolClient` `secret_when GenerateSecret`, and three ECHO rows are not secrets per their models (appstream,
  datasync, fsx).
- **theseus-a3s3 (P2, the owner's decision):** refuse the AWS session mints to the model (`AssumeRole*`,
  `AssumeRoot`, `GetSessionToken`, `GetFederationToken`, `GetDelegatedAccessToken`), keeping `GetWebIdentityToken`:
  §4's catalog row says `AssumeRole` is "core only … never by the model", a chained session does not carry the work
  session's guard policies, and nothing is lost. Both went to the owner in the question walk (rows F4 and F5), not yet
  answered.
- **theseus-u4pe (P3):** the seven secret-bearing operations that always fail closed. **theseus-b586 (P3):** pin step
  3's decisions (an invariant test that every MINT-note operation is a Write and secret-bearing; all 38 mints in
  `tests/mints.rs`), drop the two `RETRY` rows that contradict their models (signin `CreateOAuth2Token` and
  amplifyuibuilder `ExchangeCodeForToken` spend a code, so a repeat fails rather than minting twice), and name
  datazone's ECHO members instead of `*`.
- **27 reads became writes** (the STS two, s3 `CreateSession`, ssm `GetAccessToken`, the deadline, emr, lakeformation,
  gamelift, finspace-data, bedrock-agentcore, redshift and kinesis-video-signaling mints among them). The operator's
  config has no `aws.call` line in `[policy.tools]`, so `[policy.aws]` decides: reads open, writes notify. These 27 now
  run with a notice instead of silently; none is named in the config.

**The join** (the amhb stack's first, under one lock `cloud-b11-amhb-join` and one gate; the gate is told in Item
234). The joiner waited for install #9's done line (22:18:25), then took the lock at 22:19:15 with
main = origin/main = 02de4b70. The merge: no conflict (no file in common with main since 57f265f2), the cloud files
removed, then `aws-mints/joinfix.py` (five edits in three files: `NAMED`, the two rows, the key's invented value, the
test), each "applied", a rerun a no-op; staged 7 files, +1,086 −15, tree 2b4ea2b7, the dry run's, and aws-mints' 7
files byte for byte R39's 7e73ab24; the names family of the scrub 0 (its URL hits the tests' invented fixtures);
tables.rs 1,799 lines, the largest; the store at format 23. The signed merge **c500c3c1** (02de4b70 and 430e30af),
22:19:25. Before the gate, the stack's 535 targeted tests passed, among them theseus-core's `aws::` 145 with the join
fix's test and the rule's two, and the catalog's 29. The stack's gate added aws-mints' 5 tests (the catalog's mints
test, tests_mints' 2, the rule's 2) to the voice gate's 3,096. Pushed with the stack at 22:49:39; the branch deleted on
origin; done line 22:50:05; theseus-ye7o and xscd closed with c500c3c1.

**The install** (not installed yet: on install #10's list). On the owner's daemon, 38 credential mints and 36 stored
secrets read back are secret-bearing, each to a reviewed decision; the walk finds a name in any case, with Lightsail's
SSH private key held by name, so an SSH access answers with handles where main failed closed; ECR's and ECR Public's
`GetAuthorizationToken` answer with a handle instead of failing closed. **27 operations that were reads are writes, so
they run with a notice** under the operator's `[policy.aws]` `write = "notify"` (none is named in the config; R39
checked). No config key.

**Divergences.** The brief expected two of the three mints to fail closed on their rows alone; the models' sensitive
marks held all three, and the any-case walk was built for ECR and Lightsail instead. The brief's rule hit 288
operations; the session tightened it (`NextToken`, a tag's key, rows that name a member) to 263 decisions. The ECHO
rows stay as on main, by design, until the owner decides `WhenPresent`. Two `RETRY` rows contradict their models
(harmless: a repeat fails). aws-toolset.md §3.1 and §4 are amended in this version (Part I).

**Known gaps.** theseus-37ds (P2, credentials the rule cannot see; older than the branch); theseus-qan5 and
theseus-a3s3 (P2, the owner's decisions, walk rows F4 and F5, unanswered); theseus-u4pe and theseus-b586 (P3). The
rule is a test over the compiled catalog, so a weekly model update that adds a credential-shaped member fails the
build until a person decides it.

### Item 233. Health counts the owner's own sessions apart from the imported and erased ones, on the projection's path and the fallback, reading one META record a tag, and the CLI and the cockpit name them; with R40's join fix, theseus-sim's lifecycle bench counts every session key from health again (theseus-revl; found by local reviewer R32 at imported-skip's review, Item 219; the eleventh cloud batch's health-imported session, fired 2026-10-06 18:10 from 57f265f2, Sonnet 5.5, its report at 18:42; ec4ed68d and bc42796f; reviewed 20:14 to 21:30 by local reviewer R40, the first of its two, and accepted with a join fix; joined 22:50 at 47833aa9, a signed merge onto c500c3c1, the second of the amhb stack's three merges under one lock and one gate, by the batch-11 amhb joiner; not installed yet: on install #10's list)

**Why.** Health's `sessions` was every SESSION key: the index's projection adds one a key (`store.rs` `sums_of`), and
the kept-whole fallback (`session_totals`' `list_sessions`, used only until a store's terms are built) counts the same,
imported and erased sessions included. Since install #8's import of the owner's prior assistant history (21,779
episodes, each an imported session; Item 201's importer), `theseus health`'s first line and the cockpit's Systems view
said "sessions 21,785" where the owner's own were a few hundred, while `turns` stayed as it was. R32 found it with a
probe while reviewing imported-skip; the importer's reviewer R24 (Item 201) had recommended the same. theseus-revl (P3)
asked for the two apart.

**What landed** (theseus-protocol, theseus-core's `rpc/`, the CLI's render, the cockpit, theseus-sim's lifecycle
bench by the join fix; the merge 18 files, +413 −13, without the cloud files; one additive protocol type, no package,
no config key, and no store format change (23 stays 23)).
- **The wire** (ec4ed68d). `import::HealthImported { sessions, erased }` and `HealthResult.imported`, with
  `#[serde(default)]`; `cockpit/src/protocol.gen` regenerated (`HealthImported.ts` new). `sessions` keeps its name and
  now means the owner's own. theseus-protocol's lib.rs went to its raised ceiling, 2,719 of 2,719 (from 2,715, with the
  reason in `scripts/long-files.txt`).
- **The counts** (ec4ed68d). `Core::session_totals`: on the projection's path, own = the keys less the tags'
  `sessions`, imported = the tags' `sessions` less their `erased`, erased = the tags' `erased`, with the tags' counts
  from `imported_counts` (`import::write::list`: a META prefix read, one record a tag, none a session); on the
  fallback, the same three from the session records it already reads. An imported session adds no turns, tokens or
  cost to the projection, so `turns`, usage and cost keep their meaning. `import_batch` writes a tag's META counts in
  the same append as the episodes they count, so during an import the counts never lag the keys.
- **The words** (bc42796f). The CLI's `render/imported.rs` (`sessions_words`, one `mod` line in render.rs, which stays
  at its ceiling of 3,100) puts them on health's first line: `sessions 2 · imported 30 · erased 20 · turns 2 · …`, the
  imported and erased counts grouped (`21,151`), the own count unformatted as before; the golden `health_imported`
  (the `health_json` golden unchanged: `--json` prints the daemon's value as sent). The cockpit's `lib/sessionwords.ts`
  and one field in `views/Systems.tsx`: "imported sessions N · erased M" beside "sessions · turns".
- **R40's join fix** (theseus-sim's `lifecycle.rs`): `lifecycle::health_keys`, health's `sessions` plus the import's
  held and erased sessions (an older daemon's `sessions` alone), at the bench's two readers, with
  `the_keys_from_health_hold_the_imports_sessions`.

**How it is proven.**
- **The session's tests:** `rpc/tests_health_imported.rs` (new): four live sessions, imports of 300 and 200, one tag
  erased; at each stage own equals `live_sessions().len()`, imported and erased equal the tags', and the three sum to
  `session_count`; then the same through the fallback (a store whose terms an older writer left not whole). The reads
  at health (`records_read_here`) at 21,151 imported sessions in two tags: **0 with no import, 1 at 1,000 (one tag), 2
  at 21,151 (two tags)**: one record a tag. Two plants caught (the projection's whole count back: "left 303 right 3";
  the CLI's import words dropped). `npm test` 92 with `sessionwords.test.ts`; theseus-protocol 31; the CLI 169.
- **The review** (R40, review stack 6f314bf8, the join fix 9a87a662, then bench-fair's merge f0481d8f, on fc59e7f6):
  the build, clippy `-D warnings`, theseus-protocol 31 of 31, shape; the families **266 of 266**; the cockpit 92 and
  `tsc -b` clean; **the whole suite on the stack 3,046 of 3,047**, the one red theseus-jtrc's known load flake
  (`bench_profile`'s first-byte retry test, passing alone; Item 236 fixes it). **Seven plants, all
  caught**: the report's two and four of R40's (the fallback counting every session as the owner's; the held count
  keeping the erased; `#[serde(default)]` dropped, so the CLI cannot read an older daemon's health, three goldens
  failing; the imported counts from a read of every session record, the reads test failing with (4, 1,004, 21,155)
  against (0, 1, 2)), and a cockpit plant.
- **The owner's shape** (R40): live and scaled, a scratch daemon with 3 own sessions, a tag of 4,000, a tag of 1,000,
  then that tag erased; at each stage own + imported + erased equalled the keys main's build counted on the same store
  (3; 4,003; 5,003; 5,003). In process, the owner's exact shape (300 own, one tag of 21,779, then erased): health read
  **0, 1 and 1** records and said (300, 0, 0), (300, 21,779, 0) and (300, 0, 21,779) against 22,079 keys.
- **Wire, both ways, live:** the new daemon with the owner's installed CLI (17:22 that day) prints `sessions 3 · turns
  3` with no import words, its `--json` passing `imported` through; main's daemon with the new CLI prints every key
  and no import words (the serde default gives zeros); neither cockpit fails against the other daemon.
- **FAST:** health is on the cockpit's 2 s poll; its new work is the tags' records (two at most here). An A B B A of
  200 calls a run on the 5,000-session store (debug, beside builds): new 11.41 ms p50 against main's 10.88, where one
  build's two runs differ by 0.7 ms; no cost past the noise. `session_totals` runs only in `health`: nothing on the
  start or turn path.
- **Live, the report's check** (R40, a fresh scratch daemon, the stand-in model): two asks, then `theseus import
  openclaw` of two synthetic files (30 and 20 episodes), then `theseus import erase` of the second tag; the first line
  read `sessions 2 · turns 2`, then `sessions 2 · imported 50 · turns 2`, then `sessions 2 · imported 30 · erased 20 ·
  turns 2`; `--json`'s `imported` `{"erased": 0, "sessions": 0}`, `{… 50}`, `{"erased": 20, "sessions": 30}`; the
  erase "20 sessions and 20 nodes tombstoned in 1 frame (52 ms)"; the cockpit's Systems words the same.

**What the review found.**
- **One consumer the report missed: theseus-sim's lifecycle bench.** With `--store` it runs on a copy of a real store
  (the wal-synced lane ran it on the owner's), where it read health's `sessions` as every key twice: the restore row
  checks that the restored store serves every session the restore counted, and the budgets scale the cold, vault and
  kill budgets with the store's parked sessions. Live on a copy of the shape store: "8 of the 5008 session(s) the
  restore counted: SESSIONS MISSING", the bench failing, and the cold budget 50.2 ms where main's daemon gave 150.2 (on
  the owner's ~22,000 keys, 250 ms would have fallen to about 60). The join fix: "5008 of the 5008", the budget 150.2
  ms; theseus-sim's lifecycle tests 8 of 8.
- **theseus-mce3 (P2): an erase's count is wrong after a cut erase, for good.** `erase` writes the tag's `erased` count
  only in its last frame, and a rerun skips what is erased already. Live under a fake busy PSI (the erase waiting the
  store's 10 s bound between frames), SIGKILL after two frames (4,000 sessions tombstoned): health said `imported
  10,500` throughout and after the restart; the rerun ("erased …: 6000 sessions … in 4 frames") left `imported 4,500 ·
  erased 6,000` against the truth, 500 held and 10,000 erased. It is the importer's own erase count (Item 201), on main
  already (`import list` says the same); health only puts it on the first line. Filed for batch 10's daemon-stops, whose
  stop between an erase's frames (theseus-autz) will make it common. The owner's daemon has never run an erase.

**The join** (the amhb stack's second; the lock and the gate are told in Items 232 and 234). The merge onto c500c3c1:
`rpc/methods.rs` and `rpc/mod.rs` auto-merged with main's own hunk each since fc59e7f6 (the cb and voice stacks'
changes), no conflict; the cloud files removed; `health-imported/joinfix.py` (four "done" lines: the helper, the
budgets' count, the restore row's count, the test); staged 18 files, +413 −13, tree ec2a4889, the dry run's,
health-imported's 17 files with lifecycle.rs equal to R40's 9a87a662 less main's own hunks; `--check`'s five hits
ts-rs's trailing spaces in the generated `HealthImported.ts` and `HealthResult.ts`; the names family 0; lib.rs 2,719 of
2,719 and render.rs 3,100 of 3,100, both at their ceilings; format 23. The signed merge **47833aa9** (c500c3c1 and
8ab795ab), 22:19:29. Its tests in the stack's 535: theseus-core's `rpc::` 44 with `tests_health_imported`'s two, the CLI
crate's 169 with `render::imported`'s two and the golden, theseus-sim's 55 with the join fix's lifecycle test; the
cockpit 92 of 92 and `tsc -b` clean. The gate's suite gained its 6 tests. Pushed with the stack at 22:49:39, the branch
deleted on origin, done line 22:50:05; theseus-revl closed with 47833aa9.

**The install** (not installed yet: on install #10's list). On the owner's daemon, `theseus health`'s first line names
the owner's own sessions apart, "sessions N · imported 21,779", and the cockpit's Systems view adds "imported sessions
21,779". The installed CLI and cockpit ship with the daemon; an older CLI ignores the field, and a newer CLI reads an
older daemon's health as zeros. No config key.

**Divergences.** The brief's three steps landed as two commits, since the core cannot build against the old wire
type. The report said the lifecycle bench was unaffected; on a `--store` copy of an imported store it was not, and the
join fix carries it. The erase's count was left as found (daemon-stops' area).

**Known gaps.** theseus-mce3 (P2: the erase's count after a cut erase). theseus-protocol's lib.rs and the CLI's
render.rs are at their ceilings, so the next field or line in either needs a split (theseus-pf8a, theseus-wdcw) or a
raise. theseus-core's AGENTS.md line on health's fallback ("health's fallback totals alone read every session, since
the projection's count holds imported ones") is stale: owed by both reviews (theseus-p4fr's kind).

