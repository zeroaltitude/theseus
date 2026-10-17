# CLOUD_REPORT: discord-default-forum (theseus-bh3t.6)

Branch `cloud/20261017-discord-default-forum`. One step, commit `13352845`.

## What I found
- `main` had a task forum only for a `[[channel]] kind = "forum"` binding; nothing made a channel and the DM's tasks had no post.
- A message in a post routes by channel id alone (`Routes::resolve`), so a forum post reaches a `Place` as a guild thread does; the bindings do not need to admit the forum.
- `Scripted` has no delay, so a task in a shared place that ends at once is never seen working and gets no post ("a task already over with no post gets none", kept). I added `reopen_for_link`: an index card made before its task's thread existed gets its link even when the task already ended.

## What I changed
- Config: `[discord] forum` (true) and `forum_channel` ("theseus"; id of 15-21 digits, or a name of 1-100 characters with no `#`), validated, in the template.
- `runtime/default_forum.rs` (new): `ensure_forum` (recorded id, else configured id, else name, else create; META `discord.default_forum`), `Stand` watch (Off/Pending/On), `home_of`/`Home`, `Place::forum_homes`/`home`, `route_index`, `index_pointer`, `OWNER_BITS` 274877975616, `BOT_BITS` 292057844816.
- `forum.rs`: `Forum.default`, `Post.link`, `Seen.homed`, `TaskPosts::{make, moved, owns, index_of, nudge}`, the worker's make channel, `shown`, `keep_index`, `reopen_for_link`, tag `PATCH` carries `archived:false`.
- `threads.rs`: `make_task_thread` takes a `Home` branch (`Post` asks the worker; `None` for a DM when the forum is off), `Parent` carries `homed`, `channel_place`, `tx`; `parent_of_tasks` also wants a forum-homed place; `tell_channel` removed (the place's `tx` replaces it).
- `thread_bind.rs`: one arm for an index card. `runtime.rs`: two lines (`pub(crate) mod default_forum;` and `default_forum::ready(shared);`; the spec allowed one, the `mod` line needs the second).
- Fake Discord: `fake_discord/forums.rs` (`GET/POST /guilds/{g}/channels`, forum rename, `refuse_forum`, archived-post rule, `forums_made`, `rename_forum`, `archive_thread`), `FakeGateway::ready_again`, three router lines, one line in the channel `PATCH`.
- Tests: `tests_default_forum.rs` (12) and one in `forum.rs`; the helpers of `tests_task_threads.rs` became `pub(crate)`.
- `AGENTS.md` of theseus-discord has a paragraph.

## Proof
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `scripts/shape.sh`: clean.
- nextest, discord + sim + protocol + core (config, registry, first_request, layouts): 912 run, 908 pass. The 4 failures also fail on the unmodified base (checked with `git stash`): `tests_task_threads::a_task_in_a_guild_channel_gets_a_thread...`, `a_shared_channels_task_thread_is_shared...`, `a_shared_forum_shows_a_shared_places_tasks...` (their jobs need L1/proc.run, which fails here as root) and `theseus-sim sim::the_restore_rows_store_is_the_same_with_the_cancel_row_selected`.
- New tests: the four-kinds test ran 4 of 4 times green. Tests that use `gate`/`proc_run` are racy on this VM, so the four-kinds test uses no gated tasks (its slow task is a `Slow` provider wrapper).
- Planted reverts (restored and touched, `git status` clean after each): (a) `@everyone` not denied: `the_forum_is_made_once...` FAIL; (c) recorded id ignored: `a_renamed_forum_is_found_by_its_id` FAIL; (j) `forum` ignored: `forum_false_is_todays_behaviour` FAIL; (f) `archived:false` dropped: `an_archived_index_card_still_takes_its_final_tag` TIMEOUT (waits for the tag); (h) pointer arm replaced: `every_task_gets_one_post_across_four_kinds_of_place` TIMEOUT.
- **Not done: planted reverts for (d), (e), (g) and (k)**, and runs under load. Time.

## The gate
`scripts/gate.sh` fails in its door/keel phase before any build, on history that is not mine (unsigned `Keel:` acks on d424da19e4, 238ecead06, 1c2beb2747) and on two `#[allow(dead_code)]` I then removed. It never reached the suite. I ran fmt, clippy, shape and the suite by hand, as above. No bench, no deny, no cockpit build were run.

## Tests of the spec I did not write
- (d) "the Follow mirror is unchanged" is not asserted (the DM gets exactly one `🧵` line).
- (j) compares no request log with an untrusted guild: a trusted guild skips the viewer reads, so the sets differ for that reason. The test asserts no `/guilds/N/channels` request, no record, no post, no DM line.
- (k) uses `ready_again()` on the fake gateway for the reconnect.
- `TaskPosts::moved` and `owns` are `#[cfg(test)]` (an `allow` trips the keel guard): the join drops the cfg when task channels calls them.

## Live check (the maintainer)
1. Restart the owner's daemon with no new config. In catorce `#theseus` appears, visible to the owner and the bot, with the tags needs you / running / done / failed.
2. Start a task from the DM: a post `⚙️ xxxxxx · title` with its card, the DM gets one `🧵 <#post> · title`, a message typed in the post steers the task, the end renames it `✅` and tags it done.
3. Start a task in a shared channel: it keeps its thread and the forum gets an index card with an `Open the task ↗` button; type in the card: one `-# This task works in <link>.` line, no turn.
4. Rename the forum in Discord and restart: no second forum.
5. `[discord] forum = false`: no forum request.

## Uncertain / for the owner
- A task the surface does not watch (a CLI session no place carries) has no post.
- The trusted guild is read at the start only; a bindings reload does not re-decide it.
- A homed post's place is revived at a restart only while the forum stands; with the forum off, a restart leaves that task's post unattended.
- The search is retried every 60 s after a network failure (not after a 403).
- A message in a homed post after its row retired (10 min after the end) gets the "starts nothing" line.
- Docs to change: the spec's Part III item, `docs/status.md`, and the example config are the maintainer's.
