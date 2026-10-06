# Cloud report: discord-live (theseus-ocwt, theseus-celu.37, theseus-8phq)

Branch `cloud/20261005-discord-live`, from `main` at 60b43fb6 (store format 20, untouched: nothing here adds a
stored field or record). Every change is in `crates/theseus-discord`. No new dependency; `Cargo.lock` and the
package-lock files are unchanged. No protocol type changed, so `cockpit/src/protocol.gen` is unchanged.
runtime.rs went from 3,453 lines to 3,418: the per-place start code moved into the new module and both the start
and the live path call it. render.rs is untouched.

Commits:

| Commit | Step |
|---|---|
| 777b6088 | ocwt: the bindings file read while the daemon runs |
| b888621e | celu.37: a lane's maps keep their newest keys |
| 244174ab | 8phq: a test of each disk crossing's note in the approvals DM |
| a3cb4d44 | ocwt, from the live check: a parse error said on health's one line |
| d95e39cc | ocwt, from the live check: a failure said once, not once per stat |
| 3b3e800c | ocwt, from the runs under load: the two two-life tests in tests_outbox wait for the first life's core to drop |

## 1. theseus-ocwt: the bindings file read while the daemon runs

### What I found (and where the code differed from the issue)

- As the issue says, `run` read the file once (`Bindings::load`), then `start_lanes` and `start_places`.
- **A place is more than its lane**, as the brief warned: its lane (`Shared::lanes`), its actor (whose mailbox
  sits in `Routes` by session, by channel and by DM user), its routes (`users`, `mention_only`, the `dms` list
  `approval_dm` reads), its `PlaceBits` (guild and ceiling, immutable until now), its class and its guild's word in
  the core (`guilds::tell_core`), and its row on the board (`BindingStatus.places`, which `Board::place` upserts by
  label and nothing ever removed).
- **A place's actor could never end.** `Place` holds its own `tx` (for `rebind`), so its mailbox never closes.
  It needed an explicit message to stop.
- **Dropping a lane from the map alone would race the courier.** The courier calls `refuse_unbound` at every
  outbox change, and `refuse_unbound` refuses dispatched posts as well as pending ones. A lane taken out of `lanes`
  while it is mid-write would have its post refused under it, and then its own settle would fail, though the
  message reached Discord.
- **Health shows only `detail`** (`theseus health`'s `binding_line`), not `last_error`, so "the board says why"
  has to go in `detail`. The gateway clears `detail` at a Ready, Resumed or GuildCreate.
- **The bind notice posts only for a fresh session** (`start_place`'s `fresh`). A place removed and put back keeps
  its stored session (`place_session`), so at a restart it posts no second notice, and live it doesn't either. A
  place new to the store gets its notice. The live check's step 2 ("its bind notice posts") holds only for a
  channel the store has never bound; see the live check below.

### What I changed (777b6088, a3cb4d44, d95e39cc)

New module `src/runtime/live.rs`. runtime.rs gains only the `mod` line, the watch's spawn in `serve`, a
`retired` field on `Shared`, two `PlaceMsg` variants (`Rebound`, `Unbind`) and their arms, the actor's exit line,
`binds` checking `retired`, and `refuse_unbound`'s new doc comment. `start_lanes` and `start_places` now call the
per-place helpers in live.rs (`start_lane`, `start_channel_lane`, `start_dm_lane`, `start_channel`, `start_dm`), so
the start and the live path start a place the same way.

- **The watch**: a timer of the binding's own, `live::PERIOD` = 2 s, no config key. Each tick stats the file
  (mtime, size, inode) with `tokio::fs`; only when the stamp moved is it read and parsed; only when the revision
  moved is anything done. Its first tick reads the file, so a change between the start's read and the watch is not
  missed. No inotify: an editor's save by rename swaps the inode, which a stat sees.
- **A file that does not load changes nothing.** The board's `detail` says `the bindings file does not load, so
  revision <r> stays bound until it does: <why>`, on one line (TOML's caret drawing is dropped, a3cb4d44), the log
  warns, and one `discord.error` row is written (`op: "bindings file"`, an existing kind with its reader). The same
  failure seen again (a save caught mid-write is stat'ed twice) is said once. The note is put back each tick if
  the gateway cleared `detail`, and cleared when the file loads.
- **A change**: the core is told first (`guilds::tell_core`: classes, guild words, warnings; a place whose ceiling
  names an unknown profile is left out, as at a start), then `PlaceBits` is replaced, then places are diffed by key:
  - **Removed**: its routes go at once (a message typed there resolves to no place, so it starts nothing and gets
    no answer), its actor gets `Unbind` and ends (taking its row off the board as it goes), its session is
    unwatched, and its lane is **retired**: put in `Shared::retired` and woken. A retired lane stays in `lanes`
    until it ends, so the courier's `refuse_unbound` still counts it bound and never refuses a post it is writing,
    while `binds` (the operator notice's fallback check) no longer names it. The lane checks `retired` before each
    post and after each round (`Lane::retires`, under the lanes' lock): it ends between posts, so a post it is
    sending settles as sent, and then it calls `refuse_unbound`, which refuses the rest. A post written later is
    refused at the courier's next wake, as before.
  - **Added**: as `start_places` starts one: its lane (or, if its old lane is still draining, that lane, kept and
    un-retired under the same lock), its session (a fresh one says the bind notice), routes and actor; a channel
    newly bound private outside a trusted guild has its viewers read once.
  - **Changed** (any field of its `[[channel]]`/`[[dm]]`, or its class by its guild's word): **updated in place**,
    not stopped and started: its routes (`users`, `mention_only`, the DM list's label), its actor's label, users and
    spend limit (`PlaceMsg::Rebound`), and its lane's label (`LaneMsg::Label`). I chose in place because a restart
    of the actor would drop a running turn's state and a new lane would lose the stream's message ids, so a reply
    streaming there would post again instead of editing.
- **What waits for the next start** (said in `detail` and the log): the check that the bot is in each guild (a
  `[[guild]]` added or removed: its places bind live and the bot's roles are re-read now, but the invite check
  runs only in `connect`), and the voice channels (`voice::Voice` is built from the file at the start). The file's
  format may change live: both formats read to the same places.
- No new ledger kind: the board, the log and the existing `discord.error` row say what happened.

### How I proved it

- `src/tests_live.rs`, through the fake Discord's REST and gateway (`tests_gateway`'s `Rig`, made `pub(crate)`),
  with no restart:
  - `a_place_removed_or_added_live_is_unbound_or_bound_with_no_restart`: a DM, `#lab` and `#dock`; the file
    rewritten without `#dock`: health drops it, its session's reply is refused (an `action.failed` row naming
    `discord:channel:<dock>`), a message typed in `#dock` is no turn (the `discord.message.in` count rises by one
    for `#lab`'s message only) and gets no answer, `#lab`'s reply goes; rewritten with `#pier`: its bind notice
    posts and a typed message is answered; `#dock` put back: answered in its old session, with no second notice.
  - `a_file_that_does_not_load_changes_nothing_and_a_changed_place_updates_in_place`: a broken file leaves the
    places bound and answering, the detail says why on one line, a second save with the same fault writes no
    second `discord.error` row; mended with `#lab` renamed and driven by ben alone: the detail clears, health shows
    `#lab2` and no `#lab`, the session is the same, ana's message is ignored and ben's is answered.
  - `a_post_in_flight_when_its_place_leaves_settles_as_sent_and_the_rest_are_refused`: two notices for `#dock`, the
    first held mid-write by the fake (`hold_writes_containing`); `#dock` removed; nothing is refused while it is
    held; released: the first settles `Succeeded`, the second `Failed`, one refusal row, nothing else reaches it.
  - `runtime::live::tests::a_parse_error_is_said_on_one_line`.
- Every theseus-discord and theseus-sim test: 180 run, 180 passed.
- Under load (each test `nice -n 19`, four `while :; do :; done` loops at nice 0, killed by pid): tests_outbox,
  tests_live and tests_gateway 3 runs, then tests_outbox, tests_live and tests_bound 3 runs after each later
  commit and 10 more besides, 26 batch runs in all. One test race of mine was found under load and fixed before
  the first commit: `#lab`'s reply shows live before its post settles, so the test waits for `pending == 0`.
- **A pre-existing failure under load, found and guarded (3b3e800c).** Twice in those 26 runs a tests_outbox test
  with two lives on one store (`a_post_for_a_place_no_longer_bound_is_refused_at_the_next_start`, then
  `a_crash_between_send_and_settle_leaves_one_message`) failed in the second life's `Store::open`: `Database already
  open. Cannot acquire lock.` Beside it, a first-life worker panicked at tokio's `time/entry.rs:539` (a timer polled
  after its runtime shut down), its backtrace in twilight's gateway connect (`tokio_websockets … WebSocketStream`,
  connecting to `ws://127.0.0.1:9`, where nothing listens). The first life is stopped with `shutdown_timeout(2 s)`,
  and under load that task was still mid-poll holding the core, so the store stayed open. **It is not from this
  branch**: with main's theseus-discord checked out (60b43fb6) the same batch under load failed the same way once in
  10 runs (run 6: `a_post_for_a_place_no_longer_bound…`, `Database already open`, the same worker panic). Both
  two-life tests now wait up to 30 s for the first life's `Weak<Core>` to drop before opening the store
  (`until_dropped`), so a core that really lived on would fail with that name. After the guard: 4 batch runs under
  load, green. The owner may want the same wait wherever a test reopens a store after `shutdown_timeout`, or the
  gateway's connect to stop on the runtime's shutdown; I changed neither beyond these two tests.
- Planted reverts, each restored and `touch`ed, `git status` clean of them after:
  - the watch never acting on a change (`apply` skipped): all three tests_live tests fail (`#dock leaves health`
    times out);
  - a removed place's posts not refused (the lane never retired): `a_place_removed…` fails at "the dock's reply is
    refused", `a_post_in_flight…` at its settle wait;
  - the lane dropped from the map at once and `refuse_unbound` called (no retirement): `a_post_in_flight…` fails,
    the in-flight post refused while it was being sent;
  - the error not shortened: `a_file_that_does_not_load…` fails on the one-line assertion;
  - the once-per-failure guard removed: `a_file_that_does_not_load…` fails (two `discord.error` rows).
- I ran the live check below myself on this VM (`theseus-sim discord rig`, a second `[[channel]]` added): the
  channel left `health` within 2 s; `discord say` there got no answer and `0 in` stayed; `theseus ask -s <its
  session> "hello"` answered and health's `discord outbox:` showed `1 refused · last error …: not bound here any
  more (channel:900000000000000020)`; put back, it was listed again and answered `hello again` (no second bind
  notice: its session goes on); a broken file kept every place, health's detail said why, and mending it cleared
  the detail. That run found the multi-line detail (fixed in a3cb4d44) and the doubled error row (fixed after).

### The live check for the maintainer

```sh
D=$(mktemp -d)
theseus-sim discord rig --dir $D          # prints the three start commands; run each in its own shell
printf '[[channel]]\nid = "900000000000000020"\nname = "dock"\nusers = ["900000000000000101"]\nmention_only = false\n' >> $D/state/bindings.toml
# start the three processes the rig printed (fake-discord, discord model, theseusd), then:
theseus --socket $D/sock health | grep '^discord'   # #lab, #dock, DM @ana, each with a session
DOCK=$(theseus --socket $D/sock health | grep -o '#dock → ses_[0-9a-f]*' | cut -d' ' -f3)
```

1. Remove `#dock` while the daemon runs (`head -10 $D/state/bindings.toml > $D/b && mv $D/b $D/state/bindings.toml`):
   within about 2 s `theseus --socket $D/sock health`'s `discord:` line no longer lists `#dock`.
   `theseus-sim discord say --channel 900000000000000020 --user 900000000000000101 --name ana "hello"` gets no
   reply (`theseus-sim discord read`: no bot message after it; `discord:` still `0 in`).
   `theseus --socket $D/sock ask -s $DOCK "hello"` answers in the terminal, and its post is refused: an
   `action.failed` row whose `outbox` is `discord:channel:900000000000000020` and whose error is `not bound here any
   more (channel:900000000000000020)`, and health's `discord outbox:` line shows one more refused.
2. Put it back (append the `[[channel]]` again): `#dock` is listed again, and a message typed there is answered.
   **Its bind notice does not post a second time**: the store keeps `#dock`'s session, and a restart posts no
   notice for a session that goes on either. To see a bind notice, add a channel the store has never bound (say id
   `900000000000000030`, name `pier`): its notice posts in it within 2 s.
3. Break the file's TOML (`printf 'guild_id = "9\n[[channel\n' > $D/state/bindings.toml`): every place stays bound
   and answers, and health's `discord:` line reads `ready (the bindings file does not load, so revision <r> stays
   bound until it does: … TOML parse error at line 2, column 10: unclosed array table, expected `]]`)`. Mend it
   (write the good file back): the parenthesis is gone within 2 s.

Stop it with `theseus --socket $D/sock shutdown`, and the two theseus-sim processes by their pids.

### Left, uncertain, and for the owner

- **Re-added places say no bind notice** (above). If the owner wants a notice whenever a place becomes bound live,
  it is one line in `bind_channel`/`bind_dm`, but it would differ from a restart.
- **An added place whose session cannot be opened** (`session.open` failing) keeps its new lane with no actor, and
  says so as a `discord.error` row (`op: "bind place"`); since the file's revision is then taken as applied, it is
  tried again only at the next change or start. At a start the same failure fails the whole binding.
- **A changed place mid-turn** keeps its turn; a removed one's turn runs on in the core and its reply is refused.
- The 2 s period is a constant, not a config key (few knobs); the stat costs one `statx` per tick.
- `Routes::dm_channel` keeps a removed DM's channel id as a cache; nothing routes by it.
- Docs (the maintainer's): `docs/design/roadmap-v1.1.md`'s discord2 lane row (the bindings file read live, and the
  lane maps bounded, done; C5's `BindingPort` left), `docs/status.md`'s Discord line (no restart needed for a
  bindings change, with what waits), and the spec's Part III item. `docs/setup.md` or the bindings example's
  comments, if they say a change needs a restart (I found no such sentence in `bindings.example.toml`).

## 2. theseus-celu.37: a lane's maps bounded

### What I found

- `sent` and `sealed` only grew, as the issue says; so did **`msgs`** (every message's ids, inserted at each
  create and edit, removed only when an edit finds the message gone). All three are needed only for recent
  keys: `msgs` for a later edit of the same key and for `reply`'s "did this turn stream" prefix check, `sent` to
  skip an edit that changes nothing, `sealed` to drop a stream state that comes after its post.
- **`unsure`** holds correlation ids of posts whose send may have landed; it is cleared at the lane's settle, so it
  holds only this lane's unsettled posts. A post settled elsewhere (refused by `refuse_unbound`) would leave its id;
  that is bounded by refusals in one process and I left it. `asked` was bounded already (`ASKED_KEPT`).

### What I changed (b888621e)

**Newest N**, not pruning at the settle: `KEYS_KEPT` = 256 keys per lane. Every insert into the three maps goes
through `Lane::touch` (or `Lane::seal`, which touches), which stamps the key with a counter; past 256 keys, the
quarter named longest ago (64) is forgotten from all three maps in one pass. A late live state of a key touches it
in `queue_live` before it is dropped. The task board's key is exempt: its message is sought once a process
(`board_sought`), so forgetting it would pin a second board.

Why newest N: the lane can't tell when a turn's stream has ended (the renderer lives in the place's actor, and
render.rs is at its ceiling), and a stream state can come after its post, so "post settled and stream ended" isn't
knowable in the lane. Why the bound never drops a key a late state still needs: a key is forgotten only after
192 other keys were named after it, and a late stream state names its key again. The place's actor sends a turn's
states in its events' order, and a later turn's keys reach the lane either through that actor (after the earlier
turn's states) or through later posts, so a sealed reply part would have to trail 192 messages of later turns to be
forgotten, which needs its actor to be stalled for many whole turns. That is an argument, not a proof; the owner
should know the guard is recency, not knowledge of the stream's end.

### How I proved it

- `src/courier/tests_bound.rs`, a lane driven directly against the fake's REST:
  - `a_lane_put_through_more_posts_than_its_bound_holds_at_most_the_bound`: 128 real notice posts delivered by
    `deliver_posts`, then 256 replies (sealed, then written), the bound asserted after every one: `msgs` (board
    aside), `sent`, `sealed` and `touched` hold 256 at most, the board is kept, the oldest notice is forgotten from
    `msgs` and `sent`, the newest reply is kept sealed.
  - `a_stream_state_after_its_post_is_still_dropped`: the maps at their bound, a reply streamed then posted, then
    72 more keys named, then a partial state of that reply's key: it is dropped, and the message on the fake still
    reads the final text.
- Planted reverts: the bound removed (`touch` never forgets): both tests fail (`msgs, sent, sealed, touched: (1,
  1, 1, 337)` in the second); `sealed` pruned at its post (in `write`): `a_stream_state…` fails, "a late state of a
  sealed key is dropped".
- Every theseus-discord and theseus-sim test; tests_outbox, tests_live and tests_bound 3 times under load: green.

## 3. theseus-8phq: the disk notice's test

### What I found

As the issue says: the `"disk"` arm (`disk_post`) posts `diskwords::disk_note(body)` where approvals go, under
`note:<corr>`, and no test reached it. The key is kept in the settled post's `detail.messages[].key`, and it is
also what the create's nonce is made from (`courier::nonce`).

### What I changed (244174ab)

`tests_outbox::each_disk_crossing_posts_one_note_in_the_dm_approvals_go_to`: a binding whose DM's user is an owner
(`[places] owner`), and an operator action `{"kind": "disk", "state", "left", "free_mb", "total_mb", "warn_mb",
"floor_mb"}` (disk_watch's shape) for each crossing: low from ok, below the floor from low, low from below the floor,
ok from low. Each settles, posts exactly one message in the DM, `disk_note`'s words for its body, and its settle
keeps one message under `note:<corr>` with the DM's channel and that message's id, whose create carried
`nonce("note:<corr>")`.

### How I proved it

- Planted revert: the arm renamed (`"disk_renamed_away"`): the test fails, timing out on the first settle (the post
  is refused as a kind the binding doesn't know).
- Every theseus-discord and theseus-sim test; tests_outbox 3 times under load: green.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit: fmt, shape, features, clippy,
cockpit, test build and the reader rule pass; the suite fails only on the 33 known L1 tests (theseus-sandbox's
contract tests, its bench's `spawn_100`, and theseusd's sandbox tests: a root daemon's L1 job with no job cgroup,
theseus-pv6i). Last run, on 3b3e800c: 2,796 tests, 2,763 passed, 33 failed, all of them those. One gate run (before d95e39cc) also failed theseus-core's
`term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`, on the brief's list (theseus-ynia); rerun alone it passed, and it
passed in every other gate run. The phases after the suite, run by
hand each time: protocol types unchanged; `theseus-sim bench turn --check --runs 5 --burst 0`: frames_plain 5 of 5,
frames_tool 9 of 9; `cargo deny --offline check`: advisories, bans, licences and sources ok (the advisory database
was fetched at setup). The lifecycle and jobs benches are skipped under `THESEUS_GATE_NO_BENCH`, as the brief says.
No other timing test from the brief's list failed, and no negative assertion failed.
