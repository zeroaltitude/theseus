# CLOUD_REPORT: discord-watch (theseus-u6v6, btt4, sn2z, nz3q, 02bq, 88cp)

All six steps are green offline. Code is in `runtime/live.rs`; tests in `tests_live.rs`. Nothing is on the start path
or a turn's path: the watch starts in `serve` after the places, and its work is one stat per tick. The store format
is untouched. No new dependency. A one-line `#[cfg(test)] pub(crate) use live::ticks as live_ticks;` is the only edit
to runtime.rs (plus the tests' seam in rpc_client.rs, `refuse_once`, cfg(test)).

## 1. theseus-u6v6, a failed bind is retried (64142406)
Found: `watch` set `bound = new` after a failed bind, so the place was treated as bound. Changed: failed keys live in
`failed: BTreeMap<key, why>`, outside the diff; each tick `retry` binds them against the file bound now; a key the
file drops, or that binds, leaves. `apply` binds a failed key again like an added one, and says the failure once
(`discord.error`, op "bind place"); the board's detail says "a place did not bind, and is tried again every 2 s: ...".
`add_dm` makes a retried DM land once in `Routes::dms`. A place that binds late has its viewers read (`check_privates`).
Proof: `a_place_whose_bind_failed_is_tried_again_until_it_binds` (a `session.open` for "discord #reef" refused once via
`rpc_client::refuse_once`; the detail says why; #reef binds with no second save; one bind notice; one error row; the
note clears). Plant: the retry call switched off: FAIL, "timed out: #reef binds".
## 2. theseus-btt4, the note is measured from the start's file (686b6d5c)
`waits(&started, &bound)`, computed at each change and each retry; `apply` no longer returns it. Test
`the_note_of_what_waits_holds_until_a_start_makes_it_true` (voice channel added: named; #lab's users changed: still
named; voice removed: cleared). Plant (compare the last two files): FAIL at the "still named" assertion.
## 3. theseus-sn2z, a torn save is not acted on (9eba07be)
A stamp is acted on only when the tick before saw it too (held) and its mtime is at least a period old (`settled`; an
mtime ahead of the clock counts as settled). A stamp already acted on is not read again, and one younger than a period
cannot be acted on, so two same-size writes in one mtime tick are read once they age. Test seam: `live::ticks(path)`
= (ticks ended, ticks that saw a new stamp), counted at a tick's end. Test `a_save_seen_half_written_is_not_acted_on`
(a prefix cut after a table, waits on the watch having seen it, then the rest: #pier never leaves health, a post to it
goes out, nothing refused, the full file bound). Plant (act on any moved stamp): FAIL, "#pier" gone from health.
Latency added: a change binds within two periods (about 2 to 4 s), not one. What it leaves: a writer paused longer
than a period mid-save still tears. Question for the owner: removals could wait longer (say three periods) than
additions, since a tear only ever shortens a file; not done, as the brief left it a question.
## 4. theseus-nz3q, DMs in the file's order (e8427616)
`order_dms` rebuilds `Routes::dms` in the new file's order, keeping the bound ones (a failed DM is out until it binds),
after `apply` and after a retry. Test `a_dm_put_back_live_keeps_its_place_in_the_files_order` (ana's DM removed and put
back; an operator notice goes to ana's DM, none to ben's). Plant (order_dms a no-op, the push as before): FAIL.
## 5. theseus-02bq, a changed place keeps its turn and messages (b2e5921a, fix 2a in the last commit before this)
Test `a_changed_place_keeps_its_turn_and_its_messages` (a wake.at call waiting in ana's DM, #lab's users changed
meanwhile, nonce window 0, the card approved): the messages first posted keep their ids, none repeats, and the tool line
is edited to its end. Plant (`unbind` + `bind` for `rebind`): FAIL: with the plant the tool line stays at
"waiting for approval" ("the tool line is edited to its end" times out). The first version of the test asserted this
at once and failed 5 of 5 under load (the live edit lands after the posts settle); it now waits for the edit.
## 6. theseus-88cp, a retired lane is not bound (f0faceac)
Test `a_notice_falling_back_to_a_retired_lane_is_refused` (ana's DM removed first; a post to #dock held; #dock
removed; an operator notice falling back to #dock is refused with "is not one this daemon's bindings file names", the held
post still dispatched, then sent; #dock holds only the bind notice and the held post). Plant (`binds` as
`lanes.contains_key`): FAIL, the notice is never refused ("timed out: the notice is refused").

## Runs
- `tests_live`: 9 tests; each of them 5 times under load (`nice -n 19` beside four busy loops at nice 0): 5/5 green
  after the item-5 fix.
- theseus-discord whole and the workspace suite: in the gate run below.
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): fmt, shape, features, clippy, cockpit,
  test build, reader rule green. The suite failed only on the known L1 tests (theseus-sandbox contract and bench
  `spawn_100`, theseusd `sandbox`, theseus-pv6i: 33 of 2995, including `a_host_beyond_the_list_once_approved_is_outside_text`
  at 30 s), none a test of this change. After the suite I ran the rest by hand: protocol types (no diff in
  cockpit/src/protocol.gen), `cargo deny check` (advisories, bans, licenses, sources ok). The benches were skipped
  (THESEUS_GATE_NO_BENCH=1).
- Each commit before the last was checked with fmt, clippy `-D warnings` and `tests_live`; the full gate ran once, on
  the last code commit.

## Live check for the maintainer (scratch daemon on `theseus-sim discord rig`'s stand-ins, fresh state dir)
Not run here (needs the daemon and the rig). Expected results:
1. sn2z: bind #a, #b and #c; write the file's bytes up to the table boundary before #c and flush
   (`head -c N full.toml > b.toml`, `sync`), hold 1 s, append the rest: `theseus health` lists every place at every
   instant, nothing refused. Held 5 s: the watch still sees the prefix as held, and #c leaves health for a tick or
   two until the rest lands, as before this change (the limit above).
2. btt4: add a `voice = true` channel to the file; then change another place's users: health's detail names "the voice
   channels ... wait for the next start" until a restart.
3. nz3q: two `[[dm]]` owners; remove the first, put it back: a shared place's approval card goes to the first DM in the file.

## Files for the docs
Spec/status are the maintainer's: the Part III item for the watch (theseus-ocwt) should say a change binds within two
periods, that a failed bind is retried each tick, that DMs keep the file's order and that the board's note is measured
from the start's file. crates/theseus-discord/AGENTS.md and live.rs's header are updated here.
