# CLOUD_REPORT: discord-war-room (theseus-bh3t.4)

Branch cloud/20261017-discord-war-room. Step commit: e1c2594d.

## Found
- `/task` has one required string option; no USER option existed. Guests in a task thread were sent to the task or the parent's session (both reach a model).
- A report's `body["task"]` is the task's *session* id, not its work id: the count is found by session (`Shared::comments_on_session`).

## Changed
- `runtime/war_room.rs` (new): `Place::comment_on` (called from `task_message` before `join_task`), share/unshare (`set_guest`), META `discord.task_comments.<task>` (total + last 50) and `discord.task_guests.<task>`.
- `task_cmds.rs`: optional USER options `share`/`unshare`; `What::Share`. `render/task_view.rs::detail` takes `&Comments` (Comments section, last 5). `render::report` takes `guests` (+4 lines; "N guest comment(s)"). `courier.rs::report_post` reads the count. One `mod war_room;` line in runtime.rs (3488/3500). `join_task` uses `Shared::is_owner`.
- theseus-sim: `fake_discord/overwrites.rs` (PUT/DELETE permissions and thread-members; 2 router lines, 1 state field), `slash_with` (user options).
- Tests: `tests_war_room.rs` (5), visibility of helpers in `tests_task_threads.rs` widened to pub(crate).
- Crate AGENTS.md: a war-room bullet.

## Proof
- `cargo nextest run -p theseus-discord -E 'test(war_room) or test(tests_task_threads)'`: 15/15 pass (about 2 s each).
- Planted reverts (each restored and touched, git status clean): comment branch off -> 4 tests fail (the comment tests time out waiting for the relay line; the guest was redirected); owner check on share off -> `sharing_a_thread_adds_a_member_the_owner_alone...` fails; relay line off -> 4 tests time out; report count off -> `a_guests_comment_reaches_no_session_and_no_model...` times out. Not run: a planted revert of the thread-member add or the overwrite.
- Not run: load runs (nice/busy loops); a live check against real Discord.

## Design notes for the owner
- Discord allows no subcommand beside `/task`'s `name` option, so share is `/task name:<task> share:@user`, not `/task share user:`.
- **Two existing tests changed meaning**: `a_guests_message_in_a_private_task_thread_is_a_comment_and_is_never_lost` and `the_owners_message_joins_a_running_task_and_a_guests_is_a_comment` (renamed) asserted a private thread's guest was redirected to the conversation; under this row it is a comment. The redirect stays for shared places (`a_shared_place_refuses_a_share_and_keeps_its_guests_rule`). A Follow-mirror reply by a non-owner is unchanged (still `join_task`).
- Comments are keyed to `Made.private`; a task thread whose `Made` was dropped after GRACE takes no comment path.
- A comment's relay text collapses whitespace but does not escape markdown; notices are quiet (no pings).
- No store format change. No new dependency.

## Live check (maintainer)
In the test guild, from a private channel: start a task; `/task name:<short id> share:@<test account>` (ephemeral "can watch and comment"); post from that account in the task's thread: expect a `-# 💬` line in the channel, one "The task doesn't read guests' messages" line in the thread, Details listing it, the report saying `1 guest comment`; the task's transcript (`theseus session show`) never has the text. `/task ... unshare:@account` removes them.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` FAILED in the door/keel phase, before any build: main's gate judged this tree against a newer base and listed moved compiler tests and commit signatures it could not verify (%G? is E); none involve files I touched. Run by hand instead: `cargo fmt --all --check` clean; `cargo clippy -p theseus-discord -p theseus-sim --all-targets -D warnings` clean; `scripts/shape.sh` ok; `nextest -p theseus-discord -p theseus-sim -p theseus-protocol`: 631 pass, 1 fail (`theseus-sim::sim the_restore_rows_store_is_the_same_with_the_cancel_row_selected`: target/debug/theseusd not built, environmental); theseus-core `tests_registry` + `example_template`: 20/20. The protocol, lifecycle, jobs, turn and size steps were not run. I did not regenerate protocol.gen (no protocol types changed).
