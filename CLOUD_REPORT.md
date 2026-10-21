# CLOUD_REPORT: gw-surfaces (theseus-batvg.6, theseus-dfk31)

Branch `cloud/20261021-gw-surfaces`, built on `origin/integ/google` (the remote branch held only the task-brief commit
from `main`, so it was merged in: `8853c0a`). Commits: `1c7fc28` part 1, `244f450` parts 2 and 3 (they share
`watch/mail_then.rs`, so one commit), `2e3fec5` parts 4 to 6 (one commit, written under a stop-hook push request).
Parts 1 to 3 were gated together with part 4 to 6's tree, not each alone.

## What changed
1. **Send binds its digest (dfk31).** `gmail.send {draft_id, digest}`; plan refuses a digest not last shown; run re-reads the draft and
   refuses unless live digest == the input's. `Shown.pinned` deleted. `Parsed::digest` now covers each attachment's name,
   type, size and Gmail attachment id (not bytes: that would cost a call each) and the HTML part. `gmail.draft` and
   `gmail.read draft:` give the digest in text; `Plan.card.kind = "mail"`; `ToolRuntime::preview` builds a `mail`
   preview (`ConfirmPreview::MAIL`) from the tool's own plan, so `toolrun` and `confirm.list` agree. Discord quote
   (`render/preview.rs`, `@` broken so a draft cannot mention anyone); cockpit `MailDraft` + `mailLines`.
   Callers fixed: tests, tool description. Old pin tests kept by name, rewritten to the input digest.
2. **`Check::Mail`** (`watch/mail.rs`, `spec.rs`, `check.rs`, `step.rs`, `run.rs`): no model; history cursor in META
   `watch.mail.<wake>` via new `ProbeNews.keep` (kernel), written with frames the watch writes, and at most every 6 h on
   a quiet look; cleared (empty value: META has no delete) at the end. 404 re-baselines and says so in the row.
   `[google] trusted_senders` (+template line). Trusted `then: task` -> `task_start_as(.., private)` (quote = the mail, held
   `VIA_QUOTE`, session marked private from the open's frame). Otherwise notice with the reason. `then: turn` refused;
   `build_with` takes the `[google]` config (`build` kept for the tool's plan).
3. **Follow-up** (`mail_then.rs`): on expiry a task starts with the owner's brief (not held), the notice names it.
4. **Morning note** (`initiative/morning_google.rs`, `calendar::today/briefs`): events (conflict mark), unread count +
   top 3, drafts count; failure -> "calendar/mail unreachable"; model call in `private::scoped`
   (`Facts.private`); new `Line::MorningWaiting` template.
5. **Digest** (`protocol/digest_google.rs`, `rpc/digest_google.rs`): `WorkDigestResult.google`, line in `digest_text`.
   `merged()` now leads a draft call's row with its write, and the create row names the draft id.
6. **Board** (`protocol work/horizon.rs reply_wait`, `push/work/horizon.rs`): the watch view's `Doing.label`.

## Proof (TZ=America/Phoenix)
Targeted runs of google::, watch::, initiative, digest, horizon, config::, protocol tests: all pass (3,820 of 3,823 on the
first run; the 3 failures were test/fake fixes, then passed). Planted reverts, each failing its test:
- any sender starts a task -> `a_strangers_mail_never_starts_a_turn` (key test);
- compare to shown digest -> `a_send_is_refused_when_the_input_digest_is_not_the_drafts`;
- drop `scoped(true)` -> `the_morning_note_never_uses_a_restricted_model`;
- task unheld -> `a_trusted_senders_mail_starts_a_held_task`.
Not planted: other tests. The deadline test moves the entry's expiry to now (no paused clock).
Gate (`THESEUS_GATE_NO_BENCH=1`): fmt, shape, clippy, clippy-probe, cockpit, test build and reader rule pass. Suite: only the known
L1 failures (theseus-sandbox contract/bench) plus `judge::people::tests_seen::over_the_gate_exactly_one_extraction_runs`, which
fails under load and passes alone. The phases after the suite (deny) were not run. Keel guard
(`--base origin/integ/google`): empty output.

## Store format, wire, lines
No store-format bump (new META keys and ledger reads only). Wire/TS: `WatchCheck::Mail`, `WatchCreateParams.follow_up`,
`ConfirmPreview` kind `mail`, `WorkDigestResult.google`, `DigestGoogle` (protocol.gen regenerated). `gmail.send` input change
fixed in all callers. Files: mail.rs 1,435, spec.rs 645, calendar.rs 714; none over a ceiling.

## Owner should hear
- A quiet mail look writes one frame (its `google.called` row; rows of a look are batched). At 3 m that is 480 frames/day per watch.
- Trusted sender is the `From` header only: it can be forged. Requiring Gmail's `Authentication-Results` dkim=pass would close it; not done.
- A crash between starting the task and the watch frame can start the follow-up twice.
- Row `tool` for watch and morning reads is `gmail.search`/`calendar.events` (the scope table needs a tool name).

## Left / docs
No `theseus watch-for --mail/--follow-up` CLI flags. Docs to update: spec Part III item, status.md, theseus-core AGENTS.md (watches, initiative,
`gmail.send` input). Not run here: L1 tests (root VM): `TZ=America/Phoenix cargo nextest run -p theseus-sandbox -p theseusd`.
