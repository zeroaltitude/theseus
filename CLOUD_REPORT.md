# CLOUD_REPORT: turn-stack (theseus-b4sf)

Branch `cloud/20261005-turn-stack`, from main at 60b43fb6 (store format 20; this branch adds no stored field and no
format bump). Started 20:22 UTC, deadline 23:22.

## What changed since the issue was written (the code won)

- turn.rs was 3,452 lines of its 3,523 ceiling; it is 3,482 now (no ceiling change).
- `recall_live` and the golden's two halves were already boxed, as the brief said.
- The brief's claim about opt-level 0 holds, measured: a caller's poll frame keeps a slot the size of every future it
  builds, and frames share no slots. It is what held the depth (below), more than futures held inline.
- Setup note: this VM's disk allowance ran out in the first gate (target/debug/incremental had grown to 14 GB beside
  11 GB of deps). I deleted target/debug/incremental and ran every gate after with `CARGO_INCREMENTAL=0`. Nothing
  in the tree changed for it.

## Step 1: measured on the tree as cloned (no commit)

Bisection by `RUST_MIN_STACK` over one test binary, `TZ=America/Phoenix`, 64 KiB grain, three runs at each boundary
(all three agreed at every boundary below):

| test | overflows at | passes at |
|---|---|---|
| tests_output::the_cores_output_matches_its_golden | 1,856 KiB | 1,920 KiB |
| tests_m3::a_tool_loop_reads_a_file_and_the_prefix_never_changes | 832 KiB | 896 KiB |

Future sizes (`std::mem::size_of_val`, throwaway instrumentation, not committed):

| future | bytes |
|---|---|
| `TurnRunner::run` | 163,752 (160 KiB) |
| `run_inner` | 79,664 |
| `turn_body` | 36,984 |
| `catch_up` | 17,096 |
| `run_tools` | 16,008 |
| `compile_step` (turn_body's compile step, via route's `compile_routed`) | 11,008 |
| `compact` (the compaction) | 5,104 |
| `call_model` (the call step) | 1,200 |
| tests_output.rs's `turn` | 327,576 (two of run's: the call's temporary and the awaitee) |
| tests_output.rs's `conversation` | 336,008 |
| tests_output.rs's `budget` | 328,432 |

Where the depth sat: probes reading the stack pointer at each function's first poll, against the test thread's
base (KiB below the test's frame, deepest seen):

- golden: `conversation`'s first poll at 1,131 KiB; the helper `turn` 1,200; `run` 1,364; `run_inner` 1,417;
  `turn_body` 1,538; `run_tools` 1,570; toolrun's `run_calls` 1,544-1,583; `run_inproc` 1,776; painted-stack
  high water 1,936 KiB (a lower bound: stack probes touch pages without writing them).
- so the frames were: `conversation`'s poll frame about 1.1 MiB (it builds a dozen `turn()` futures of 320 KiB, each
  in a slot of its own), `run`'s 103 KiB (run_inner's 80 KiB future), `run_inner`'s 53, `turn_body`'s 121,
  `run_tools`' 32, `run_calls`' 35, and toolrun's `run_inproc`'s 170 KiB, the largest frame inside the turn.

So the depth was poll frames, not futures held inline: `#[tokio::test]` holds a boxed half, but every poll frame on
the way down reserves a slot for every future its function builds.

## Step 2: the box at `TurnRunner::run`'s entry, and the loop's largest futures (76524812)

What changed, all in turn.rs:

- `pub fn run(&self, req) -> Pin<Box<dyn Future<Output = Result<TurnSubmitResult>> + Send + '_>>`, whose body is
  `Box::pin(self.run_body(req))`. The old body is `run_body`, with only its signature line changed, so queue-frames'
  edit at the body's end still merges. Callers (`self.runner.run(req).await` in rpc/methods.rs, rpc/driver.rs, the
  tests) are unchanged.
- The depth was mostly inside the turn once the entry was boxed, so the loop's largest futures are boxed too, with a
  small helper, `boxed(|| self.f(..))`, which builds the future in its own short frame and moves it to the heap (the
  caller's poll frame keeps the closure and a pointer; `Box::pin(f).await` in place would keep `f`'s slot):
  `run_inner` (now a fn that boxes `run_inner_body`, so `run_body`'s body is untouched), `turn_body`, `catch_up`,
  `run_tools`. Not boxed: `compile_routed` (turn/route_step.rs, route's gaps: left alone as the brief says) and
  `call_model` (1.2 KiB).

Bisection after each (pass at; overflow 64 KiB lower in every row; three runs at each boundary agreed):

| tree | golden | tests_m3 tool loop |
|---|---|---|
| as cloned | 1,920 | 896 |
| run's entry box | 768 | 640 |
| + run_inner | 704 | 576 |
| + turn_body | 640 | 576 |
| + run_tools | 640 | 512 |
| + catch_up (committed) | 576 | 448 |

After the boxes, probes put `conversation`'s poll frame at 88 KiB (was ~1.1 MiB) and the in-turn chain from `run`
to `run_inproc` at about 410 KiB; toolrun's `run_inproc` (170 KiB frame) is now the largest single frame, outside
turn.rs.

The allocations (size of each boxed future, from the golden): run_body 7,856 B once a turn; run_inner 5,704 once;
turn_body 16,136 once; catch_up 17,096 once; run_tools 16,008 per loop that runs tools. About 46 KiB of heap a turn,
where 160 KiB sat inline before.

`theseus-sim bench turn --runs 20`, debug builds of both trees, three interleaved pairs:

| | plain p50 | tool-call p50 |
|---|---|---|
| before | 73.2, 66.4, 62.7 ms | 163.4, 161.3, 149.0 ms |
| after | 66.8, 64.0, 62.5 ms | 161.8, 160.4, 146.5 ms |

Frames per turn unchanged: 5 plain, 9 tool-call (`--check` ok in every run).

## Step 3: the margin's test (f9bbc749)

`crates/theseus-core/src/tests_stack.rs`, `the_golden_conversation_runs_on_a_one_and_a_half_mib_stack`: the
golden's `conversation` (made `pub(super)`) on a `std::thread::Builder` thread with `stack_size(1.5 MiB)`, a
current-thread runtime inside, `rt.block_on(Box::pin(conversation(&mut out)))`. No environment variable: it passed
with `RUST_MIN_STACK=262144` set, and under nextest in the gate.

Proof:
- alone, three runs: pass, 3.5 s each.
- the test's own boundary (the size made a throwaway knob): overflows at 320, 384, 448, 512 KiB, passes at 576
  (three runs each). 1.5 MiB leaves about 960 KiB.
- planted, a 512 KiB `std::hint::black_box([0u8; 1 << 19])` in `turn_body` after `catch_up`, held across no await:
  this test aborts ("thread 'stack-margin' has overflowed its stack"); the golden at its default 2 MiB passes. A
  1 MiB array (`1 << 20`) overflows the golden too (at opt-level 0 `black_box` copies its argument, so the plant
  costs about twice its size). Restored, touched, `git status` clean.
- planted, the entry's box removed (`run` an async fn again, the loop's boxes kept): this test passes. Only the
  bisection shows it: golden 576 to 896 KiB, tests_m3 448 to 512. Why: with the loop boxed, the turn's future is
  about 8 KiB, so the slots it costs its callers are small; the entry's box matters most for a caller that builds
  many turns in one frame, as `conversation` does. Restored, touched, clean.
- under load (nice 19 beside four busy loops at nice 0, each a script `while :; do :; done`, killed by pid): **5 of 5
  failed, none by overflow**: each panicked in the conversation's own 30 s wait, "no wake due in 30 s"
  (tests_output.rs:256), after about 105-119 s. The golden fails the same way under the same load, on this branch
  and on the tree as cloned (`turn.rs` from abbda717: "no wake due in 30 s", 111.8 s). So it is the golden
  conversation's timing under starvation, inherited by this test, not the stack. It passes under the gate's own
  load. If the owner wants the margin test immune to it, a conversation without the wake scenario would do; I left
  it as the brief specified.
- the golden at the default stack: passes (in each gate, and alone).
- theseus-core's suite whole: in the gate's suite phase, every theseus-core test passed in the last gate (including
  the known flaky `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`, which failed in step 2's gate and
  passed in step 3's).

## Step 4: the daemon's workers

From the numbers: inside a turn the stack now needs about 410 KiB from `run`'s poll down to the in-process tool run,
plus the tool itself; the conversation, helper frames and all, fits in 576 KiB. A debug daemon's 2 MiB worker has
about 1.4 MiB above that for its own frames, so the fix covers the turn on a debug daemon's workers with room.

What the test does not hold: the frames above the turn in the daemon, the server's per-request task
(rpc/server.rs's `handle` and `dispatch`, then `turn_submit`). At opt-level 0 a dispatch `match` keeps a slot for
every method's future it can build; `turn_submit`'s slot for `run` is now a pointer, but the other methods' are
unmeasured. The maintainer's live check 2 (a daemon at `RUST_MIN_STACK=1572864`) measures exactly that. If it
overflows there, the smallest durable change is `.thread_stack_size(8 << 20)` on theseusd's runtime builder under
`cfg!(debug_assertions)` (main.rs); its cost is virtual address space only (stack pages are touched on demand), a
few worker threads' worth. I did not build it, and set no stack in theseusd's runtime or in .config/nextest.toml.

## The live check (the maintainer's)

1. On the merged tree:
   - `TZ=America/Phoenix scripts/gate.sh`: the golden passes at the default stack, and the new test passes.
   - `cargo nextest run -p theseus-core tests_stack`: one pass.
   - the golden's bisection once: build `cargo test -p theseus-core --lib --no-run`, then with that binary
     `RUST_MIN_STACK=$((576*1024)) TZ=America/Phoenix <bin> --exact tests_output::the_cores_output_matches_its_golden`
     passes, and at `$((512*1024))` aborts with "has overflowed its stack" (64 KiB either way on a different machine
     is expected; anything near 1.9 MiB means a box was lost in the merge).
2. A debug scratch daemon on a fresh state dir, Discord and the web off, the stand-in model:
   ```
   d=$(mktemp -d)
   target/debug/theseus-sim fake-model --addr 127.0.0.1:9448 --rules \
     '[{"when":"README","calls":[{"name":"fs_read","input":{"path":"README.md"}}],"text":"Reading."},{"when":"","text":"Hello."}]' &
   # $d/theseus.toml: [model] api_base = "http://127.0.0.1:9448" (and its providers'), Discord and the web off
   RUST_MIN_STACK=1572864 target/debug/theseusd --config $d/theseus.toml --socket $d/sock --state-dir $d/state 2> $d/log &
   target/debug/theseus --socket $d/sock ask "Say hello."
   target/debug/theseus --socket $d/sock ask "Read README.md and tell me its first line."
   target/debug/theseus --socket $d/sock shutdown
   grep -c "overflowed its stack" $d/log   # 0
   ```
   Both turns complete (the second with one `fs.read` call, answered `Done.` by the stand-in); the daemon is alive
   until the shutdown, and the log has no overflow. I did not run this here; the brief gives it to the maintainer.
   Kill the fake model by its pid afterwards.

## Left, uncertain, and for the owner

- The daemon's dispatch frames above the turn are unmeasured (step 4).
- toolrun.rs's `run_inproc` has a 170 KiB poll frame, the largest left on the turn path; boxing its largest awaits
  (in toolrun.rs, out of this task's scope) would take more off. toolrun.rs is at 2,494 lines.
- `compile_routed`'s future (route's gaps' file) is not boxed; at about 11 KiB plus it is small now.
- The margin test inherits the golden conversation's 30 s wake wait, which fails under heavy starvation on this VM
  (pre-existing, above).
- Docs: theseus-core's AGENTS.md could say, under the turn, that a turn's futures are boxed at `run` and at the
  loop's largest calls (theseus-b4sf), and that tests_stack.rs holds the margin; tests_output.rs's comment on the
  golden's boxed halves still reads right.

## The gate

Last run (step 3's tree, `CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): fmt,
shape, features, clippy, reader rule pass; suite 2,790 run, 2,757 passed, 33 failed, 21 skipped. The 33 are the known
L1 tests (theseus-pv6i: theseus-sandbox's 19 contract tests and its bench's `spawn_100`, and theseusd's 13 sandbox
tests). After the suite I ran the gate's later phases: protocol types unchanged (ok), and `theseus-sim bench turn
--check --runs 5 --burst 0`: frames 5 and 9 within budget, exit 0. Step 2's gate: the same 33 plus
`term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` (theseus-ynia, known); its later phases passed the same way.
