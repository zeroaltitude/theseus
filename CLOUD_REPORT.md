# CLOUD REPORT: tail7b-messages (theseus-x8b9k)

Branch `cloud/20261021-tail7b-messages`, based on `ae420a49`. Started 01:41 UTC, report at 03:30 UTC (2026-10-23).

**Ceilings:** none raised and no file left over its ceiling. `crates/theseus-discord/src/render.rs` is 2,978 / 3,009
(+3 lines, all in `restarted`). `runtime.rs` is untouched. `turn.rs` changed one word (`mod persona_step` became
`pub(crate) mod persona_step`) and stays at 3,467 lines.

**Signing:** commits are unsigned. The VM has no key, and CLOUD_TASK.md says to commit without `-S` here.
**Trailer:** each commit ends `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>`, copied from `git log -3 origin/main`
as the row's rules ask. That trailer overrides CLOUD_TASK.md's generic cloud trailer.

## The step: one head for every unasked message

### What I found

There was no shared header: each producer wrote its own first line. Some began with an emoji (📏 ⛔ 💸 🚂 🗂 🔕 🔒 🔗 📌 ⏰ 👁),
some began with a bare sentence, and some (the backlog night) began with an internal label and listed bare `tsk_…` ids.
Most core producers post through `Outbox::stage_to_operator` / `to_operator`. Six families (restart, disk, MCP, Jev's
notice, reminders, voice) get their words in the Discord binding.

### The convention and the helper

`crates/theseus-core/src/owner_note.rs`:

- `Kind::{Fyi, Action, Decision, Alert}` with tags `ℹ️ FYI`, `✋ Action`, `❓ Decision` and `⚠️ Alert`.
- `head(kind, title, trigger)` gives `"<tag> · <title>\n<trigger>"`.
- `note(kind, title, trigger, body)` gives the head and then the body (with nothing more when the body is empty).
- `is_headed(text)` checks the first line against `^(tag) · \S` and checks that the second line is not empty. The tests use it.
- `day(ms)` gives a local `YYYY-MM-DD` date, for a digest's "since when".

Every in-scope producer builds its text through `head` or `note`. The facts each message carried are kept: the old first
line folds into the title or the trigger, and the action stays last.

### The inventory

The full table is in `docs/messages.md`: where each message is built, what triggers it, its channel and kind, and its
head. A summary:

| # | Message | In / out | Head |
|---|---|---|---|
| 1 | Self backlog night (`rsi/backlog/start.rs` `night_text`) | in | FYI, or Action when an item waits for the owner's word |
| 2 | Single backlog lines (`fact/rsi_backlog.rs`), CLI `self backlog` | out: narrative lines of a session's history (one line each), and the CLI is asked | n/a |
| 3 | Weekly self digest post (`rsi/digest.rs` `week_post`) | in (the asked `self.digest` text is unchanged) | FYI |
| 4 | Self notice (`rsi/digest/notice.rs` `notice_text`) | in | FYI, or Action with waits |
| 5 | Self-join cards (`rsi/join/joining.rs` keel, merge keel, unsigned) and install notice (`join/install.rs`) | in | Decision / Decision / Action / FYI |
| 6 | Keel stop (`rsi/pr.rs` `keel_text`) | in | Decision |
| 7 | Revert cards (`rsi/revert/doing.rs` held, reverted, vetoed) | in | Action / Alert / FYI |
| 8 | Narrowed policy (`aws/narrow/proposal.rs` `Proposal::card`, `rsi/narrow.rs` `red_card`) | in | Decision / Alert |
| 9 | Home size (`turn/home_size_step.rs`) | in; the runnable split is still the first backticked text | FYI |
| 10 | Privacy warning and held form (`turn/personal_step.rs`) | in | Alert |
| 11 | Budget notice in a place (`turn/budget_step.rs`) | OUT (task flow) | n/a |
| 12 | Budget question card (Discord `render.rs` `card`) | OUT (task flow) | n/a |
| 13 | Day ceiling, runaway line and warning (`day_ceiling.rs`; `turn/ceiling_step.rs` calls `warning_text`) | in | Alert |
| 14 | AWS month warning (`aws/month_warn.rs`) | in | Alert |
| 15 | Hands hour alert and runaway (`aws/hands/watch.rs`, `runaway.rs`) | in | Alert |
| 15b | Hands group line (`aws/hands/grid.rs`) | out: the owner's own call's live status, edited in place | n/a |
| 16 | Google lapse (`google/tend.rs` `lapse_text`) | in | Action |
| 17 | Morning note (`initiative/speak.rs` `headed`, from `initiative_post`) | in: the head only, the persona's words unchanged | FYI |
| 18 | Consolidation finding and posted digest (`initiative/digest.rs` `posted_text`) | in | FYI |
| 19 | Ontology mint and ask cards (`judge/onto/stage.rs`) | in (text only) | FYI / Decision |
| 20 | Ontology filed-late line, night lines (`judge/onto/mod.rs`, `digest.rs`) | out: a reply's own footer arriving late; lines inside the morning note, which has the head | n/a |
| 21 | Jev notice (Discord `runtime/jev.rs` `notice_text`) and notices paused (`judge/notice.rs` `paused_text`) | in | Alert / FYI |
| 22 | Restart notice (Discord `render::restarted`) | in | FYI |
| 23 | Bind notice (Discord `runtime/succession.rs`) | out, follow-up: its words are pinned in `runtime.rs` (add nothing) and in theseusd's tests | n/a |
| 24 | Reminder (Discord `runtime/remind.rs` `text`), routine failure (`rpc/routines_watch.rs` `routine_note`), routine month spent (`routine.rs`) | in: the owner's note whole under the head | Action / Alert / Action |
| 25 | Voice deaf, lost, rejoined, still deaf (Discord `runtime/voice/deaf.rs` constants) | in | Alert / Alert / FYI / Action |
| 26 | Disk crossing (Discord `diskwords.rs`) | in | Alert / FYI |
| 27 | needs-you ping and cards | OUT (task flow) | n/a |
| + | MCP changed (Discord `courier.rs` `mcp_note`) | in | FYI |
| + | Watch notices (`watch/run.rs` `notice_text`: met, changed, expired, failing) | in | FYI, Alert when failing |
| + | Earned autonomy card (`rpc/earned.rs` `card_post`; the CLI's `card_text` is unchanged) | in | Decision |
| + | Seal card (`rpc/thread_seal.rs`), link card (`thread_link_seen.rs`) | in | Decision |
| + | Retraction card (`retract.rs` `card_text`) | in | Decision |
| + | Standing kept / ask cards (`capture/mod.rs` `kept_card`, `ask_card`) | in | FYI / Decision |
| + | Persona file missing (`turn/persona_step.rs` `missing_text`) | in | Alert |
| + | Learning-loop proposal notice (`learning/propose.rs`) | out: the learning-loop row may be in flight beside this one | n/a |
| + | Job status at a wait's beat (`toolrun/jobs_tools/wait_status.rs`), noop retry (`turn/noop_step.rs`) | out: the owner's own job's or turn's live progress | n/a |
| + | Drafts (`draft*`), publish (`rpc/publish.rs`), policy notice (`external.rs`) | out: other rows' areas, or the owner's own act | n/a |
| + | The TUI's notices (`theseus_client::notify`) | out: the work board's one-line pings (task ends, questions), shared with the task flow | n/a |

### Every in-scope message, before and after

Each pair uses one invented fixture: the same inputs before and after.

#### Self backlog night (the owner's example)

~~~
BEFORE
Self backlog, last night:
- started: Turns fail with timeout (tsk_ef56)
- filed: Calls from harness fail (tsk_ab12) (tsk_ab12)
- filed, waits for your word (keel): Calls from harness fail (tsk_cd34) (tsk_cd34)
- dropped for size: Calls from harness fail (tsk_gh78) (tsk_gh78)
Reorder or drop with `theseus self move <item> <rank>` / `theseus self drop <item>`; `theseus self backlog` lists it.

AFTER
✋ Action · Self-improvement backlog: 1 waiting for your word, 2 new, 1 started, 1 dropped
from last night's read of the day's failures and slow turns
- started work on: Calls from harness fail (tsk_ef56): seen 3 times since 2026-09-21 (item tsk_ef56)
- new: Calls from harness fail (tsk_ab12): seen 3 times since 2026-09-21 (item tsk_ab12)
- new, waits for your word (keel): Calls from harness fail (tsk_cd34): seen 3 times since 2026-09-21 (item tsk_cd34)
- dropped for size: Calls from harness fail (tsk_gh78): seen 3 times since 2026-09-21 (item tsk_gh78)
Reorder or drop with `theseus self move <item> <rank>` / `theseus self drop <item>`; `theseus self backlog` lists it.
~~~

(The fixture's titles carry their own id. A real item reads `- new: Calls from harness fail: seen 3 times since
2026-10-21 (item tsk_ab12)`.) The count is the cause's `Meta.count` and "since" is the task's `created_at_ms`. The item
titles themselves ("Calls from harness fail") are not reworded: they also feed the locked needs-you card's "Theseus's
self-repair: <backlog title>", so changing them would move the task flow. See the follow-ups.

#### Weekly self digest

~~~
BEFORE
What Theseus changed about itself this week (2026-09-21 to 2026-09-28)
Nothing changed.

AFTER
ℹ️ FYI · What Theseus changed about itself this week
the weekly self digest, 2026-09-21 to 2026-09-28
Nothing changed.
~~~

#### Self notice

~~~
BEFORE
Self, night of Sep 21 ($1.00 of $30 · 0 halts)

Waits for you
• graduate ext-fetch asks for network (keel): theseus self backlog show 7

Filed 0 · updated 0 · dropped 0 — "halt self" stops everything

AFTER
✋ Action · Self-improvement last night: 1 waits for you
the night of Sep 21 ($1.00 of $30 · 0 halts)

Waits for you
• graduate ext-fetch asks for network (keel): theseus self backlog show 7

Filed 0 · updated 0 · dropped 0 — "halt self" stops everything
~~~

#### Self change installed

~~~
BEFORE
Theseus changed itself: self branch self/2026-10-20-lsp (tsk_ab12) joined main as 4a1c9e0b77aa and is installed and running.
What: w
Why: y
Review: no review row found
Numbers: main's gate on the merge green, 30 s; 1 try(s)
Cost: $0.5000 (build $0.5000, review $0.0000)
Revert: `theseus self revert 4a1c9e0b77aa55`

AFTER
ℹ️ FYI · Theseus changed itself
self branch self/2026-10-20-lsp (tsk_ab12) joined main as 4a1c9e0b77aa and is installed and running
What: w
(the rest as before)
~~~

#### Self-join keel card

~~~
BEFORE
Self branch self/b (tsk_1) is reviewed and waits for you: it touches the hard keel (sealed).
- sealed: x
The change:
```
1 file
```
Join it by hand when you agree, or drop it; Theseus never joins it on its own.

AFTER
❓ Decision · Join or drop self branch self/b
its review passed (tsk_1), but it touches the hard keel (sealed)
- sealed: x
The change:
```
1 file
```
Join it by hand when you agree, or drop it; Theseus never joins it on its own.
~~~

#### Self-join merge keel, unsigned, keel stop

~~~
BEFORE
Self branch self/b (tsk_1) stopped at the keel on its merge: main's keel guard found what needs your yes. Its gate log: /state/self/gates/b.log.
AFTER
❓ Decision · Self branch self/b needs your yes to join
main's keel guard stopped its merge (tsk_1)
Its gate log: /state/self/gates/b.log.

BEFORE
Self branch self/b (tsk_1) passed its review and main's gate, but [self] repo names no user.signingkey, and an unsigned merge never reaches main. Set one for the repo, or join it by hand.
AFTER
✋ Action · Set a signing key to join self branch self/b
it passed its review and main's gate (tsk_1), but [self] repo names no user.signingkey
Without one, an unsigned merge never reaches main. Set one for the repo, or join it by hand.

BEFORE
Self branch self/b (tsk_1) stopped at the keel: main's keel guard found what needs your yes. It waits for you and is never retried around. Its report: /state/self/reports/b.md. Read it, then ack the finding with a signed Keel: trailer, or drop the branch.
AFTER
❓ Decision · Ack or drop self branch self/b
it stopped at the keel (tsk_1): main's keel guard found what needs your yes
It waits for you and is never retried around. Its report: /state/self/reports/b.md. Read it, then ack the finding with a signed Keel: trailer, or drop the branch.
~~~

#### Revert cards

~~~
BEFORE
Theseus could not revert its own change 4a1c9e0b77aa (self branch self/b, tsk_1): it stopped at the gate. main's gate is red.
The change is still on main. Revert it by hand when you agree: `git revert -m 1 4a1c9e0b77aa55`.
AFTER
✋ Action · Theseus could not revert its change 4a1c9e0b77aa: revert it by hand
its revert of self branch self/b (tsk_1) stopped at the gate: main's gate is red
The change is still on main. Revert it by hand when you agree: `git revert -m 1 4a1c9e0b77aa55`.

BEFORE
Theseus reverted its own change: self branch self/b (tsk_1), joined as 4a1c9e0b77aa, is undone as cc11.
Why: the gate signal turned red within 24 h of the join.
Evidence: a red gate. Found by testing 3 commits of main.
The revert passed main's gate and is signed and pushed, and the install is under way.
To put the change back, revert the revert: `git revert cc11`.
AFTER
⚠️ Alert · Theseus reverted its own change
the gate signal turned red within 24 h of the join
Self branch self/b (tsk_1), joined as 4a1c9e0b77aa, is undone as cc11.
Evidence: a red gate. Found by testing 3 commits of main.
The revert passed main's gate and is signed and pushed, and the install is under way.
To put the change back, revert the revert: `git revert cc11`.

BEFORE
Your veto of self branch self/b (tsk_1) is done: joined as 4a1c9e0b77aa, reverted as cc11, signed and pushed to main. Its backlog item is dropped and will not be filed again.
AFTER
ℹ️ FYI · Your veto of self branch self/b is done
you vetoed it (tsk_1); it was joined as 4a1c9e0b77aa
Reverted as cc11, signed and pushed to main. Its backlog item is dropped and will not be filed again.
~~~

#### Narrowed policy

The proposal card was `Proposal::words()` (unchanged, still used by the dry run). Its first line is now the head, and
the rest of its paragraphs are unchanged:

~~~
BEFORE
A narrower policy for theseus-hand-owner (proposal 8dea8b04c1402132).

Evidence: 14 days of CloudTrail, 412 events of this role over 9 pages, calling 2 distinct actions.
...
AFTER
❓ Decision · Apply or decline a narrower AWS policy for theseus-hand-owner
the weekly read of its CloudTrail found it allows more than it calls (proposal 8dea8b04c1402132)
Evidence: 14 days of CloudTrail, 412 events of this role over 9 pages, calling 2 distinct actions.
...

BEFORE
theseus-hand was denied s3:GetObject (2 AccessDenied since the owner applied its narrower policy nar_1). If a legitimate call was refused, revert with `theseus self narrow revert nar_1`; the policy before it comes back through a change set.
AFTER
⚠️ Alert · AWS denied theseus-hand after its narrower policy
2 AccessDenied since the owner applied its narrower policy nar_1
Denied: s3:GetObject. If a legitimate call was refused, revert with `theseus self narrow revert nar_1`; the policy before it comes back through a change set.
~~~

#### Home size

~~~
BEFORE
📏 DM @owner's home is 40,000 tokens, 4% of the model's window. `theseus history` shows what it holds.
AFTER
ℹ️ FYI · DM @owner's home is getting long
it holds 40,000 tokens, 4% of the model's window, past the warning line
`theseus history` shows what it holds.
~~~

With a stretch to split, the body is unchanged: "To see what moving its oldest turns into a thread would save: `theseus
thread split home@… --dry-run`. …". It is still the first backticked text, and
`the_warning_names_the_place_and_runs_as_written` runs it as written.

#### Privacy

~~~
BEFORE
⚠️ Privacy: Jev read a message in a shared place as trying to draw out your personal details (0.91):
> where does your owner live
The reply was not held; the place rule keeps your private recall out of shared places.
AFTER
⚠️ Alert · Someone may be after your personal details
Jev read a message in a shared place as trying to draw out your personal details (0.91):
> where does your owner live
The reply was not held; the place rule keeps your private recall out of shared places.

BEFORE
⚠️ Privacy: Jev read a message from `guest` in a shared place, `#lobby`, as trying to draw out your personal details (0.91):
> where does your owner live
The reply was held: no model was called, and the requester was told "I don't share that here."
AFTER
⚠️ Alert · A reply was held to keep your personal details private
Jev read a message from `guest` in a shared place, `#lobby`, as trying to draw out your personal details (0.91):
> where does your owner live
The reply was held: no model was called, and the requester was told "I don't share that here."
~~~

#### Day ceiling (the runaway line reads the same, with "daily runaway line" and `daily_runaway_usd`)

~~~
BEFORE
⛔ Today's model spend reached the $5 daily ceiling ($5 spent): a turn's call was not made, and no model call will be, however small, until the day turns at 2026-10-23 00:00 local time. A refused turn or task ends; it does not wait for the day to turn. A higher `[kernel] daily_spend_ceiling_usd` applies at the daemon's next start.
AFTER
⚠️ Alert · Model calls are stopped for today
Today's model spend reached the $5 daily ceiling ($5 spent).
A turn's call was not made, and no model call will be, however small, until the day turns at 2026-10-23 00:00 local time. A refused turn or task ends; it does not wait for the day to turn. A higher `[kernel] daily_spend_ceiling_usd` applies at the daemon's next start.

BEFORE
⚠️ Today's model spend is $6, 1.2× the $5 daily ceiling, and the work goes on: the ceiling warns (`[kernel] daily_spend_ceiling_mode = "notify"`). No runaway line is set (`[kernel] daily_runaway_usd = 0`). The next warning comes at $5. To stop now: `theseus stop`.
AFTER
⚠️ Alert · Today's spend passed the daily ceiling, and the work goes on
Today's model spend is $6, 1.2× the $5 daily ceiling.
The ceiling warns (`[kernel] daily_spend_ceiling_mode = "notify"`). No runaway line is set (`[kernel] daily_runaway_usd = 0`). The next warning comes at $5. To stop now: `theseus stop`.
~~~

#### AWS

~~~
BEFORE
⚠️ AWS spend this month in account 111122223333 is $90.00, past its $80.00 warning line (`[aws.accounts.111122223333] monthly_warn_usd`). The month's budget is $100.00: AWS spend still stops at its lines, so this only warns.
AFTER
⚠️ Alert · AWS spend in account 111122223333 is past its warning line
this month's spend is $90.00, past $80.00 (`[aws.accounts.111122223333] monthly_warn_usd`)
The month's budget is $100.00: AWS spend still stops at its lines, so this only warns.

BEFORE
💸 AWS this hour: $3 reserved and spent by hands on account 111122223333, past its $2 line (hourly_alert_usd). Only an alert: nothing was refused.
AFTER
⚠️ Alert · AWS hands spent past this hour's line on account 111122223333
$3 reserved and spent by hands this hour, past its $2 line (hourly_alert_usd)
Only an alert: nothing was refused.

BEFORE
🚂 AWS runaway mode on account 111122223333: $30 reserved and spent this hour, 10 × its $2 line (hourly_alert_usd). New AWS actions that reserve are refused until 18:00; running hands finish, and cancels, lists and status reads still run. To change it, raise hourly_alert_usd or runaway_factor under [aws.accounts.111122223333] and restart.
AFTER
⚠️ Alert · AWS runaway mode on account 111122223333: new AWS actions are refused
$30 reserved and spent this hour, 10 × its $2 line (hourly_alert_usd)
New AWS actions that reserve are refused until 18:00; running hands finish, and cancels, lists and status reads still run. To change it, raise hourly_alert_usd or runaway_factor under [aws.accounts.111122223333] and restart.
~~~

#### Google lapse, persona file

~~~
BEFORE
⚠️ Google access lapsed: run `theseus google auth`
AFTER
✋ Action · Sign in to Google again
Google's refresh for wren@example.com was refused: access lapsed
Run `theseus google auth`.

BEFORE
The file /p/SOUL.md of persona wren cannot be read (denied). A request whose persona has no readable file compiles on the built-in default; its section says the file is missing.
AFTER
⚠️ Alert · Persona wren's file cannot be read
the compile met /p/SOUL.md unreadable: denied
A request whose persona has no readable file compiles on the built-in default; its section says the file is missing.
~~~

#### Initiative and the posted digest

~~~
BEFORE
Good morning: 2 tasks ended.
AFTER
ℹ️ FYI · Morning note
what happened in the last day, told once each morning
Good morning: 2 tasks ended.

BEFORE
Two notes say the same.
AFTER
ℹ️ FYI · Something the night's consolidation noticed
from the nightly pass over your memory
Two notes say the same.

BEFORE
◆ 2 reported
📋 task a: done
AFTER
ℹ️ FYI · What happened since your last digest
the scheduled digest ([discord] digest_at)
◆ 2 reported
📋 task a: done
~~~

The `initiative.spoke` row keeps the spoken words without the head, so the ratings and the week's measure read what the
persona said.

#### Ontology, Jev

~~~
BEFORE
🗂 Jev started a new topic and filed this session under it: harbour › tides (90% sure). `theseus ontology undo jdg_1` takes it back.
AFTER
ℹ️ FYI · Jev started a new topic: harbour › tides
Jev filed this session under it (90% sure)
`theseus ontology undo jdg_1` takes it back.

BEFORE
🗂 Is this harbour? (60% sure)
`theseus ontology member ses_1 +c` files it.
AFTER
❓ Decision · Where does this session belong?
Jev was not sure enough to file it (60% sure)
Is this harbour?
`theseus ontology member ses_1 +c` files it.

BEFORE
🔔 notified after it ran: `proc.run` · Jev: 95% risky
> rm -r build
The call did not wait. Judgment `jdg_1`.
AFTER
⚠️ Alert · Jev flagged a call that already ran: `proc.run`
notified after it ran: Jev: 95% risky
> rm -r build
The call did not wait. Judgment `jdg_1`.

BEFORE
🔕 Jev's notices are paused until tomorrow: more than 30 notices today.
AFTER
ℹ️ FYI · Jev's notices are paused until tomorrow: more than 30 notices today
the day's brake on Jev's notices
~~~

#### Routines, watches, reminders

~~~
BEFORE
✗ Tide check failed: a timeout
AFTER
⚠️ Alert · ✗ Tide check failed: a timeout
its latest scheduled run did not finish

BEFORE
🔁 Tide check: this month's $1.00 is spent, so its runs are skipped until the 1st. Raise it with `theseus routine update ab12cd --month <usd>`.
AFTER
✋ Action · 🔁 Tide check: runs are skipped until the 1st
this month's $1.00 is spent
Raise it with `theseus routine update ab12cd --month <usd>`.

BEFORE
👁 the deploy landed → MERGED
AFTER
ℹ️ FYI · Watch met: the deploy landed
a watch set for you, checked on its own
👁 the deploy landed → MERGED

BEFORE
⏰ stretch
AFTER
✋ Action · Reminder
⏰ a reminder you set
stretch
~~~

A late reminder's trigger line reads `⏰ a reminder you set · every 1d, #4 · 2 min late, the daemon was down`, which
was the old `-#` line. The owner's note is never reworded.

#### The DM's cards

~~~
BEFORE
Earned autonomy: 1 thing you approve every time.
1. `proc.run cargo test`: you approved all 0 since Dec 31, never declined. Let it run with a notice?
   ...
AFTER
❓ Decision · Earned autonomy: 1 thing you approve every time
the weekly read of your approvals ([policy.earned])
1. `proc.run cargo test`: you approved all 0 since Dec 31, never declined. Let it run with a notice?
   ...

BEFORE
🔒 This looks embargoed: it looks embargoed
Thread «harbour» (harbour).
AFTER
❓ Decision · Seal thread «harbour»?
This looks embargoed: it looks embargoed
Thread «harbour» (harbour).

BEFORE
🔗 Notes of «datatidy stats» answered questions in «datatidy pipeline» twice.
theseus thread link pipeline stats
AFTER
❓ Decision · Link threads «datatidy pipeline» and «datatidy stats»?
Notes of «datatidy stats» answered questions in «datatidy pipeline» twice.
theseus thread link pipeline stats

BEFORE
⚠️ Retracted: “the tide is at 5” was marked wrong (wrong date). 0 tasks used it and may need a re-run:
Re-run?
AFTER
❓ Decision · Re-run the tasks that used a retracted note?
Retracted: “the tide is at 5” was marked wrong (wrong date).
0 tasks used it and may need a re-run:
Re-run?

BEFORE
📌 Kept as a standing order everywhere: x
AFTER
ℹ️ FYI · Kept your words as a standing record
Jev read your message as an order, a preference or a fact to keep
📌 Kept as a standing order everywhere: x

BEFORE
📌 Keep as a standing order everywhere?
AFTER
❓ Decision · Keep your words as a standing record?
Jev read your message as one to keep, and asks first
📌 Keep as a standing order everywhere?
~~~

#### Discord's own: restart, disk, MCP, voice

~~~
BEFORE
Restarted <t:1790000000:T> onto the vault's config note, which had changed since my local copy: model.
AFTER
ℹ️ FYI · Restarted <t:1790000000:T> onto the vault's config note
the note had changed since my local copy: model

BEFORE
Free space under the state dir is low: 4,000 MB free of 100,000 MB (health warns below 5,120 MB).
AFTER
⚠️ Alert · The disk is low
free space under the state dir: 4,000 MB free of 100,000 MB (health warns below 5,120 MB)

BEFORE
Free space under the state dir is below the floor: 4,000 MB free of 100,000 MB (floor 1,024 MB). New jobs are refused, and running ones are stopped.
AFTER
⚠️ Alert · The disk is below the floor: new jobs are refused
free space under the state dir: 4,000 MB free of 100,000 MB (floor 1,024 MB)
Running jobs are stopped.

BEFORE
Free space under the state dir is back to normal: 4,000 MB free of 100,000 MB.
AFTER
ℹ️ FYI · The disk is back to normal
free space under the state dir: 4,000 MB free of 100,000 MB

(back over the floor, still low: ℹ️ FYI · The disk is back over the floor, but still low / <facts> / Jobs run again.)

BEFORE
MCP server tide changed its tools (+1 tool). The new list is offered from the next turn.
AFTER
ℹ️ FYI · MCP server tide changed its tools
the server sent a new list (+1 tool)
The new list is offered from the next turn.

BEFORE  I can't hear the call (Discord's voice encryption didn't finish setting up). Rejoining…
AFTER   ⚠️ Alert · Voice can't hear the call / Discord's voice encryption didn't finish setting up / Rejoining…
BEFORE  I've stopped hearing you (your audio isn't reaching me). Rejoining…
AFTER   ⚠️ Alert · Voice stopped hearing you / your audio isn't reaching me / Rejoining…
BEFORE  I can't hear the call (Discord didn't say whose audio is whose). Rejoining…
AFTER   ⚠️ Alert · Voice can't hear the call / Discord didn't say whose audio is whose / Rejoining…
BEFORE  Rejoined: I can hear you now
AFTER   ℹ️ FYI · Voice rejoined: I can hear you now / the rejoin after the call went deaf
BEFORE  Still can't hear: try /leave and /join
AFTER   ✋ Action · Voice still can't hear: try /leave and /join / the rejoin did not fix it
(" / " marks a line break in the voice rows.)
~~~

### Character counts (the fixtures above)

Every message grows by no more than its two head lines (often less, because the old first line folds into the head).
There is one exception. The backlog night grows by 283 characters against a 149-character head, because each item
line now says how often its cause showed and since when, which the task asks for.

| Message | Before | After | Change | Head (2 lines) |
|---|---|---|---|---|
| self backlog night | 393 | 676 | +283 | 149 |
| weekly self digest | 87 | 118 | +31 | 101 |
| self notice | 199 | 253 | +54 | 100 |
| self-join installed | 314 | 321 | +7 | 131 |
| self-join keel card | 210 | 227 | +17 | 109 |
| self-join merge keel card | 144 | 137 | -7 | 98 |
| self-join unsigned card | 187 | 235 | +48 | 142 |
| keel stop | 255 | 282 | +27 | 119 |
| revert held | 222 | 256 | +34 | 159 |
| reverted | 366 | 370 | +4 | 93 |
| veto done | 174 | 203 | +29 | 101 |
| narrowed policy red | 240 | 283 | +43 | 125 |
| home size | 101 | 152 | +51 | 113 |
| privacy warning | 216 | 258 | +42 | 142 |
| privacy held | 248 | 302 | +54 | 178 |
| day ceiling | 331 | 374 | +43 | 105 |
| day runaway line | 330 | 373 | +43 | 110 |
| day warning | 264 | 311 | +47 | 125 |
| AWS month warning | 224 | 247 | +23 | 160 |
| AWS hands hour alert | 145 | 186 | +41 | 150 |
| AWS hands runaway | 339 | 375 | +36 | 150 |
| Google lapse | 50 | 126 | +76 | 99 |
| morning note | 28 | 104 | +76 | 75 |
| consolidation finding | 23 | 115 | +92 | 91 |
| posted digest | 27 | 116 | +89 | 88 |
| ontology mint card | 131 | 137 | +6 | 92 |
| ontology ask card | 74 | 152 | +78 | 90 |
| Jev notices paused | 70 | 109 | +39 | 109 |
| routine failed | 30 | 81 | +51 | 81 |
| routine month spent | 142 | 144 | +2 | 83 |
| watch notice | 28 | 106 | +78 | 77 |
| earned autonomy card | 305 | 369 | +64 | 112 |
| seal card | 70 | 104 | +34 | 76 |
| link card | 111 | 177 | +66 | 142 |
| retraction card | 110 | 165 | +55 | 118 |
| standing kept card | 40 | 152 | +112 | 111 |
| standing ask card | 38 | 142 | +104 | 103 |
| persona file missing | 177 | 208 | +31 | 91 |
| restarted | 102 | 112 | +10 | 112 |
| disk low | 97 | 116 | +19 | 116 |
| disk below the floor | 148 | 163 | +15 | 137 |
| disk back to normal | 78 | 95 | +17 | 95 |
| mcp tools | 88 | 123 | +35 | 79 |
| jev notice | 107 | 151 | +44 | 96 |
| reminder | 9 | 48 | +39 | 40 |
| voice deaf | 87 | 99 | +12 | 88 |
| voice lost | 67 | 76 | +9 | 65 |
| voice rejoined | 28 | 79 | +51 | 79 |
| voice still deaf | 38 | 81 | +43 | 81 |

### What I changed (commits)

- `daf76c52` core: one head for the owner's unasked messages, on self-improvement's (the helper, the key test, rsi's
  families, the narrower-policy card).
- `be03c140` core: the head on the turn's warnings, spend, AWS, Google and personas.
- `8886ea97` core: the head on initiative, the ontology, Jev, routines, watches and the DM's cards. It also carries the
  two Discord gateway tests whose text these core changes move (seal, reply-watch).
- `e07b09e5` discord: the head on the binding's own owner messages, the Discord key test, and the reminder's
  expectations, including theseusd's `tests/wakes.rs`.
- `96bdd57e` docs: `docs/messages.md` (the catalog) and core AGENTS.md's "Messages to the owner" rule.

Several modules became `pub(crate)` so the crate-level key test can reach each producer's pure text function:
`rsi::digest` and `rsi::digest::notice`, `rsi::backlog::start`, `rsi::join::joining`, `rsi::revert::doing`,
`rpc::{earned, routines_watch, thread_seal}` and `turn::persona_step`. Nothing outside the crate sees more.

The gate ran on the head commit. The intermediate commits were not each built and gated separately: the VM's disk
allowance did not allow it. Each is cut so that its own key-test variant names only the families it touched.

### How I proved it

- **Key tests:** `tests_owner_note::every_unasked_message_starts_with_a_kind_and_a_title` (core, 39 families) and
  `tests_owner_note::every_unasked_discord_message_starts_with_a_kind_and_a_title` (Discord, 9 families). The voice
  notices are checked in `runtime/voice/deaf.rs`'s own test, because `runtime::voice` is private to `runtime.rs` and
  `runtime.rs` takes no line. Also `tests_owner_note::the_backlog_night_says_each_item_in_plain_words` and
  `owner_note::tests::a_head_is_a_tagged_title_and_a_trigger`.
- **Planted reverts:**
  - `google/tend.rs`'s `lapse_text` was put back to `format!("⚠️ {}", LAPSED)`. The core key test failed with
    `families without a head: Google lapse: "⚠\u{fe0f} Google access lapsed: run `theseus google auth`"`.
  - `diskwords.rs`'s "back to normal" arm was put back to a plain `format!`. The Discord key test failed with
    `families without a head: disk back to normal: "Free space under the state dir is back to normal: …"`.
  - Both files were restored with `cp`, then `touch`ed; `git status` was clean after.
- **Suites:** `TZ=America/Phoenix cargo nextest run -p theseus-core -p theseus-discord -p theseus -p theseus-tui`: 5,859
  run, 5,850 passed and 9 failed on the first run. All 9 were text expectations of the new heads; I fixed them (listed
  below) and reran all 9 green. The full gate (below) then ran everything.
- **`cargo clippy --workspace --all-targets -- -D warnings`:** clean. **`cargo fmt --check`:** clean.

### Changed test expectations (before → after)

- `tests_day_notify.rs`: `"⚠️ Today's model spend is"` → `"⚠️ Alert · Today's spend passed the daily ceiling, and the work
  goes on\nToday's model spend is"`; `starts_with("⛔ Today's model spend reached the $5 daily runaway line")` →
  `starts_with("⚠️ Alert · Model calls are stopped for today\nToday's model spend reached the $5 daily runaway line")`.
- `rsi/digest/tests_notice.rs` golden: the first line `Self, night of {m} {d} ({w} · $4.12 of $30 · 0 halts)` → two
  lines: `✋ Action · Self-improvement last night: 1 fixed, 1 made, 1 retired, 1 waits for you` and `the night of …
  (… · $4.12 of $30 · 0 halts)`. The rest is byte for byte.
- `tests_work_digest.rs`: `starts_with("◆ ")` → `starts_with("ℹ️ FYI · What happened since your last digest\n")`, and its
  third line `starts_with("◆ ")`.
- `tests_routine_trust.rs`: `"✗ Tide check blocked: fs.edit needs approval (not in its grant)"` → `"⚠️ Alert · ✗ Tide
  check blocked: fs.edit needs approval (not in its grant)\nits latest scheduled run did not finish"`;
  `starts_with("✗ Tide check failed: ")` → `starts_with("⚠️ Alert · ✗ Tide check failed: ")`.
- `turn/home_size_step.rs` unit test: `"#harbor's home is 512,300 tokens, 51% of the model's window"` → `"#harbor's home is
  getting long\nit holds 512,300 tokens, 51% of the model's window"`. The `< 450` characters check and the
  no-`split`-without-a-stretch check stand.
- `watch/tests_watch.rs`'s `posts` helper now asserts every watch notice `is_headed` and returns the text past the head.
  So the watch tests' own `starts_with("👁 …")` expectations stand unchanged and each also checks the head. No assertion
  was dropped.
- Discord `runtime/remind.rs`: `"⏰ stretch"` → `"✋ Action · Reminder\n⏰ a reminder you set\nstretch"`; the late form
  `"⏰ stretch\n-# every 1d, #4 · 2 min late, the daemon was down"` → `"✋ Action · Reminder\n⏰ a reminder you set · every
  1d, #4 · 2 min late, the daemon was down\nstretch"`.
- Discord `diskwords.rs`: its three `assert_eq!` texts → the headed texts above.
- Discord `tests_gateway/remind.rs`: `starts_with("⏰ stretch")` → `starts_with("✋ Action · Reminder\n⏰ a reminder you
  set") && ends_with("\nstretch")`. `tests_gateway/seal.rs`: `starts_with("🔒 This looks embargoed: …")` →
  `starts_with("❓ Decision · Seal thread «security fix»?\nThis looks embargoed: …")`. `tests_gateway/reply.rs`:
  `starts_with("👁 ben replied in #lab: \"")` → the watch head and then that line.
- theseusd `tests/wakes.rs`: the `reminders` filter `starts_with("⏰ stretch")` → `starts_with("✋ Action · Reminder\n") &&
  ends_with("\nstretch")`.
- Texts kept so that existing tests still hold: "an unsigned merge never reaches main", "stopped at the keel", "could
  not revert", "paused until tomorrow: <why>", "notified after it ran", "looks embargoed", "Notes of «…» answered
  questions in «…» twice", "Re-run?", "Today's model spend reached", "% of the model's window", "theseus history".

**Goldens:** `tests_output::the_cores_output_matches_its_golden` and the CLI goldens (`crates/theseus/tests/golden.rs`)
pass unchanged under `TZ=America/Phoenix`. None of their lines moved.

### The locked flow did not move

`git diff --stat ae420a49 HEAD` on `runtime.rs`, `tests_default_forum.rs`, `tests_task_home.rs`, `tests_walk1b.rs`,
`tests_task_owed.rs`, `tests_forum_archive.rs`, `tests_task_threads.rs`, `render/` (board_v2, approval and the rest),
`tests_gateway.rs`, `needs_you*` and `turn/budget_step.rs` is empty. In the gate's suite, all 73 of their tests passed,
`render::board_v2`'s golden and `render::approval::tests_approval::the_approval_card_reads_as_the_owner_approved_it`
included, with none failing.

### Live check for the maintainer

On a scratch daemon of this build (its own `--config`, `--socket` and `--state-dir`, a fresh state dir) with a Discord
test DM bound on the stand-ins or a test guild:

1. `theseus --socket <sock> remind --in 1m "stretch"`: the DM gets `✋ Action · Reminder`, then `⏰ a reminder you set`,
   then `stretch`, with Done and the Snooze buttons as before.
2. Set `[kernel] daily_spend_ceiling_usd = 0.01` with `daily_spend_ceiling_mode = "notify"` and run one `theseus ask`: the
   DM gets `⚠️ Alert · Today's spend passed the daily ceiling, and the work goes on` and the spend line.
3. Start a task from the DM ("Make this a task"): the task reply, the 🔔/🧵 lines, the board, the forum post and its
   report read exactly as before. This is the locked flow; nothing there carries a head.
4. With `[self] mode = "act"` and a seeded night (as `rpc/tests_self_backlog.rs` seeds it), the backlog night reads as in
   the before/after above.

### Left, uncertain, and choices for the owner

- **Backlog titles.** The owner's example quoted "Calls from harness fail". The item lines now say how often and since
  when, but the title words come from `rsi/backlog/read.rs`. Those titles also reach the locked needs-you card ("Theseus's
  self-repair: <backlog title>"), so rewording them would move the task flow. A follow-up could give each cause a
  plain-words line (for example "tool calls made by the harness failed") used only by the night's note.
- **Kinds I chose:** the home-size warning is FYI (the split is optional). The day-ceiling warning is Alert (the work
  goes on). A routine's failure keeps its `✗` inside the title: Discord's `tests_gateway/horizon.rs` looks for "✗ Morning
  digest failed" in the post. The jev notice is Alert.
- **Not headed (follow-ups):** the bind notice (pinned in `runtime.rs`, which is at its ceiling, and in theseusd's
  tests), the learning loop's proposal notice (that row's area), the job beat status and the hands group line (live
  progress), and the TUI's notices (`theseus_client::notify`, the work board's pings that the task flow shares).
- **Discord file outside the row's list:** `runtime/jev.rs`, the Jev notice's words. It is a small edit, and the
  `notified after it ran` phrase that `tests_gateway.rs` looks for is kept. Also `courier.rs`'s `mcp_note`. Neither is
  `report_post` nor `failed_post`.

### Store format

Unchanged at 49. A message's text is not a stored layout. The `spend.ceiling` and `spend.day_warning` rows carry the new
text, as they carried the old.

### A docs/status.md sentence for the integrator

"Every message Theseus sends the owner unasked now opens with a kind tag and an operational title (ℹ️ FYI, ✋ Action,
❓ Decision, ⚠️ Alert) and a line saying what triggered it (`owner_note`, theseus-x8b9k). The catalog is
`docs/messages.md`, and the Discord DM task flow is unchanged."

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` at `96bdd57e`, run on a clean build
tree. A first run died in its test build from a full disk. The VM's disk allowance does not hold two copies of the
workspace's test binaries, so I ran `cargo clean`, then turned incremental off and reran.

- door, keel, keel tests, fmt, shape, features, clippy, clippy-probe, cockpit, test build: passed.
- reader rule (`tests_registry`, 15 tests): passed.
- **suite:** 7,963 run, 7,929 passed and 34 failed, 53 skipped. All 34 are the known L1 failures of a root VM with no job
  cgroup (theseus-pv6i): 19 `theseus-sandbox::contract` tests, `theseus-sandbox::bench spawn_100`, and 14
  `theseusd::sandbox` tests. Nothing else failed. `the_cores_output_matches_its_golden` passed under
  `TZ=America/Phoenix`. `a_filtered_page_equals_the_scans_answer` and `python3s_repl_computes_on_the_screen` passed.
- **After the suite (run by hand, as the brief says):** protocol types: `cockpit/src/protocol.gen` is unchanged (no
  protocol type was added). compiled-under-lock: every compile was in the test build, before the lock. The benches are
  off under `THESEUS_GATE_NO_BENCH`.
- **Keel guard:** `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` printed nothing and exited 0.
