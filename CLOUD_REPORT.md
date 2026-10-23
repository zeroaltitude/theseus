# CLOUD_REPORT: ops-cal (theseus-jid7t.1)

Branch `cloud/20261023-ops-cal`, base `2d1d8d6f`. Commits (unsigned: the VM has no key):

- `95ffd7e` config: `[google] calendars`
- `b431b1d` google: the cache, `calendar.window`, the sensitive level, the tool, the brief, `check`, protocol, docs

Store format: **49, no bump** (a new META key and no new record kind). `MANIFEST_FORMAT` is untouched.
`lib.rs` added 4 lines (`pub mod calendar;`, one doc line, `CALENDAR_WINDOW`, `CALENDAR_CHANGED`).
Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): **empty output, exit 0**.

## What I built, by item

1. **`[google] calendars`**: `config/google_calendars.rs` (`CalendarEntry`, `CalendarTable`, `CalendarSpec`, `specs`,
   `validate`), wired in `config/google.rs` (`calendars` with a default of `["primary"]`, not serialized when default;
   `calendar_specs()`). Palette: brass `#c9a24a`, verdigris `#5fa08d`, lapis `#5f7fb8`, amethyst `#9a6fa3`, by position.
   Load checks: ids unique (case-insensitive, so a second `primary` is refused), no whitespace, `#rrggbb`, labels
   unique, list non-empty. The template's `# [google]` block has the key with an invented `example.test` id. I changed
   the template test's line count from 6 to 7 (a count, not an assertion removed).
2. **`theseusd check`**: it had no Google line, so I added `google::calendar_check` (printed in `check`, `main.rs`):
   `google calendar <label>: ok|not_found|busy_only|forbidden|auth_lapsed[: fix]`. Built on `Google::calendar_reach`
   (one `events.list maxResults=1`, then `freebusy.query` on a 403).
3. **Cache**: `google/calendar_cache.rs`. TTL 5 min, week widening, `syncToken`, 410 re-read, free/busy on 403,
   at most 4 windows a calendar and 1,500 events a window, memory only. Every call is `Google::call`; its row is
   returned to the caller to write (`resource` is the calendar's label). The clock is real; tests age it with
   `Cache::age` (a `cfg(test)` hook), not a paused clock, because a paused clock with real sockets can fire client
   timeouts.
4. **`calendar.window`**: protocol `calendar.rs` (`CalendarWindowParams/Result`, `CalendarWindow`, `CalendarEvent`,
   `CalendarStatus`), `rpc/calendar.rs`. Owner on a private connection only (`owner_in_private`; MCP clients are barred
   earlier by the surface table). Span over 62 days or `to <= from` refused. Not a model tool. With no `[google]` (or
   no calendar service) every calendar is `off` and nothing is read.
   **Deviation:** dispatch rides `rpc_google`'s existing arm (`import.rs` `prefixed` + `rpc_google`), not a new arm in
   `server.rs`: `dispatch` is at clippy's cognitive-complexity limit.
5. **`calendar.changed {labels}`**: `Event::CalendarChanged`, sent by `Core::calendar_told` through
   `publish_watchers` (the connections that watch every session; `bus::wide` lists it). A tender
   (`Google::calendar_tend`, one 5-minute timer wait, started by the first window read, ends when no window is kept)
   refreshes kept windows.
6. **Sensitive mark**: `google/private.rs`: `Level {Open, Private, Sensitive}`, `private::level(store, cfg, session)`,
   `session_level(_on)`, `sensitive_session`, `sensitive_lineage`, `over`, `recalled_sensitive_from`; `record` now
   returns a `Vec` (private + `google.sensitive.<sid>` in the same frame); `followed` and `inherited(_on)` carry both
   levels (they return `Vec` now). The task-local is a `Level` (`scoped`/`within`/`bound` accept a bool, meaning
   private). `data_gate` reads the level (message differs for sensitive). `turn/data_step.rs` has `session_level`;
   `turn.rs` and `compaction.rs` pass it (one line each, no net lines). `memory_lookup` marks its answer sensitive.
   `Facts.private` in `initiative/speak.rs` is a `Level`. `rpc/publish.rs` `sensitive_stays_private` refuses a node of a
   sensitive session to a shared place (this one **did fail** before the fix, as the brief expected).
7. **Tool**: `calendar.events` takes a label or id (case-insensitive) and `calendars: "all"` (lines `- [Label] …`; a
   failing calendar is one `<label> unreachable` line; extra rows ride `google_called_also`). An address not in the
   config is read as before; any other unknown name is refused listing the configured labels. The calendar names are
   read only when needed (not for the primary alone), so the default config makes the same requests as before.
   The tool definition is now at the budget (a test caps it at 220 tokens), so the description is one short sentence.
8. **Brief**: `initiative/morning_google.rs` reads each calendar, merges by start, labels each line when more than one
   is configured (a single default calendar reads byte for byte as before, including `calendar unreachable`). A
   successful read of a sensitive calendar sets `Day.sensitive`; `Day::level()` makes the note's call
   `Level::Sensitive`. The note has **no session**, so there is no session to mark; the level is carried by the call.
   Added `initiative_post_as`: a sensitive note's `initiative.spoke` row holds **no text** (the voiced words reach the
   DM only).
9. **Docs**: core `AGENTS.md` (new bullet after the Google private mark), `docs/privacy.md` (one paragraph).

## What I found about the two floors at the base

I read both before building. **They are already US-only for any private session**: `data_gate` refuses any call under
`private::in_scope()` unless `cleared_for_sensitive` (an allowlist of `Us`), and `private_session` skips every profile
that is not cleared. EU is refused at the base too (`tests_private_background` already serves the provider from an EU
host to prove it). So the brief's premise that "the base lets EU through" is **not true here**, and the EU refusal tests
are green on the base. What the level adds is the mark that the page and ops-verbs read, its propagation, the shared-place
refusal and the ledger hold. The red-on-base evidence is plant P5 below (the sensitive record alone, which the base does
not know).

## Tests, and their plants

Red first was done where the base could fail: new behaviours have no base code to run, so their "red" is the planted
revert. Each plant: change one line, run the filter, restore the file and `touch` it, `git status` clean.

| Plant | Failed tests (failing line) |
|---|---|
| P1 `record()` drops the sensitive key | `a_sensitive_result_adds_a_second_record…` (tests_sensitive.rs:284), `a_sensitive_read_keeps_the_next_turn_on_a_us_model` (:74) |
| P2 `followed()` ignores sensitive | `a_session_that_recalls_a_sensitive_note_becomes_sensitive` (:243) |
| P3 `inherited_on()` drops it | `a_task_under_a_sensitive_session_is_sensitive` (:179) |
| P4 publish guard off | `a_sensitive_node_is_not_published_to_a_shared_place` (:344) |
| P5 `session_level_on` ignores the sensitive key (the base) | 5 tests incl. `a_background_call_of_a_sensitive_session_is_refused_at_the_chokepoint` (:144) |
| P6 cache never fresh | `a_window_read_twice_within_five_minutes_makes_one_list` (calendar_cache:208), `after_five_minutes…` (:281) |
| P7 410 not special | `a_410_drops_the_token_and_reads_the_window_again` (:325) |
| P8 brief never sensitive | `a_sensitive_calendar_makes_the_brief_us_only` (tests_morning.rs:449) |
| P9 note row keeps text | `no_event_content_is_in_any_ledger_row` (tests_morning.rs:545) |
| P10 window answers anyone | `anyone_else_is_refused_and_gets_no_event_data` (:509) |
| P11 sync refresh always "changed" | `calendar_changed_is_sent_once…` (:604), 410 and sync tests |
| P12 tool omits the sensitive mark | `a_calendar_is_read_by_label_or_by_id…` (tests_calendar.rs:714) |
| P13 no span limit | `a_span_over_62_days_is_refused` (:530) |
| P14 chokepoint ignores the level | `a_background_call…` (:147), `a_sensitive_read_keeps…` (:88) |

(P11's first plant, on the full re-read path, did not fail anything; I re-planted on the sync path.)

New test files/modules: `google/tests_calendar_cache.rs` (13), `tests_sensitive.rs` (8), `tests_calendar.rs::configured`
(4), `tests_morning.rs::calendars` (5), `config/tests_google.rs` (+4), protocol `tests/wire.rs::calendar_window_and_changed`.
Covered: config (string, table, default, duplicate, second primary, bad colour, template); check (ok, 404, 403+busy,
403 no busy, lapsed); cache (one list, sync token, 410, 403 busy, changed once / not on none); window (statuses and
colours, refusal without event data, off, 62 days); floor (CN and EU refused on the next turn, US runs, background call
refused, task and lineage, recall, personal read is private not sensitive); shared places (publish refused, calendar
tools not offered); no titles in the ledger; tool (label, id, all, unknown, default config unchanged); brief (labels,
single calendar unchanged, US-only, unreachable).

**Wire deviation:** `tests/wire/` holds notification fixtures only (`every_fixture_decodes…` reads every file there), so
the window result is `tests/wire/calendar/window.json` (subdirectories are skipped) and the notification is
`tests/wire/calendar_changed.json`; that test's fixture count went 45 → 46. The cockpit types regenerated
(`cockpit/src/protocol.gen/Calendar*.ts`, `index.ts`).

## Results

- `cargo nextest run -p theseus-core -p theseus-protocol -p theseus-google`: **4458 passed, 0 failed** (TZ=America/Phoenix).
- `cargo nextest run --workspace` without those three: 3583 passed, **34 failed, all the known L1 cases**
  (`theseus-sandbox::contract` ×19, `bench spawn_100`, `theseusd::sandbox` ×14; the root-daemon cgroup refusal).
- Gate: `fmt`, `shape`, `features`, `clippy`, `clippy-probe`, `cockpit`, door and keel phases **passed**.
  The **test build phase failed three times for lack of disk** ("No space left on device"; this VM's allowance is about
  39 GB and the gate's builds exceed it), so I ran the suite phase by hand as above. I did not run the phases after the
  suite (deny, benches) and did not fetch `cargo deny`; `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check` are clean.
- Each commit was not gated on its own: the two commits were made together after the checks above.

## Live check for the maintainer

1. `theseusd check` against a config with `[google] calendars = ["primary", { id = "<shared id>", label = "Work", sensitive = true }]`:
   one `google calendar <label>: ok` (or `not_found` / `busy_only` / `forbidden` / `auth_lapsed`) line each.
2. From the CLI: send `calendar.window {from_ms, to_ms}` on the socket; both calendars with statuses. The same call
   from a Discord shared place is refused with "calendar.window is the owner's".
3. In a session, call `calendar_events {calendars:"all"}`; then `theseus` the session's next turn pinned to a non-US
   profile should say `data_gate`. `theseus ledger` rows for those calls should hold no title.

## Left open / for the owner

- `status.md` sentence: "ops-cal: `[google] calendars`, a memory-only calendar cache, `calendar.window` and
  `calendar.changed`, and a sensitive level beside private (US-only floors, no shared place, no ledger text), with the
  morning brief over every calendar."
- **Level function for ops-verbs:** `pub(crate) fn private::level(store: &Store, cfg: &Config, session_id: &str) -> private::Level`
  (`Open | Private | Sensitive`, lineage-aware, fails closed to `Sensitive`).
- A transient failure (5xx) with no kept window is reported as `forbidden` with a retry sentence: the status set has no
  "unreachable". Consider adding one.
- `calendar.changed` goes to the "watch every session" connections, not only to connections that read a window.
- The morning note has no session to mark (see item 8). A later conversation in the DM is not marked by it.
- Not covered: a test of the refresh tender's 5-minute timer itself (the refresh and notify path is tested with the aged clock).

## Data-routing review

Paths calendar content can take (G = Google's response):

1. **G → `calendar.events` tool result** (`google/calendar.rs` `run_async` → `finish`): node in the session's WAL,
   marked `private` and, for a sensitive calendar, `sensitive` in the same frame (`toolrun/complete.rs` →
   `private::record`). Floors: route step (`data_step::private_session`) and `data_gate`. Pinned by
   `a_sensitive_read_keeps_the_next_turn_on_a_us_model`, `a_background_call_of_a_sensitive_session_…`.
2. **Node → later model calls** (turn, compaction, keep-warm, memory pass, people judge, naming, census, replay,
   learning): through `scoped`/`bound`/`for_session`/`over_sessions` at the session's level; chokepoint refuses non-US.
   Pinned by P5/P14 tests (keep-warm) and the turn test.
3. **Recall** to another session (`followed`, `memory_lookup`): carries the sensitive record. Pinned by
   `a_session_that_recalls_a_sensitive_note_becomes_sensitive`. Recall in a shared place draws only on that place's
   own sessions (existing rule, not re-tested for sensitive).
4. **Tasks** (`inherited`, lineage): `a_task_under_a_sensitive_session_is_sensitive`. `task_start.rs:380` writes a
   private mark when `d.private` is set and I did not add a sensitive mark there (that path is for a task whose own
   brief was private; the parent-inheritance path next to it does carry it). **Unsure whether `d.private` can be true
   for a sensitive parent without the inherited path also running; I believe not.**
5. **`place.publish`** of a node: refused for a sensitive session to a shared place. Pinned. Shared-place sessions are
   never offered the calendar tools (`a_shared_place_is_offered_no_calendar_tool`).
6. **`calendar.window`** (`google/calendar_cache.rs` → `rpc/calendar.rs`): to the owner on a private connection only;
   in-memory cache. Pinned by `anyone_else_is_refused…`.
7. **Brief**: G → `Brief` lines (scrubbed) → the note's facts → model call at `Level::Sensitive` → `Spoken` → outbox
   post to the owner's DM and an `initiative.spoke` row **without text** when sensitive. Pinned by
   `a_sensitive_calendar_makes_the_brief_us_only`, `no_event_content_is_in_any_ledger_row`. The DM post is a store
   record (an outbox action) and a Discord message by design; this is the product of the brief.

Where it does **not** go (checked by reading, plus the ledger scan test over every row): `google.called` rows (method,
status, attempts, label; no title, place, guest or link); `calendar.changed` (labels); `check` lines (labels and a
fixed fix sentence; error words come from Google's status text only); `data_gate.blocked` rows (provider, model, origin);
the narrative's `tool.done` line (size only); no `tracing` line mentions an event; the cache is never persisted.

Unsure of / not changed:
- **`tool.ended` notification** carries `preview`, the first 2,000 characters of any tool result, to the session's
  watchers. That is existing behaviour for all Google results and goes only to watchers of a session (owner surfaces);
  it is not a ledger row. A sensitive result's preview is therefore on that wire. Left as is; flagging.
- **Memory index**: result nodes are indexed by the index tender like any node; recall follows the mark but the index
  files hold the text on disk. Existing behaviour for private Google content.
- **Provider telemetry**: I did not audit OTLP spans for request bodies; I saw only digests and counts.
- A calendar's *label* (its own summary, e.g. "Team calendar") appears in rows, `check` lines and the notification.
