# Cloud report: push-once (theseus-celu.36, theseus-cny7)

Branch `cloud/20261005-push-once`, from `main` at 80ef1dea (store format 16, unchanged: nothing stored changes).
Started 08:35 UTC, report 10:25 UTC, on a 4-core VM. All times and numbers below come from this VM.

| Step | Commit | What |
| --- | --- | --- |
| 1 | 90a5121b | the measurement, and main's numbers |
| 2 | 3c13516d | each notification serialized once, one shared line per queue |
| 3 | 2cc6a3be | V8: `profile.use` and `session.recompile` name the surface |

**A slip in step 1:** `crates/theseus-core/src/tests_actor.rs` (step 3's tests, drafted early) went into 90a5121b by
mistake. Its `mod` line came only in 2cc6a3be, so in step 1 nothing compiles it and nothing changes. I didn't
rewrite pushed history. If you want it out of step 1, move it when you squash or rebase at the merge.

## What changed since the designs (the code wins)

- **The cap is in `outbound.rs`**, as the brief says: one ordered queue per connection, counted until written. I kept
  it exactly as it was. A shared line counts as one queued message, past `BACKLOG_CAP` a notification is dropped and
  counted with its stream, responses always go, and one `events.lost` follows the drain. `BACKLOG_CAP`, the
  `events.lost` type, S6's latest-wins queues and the disconnect of a slow client are untouched.
- **`node.written` is small.** It carries only `session_id`, `node_id` and `kind`, not the node's content, so the
  "large `node.written`" in the measurement is padded on purpose (48 KiB of id plus 17 KiB of kind). The largest real
  notification a turn sends is `turn.ended`, which carries the reply, so the measurement has a 48 KiB `turn.ended`
  row as well.
- **The stand-in model sends a reply in one delta** (`theseus-sim fake-model`'s `text_delta` is the whole text). On
  it, "each delta once" means once per turn, and a long reply can't get past a 4,096-message cap on its own. The live
  check below gets past the cap with many turns.
- **Every surface goes through `serve_connection`** (the socket, `--stdio`, the web UI's WebSocket, and Discord's
  in-process client), so they all share the one line. Only the tests' raw channels
  (`From<UnboundedSender<Message>>`) still read `Message`s. Each takes its own copy of the note's message and never
  causes a serialization. The output golden's tap and the narrator tests read messages, as before.

## Step 1: the measurement (90a5121b)

**Found.** `SessionBus::publish` did `msg.clone()` per watcher, `EventSink::send` cloned it again for the requester,
and each connection's writer serialized its own copy (`write_line`). That is N deep copies and N serializations per
notification.

**Changed.** Added `tests_push_once::measure_serializations_per_notification`, ignored by default. It publishes 200
rounds of a `model.delta`, a padded 64 KiB `node.written` and a 48 KiB `turn.ended` through a turn's `EventSink` to
1, 10 and 100 watchers on capped queues (`outbound::channel`). It writes every queue out the way a writer does (into
`tokio::io::sink()`) and prints the counts and the time per watcher. The counter is `outbound::counts`: thread-local,
test builds only, bumped where the bus copies a message and where a message or line is serialized.

Run it with:
`cargo nextest run -p theseus-core --run-ignored only -E 'test(measure_serializations)' --no-capture`

**Main's numbers** (debug build, this VM):

```
notification          watchers  ser/note  copies/note  ns/watcher
model.delta                   1       1.0          1.0       10129
model.delta                  10      10.0         10.0        5651
model.delta                 100     100.0        100.0        6243
node.written (64 KiB)         1       1.0          1.0     1250886
node.written (64 KiB)        10      10.0         10.0     1158317
node.written (64 KiB)       100     100.0        100.0     1292982
turn.ended (48 KiB)           1       1.0          1.0      934643
turn.ended (48 KiB)          10      10.0         10.0     1009578
turn.ended (48 KiB)         100     100.0        100.0      983281
```

## Step 2: serialize once (3c13516d)

**Changed.**
- `outbound.rs`:
  - A queue holds an `Item`: either `Message` (responses, and anything sent to one connection alone), which the writer
    serializes, or `Line(Arc<str>)`, a notification's NDJSON line with its newline.
  - `Note` wraps a notification on its way out and serializes it lazily (`OnceCell`) at the first queue that takes it.
  - `Outbound::notify(&Note, stream)` keeps the cap logic exactly as it was. A note that is dropped, or that reaches
    only raw channels, is never serialized. A message that doesn't serialize is skipped, as the writer always skipped
    one.
- `bus.rs`:
  - `publish` and `publish_all` build one `Note`.
  - `EventSink::send` builds one `Note` and hands it to the requester and then to `deliver`, so the requester and
    every watcher, including all-session watchers, queue the same `Arc`.
- `narrative.rs`: one `Note` per line for all subscribers.
- `rpc/server.rs`: `write_item` writes a `Line` as it is, and serializes a `Message` as before. `write_one` still sends
  `events.lost` after whichever item drains the queue.
- `rpc/methods.rs` (`profile_use`'s notification): this call site follows the new signature and nothing more.
- The wire is byte for byte the same: a line is `serde_json::to_string(&msg)` plus `\n`, exactly what `write_line`
  wrote before. No protocol type changed, so `protocol.gen` is unchanged.

**The measurement after** (same build profile and machine):

```
notification          watchers  ser/note  copies/note  ns/watcher
model.delta                   1       1.0          0.0       10648
model.delta                  10       1.0          0.0        1974
model.delta                 100       1.0          0.0        1617
node.written (64 KiB)         1       1.0          0.0     1430118
node.written (64 KiB)        10       1.0          0.0      137278
node.written (64 KiB)       100       1.0          0.0       13748
turn.ended (48 KiB)           1       1.0          0.0      825623
turn.ended (48 KiB)          10       1.0          0.0       72522
turn.ended (48 KiB)         100       1.0          0.0       12018
```

Serializations per notification fell from N to 1, and deep copies from N to 0. At 100 watchers the time per watcher
fell from 6.2 µs to 1.6 µs for a delta, and from 1.29 ms to 13.7 µs for a 64 KiB notification. With one watcher it
is the same work as before, now done on the publisher's thread rather than the writer's.

**Proof, offline:**
- `tests_push_once::the_requester_and_two_watchers_write_one_line_byte_for_byte`: the requester (also a watcher) and
  two watchers each queue the delta once, all the same `Arc`, with one serialization and no copy. Written through
  `write_item`, the bytes equal the old `to_string + "\n"` three times.
- `tests_push_once::a_notification_no_connection_queues_is_never_serialized` covers three cases, each with no
  serialization: no watcher (0 serializations, 0 copies), a raw channel (0 serializations, 1 copy of its own), and a
  capped queue already at `BACKLOG_CAP` (dropped, 0 serializations).
- `tests_push_once::a_large_notification_to_a_hundred_watchers_is_one_copy`: a 64 KiB `node.written` to 100 capped
  watchers and the requester is one serialization, and `Arc::strong_count` is 101 with every queue holding the same
  pointer.
- These assert exactly what they asserted before:
  - outbound.rs's cap tests: `events.lost { dropped: 3, streams: [executions, narrative, session:ses_1] }`;
  - bus.rs's three tests;
  - narrative's tests;
  - all of `tests_push` (the lag test's `events.lost` streams `["executions"]` and the exact count).
  - Through the subset filter `test(push_once) | test(/^bus::/) | test(/^outbound::/) | test(tests_push::) |
    test(narrative::)`: 22 of 22 pass.
- The core's output golden is unchanged (no golden line moved).
- `tests_m3::a_plain_turn_stays_within_its_frame_budget` passes.
- `theseus-sim bench turn --check --runs 5 --burst 0`, both kinds:
  - plain: 5 frames (budget 5), p50 42.9 ms after against 47.4 ms on main;
  - tool-call: 9 frames (budget 9), 95.4 ms against 102.1 ms;
  - this VM is noisy, so the turn bench didn't move.

**Planted reverts** (each restored, `touch`ed, and `git status` checked):
1. A deep copy per watcher again (in `deliver`, `note.message().clone()` and a fresh `Note` per watcher). It broke
   `the_requester_and_two_watchers_write_one_line_byte_for_byte` (serializations `(3, 0)` against `(1, 0)`) and
   `a_large_notification_to_a_hundred_watchers_is_one_copy` (`(101, 0)` against `(1, 0)`).
2. The cap skipping shared items (`queue` not counting an `Item::Line`). It broke
   `outbound::tests::past_the_cap_notifications_drop_until_the_queue_drains` (dropped 0 against 2) and
   `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` (no `events.lost` within 30 s).
3. `events.lost` not sent after a shared item drains the queue (`write_one` sending it only after an
   `Item::Message`). It broke `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` (timed out
   waiting for a line at tests_push.rs:762).

**Under load** (AGENTS.md's recipe: nice 19, four busy loops at nice 0, killed by their pids). I ran the push, bus,
outbound, narrative and push_once tests 5 times:
- 21 of 22 passed in all 5 runs.
- `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` hit nextest's 120 s kill in each run.
  **It does the same on main under the same load**: I checked out 90a5121b's theseus-core and got the same timeout at
  121 s. It opens thousands of sessions and pushes past the cap, and at nice 19 behind four spinning loops on four
  cores it starves.
- Under two busy loops it passed 5 of 5 (9.3 to 11.5 s).
- I'd count this as a finding about the test under full load, not about this change. Consider a smaller `EVENTS` for
  it, or a longer slow-timeout in `.config/nextest.toml`.

**A finding on the way: the output golden was at its stack's edge on main.**
`tests_output::the_cores_output_matches_its_golden` runs whole turns with its future pinned on the test thread's
2 MiB stack.
- On main it overflows at `RUST_MIN_STACK` 2,000 KiB and passes at 2,048 KiB, so it had under 48 KiB of room.
- This change added a few debug frames and tipped it over: SIGABRT, "has overflowed its stack". The deepest point
  (gdb) is not in this change's code. It is `ToolRuntime::complete` → `Kernel::accept_completion_with` → serde's
  decoding of an `Execution`, beneath the scenario's async frames.
- The fix is in the same commit: the test boxes its two scenarios (`Box::pin(conversation(..)).await`, likewise
  `budget`), which moves their state to the heap. It now passes at 1,900 KiB (fails at 1,800), roughly 150 KiB of
  room. Its golden lines are unchanged.
- Any change that grows the turn's futures could have tipped it, so other branches may meet the same thing. If one
  does, this commit fixes it.

## Step 3: V8, every operator act's `by` (2cc6a3be)

**Changed.** `profile.use` passes `conn.actor(None)` to `switch_profile`, so `profile.changed`'s row, the method's
answer and its notification carry it. `session.recompile` passes `conn.actor(None)` to `request_recompile`, which
`context.recompile_requested`'s row carries. Neither method's params carry an author, so `None`. `Conn::actor`'s doc
now names both acts. Only the `by` argument changed in `profile_use`, so route-gaps' work beside it is untouched.

**Every method that takes a `Conn`, as I found them:**
- **Already name the actor:**
  - through `conn.actor`: task.cancel, wake.cancel, execution.cancel, execution.stop;
  - through `conn.answerer`, which goes through `actor`: action.confirm, policy.tighten, policy.untighten,
    policy.trust, place.publish, the ontology writes and proposal answers, the MCP methods in rpc/mcp.rs,
    judge.label (author `None`, Discord ids), memory.label, pack.promote and pack.rollback, the judge runs, and
    extension.revoke.
- **Use the label as the connection's identity, which is right because it keys that connection's subscriptions, not
  an act:** session.watch and session.unwatch, executions.watch and executions.unwatch, narrative.watch and
  narrative.unwatch, session.wait's slot, and turn.submit's requester key in its `EventSink`.
- **Read only the surface:** session.open (`session_open_on(surface)`, for the MCP server's sessions), and
  aws.bootstrap and aws.confirm_alerts (CLI only).
- **The budgets:** budget.list is a read. A budget reset is an answer through action.confirm, already `the CLI` (see
  `tests_m3`'s `budget.reset` test).
- **turn.submit's author** falls back to the label (`p.author.unwrap_or(conn.client)`), so a CLI message's author is
  `sock#7`. I left it, as the brief says. My view: it should follow, as `conn.actor(p.author.as_deref())`, so a
  message from the CLI or the web UI is authored by `the CLI` or `the web UI`. But that moves the author into nodes,
  history and the compiled transcript (the model sees authors), and probably the output golden, so it is a decision
  for the owner, not a quiet change.

**Proof.** `tests_actor.rs` drives each act through `serve_connection` with three clients:
`Client::new("sock#7", Surface::Cli)`, `Client::new("web#35", Surface::Web)` and an unnamed `tide#3`. It reads the
newest row:
- `a_profile_use_names_its_surface_not_its_label`: the answer's and the `profile.changed` row's `by` are `the CLI`,
  `the web UI` and `tide#3`.
- `a_recompile_names_its_surface_not_its_label`: `context.recompile_requested`'s `by` is the same three, with
  strategy `transcript`.

**Planted reverts:**
- `profile.use` given `conn.client` again broke `a_profile_use_names_its_surface_not_its_label` (`"sock#7"` against
  `"the CLI"`).
- `session.recompile` given `conn.client` again broke `a_recompile_names_its_surface_not_its_label`
  (`(Some("sock#7"), Some("transcript"))` against `(Some("the CLI"), ..)`).

## The live check (the maintainer's)

A scratch daemon on the stand-in model, with a fresh state dir, Discord off, and the web UI on loopback (port 7533,
so it stays clear of the operator's 7433). Build first; the commands use the build's `target/debug` (or
`target/release-thin`).

```bash
D=/tmp/push-once; rm -rf $D; mkdir -p $D
B=$PWD/target/debug
cat > $D/rules.json <<'EOF'
[{"when": "long", "text": "PLACEHOLDER"}, {"when": "", "text": "ok"}]
EOF
python3 - $D/rules.json <<'EOF'
import json, sys; p = sys.argv[1]; r = json.load(open(p))
r[0]["text"] = " ".join(f"tidewater lantern {i}." for i in range(4000)); json.dump(r, open(p, "w"))
EOF
$B/theseus-sim fake-model --addr 127.0.0.1:9461 --rules $D/rules.json & FAKE=$!
cat > $D/theseus.toml <<EOF
[server]
state_dir = "$D/state"
socket = "$D/theseus.sock"
[model]
api_base = "http://127.0.0.1:9461"
[secrets]
anthropic_api_key = "env:PUSH_ONCE_FAKE_KEY"
[discord]
enabled = false
[web]
port = 7533
EOF
PUSH_ONCE_FAKE_KEY=sk-not-a-key $B/theseusd --config $D/theseus.toml & DAEMON=$!
T="$B/theseus --socket $D/theseus.sock"
S=$($T sessions open | tail -1); echo "$S"
```

1. **One line for every watcher.** Run two JSON watches into files, open `http://127.0.0.1:7533/` on session `$S`,
   then ask:

   ```bash
   $T --json watch $S > $D/w1.ndjson & W1=$!
   $T --json watch $S > $D/w2.ndjson & W2=$!
   $T ask -s $S "a long one, please" > /dev/null
   kill $W1 $W2
   cmp $D/w1.ndjson $D/w2.ndjson && echo identical
   grep -c '"model.delta"' $D/w1.ndjson
   ```

   Expect `identical` and one `model.delta` per turn (the stand-in sends a reply as one delta), and the web UI
   streams the reply. `--json watch` re-serializes what it parsed, so for the wire's own bytes also tap the socket
   raw:

   ```bash
   python3 - $D/theseus.sock $S <<'EOF' > $D/raw.ndjson &
   import socket, sys, json
   s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
   s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "session.watch",
                          "params": {"session_id": sys.argv[2]}}) + "\n").encode())
   f = s.makefile("rb")
   for line in f: sys.stdout.buffer.write(line); sys.stdout.flush()
   EOF
   ```

   Run it twice into two files during one `ask`. The notification lines (all but each file's first response) must
   be equal byte for byte. Kill the taps by their pids.
2. **The cap.** Start a third `$T watch $S > $D/w3.txt 2>&1 & W3=$!`, then `kill -STOP $W3`. Then run about 600
   turns, since one turn sends about ten notifications and the cap is 4,096:
   `for i in $(seq 600); do $T ask -s $S "short $i" > /dev/null; done`. Then `kill -CONT $W3`, wait a few seconds,
   and run `kill $W3`. Expect `$D/w3.txt` to hold one line saying events were lost, naming `session:$S`, and
   `$T health`'s push line (`push: … · lost N`) to show `lost` above what it read before the turns.
3. **`by`.** Run these:

   ```bash
   $T profile use glm
   $T sessions recompile $S --strategy transcript
   ```

   Then press Recompile on `$S` in the web UI (Session deck → Recompile → transcript), and read the rows:

   ```bash
   $T --json ledger --kind profile.changed -n 1
   $T --json ledger --kind context.recompile_requested -n 2
   ```

   Expect `by` to be `the CLI` for the profile switch, then `the CLI` and `the web UI` for the two recompiles, and
   never a `sock#` or `web#` label. Stop with `$T shutdown`, then `kill $FAKE`.

I couldn't run these here: the brief makes the live check the maintainer's. The CLI's flags are checked against
the build's `--help` (`sessions recompile <SESSION> --strategy transcript`). The web control's labels and the
scratch config's keys are read from the cockpit's source and the template, not from a running daemon.

## Left, uncertain, and for the owner

- **turn.submit's author** (above): should it follow `actor`? It's a decision because it changes what the model reads.
- **The output golden's stack**: fixed here with about 150 KiB of room. A sturdier fix would run the golden on a
  thread with an explicit 8 MiB stack.
- **The lag test under full load** times out on main too (above).
- **The measurement uses a debug build**, as the suite does. Release numbers will be smaller, but the ratios (N to 1
  serializations, N to 0 copies) don't depend on the profile.
- **Docs to change at review:**
  - `crates/theseus-core/AGENTS.md`, "The protocol server": the bus and the queue now share one serialized line per
    notification (`outbound::Note`, `Item::Line`).
  - The same file's Tests list: add `tests_push_once.rs` (with its ignored measurement) and `tests_actor.rs`.
  - Review 2's S6: done for serialization; latest-wins and the slow-client disconnect remain.
  - roadmap-v1.1's push2 and V8 rows.
  - stage2 §2.5: the queue's item is now a message or a shared line.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit:
- **Up to the suite**, every phase passed (fmt, shape, features, clippy, cockpit, test build, reader rule).
- **The suite** failed only on the 33 known L1 tests, the same set at each step (theseus-pv6i): 19 of
  theseus-sandbox's contract tests, its bench's `spawn_100`, and 13 of theseusd's sandbox tests.
  - Step 1: 2,562 tests, 2,529 passed.
  - Step 2: 2,565 tests, 2,532 passed. One flaky test passed on its third try: theseus-sim's
    `the_kernel_holds_its_invariants_under_seeded_faults`, the kernel sim's put-back check (theseus-81ig, on the flaky
    list).
  - Step 3: 2,567 tests, 2,534 passed.
- **The phases after the suite**, which I ran by hand at each step, all passed:
  - protocol types: `protocol.gen` unchanged;
  - `theseus-sim bench turn --check --runs 5 --burst 0`: frames_plain 5 of 5, frames_tool 9 of 9;
  - `cargo deny --offline check`: advisories, bans, licences and sources ok.
  - The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`.
- None of the listed timing tests failed in any gate run. theseus-kernel's `the_deadline_stops_the_whole_tree_too`
  passed in all three.
