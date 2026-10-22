# Cloud report: tail4-discord (theseus-ono8g, theseus-p5g63)

Branch `cloud/20261021-tail4-discord`, base `8260dbab`. Store format: unchanged (49). No new dependencies.

## theseus-ono8g: the approval card in the plain words (a744fdc)

**Found.** `render::card`'s approval branch drew `` `tool` summary `` + the rule's reason (with `§3.9`) + a long trust clause. `ConfirmRequest.plain` already carries the words the TUI and `theseus answer` print.

**Changed.** New `crates/theseus-discord/src/render/approval.rs`: `card(req, plain, route, elsewhere)` builds the approved layout; `classic(..)` is the old body moved out unchanged, used when `plain` is `None`. `render.rs` shrank from 3,003 to 2,975 lines. `render::strip_ask` (one function) replaces the three-way trim in `runtime.rs` (7 lines out, 1 in) so a settled card still drops its opening. `plain.rs` in core is untouched.

Choices the owner should hear about:
- The why line is built in the Discord crate from `req.external_text`: "the task quoted outside text" (`task.start`), "read web text" (`http.*`/`web.*`), else "read outside text"; "the session" when no task. The time is Discord's `<t:N:t>`, which each viewer sees in their own clock (so "07:25" appears as the viewer's local time); the crate has no time-zone library and gets none. A floor call shows core's floor sentence; a posture ask says "you set this kind of call to ask first" (core's `held` words name the tool, which the owner's rules forbid on this card).
- Risk "writes" shows as "writes a file" for `fs.*`; other risks pass through (`network`, `runs a program`, ...).
- "(new file, N bytes)" comes from `fs.write`'s input size and the tool's reason starting "create "; an overwrite says "(replaces the file, N bytes)".
- The button line is fixed text (core's `yes`/`no` differ from the approved words). The model's own one-line `why` and the old reason text are no longer shown.
- **Information lost, flag for the owner:** the old reason for a private-address fetch from a shared place ("this is a shared place, so the page joins a conversation others can read") is not on the new card (`tests_outbox::a_private_address_card_from_a_shared_channel_says_where_the_page_goes` asserted it). Follow-up: carry it in `plain.held` in core.
- A tool core does not gloss has no verb to bold: the line reads `*who* wants to wake.at {...}` (the tool name shows). Follow-up: gloss more tools in `needs_you/plain.rs`.
- `render/card_v2.rs` takes the card's content as given, so it shows the same words; unchanged.

### Card before and after

Outside text (the approved example):

```
before: **Approve?** task `490f10`: `fs.write` <home>/.../out/harbour.md
        create <home>/.../out/harbour.md (227 bytes): fs.write — approve (this session read external text (task.start quoted text, at 07:25), and a call that acts waits for approval after that (§3.9))
        -# expires <t:..:R> · **Approve + trust session** also lets its later calls run at their postures · you can also answer with `theseus answer`
after:  ✋ **Approve?** task `490f10` · *[walkthrough] harbour summary* wants to **write** `~/projects/walkthrough/out/harbour.md` (new file, 227 bytes)
        risk: writes a file · why it asks: the task quoted outside text at <t:..:t>
        ```diff ... ```
        **Approve** runs this one call · **Approve + trust** also lets its next calls run as usual · **Decline** refuses it; the task is told and carries on
        -# expires <t:..:R> · or `theseus answer 490f10 approve`
```

Posture (no outside text): same, with `risk: writes a file · why it asks: you set this kind of call to ask first`, no trust clause, no diff when none.

Floor: opens `🔒 **Approve?** ...`, `risk: touches secrets · why it asks: it touches Theseus's own state or a secret, which always asks`, no trust clause; the settled line ends `(floor)`. Before: `🔒 **Floor: Theseus's own state or secrets. Approve?** `.

### Old-words assertions changed (old -> new)
- `tests_gateway.rs` `approve_pressed_in_a_private_channel...`: `<@ANA> **Approve?** `fs.write`` -> `<@ANA> ✋ **Approve?** *Write the proof file.* wants to **write** `; settled `✅ **Approved** by discord:ana · `fs.write`` -> `... · *Write the proof file.* wants to **write** `outside/proof.txt``.
- `tests_outbox.rs`: four card lookups `starts_with("**Approve?** {WAKE}")` / `"**Approve?** `http.fetch`"` -> `starts_with("✋ **Approve?** ")`; the `fs.write a.txt` start -> `✋ **Approve?** *write a* wants to **write** `a.txt``; the settled `❎ **Declined** by cli#1 · `fs.write` a.txt` -> `... · *write a* wants to **write** `a.txt``; `WAKE` -> `*remind me* wants to wake.at {...}` (note, settle and DM-settle expectations); the fetch card's old reason text -> `wants to **fetch** `{url}`\nrisk: network · why it asks: you set this kind of call to ask first`.
- `render.rs` (~1672, ~2169), `courier.rs` (~2169) and `card_v2.rs` (~97) build requests with no `plain`, so they keep the old shape and did not change.

### Tests and plants (`render/approval/tests_approval.rs`)
- `the_approval_card_reads_as_the_owner_approved_it` (golden, doc comment records the owner's approval and the lock). Plant: old `` `tool` `` in the line -> FAIL (3 tests fail with the reason text plant too; the tool-name plant failed the golden, posture and no-spec tests).
- `an_approval_card_names_no_spec_clause`. Plant: `req.reason` back in the card -> FAIL (with the golden, posture and floor tests).
- `a_card_without_plain_words_keeps_the_old_shape`. Plant: default `plain` instead of `None` -> FAIL.
- `a_posture_ask_has_no_trust_button_clause`, `a_floor_ask_keeps_its_lock_and_its_rule`.
- First plant run did not apply (pattern missed after fmt); redone, failed as above. Every restore was `touch`ed and `git status` checked.

## theseus-p5g63: two forums in one start (eec9935)

**Cause: the fake's, not the daemon's.** `FakeDiscord::watch_guild_file` rereads the guild at every request (`reread_guild`) and replaced the whole guild, so the forum the bot had just made (in `guild.channels`) vanished. The recorded forum then answered `404 Unknown Channel` on `GET /channels/<forum>` (hence `task forum tags`), the daemon's own look found it gone and made a second. The scratch runs use the watched guild; the in-tree rigs use `set_guild`, which is why no test saw it. The daemon's `ensure_forum` already re-reads the META record after taking its gate (checked: two Shareds racing over a recorded forum make none), so no daemon change was needed or made.

**Changed.** `fake_discord/forums.rs` `keep_forums_set_guild`, called by `reread_guild` (`fake_discord.rs`, +~32 lines with its test; file at ~2,483 of the 2,500 default).

**Proof.**
- `theseus-sim fake_discord::tests::a_forum_the_bot_made_survives_the_guild_files_reread`. Plant: `set_guild` back in `reread_guild` -> FAIL.
- `tests_default_forum::one_start_on_a_guild_with_no_forum_makes_one_forum` (task home on, guild watched from a file, two Shareds racing over the store) and `task_posts_get_their_tags_on_a_forum_the_bot_made`. Plant A (fake fix reverted): both FAIL. Plant B (`read_tags` asks `f.channel + 1`): the tags test FAILs.
- `cargo nextest run -p theseus-discord` 617 pass after the card step; `-p theseus-sim` 101 pass.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (run twice, once per commit; `CARGO_PROFILE_*_DEBUG=0`, because the VM's disk allowance filled at full debug info). fmt, shape, features, clippy, clippy-probe, cockpit, test build and the reader rule pass. The suite: 7827 run, 7793 pass, 34 fail, and all 34 are the known L1 cases (theseus-sandbox contract and bench `spawn_100`, `theseusd::sandbox`). The core golden did not fail with the TZ set. After the suite: `protocol_types` clean (no cockpit diff). Benches skipped (NO_BENCH). **`deny` could not run:** `cargo deny fetch` never ran (the setup chain stopped at an early disk failure) and offline `cargo metadata` needs a download; the maintainer's gate covers it. No dependency changed.

Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, exit 0.

Commits are unsigned (no key on the VM).

## Live check for the maintainer
On a scratch daemon with a watched-guild fake and a fresh state dir, start a task from the DM that reads outside text and writes a file: the DM post is the approved card; `theseus-sim`'s fake log shows one `POST /guilds/<g>/channels` -> forum and the post carries the running tag; no `discord.error` `task forum tags`.

## `docs/status.md` sentence
"The Discord approval card speaks the plain words (who wants to do what, risk, why it asks, what each button does; owner-approved 2026-10-22), and the stand-in keeps the forums the bot made, which ended the second forum and the tag 404 in scratch runs."

## Follow-ups
Carry the shared-place page notice in `plain.held`; gloss more tools in core's plain words (so no unglossed tool shows its name); `cargo deny fetch` on this VM.
