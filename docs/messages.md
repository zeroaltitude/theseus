# Messages to the owner

Every message Theseus sends the owner unasked opens the same way (theseus-x8b9k), so a person who meets one for the
first time knows what it is for without a paragraph of explanation:

```
<kind tag> · <operational title>
<what triggered it, in plain words>
<the body: the facts the message carried before>
<the action, if any, last: a command or a button>
```

The kinds:

| Tag | Meaning |
|---|---|
| `ℹ️ FYI` | Nothing to do: just telling you. |
| `✋ Action` | You need to do something. |
| `❓ Decision` | Choose (the buttons or the commands say how). |
| `⚠️ Alert` | Something is wrong. |
| `⏰ Reminder` | A reminder you set: your own words are the title (`⏰ Reminder · stretch`), then how it came. |

The tag is the message's one icon: a title never starts with a glyph of its own (`⚠️ Alert · Tide check failed: a
timeout`, never `⚠️ Alert · ✗ Tide check failed`). `head` drops one a title brings (a routine's `✗` line), and
`is_headed` refuses a text that keeps one; a reminder's title is the owner's words as he wrote them (the owner,
2026-10-22).

The head is built by one helper, `theseus_core::owner_note` (`head`, `note`; `is_headed` is what the tests ask). Every
new unasked message uses it. A digest's items are one line each in plain words (what, how often, since when), never a
bare internal label or id: a self-repair backlog item's title is plain words ("Harness tool calls failing", not "Calls
from harness fail"; `rsi/backlog/read.rs`, and `plain_title` reads an item filed under the old words the same way,
in the backlog and on the needs-you card's asker line, theseus-jsdj5). The key test is theseus-core's
`tests_owner_note::every_unasked_message_starts_with_a_kind_and_a_title`, with the Discord binding's
`tests_owner_note::every_unasked_discord_message_starts_with_a_kind_and_a_title` and `runtime/voice/deaf.rs`'s.

**The Discord DM task flow is locked** (the owner, 2026-10-22) and does not use the head: the task reply, the started and
thread lines (🔔 / 🧵), the task board (📋), the forum posts and live cards, the approval and question cards (`needs_you`),
the budget question card, the place-channel budget notice, and the task reports and failure posts.

## The catalog

Core paths are under `crates/theseus-core/src/`, Discord's under `crates/theseus-discord/src/`. "DM" is the owner's DM
(`stage_to_operator` / `to_operator`); "place" is the session's own place.

| Message | Built in | Trigger | Channel, kind | Head |
|---|---|---|---|---|
| Self backlog, last night | `rsi/backlog/start.rs` `night_text` | the nightly backlog read filed, started or dropped items | DM, `self_digest` | FYI, or Action when an item waits for the owner's word |
| Weekly self digest | `rsi/digest.rs` `week_post` | a week since the last digest, `[self]` posting | DM, `self_digest` | FYI |
| Self notice (the night's) | `rsi/digest/notice.rs` `notice_text` | the first tick after `[initiative] morning_hour` with something landed or waiting | DM, `self_digest` | FYI, or Action when something waits |
| Self change installed | `rsi/join/install.rs` `notice` | a joined self branch installed | DM, `self_join` | FYI |
| Self-join keel card | `rsi/join/joining.rs` `keel_card` | a reviewed branch touches the hard keel | DM, `self_join` | Decision |
| Self-join merge keel | `rsi/join/joining.rs` `merge_keel_card` | main's keel guard stopped the merge | DM, `self_join` | Decision |
| Self-join unsigned | `rsi/join/joining.rs` `unsigned_card` | no `user.signingkey` in `[self] repo` | DM, `self_join` | Action |
| Keel stop | `rsi/pr.rs` `keel_text` | the keel guard stopped a self branch's push | DM, `self_keel` | Decision |
| Revert held | `rsi/revert/doing.rs` `held_text` | an automatic revert stopped at a phase | DM, `self_join` | Action |
| Reverted | `rsi/revert/doing.rs` `reverted_text` | a signal turned red within the revert window | DM, `self_join` | Alert |
| Veto done | `rsi/revert/doing.rs` `vetoed_text` | the owner's veto was carried out | DM, `self_join` | FYI |
| Narrower policy | `aws/narrow/proposal.rs` `Proposal::card` | the weekly CloudTrail read of a hand role | DM, `self_narrow` | Decision |
| Narrower policy red | `rsi/narrow.rs` `red_card` | AccessDenied after an applied narrower policy | DM, `self_narrow_red` | Alert |
| Home size | `turn/home_size_step.rs` `warning_text` | a place's home passed `[threads] home_warn_percent` | DM, `home_size` | FYI |
| Privacy warning | `turn/personal_step.rs` `warning_text` | Jev read a message as after the owner's details | DM, `privacy_warning` | Alert |
| Privacy, reply held | `turn/personal_step.rs` `held_text` | the same in a shared place: the reply was held | DM, `privacy_warning` | Alert |
| Day ceiling / runaway line | `day_ceiling.rs` `notice_text`, `runaway_text` | a model call refused at the day's line | DM, `spend_ceiling` | Alert |
| Day warning | `day_ceiling.rs` `warning_text` | today's spend passed the ceiling under notify | DM, `spend_ceiling` | Alert |
| AWS month warning | `aws/month_warn.rs` `warning_text` | the month's AWS spend passed `monthly_warn_usd` | DM, `spend_ceiling` | Alert |
| AWS hands hour alert | `aws/hands/watch.rs` `words` | hands spent past `hourly_alert_usd` | DM, `hands_alert` | Alert |
| AWS runaway mode | `aws/hands/runaway.rs` `entered` | spend reached `runaway_factor` × a line | DM, `hands_alert` | Alert |
| Google lapse | `google/tend.rs` `lapse_text` | Google refused the token's refresh | DM, `spend_ceiling` | Action |
| Morning note, consolidation finding | `initiative/speak.rs` `headed` (from `initiative_post`) | the day's first tick after `morning_hour`; the night's consolidation | DM, `initiative` | FYI (the persona's words unchanged below) |
| Posted digest | `initiative/digest.rs` `posted_text` | a `[discord] digest_at` slot with lines | DM, `digest` | FYI |
| Ontology mint | `judge/onto/stage.rs` `minted_text` | Jev minted a category and filed a session | DM, `onto_card` | FYI |
| Ontology ask | `judge/onto/stage.rs` `ask_text` | Jev was not sure where a session belongs | DM, `onto_card` | Decision |
| Jev notice | Discord `runtime/jev.rs` `notice_text` | Jev's notice on a call that ran | DM, `jev_notice` | Alert |
| Jev notices paused | `judge/notice.rs` `paused_text` | the day's brake | DM, `jev_paused` | FYI |
| Earned autonomy card | `rpc/earned.rs` `card_post` | the weekly read of approvals | DM, `earned_card` | Decision |
| Seal card | `rpc/thread_seal.rs` `seal_text` | a thread looks embargoed | DM, `seal_card` | Decision |
| Link card | `thread_link_seen.rs` `Pair::text` | another thread's notes answered twice | DM, `link_card` | Decision |
| Retraction card | `retract.rs` `card_text` | a retracted note reached tasks | DM, `retraction` | Decision |
| Standing kept / ask | `capture/mod.rs` `kept_card`, `ask_card` | `directive.v1` kept the owner's words, or asks | DM, `standing_card` | FYI / Decision |
| Persona file missing | `turn/persona_step.rs` `missing_text` | a persona's file cannot be read | DM, `notice` | Alert |
| Routine failed / blocked | `rpc/routines_watch.rs` `routine_note` | a routine's run failed or was blocked | place, `reminder` | Alert |
| Routine month spent | `routine.rs` `month_spent_text` | a routine's month budget is spent | place, `notice` | Action |
| Watch notice | `watch/run.rs` `notice_text` | a watch met, changed, ended or cannot check | place, `notice` | FYI, Alert when failing |
| Reminder | Discord `runtime/remind.rs` `text` | a wake's reminder fired | place, `reminder` | Reminder (the owner's note is the title; a second line on is the body) |
| Restart notice | Discord `render.rs` `restarted` | a restart onto the vault's changed config | operator channel, `restarted` | FYI |
| Disk | Discord `diskwords.rs` `disk_note` | free space crossed a line or came back | operator channel, `disk` | Alert / FYI |
| MCP changed | Discord `courier.rs` `mcp_note` | a server's tools or a prompt changed | operator channel, `mcp_changed` | FYI |
| Voice deaf / lost / rejoined | Discord `runtime/voice/deaf.rs` constants | the call's hearing | place, `notice` | Alert / FYI / Action |
| New persona | `personas/note.rs` `text` | a persona was added without a soul (CLI or cockpit); asked once | DM, `persona_soul` (buttons Write its soul, Decline) | Action |
| Personas moved | `personas/migrate.rs` `note_text` | the first start that read `[personas]` from the config into the ontology | DM, `notice` | FYI |
| Persona soul over its cap | `turn/persona_step.rs` `over_cap_text` | a compile carried a soul over `[context] persona_max_tokens`; once a digest | DM, `notice` | Alert |

## Not headed, and why

| Message | Built in | Why |
|---|---|---|
| The DM task flow (above) | `needs_you*.rs`, `turn/budget_step.rs`, Discord's task files | locked by the owner |
| Bind notice | Discord `runtime/succession.rs` `bind_notice` | its words are pinned in `runtime.rs` (at its ceiling) and theseusd's tests; a follow-up |
| Backlog's single lines | `fact/rsi_backlog.rs` | narrative lines of a session's history, one line each, not posts |
| Ontology's night lines | `judge/onto/digest.rs` | lines inside the morning note, which has the head |
| Late filing line | `judge/onto/mod.rs` `filed_late` | a reply's own footer, arriving late |
| Hands group line | `aws/hands/grid.rs` | the owner's own call's live status, edited in place |
| Job status at a beat | `toolrun/jobs_tools/wait_status.rs` | the owner's own job's live progress |
| Learning loop proposal | `learning/propose.rs` | the learning loop's row is in flight beside this one |
| Drafts, publish, policy notice | `draft*`, `rpc/publish.rs`, `external.rs` | other rows' areas, or the owner's own act |
| The TUI's notices | `theseus_client::notify` | the work board's one-line pings (task ends, questions), shared with the task flow |
