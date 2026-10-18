# CLOUD_REPORT: discord-tasks (theseus-ypnxt.5)

Branch `cloud/20261017-discord-tasks`, based on `40aa83cd`.

## What changed
- **A, links.** `crates/theseus-core/src/config/cockpit.rs`: `public_url` is `Option`, a skipped `base` is settled once by `resolve(web.port)` (called at the end of `Config::from_toml`, `config.rs`); `turn_url` uses `public_url` else the base. `is_default` is `public_url.is_none()`, so `--sparse` writes nothing new. Template comment updated. The report (`render::report`, new `place` arg) puts `· <#id>` on its first line; `courier.rs` `report_post` reads it through the new `TaskThreads::place_of_task` (`runtime/threads.rs`). `task_channels.rs` and `war_room.rs` untouched. No listener or `[web] bind` change.
- **B, /tasks.** `show:` is now `open` (default, the old "all": the board from memory), `needs_you`, `working`, `done`, `all`. `done` and `all` call `task.list {target}` (`runtime/tasks.rs::finished`) and render `render/history.rs`: ten newest, how it ended, when, `$`, the report's first line (via the evidence node and `outbox.said`), the post/channel link, then "+N more". The old `all` was renamed `open` (no alias).
- **C, offer chip.** Core `judge/offer.rs` (moved `note_offer`/`take_offer` out of `inbound.rs`): a reply written without the brief is left waiting; a batch landing within 2 minutes posts `{"kind":"offer","turn_id","brief"}` to the reply's place (one frame, only then, skipped if the turn made a task). Binding `courier/offer.rs` edits the reply's last message to add the button (reply remembered in `Offers::replied`). Briefs are kept in META `discord.offers`, newest 64 (`runtime/start_task.rs`). No new protocol notification, no protocol or store-format change.
- **D, voice.** `runtime/voice/tasks.rs`: "pause it"/"pause that"/"pause the task" and "go on"/"resume it"/... call `task.pause`/`task.resume` as the speaker (owner only, whole utterance); target is the pending question's task, else the only running (to pause) or only paused (to resume) task of this private place; otherwise an ordinary voice turn. The RPC's result is spoken once ("Paused.", "Going on.", "It was already paused.", "It was not paused.").
- AGENTS.md updated in theseus-core and theseus-discord.

Commits: ca98d2ec (A config), 3fb3f2b1 (D voice), daf30dc5 (A report link + B + C), plus a final lint-split commit. **Deviation:** I split by file, not strictly A/B/C/D, because courier.rs/render.rs/tests carry hunks of several parts.

## Tests and plants
| Behaviour | Test | Plant -> result |
|---|---|---|
| port-following link, set wins, default :7433 | `config::cockpit::tests::the_templates_cockpit_section_is_the_default_and_the_link_follows_it` | resolve ignores port -> FAIL |
| report `<#id>`, none unchanged | `render::tests::a_report_links_the_tasks_own_post_or_channel`; assertions in `tests_task_threads` (thread) and `tests_task_channels` (channel) | drop mention -> first two FAIL |
| /tasks done/all from store, open unchanged | `tests_board::commands::tasks_done_and_all_read_...`, `snapshot::tests` | `Show::Done\|All` not reading store -> FAIL |
| late chip by edit | core `tests_offer::a_batch_that_lands_after_the_reply_posts_the_offer_late`, `judge::offer::tests::*`, discord `tests_board::offer::a_late_offer_adds_the_button_...` | not keeping brief/never editing -> FAIL |
| 3 min late adds nothing | `judge::offer::tests::a_batch_within_two_minutes...` (pure, with fabricated instants) | n/a |
| trivial/quick never early or late | `tests_offer::a_trivial_or_quick_turn_*` | structural (owed only registered after the detour check) |
| briefs survive restart | `start_task::tests::the_briefs_are_kept_in_the_store_so_a_restart_finds_them` | META write removed -> FAIL |
| voice | `voice::tests_tasks::{pause_it_after_a_question..., pause_it_and_go_on..., pause_it_with_two_running..., a_guests_pause_it_does_nothing, only_whole_utterances...}` | accept a guest -> `a_guests_pause_it_does_nothing` FAIL. "Pause the wrong task" plant not run |

Not done as the brief asked: no planted "read the memory model for done"; "a task ended 3 days ago after a runtime restart" is tested as "after Clear done" (the sim rig has neither a restart nor a settable kernel clock).

## Budgets
Not run: the turn bench and the owner-config fixture (no bench here). A plain turn adds one mutex lock (`take_offer`) and no frame; no request text changed.

## Overlaps
Edited other rows' files only by the lines needed: `runtime.rs` (`mod start_task` -> `pub(crate) mod start_task`, no line added), `runtime/threads.rs` (one method), `render.rs` (`mod history` line, report arg), `courier.rs`, `tests_task_channels.rs`/`tests_task_threads.rs` (assertions). Names used: `task.list` only; no `task.get`.

## Proof nothing was lost
Existing `/tasks` default output is pinned by the old test (default now `open`). core + discord + protocol suites: 3789 passed, 5 timed out. All five timeouts (`tests_task_threads` x4, `tests_default_forum` x1) pass when run alone in 3-4 s, but two of them still timed out when only those two files ran together; I did not run them against the base commit, so whether that is VM load or my change is **unverified**.

## For the joiner
- The sim's disk filled repeatedly (build artifacts); not a code issue.
- keel guard against `40aa83cd`: empty output (the lint allows I first added were removed by splitting functions). Against this clone's `origin/main` it lists other commits' items, none mine.
- Gate (`scripts/gate.sh`) was **not run**; clippy (`-D warnings`) on core+discord, rustfmt, and the suites above were.
- Not run: live check. Suggested: scratch daemon with `[web] port = 17433`, `theseus-sim discord`, press `/tasks show:done` after a task ends and after clearing the board; hold the fake Jev's batch 1 s and watch the chip appear by edit.
- Owner install note: nothing to change in `/etc/theseus/theseus.toml`; a hand-set `public_url` still wins.
