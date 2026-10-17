# CLOUD_REPORT: discord-emoji-catchup (theseus-ss36w)

Branch `cloud/20261017-discord-emoji-catchup`. Three commits on the task commit: `99e22d4f` (emojis), `fd62c02d` (digest),
`667e079e` (catch-up). No store change (META keys only), no new dependency, no model-visible text.

## Step 1: app emojis (99e22d4f)
- **Found:** no app-emoji code, no `assets/`; the fake knew `/applications/@me` only. Twilight 0.17.1 types the routes.
- **Changed:** `crates/theseus-discord/assets/emoji/` (six images, `make.py`, standard library only: two GIFs, four PNGs,
  128x128, 7.6 KB at most); `runtime/emoji.rs` (`state(WorkState)`, `job()`, `on_ready`: list, add the missing, table + META
  `discord.app_emojis`; kept ids draw until the list answers; a failed list clears the table); one `mod` line and one call in
  `runtime.rs` (3,491 of 3,500); one call per site in `render/{board_v2,snapshot,task_view,card_v2}.rs`; the two config keys
  `[discord] app_emojis` (true) and `digest_at` (`["08:00","18:00"]`) with the template lines; the fake's
  `fake_discord/emojis.rs` (GET/POST/DELETE, 256 KB check, `refuse_app_emojis`, `drop_app_emoji`) and 3 lines in
  `fake_discord.rs` (2,413 lines).
- **Proved:** `runtime::emoji::tests`: uploaded once (6 POSTs), reused after a restart with no POST (kept ids drawn at once),
  re-uploaded when deleted (7th POST, new id drawn), Unicode when the route fails and when off (no request at all), the board's
  header draws the markup with a table and the old bytes without; images fit 256 KB and names are valid. The existing goldens
  (`board_v2.txt` and the render tests) pass unchanged.
- **Choices to hear:** select-option and button labels stay Unicode (Discord draws no custom emoji in them); `⛔ ⌛` have no
  art; the table is process-global, so tests run under nextest (as the gate does); a task view's "Last calls" heading gains 🔧
  (the one site whose Unicode output changed); `WorkState::Ready` draws 💤 in a task view where it drew none.
- **Config test:** `tests_persona::unset_fields_are_not_written_back` checked `!shown.contains("emoji")`, which `app_emojis`
  now trips; I rewrote it to check for the key (`emoji =` at a line start). Same meaning, not weakened.

## Step 2: the digest post (fd62c02d)
- **Found:** `/digest` was ephemeral and on demand. **Changed:** `theseus-core/src/initiative/digest.rs` (pure `due(now, slots,
  last)`; `post_digest_if_due`, one lock and a comparison per tick; marks `discord.digest.<date>.<HHMM>` and
  `discord.digest.last` in the post's frame); one call in `wake_home.rs::due_without_turns` beside the morning note (not in
  `harness.rs::drive`: it is at clippy's 100-line limit); `digest` in `pacer::UNPROMPTED`; the words moved to
  `theseus_protocol::digest_text`, which `/digest` calls too; courier kind `digest` (plans like a Jev post). `Parts::for_tests`
  and the three Discord test config helpers default `digest_at = []` and `app_emojis = false`, so no rig posts a digest or fills
  the emoji table unasked.
- **Proved:** `initiative::digest::tests` (slot arithmetic), `tests_work_digest.rs` (a slot with lines posts once and never twice
  after the claim is forgotten, with the marks read back; no lines posts nothing and is marked; the slot waits unclaimed behind a
  just-written unprompted post and posts when the window opens, by explicit `now`, not a paused clock).
- **Left:** the "restart" in the test forgets the in-memory claim on the same core; it does not reopen the store.

## Step 3: the catch-up (667e079e)
- **Found:** `deliver_posts` stops at the first error; each waiting post then went out on its own. **Changed:**
  `courier/catchup.rs` + three lines in `courier.rs` (a `catchup` field, the call at the top of `deliver_posts`, `begin` where a
  delivery fails). A silent report / turn failure / notice written since the first post that met the outage, three or more, is one
  silent message ("While Discord was unreachable (14:02–14:19): 3 tasks reported, 1 failed: …"); each post is dispatched first and
  settled after, newest first (`folded_into` in its detail). Never folded: card, needs-you, settle, reply (so an owed answer),
  reminder, Jev notice, a post that may have landed (`unsure`), quiet pointer notes.
- **Proved:** `courier/tests_catchup.rs` on the fake's Down then Up: four silent reports are one message, each settled with its
  id, while a reminder written in the outage posts on its own and nothing is sent twice; two silent reports, or reports that ping,
  are not folded. Unit tests: the kind rule (card, needs_you, settle, reply, reminder, jev_notice, met watch, quiet pointer never
  fold), the text and its 2,000-character fit.
- **Uncertain:** a card is covered at the kind level (unit test), not by a real card in the lane test (a card needs a kernel
  question). A send that landed followed by a failed settle retries against the same message (`Folding`), but a *process crash* in
  that window sends the unsettled posts on their own again (RESENT-marked past the nonce window); not tested. The first post that
  meets a Down outage is `unsure` (its connection closed after the request) and is never folded.

## Planted reverts
- `"card"` added to the fold rule → `courier::catchup::tests::only_silent_news_written_in_the_outage_folds` failed.
- digest posted with no lines (`if false` for the empty check) → `tests_work::digest::a_slot_with_no_lines_posts_nothing_and_is_marked` failed.
- slot claim and mark check removed → `tests_work::digest::a_slot_with_lines_posts_once_and_never_twice_across_a_restart` failed.
Files restored and touched after each.

## Gate and tests
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): **fails in the keel phase, then the door, on erosions I did
  not make** (the range base is `9fa84735`, older than my branch point): `crates/theseus-sim/src/lifecycle.rs:155` budget-raised,
  `scripts/keel-guard.py:59` and `scripts/test_keel_guard.py:59` keel-file, plus moved/renamed-test notes. **Keel findings expected
  from me: none.** I ran the later phases myself: `cargo fmt --check` ok, `scripts/shape.sh` ok, `cargo clippy --workspace
  --all-targets -D warnings` clean (after two fixes of mine), then `cargo nextest run` of theseus-core, theseus-discord,
  theseus-protocol, theseus-sim. Not run: cockpit, deny, benches, lifecycle/jobs/turn/size, theseusd and the other crates' suites
  (a full workspace test build exhausted this VM's disk allowance twice; I built with `CARGO_PROFILE_DEV_DEBUG=0`).
- Failures, all outside my change: `theseus-sim::sim the_restore_rows_store_is_the_same_with_the_cancel_row_selected` and
  `tests_task_threads::a_shared_channels_task_thread_is_shared_and_no_private_title_reaches_it` (a 120 s hang) fail on the base
  tree too (stash run); `a_task_in_a_guild_channel_gets_a_thread…` hangs on base too; `a_shared_forum_shows…`, some `tests_board`
  and `tests_horizon::the_agenda_expands…` failed only in parallel runs and passed alone (load). Two `tests_board` runs earlier
  failed because the emoji table filled in the gateway rigs; fixed by `app_emojis = false` in the rig helpers.
  Final discord run: 452 of 455 passed, the 3 `tests_task_threads` hangs above.

## Live check for the maintainer (the rig, the owner's keys not needed)
1. Emojis: `cargo nextest run -p theseus-discord -E 'test(emoji)'` shows 6 uploads, reuse, re-upload. Against the fake, a
   `theseus-sim discord` scratch daemon with `[discord] app_emojis = true`: `GET _fake/…` is not needed; `fake.app_emojis()` lists six.
2. Digest: scratch daemon with `[discord] digest_at = ["HH:MM"]` one minute ahead, a DM bound, a finished task in the last day: one
   message to the DM `◆ 1 reported since …`; restart the daemon: no second one; with nothing finished: none.
3. Catch-up: `fake.set_mode(Mode::Down)`, finish four tasks with `silent = ["reports"]`, `set_mode(Mode::Up)`: one message
   "While Discord was unreachable (…): 4 tasks reported: …".
## The owner's check (test channel)
In the test guild's DM: the board's header and "Now" lines show the spinning gear-ring and the pulsing red dot (animated), a
finished task a green check; delete `theseus_done` in the developer portal's Emojis tab and restart: it comes back. Set
`digest_at` a minute ahead and confirm one message, not two across a restart. Turn off the network to Discord for a minute while
tasks finish (with `silent = ["reports"]`) and confirm one message afterwards, with a card still arriving on its own.

## Docs to change (not mine to edit)
Spec Part III item for ss36w; `docs/status.md`; the notices doc (digest is an unprompted post on the shared pacer); the
design row of `roadmap-v2` for discord-emoji-catchup. AGENTS.md of theseus-discord and theseus-core are updated.
