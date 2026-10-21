# CLOUD_REPORT: gw-calendar (theseus-batvg.5)

Branch `cloud/20261021-gw-calendar`, from `main` at `1d092de`. One step.

## G5: `calendar.events` and `calendar.put`

**Found.** G1's seams were all there: `Google::call`, `finish`, `Source::Event`, the scope map naming both tools, the
`calendar` family, `google.call`'s classifier (which already holds `calendar.acl.*` and `calendars.insert/delete/clear` as
authority), and the template's `# gw-calendar:` marker. Used, not redefined.

**Changed** (commit `a748ed2`):
- `crates/theseus-core/src/google/calendar.rs` (new): both tools. `calendar.events` lists a window (default today),
  `singleEvents` ordered by start, with organizer, attendees and their responses, call link, location, and the first
  400 characters of the description. Times read in the daemon's zone (`recall::stamp`); an all-day event is a date
  (`Wed 2026-10-21, all day`, a span names its last day). Its result is outside text (`calendar:<id>`) unless the
  account organizes every event in it (`organizer.self`, or the organizer's address), handed to `finish`.
  `calendar.put` creates (`events.insert`), changes (`events.patch`) or cancels (`events.delete`).
- `google/mod.rs`: `pub mod calendar`, one `all.extend(calendar::tools(self))` line, `NAMES` now three.
- `crates/theseus-google/src/fake/calendar.rs` (new) with a `mod` line and one `if let` line in `fake.rs`: create and
  change echo the event as the account's own, delete answers 204, a list answers empty until a test gives it a body.
- Template: both tools' lines (commented, `notify`) after the marker, which G1's template test requires to stay.
- `google/tests_calendar.rs` (new), 13 tests. `config/tests.rs`: the `[policy.tools]` count 60 to 62 (two tools).

**Design choices the owner should hear about.**
1. `calendar.put` is authority when it names attendees (with or without `notify_attendees`), when it cancels (with or
   without notify), or when it changes an existing event with `notify_attendees`. This is stricter than the brief
   ("attendees with notify"): Google's `sendUpdates` decides only whether it emails; an invited calendar gains the
   event and a cancelled one loses it either way. The plan sees the call, never the event, so a change to an existing
   event with notify false and no attendees named runs at its posture; a pre-read guard would need two
   `google.called` rows from one tool result, which the one-row contract does not allow. Say so if the owner wants it.
2. An all-day `end` is the event's last day (Google's own `end.date` is the day after; the tool adds it). `end` is
   optional for dates.
3. A create with `notify_attendees` and no attendees sends `sendUpdates=none` (no one to tell).
4. Descriptions and schemas are tight to hold the ~220-token census (put was 283 tokens first): the schemas carry no
   `additionalProperties`; the code rejects unknown fields itself.

**Proof.**
- `TZ=America/Phoenix cargo nextest run -p theseus-core -E 'test(google::) or test(config::)'`: 198 passed, 0 failed.
- Planted reverts (restored from a copy, `touch`ed, `git status` checked):
  - invitation authority removed (`Some("invites people…")` to `None`): `inviting_attendees_asks_whatever_the_posture`
    and `a_change_asks_when_it_tells_the_attendees_and_not_when_it_does_not` failed.
  - owner-authored rule off (`organized_by` false): `an_event_the_account_organizes_is_not_external`,
    `an_invitation_from_a_stranger_is_external` and `an_event_with_no_attendees_runs_without_asking` failed.
  - cancel authority removed: `cancelling_an_event_with_attendees_asks` failed.
  - The all-day, deferral and token-size tests were fixed by their own first failures (a string-compared date order,
    283 tokens), not planted.
- The key test uses posture `open` on both tools. It shows the plan's authority, `decide` returning Approve,
  `posture_only` false (which is the first question every loosening layer asks: earned grants, scoped grants, modes,
  a routine's grant), and a whole turn that waits with no POST sent. No earned grant can be built for `calendar.put`
  (grants cover `proc.run` and file tools only), so that layer is shown by `posture_only`, as G1's send test does.
- `calendar_acl_and_calendar_delete_are_authority` runs through `google.call`'s plan.

**Live check for the maintainer** (nothing here touched a real account):
`theseus google status` should list `calendar.events` and `calendar.put` under tools on. Ask for today's events: a
listing of the owner's own events is not outside text, one with a stranger's invitation holds the session. Ask it to
put a 10-minute event with no attendees: it runs with a notice. Ask it to invite someone: a card appears whatever the
config's postures say, and nothing is sent until the yes.

**Left or uncertain.** The `NAMES` line in `google/mod.rs` will conflict with the other Google rows' edits at the join
(each changes the array's length). The `tests.rs` count of 62 will need renumbering at the join as other rows add
tools. Docs to change: the Google design's §3.1 rows 6 and 7 should say the authority rule above; `AGENTS.md` of
theseus-core could list `google/calendar.rs`.

## Gate

`CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (debug
info off and incremental off, because this VM's disk filled twice). Door, keel, fmt, shape, features, clippy,
clippy-probe, cockpit, test build and the reader rule all passed. The suite: 7262 run, 7229 passed, 33 failed, all of
them the known L1 cases (theseus-sandbox's 20 contract tests and `spawn_100`, and 12 of theseusd's `sandbox` tests;
the VM is root). No other failure. `cargo deny check` says `advisories FAILED` (bans, licenses and sources ok): I
changed no dependency (Cargo.lock untouched), so I take it as the advisory database's news since the base. Not run:
the benches (left alone) and the protocol-types phase (no protocol type changed). The commit was pushed before the
suite finished, to keep the work safe on an ephemeral VM; the gate's other phases and the 198 tests had passed.

Keel guard (`python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`): empty output, exit 0.
