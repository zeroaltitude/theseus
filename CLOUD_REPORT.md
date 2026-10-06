# Cloud report: discord-tests (branch cloud/20261006-discord-tests)

Commits: 3e48f514 (o2tm, sj0t, 0bq1), 4e31f233 (9ggu), d04d17e5 + d2e542f7 (yduk: the lock, then its test).

## Reproduction
The flakes did not reproduce here: the old binary in the brief's shape (tests_outbox + tests_live + tests_gateway, one
process, nice 19, 4 busy loops, 16 threads) passed 12 of 12 batches, and 60 runs of the card-settle test alone under load
passed. So the three tests_outbox fixes are argued from the code and proved by planted reverts, not by a before/after
count. A post-fix batch also showed one failure of my own new 9ggu test (see step 4).

## 1. theseus-o2tm (3e48f514)
Found: a fixed 300 ms sleep before asserting a refused PUT. Changed: `until(.., 10, ..)` for the refused PUT, then assert
the board unpinned. Plant: the `self.pin(c, m)` call in courier/board.rs removed: fails "timed out: the pin was asked
for and refused" (10 s). Restored and touched.

## 2. theseus-sj0t (3e48f514)
Found: the card was "first DM message containing fs.write", and the tool line names it too. Changed: the card is the
message whose id is in the card post's settled `detail.messages` (`card_message_id`, from `kernel.outbox_actions()` by
`kind_of == "card"`), so the check on its words still tests them. Plant: the settle edit skipped in `Lane::settled`
(courier.rs ~926): fails at the card-words assertion, card found.

## 3. theseus-0bq1 (3e48f514)
Found: not reproduced (42 loaded batches, 60 solo runs). My reading, **inferred, not observed**: the fourth pending post
is the bind notice. The test waits until the notice is on the fake, which is before the lane's settle of it is in the
store, so under load the settle is still pending when the count runs. Changed: wait for `pending == 0` after the bind
notice, before the fake goes Down; the count stays 3 (reply, card, settle wait), and its message now names each
pending post's kind and target (`pending_kinds`), so a recurrence says which post it is. Plant: `Outbox::closed`
writing the settle twice: fails "left: 4 right: 3" with `[card, reply, settle, settle]` listed. If it recurs with a
different fourth kind, that is a finding the message will show.

## 4. theseus-9ggu (4e31f233)
Changed: `Outbox::stopped()` (theseus-core outbox.rs, beside `stopping`: a `wait_for` on the `flight` watch, no polling,
true at once if the stop already began); `event_loop` selects the shard's next event against it and returns, board
"disconnected", log line "discord gateway loop ended at the daemon's stop"; taken before the loop. The live watch also
ends on the same stop, at its spawn in `serve` (runtime.rs), not in live.rs: it never ended before and holds
`Arc<Shared>`. The select adds no wait to a stop: it is woken by the stop the daemon already waits on.
Test `the_gateway_loop_ends_at_the_daemons_stop_and_leaves_no_core_behind`: core + binding, `run` task held,
`core.stopping_on("SIGTERM")`: `run` returns within 10 s, and after `rt.shutdown_timeout(30 s)` the core's `Weak` is
gone. Plant: the select's stop made `pending`: fails "the binding's run ends at the stop: Elapsed".
Note: my first version used `shutdown_timeout(5 s)` and failed once in 30 starved batches with "core outlived its
runtime" (count 2) and a twilight-http-ratelimiting actor panic in a lane's request polled during runtime shutdown. The
lanes, courier and route tasks do not end at the stop (not in my brief), so under starvation 5 s was not enough for
the runtime to wind down; the test now gives 30 s (it returns when the workers are done). 30 runs under load pass.
Whether the lanes and courier should end at the stop too is open; in a daemon they hold the core only until the
process exits.

## 5. theseus-yduk (d04d17e5, test d2e542f7)
Changed: `Shared::refuse_unbound` holds the lanes' lock across `Outbox::refuse_unbound`. Checked: nothing under it
(`open_targets`, `open_for`, `settle`, the kernel) takes the binding's locks or calls back; lock order is `lanes`
before `retired` everywhere (`lane_retires`, `unbind`, `bind_*`), outbox and kernel locks only inside `lanes`; no caller
holds `lanes` when calling it (`retires` calls `lane_retires` first, which drops it). Cost: the courier's pass holds the
lock across `open_targets` (memory) at every outbox change, and across store reads and a synced settle per post when it
refuses, while each lane's `retires` (twice a round) waits on it. The race cannot be ordered by a test: the test
(`a_place_removed_live_and_put_back_has_its_new_post_sent_not_refused`) removes and re-adds a channel live, waits on
health each time, and the new post settles Succeeded, nothing refused, message on the fake. 5 runs under load pass.

## Proof
- Whole binary, brief's shape, 30 batches on the 9ggu build: 29 pass, 1 fail (my 5 s test, above). 30 on the final
  build (before the yduk test was appended, an ordering slip of mine): 30 pass. After appending it: yduk + 9ggu tests
  5 runs each under load pass; the two two-life tests 30 runs under load pass; theseus-discord suite whole: 131 pass.
- Gate: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, features, clippy, cockpit, test
  build, reader rule ok; suite failed only on the known L1 tests (theseus-sandbox contract + bench, theseusd sandbox,
  root-daemon limit, theseus-pv6i) and nothing else. protocol types (no diff) and `cargo deny` (bans, licenses,
  sources) pass run by hand; the turn bench was skipped (NO_BENCH). The gate ran before the yduk test was added;
  that commit is a test only, passing alone and in the suite binary.

## Live check (maintainer; I could not run it, no owner keys; names are the rig's)
Start the stand-ins with `theseus-sim discord rig --dir <fresh dir>` (see `theseus-sim discord --help` for its flags,
it prints the REST and gateway addresses to put in a scratch config's `[discord]` rest_proxy / gateway_proxy), and a
scratch `theseusd --config <c> --socket <s> --state-dir <fresh>`.
1. Stop the rig's gateway, then `kill -TERM <the scratch daemon's pid>`: it exits in its usual stop time, and its log
   has "discord gateway loop ended at the daemon's stop".
2. Hold a post at the fake (its hold-writes option), remove the place from the bindings file, wait for health to drop
   it (about 2 s), put it back, release the hold: the held post settles sent (`outbox` Succeeded) and no
   `not bound here any more` row appears in the ledger.

## Open
Lanes, courier and route do not end at the stop (above). Docs: AGENTS.md of theseus-discord could add that the
gateway loop and bindings watch end at `Outbox::stopped`, and refusal runs under the lanes' lock.
