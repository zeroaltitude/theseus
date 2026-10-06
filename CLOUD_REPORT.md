# Cloud report: discord-bound (theseus-6809, theseus-whb0, theseus-8u7m)

Branch `cloud/20261006-discord-bound`, from main at d279767f (store format 23, unchanged: nothing here is stored).
Started 19:43 UTC and finished 21:25 UTC on 2026-10-06.

Commits, on top of the task commit 0df7d8f1:

- 9a246a87 `discord: a lane keeps the turns its renderer holds whole, and forgets a turn the renderer drops (theseus-6809)`
- 01c5ef55 `discord: the renderer's per-key maps drop a turn's keys with the turn (theseus-whb0)`
- 743e6cdb `discord: the board's exemption from a lane's bound is held by a test through the lane (theseus-8u7m)`

Nothing here is on the start path or a turn's path. The place's actor sends one more lane message at each turn's
start and at `/new`. A lane's write still costs one map insert, plus a prefix check against at most 8 held turn ids
in `touch`. Dropping a turn walks the lane's maps once. No new dependency. Line counts: render.rs is 2,990, under its
3,001 ceiling and 11 lines shorter than at main. runtime.rs is 3,430 of 3,500. courier.rs is 1,583.

## Step 1: theseus-6809, a held turn's keys are kept, and forgotten when the renderer drops the turn

**What I found.** It matches the brief. `Lane::touch` treated every key alike under recency, and only the board was
exempt. The renderer's `update_tool` re-renders a line in any of its 8 held turns, so a late result past 192 later
keys and past the nonce window posted a second line. Where the code differs from the brief, or adds to it:

- A notice card's key was `notice:<tool_use_id>`, with no turn in it. I keyed it under the turn that holds the call's
  line: `<turn_id>:notice:<tool_use_id>` (`render/held.rs`, `Renderer::notice_key`). That is the same turn
  `update_tool` finds. So the card is kept while the line is, and forgotten with it, under one rule: a held turn's
  keys all begin `<turn_id>:`.
  - If no held turn shows the call, the card keeps the old key `notice:<id>` under recency. The renderer drops that
    card at its next turn drop.
  - The notice create carries no nonce, so a changed key changes nothing on Discord's side. Two render tests' expected
    keys changed to `t1:notice:u1`.
- `Renderer::streamed` had no caller, and is removed.
- `LaneMsg::Held` is not a live op, so `take` handles it even while Discord is away (only `Live` is dropped then).

**What I changed.**
- `LaneMsg::Held(Vec<String>)` carries the turns the renderer holds, oldest first.
  - The place's actor sends it after every `TurnStarted` it renders (runtime.rs `handle`), and from `rebind` after it
    replaces the renderer: an empty list, so every held turn is dropped.
  - The lane needs nothing else of the renderer.
- New `courier/held.rs` holds `hold`, `held_key` and `release`.
  - `touch` leaves a held turn's key out of `touched`, so it never counts against `KEYS_KEPT`.
  - A turn that leaves the list is forgotten from `msgs`, `sent`, `sealed` and `touched`.
  - If a waiting live op still names the dropped turn's key, the forget waits until `apply_live` has written that op,
    or `away` has dropped it. Otherwise a late state queued in the same batch as the next turn's start would post again.
- New `render/held.rs` holds `Renderer::held`, `drop_past_recent` (called from the `TurnStarted` arm) and
  `notice_key`.
- The `a_place_draws_nothing…` test in runtime.rs now expects the turn's `Held` beside `typing`.
- The crate's AGENTS.md bound line is updated.

**The memory bound.**
- Per lane: the board's key, plus every key of the 8 held turns, plus 256 others. A turn names at most `max_loops`
  (40 by default) × (that loop's text parts + 1 tool line + its notice cards), plus a footer. That makes
  8 × (40 × (P + 1 + C) + 1) + 256 + 1 keys: **905** with one part a loop and no cards.
  - `sent` holds up to 2,000 characters a key, so a lane at 905 keys is about 1.8 MB at worst.
  - A loop whose text runs to many parts raises P. A loop's text is bounded by the model's output tokens, about 1,900
    characters a part.
- Per renderer (step 2): `emitted` and `menus` hold the same 8 turns' stream keys, and `notices` holds their calls'
  cards.

**How I proved it.**
- Lane test `courier::tests_bound::a_held_turns_tool_line_is_edited_after_later_turns_pass_the_bound`, the probe:
  - The lane is first filled to its bound with 256 `glide:` keys.
  - Then a held job's tool line, then 7 turns of 19 loops (a text part and a tool line each, each applied, with
    `Held` sent as the actor sends it).
  - Then `set_nonce_window_ms(0)` and the late result. It asserts one tool line, edited to the result. Then the turn
    is dropped, and none of its keys remains in any map.
- Actor test `runtime::tests_held::a_turns_start_and_a_rebind_tell_the_lane_the_held_turns`, in a new file
  `runtime/tests_held.rs`: a place from `place_for_tests` with its lane swapped for a channel.
  - Turn n's start sends turns max(0, n−7) to n, so the ninth start drops turn 0.
  - `/new` (`Control::New`, through `rebind`) sends `[]`.
- **Plant: the recency bound alone.** I took `|| self.held_key(key)` out of `touch`. The lane test failed:
  `assertion left == right failed: one tool line: [...] left: 2 right: 1`. I restored the file and touched it.
- **Plant: the rebind's send removed.** The actor test failed: `assertion left == right failed: the rebind left: []
  right: [[]]`. I restored the file and touched it.
- A first try of `hold` also took a newly held turn's keys out of `touched`. With that in, the first plant passed,
  because that path protected the key too. I removed it: the actor always sends `Held` before a turn's first key. Now
  `touch` is the one guard, and the plant shows it.

## Step 2: theseus-whb0, the renderer's maps drop a turn's keys with the turn

**What I found.** It matches the brief. `emitted`, `menus` and `notices` were never pruned.

**What I changed.** `drop_past_recent` (render/held.rs) pops the turns past `RECENT_TURNS`:
- It removes those turns' keys from `emitted` and `menus`.
- It keeps in `notices` only the calls whose lines a held turn still shows. So `PolicyTightened` re-renders only the
  cards whose message ids the lane still keeps.

render.rs gains only `mod held;` and the call, and drops `streamed`, so it is net shorter. The ceiling is unchanged.

**How I proved it.**
- `render::held::tests::a_renderers_maps_hold_only_its_held_turns_keys` runs with notice embeds off and on. It drives
  3 × `RECENT_TURNS` turns, each with a text part, a notified tool line and (embeds on) a card. It asserts:
  - `emitted` holds only the kept turns' 16 keys.
  - `menus` holds only kept keys (8 with embeds off).
  - `notices` holds exactly `use_16` to `use_23`.
- **Plant: all three `retain`s removed.** It failed on `emitted`: `false: ["turn_0:L0:p0", "turn_10:L0:tools", …]`
  (48 keys).
- **Plant: the `notices` retain alone removed.** It failed: `left: ["use_0", …, "use_9"] right: ["use_16", …,
  "use_23"]`.
- After each plant I restored the file, touched it, and checked `git status`.

## Step 3: theseus-8u7m, the board's exemption held through the lane

**What I found.** It matches the brief. The old bound test inserts the board's id into `msgs` directly, so it passes
with the exemption removed. I confirmed this under the plant below.

**What I changed.** One test, `courier::tests_bound::the_board_written_through_the_lane_is_edited_past_the_bound`:
- The board goes in as a live upsert under `BOARD_KEY` through `apply_live`, so it is created and pinned.
- Then 256 `glide:` keys, then `set_nonce_window_ms(0)`, then the board written again.
- It asserts one message beginning with `BOARD_HEAD`, edited to the new state and pinned, and exactly one `PUT …/pins/`
  in `seen()`.

**Plant: `key == render::BOARD_KEY ||` removed from `touch`.** The new test failed with `one board: [...] left: 2
right: 1`. The old `a_lane_put_through_more_posts_than_its_bound_holds_at_most_the_bound` passed under the same plant.
I restored the file and touched it.

## Runs under load, and the suites

- **Under load:** each of the four new tests 5 times at `nice -n 19`, beside four busy loops at nice 0 (`ps`
  confirmed 96–99% CPU each; the loops were killed by their pids). All 20 runs passed:
  - the probe in 29.4–30.0 s;
  - the actor test in 1.7–1.8 s;
  - the renderer test in 0.85 s;
  - the board test in 13.6–15.7 s.
- **theseus-discord's suite, whole, under the same load:** 133 passed, 0 failed.
- **The workspace suite:** run by each commit's gate, three times. My first try at the load runs refused a
  `sh -c 'while :; …'` loop, so I put the loop in a script file under the scratchpad. The first script run started no
  loops (a relative path), and I reran it.

The probe and board tests take about 10 s and 5 s unloaded. That is the fake's per-request answer over about 550 and
260 HTTP writes, not a wait in the lane.

## The live check (the maintainer's)

**The rig.** `theseus-sim discord rig --dir /tmp/rig-bound` lays out the config, the fake `op`, the bindings and the
guild, and prints how to start `fake-discord`, the model stand-in and the daemon. Start `fake-discord` and the daemon
as it prints. In place of `theseus-sim discord model`, start
`theseus-sim fake-model --addr 127.0.0.1:9448 --rules /tmp/rig-bound/rules.json`, with `[model] api_base` pointed at
it. Add `[tools] proc_sync_secs = 2` to the rig's config, so the job goes to the background at once. Invented names
only.

**Check 1: a late job's tool line.**
1. In `rules.json`, the first rule: `{"when": "BUILD-SLIPWAY", "calls": [{"name": "proc.run", "input": {"argv":
   ["sleep", "240"]}}], "text": "started"}`.
2. Then a rule `{"when": "LOOP-MANY", "calls": [{"name": "fs.list", "input": {"path": "."}}], …}` that calls a tool
   each loop, for 19 or more loops. If one rule can't hold that, a few turns of `max_loops` each will do. Approve the
   `proc.run` card when asked.
3. `theseus-sim discord say --fake <rest> --channel <lab> --user <ana> --name ana "BUILD-SLIPWAY"`.
4. Then seven `say … "LOOP-MANY n"` turns, until `theseus-sim discord read --fake <rest>` shows more than 256 messages
   after the job's line.
5. Wait for the job to end.
6. **Expect:** `theseus-sim discord read --fake <rest> messages` shows exactly one message holding `sleep 240`, whose
   last version says it ended (`✅ ok` with its ms), and whose earlier versions show `⏳ running in the background`.
7. Second creates: the stand-in started from the command line keeps a nonce forever, so a second create for the
   line's key is deduped, not posted, and the check alone can't show the bug. To see it:
   - Read the `discord.message.out` ledger rows for `"part": "<turn>:L0:tools"` (e.g.
     `theseus --socket /tmp/rig-bound/sock --json ledger --kind discord.message.out -n 2000`). There should be exactly one row for
     that key. Before this change there are two: the second create is written, and the fake answers it `deduped`.
   - The fake's `--log` (`fake.log`) shows that second create as `deduped`, so it is visible there too.

**Check 2: the task board.**
1. In the same rig, make a task homed in the place (a turn that calls the task tool, or `theseus task …` against the
   session), so the board posts and pins.
2. Run turns until more than 256 keys are named there.
3. Change the task.
4. **Expect:** `discord read` shows one message beginning `📋 **Task board**`, pinned (`"pinned": true`), with the
   change as its last version. `fake.log` shows exactly one `PUT /channels/<lab>/messages/pins/<id>`.

## Left open, or uncertain

- **A reply that arrives after its turn is dropped.** After a long outage, a turn's reply post can be delivered after
  the renderer dropped that turn. Its keys are then not held, and the lane has forgotten the stream's ids. So the
  reply posts its parts fresh, which Discord's nonce returns inside its window, and it replies to the anchor. That is
  main's behaviour in the same case. Holding it would need the lane to keep a dropped turn's ids until its reply post
  lands. I left that out.
- **A late `ToolEnded` after its turn is dropped.** It has no line to update, as on main, and now no card to update
  either: the card leaves with the turn.
- **Notice keys changed** from `notice:<id>` to `<turn>:notice:<id>`, and the `discord.message.out` row's `part` names
  the new key. Nothing reads the old form.
- **Docs.** The bound line in the crate's AGENTS.md is updated. The maintainer may want Part III's item for this step
  to give the 905-key bound.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, before each commit. Every run ended
`gate: FAILED in suite`, on the known L1 cases only:

- **Every run:** the 33 known L1 failures (theseus-pv6i, the VM's root daemon): 20 in theseus-sandbox (its contract
  tests and the bench's `spawn_100`), and 13 in `theseusd::sandbox`.
- **Step 2's run:** also `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`, a
  known load test (theseus-1g8j‡). It failed with `the pool thread took the idle thread's policy left: 0 right: 5`, and
  passed alone.
- **Step 3's first run:** about 77 job tests failed: `the turn looked at its 0.4 s job 0 times`, 30 s timeouts. The
  disk was full: 878 MB free, 16 GB of it in `target/debug/incremental`. I deleted that directory (inside the repo,
  rebuildable) and reran with `CARGO_INCREMENTAL=0`. The rerun had only the 33 L1 failures: 2,960 passed, 33 failed,
  24 skipped.
- **The phases after the suite**, run by hand for each commit, all passed:
  - protocol types: `cockpit/src/protocol.gen` untouched.
  - No "compiled under the lock" note.
  - `cargo deny --offline check`: advisories, bans, licenses and sources ok.
- **Before the suite,** fmt, shape, features, clippy, cockpit and test build all passed in every run.
