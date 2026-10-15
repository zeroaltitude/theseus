# CLOUD_REPORT: discord-calendar (HZ11, theseus-4zc4d)

Branch `cloud/20261013-discord-calendar`. One step, commit `0cdcd65f`.

## What I found
- `work.agenda` lines carry id, occurrence, verb, title, session and purpose, but **no place and no milestone producer**:
  nothing emits `AgendaWhat::Milestone` yet, so milestone events are built and unit-tested (`plan.rs`) but cannot be seen live.
  Check-ins are `Due` with purpose `check`; only `remind` counts as a reminder.
- No place on the line: privacy is decided from `session_id` through a new `Runner::speaks_privately` (core `recall.rs`, wraps `place_of`).
- Discord statuses move forward only (scheduled → active → completed; scheduled → canceled). Settling therefore sends active, then completed.
- The agenda cannot say a run was *skipped*, so a passed event is always Completed; a future-cancelled item has its event **deleted** (brief item 2).
- No access to Discord's docs here (network blocked). The guild limit of **100 active/scheduled events** and error **30038** are from memory, unverified. `max_events` is capped at 50, the guild's own events are counted at each listing, and any create refusal is told once in health.

## What I changed
- `theseus-core`: `config/calendar.rs` (`[discord.calendar]`: enabled=false, guild, days=7, max_events=10; enabled without a 15–21-digit guild id is refused at load, saying why), template section, `Runner::speaks_privately`.
- `theseus-discord`: `runtime/calendar.rs` (+ `calendar/plan.rs` pure rules, `calendar/rest.rs`); `runtime.rs` gets `mod`, one local, one call at Ready. Not trusted in the bindings file (`private = true` on the guild) → nothing is written, health says why (checked at start; a live change of trust needs a restart).
- `theseus-sim`: `fake_discord/events.rs`: scheduled-event routes (create/list/patch/delete) enforcing name ≤100, description ≤1000, status transitions, and a guild limit (code 30038); `set_event_limit`, `refuse_events`, `events()`.
- Design: one `work.agenda` read per pass; pass at Ready, 10 min after the first relevant board change, a minute after an event's time, and every 6 h. Each event's id is in META `discord.calendar` and its description ends `theseus-calendar:<key>`; the first pass of a run lists the guild and reconciles (adopts, drops, deletes doubled). Names: the title only for private-place items, else `Theseus · reminder|routine|milestone`; any title with `$ € £`, "usd" or "dollar" is neutral. Location: `#channel` for a private item speaking in a bound channel of the guild, else `Theseus`. Reminder 15 min, run 30 min, milestone ~24 h. Test pace via `PACES` keyed by state dir (`#[cfg(test)]`).
- No store change, no new dependency, `Cargo.lock` untouched.

## How I proved it
- `cargo nextest run -p theseus-discord calendar`: 17 tests pass (6 `plan.rs` rules, 9 through the stand-in: create/move/settle/cancel, ten-event bound with the eleventh made when one goes, restart by META and by marker with no duplicate, disabled makes zero scheduled-event requests (with an enabled control), untrusted guild gets nothing + health line, money title neutral, shared place neutral vs private titled, full guild told once, 403 told once, a routine's run as an event). Core: 3 config tests.
- Planted reverts (restored and `touch`ed): trust check off → `a_guild_the_bindings_file_does_not_trust_gets_nothing_and_health_says_why` fails; marker not read → `a_restart_finds_its_events_by_the_store_and_by_their_marker` fails.
- Full suite (`cargo nextest run --workspace --no-fail-fast`, 5705 tests): only failures were the known L1 sandbox ones (theseus-sandbox contract/bench, theseusd::sandbox) and `term::tests_idle::a_repl_computing_is_not_idle`, which passed when rerun alone (timing under load).
- fmt, shape, clippy `-D warnings` clean; `cargo deny check` ok.

## Live check for the maintainer (needs the owner's OK)
In a scratch guild marked `private = true` in the bindings file, with the bot given **Manage Events**:
1. config: `[discord.calendar]` `enabled = true`, `guild = "<id>"`; restart the daemon.
2. `theseus` a reminder for tomorrow, and a routine `every 1h`: within seconds the Events tab shows `<title>` (reminder) and the routine, location `Theseus`; `theseus health` shows no calendar error.
3. Snooze the reminder: the same event moves. Cancel it: the event disappears. Let one pass: it shows completed.
4. Untrusted guild: remove `private = true`, restart: no events, health says the guild is not trusted.
Locally: `cargo nextest run -p theseus-discord calendar`.

## Left / uncertain
- Milestones: no producer in the agenda yet; handled when one appears. "Skipped" runs are not distinguished (always completed).
- Guild limit/error code unverified (above). A refused create other than a full guild is told once and the pass stops.
- Trust is read at start only. Items with no session never get a title.
- A person deleting an event by hand: it is remade at the next listing (every 6 h or restart).
- Docs to update at review: spec Part III item, `docs/status.md`, `crates/theseus-discord/AGENTS.md` (calendar paragraph), the horizon design's HZ11 row.

## Gate
`scripts/gate.sh` stops at its door/keel phases here: base-range keel findings, not mine (renamed/moved tests from main, and two `Keel:` acks whose signatures cannot verify on this VM; one `budget-raised` in `crates/theseus-sim/src/lifecycle.rs:155`, from main's commits). **Keel findings expected: none from this branch.** I ran the later phases by hand (fmt, shape, clippy, full suite, deny): green except the known L1 failures above.
