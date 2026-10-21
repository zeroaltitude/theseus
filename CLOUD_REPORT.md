# CLOUD_REPORT: tail-mail (theseus-g4lit, theseus-36sm6, theseus-55j04, theseus-axbqw, theseus-u4dvr)

Branch `cloud/20261021-tail-mail`, from `main` at `6918e3c1`. Store format unchanged (48): nothing new is stored. No new
dependency, no config or template change, no line ceiling touched (`google/mail.rs` and the new modules are small).
One deviation to say first: item 2 (36sm6) needed the fake's `list_messages` to learn the exclusion terms, and that
hunk shares `fake/gmail.rs` with item 5's `replace_draft_file`, so the fake's change rides the u4dvr commit.

## Commits (one item each)

| Item | Commit | What |
|---|---|---|
| axbqw | `e4836a8` | `preview.rs`: `**(new)**` only on lines starting `To: `, `Cc: `, `Bcc: ` |
| u4dvr | `1e3e1c1` | digest file entry `[name, mime, size, sha256]`; `fetch_draft` reads each attachment once (`Mail::attachment`, shared with `save`); inline `body.data` parts are hashed (`mime::walk` no longer skips them; `save` says an inline part has nothing to fetch); fake: `replace_draft_file`, `-in:drafts`/`-in:sent` terms, whole-term `in:sent` |
| g4lit | `43f4d66` | `watch/mail_auth.rs` (`authenticated`), `Seen.unverified`, `mail_trust`, notice text, `"unverified"` in the news detail |
| 36sm6 | `0dfe354` | `look` lists `(q) -in:drafts -in:sent`; drops `DRAFT`/`SENT` labelled matches (`read_one`) |
| 55j04 | `7e725d4` | `watch/quiet.rs` (`Quiet`, `QUIET_ROW_MS`, `is_quiet`), `Entry.quiet`, `watch_seen(wake_id, ..)`, `watch_leave` |

Design notes the owner may want to hear:
- g4lit: an unlisted sender keeps today's line and wins; `mail_trust` clears `unverified` when any sender is unlisted
  (so `unverified` means "listed but not authenticated"). The authserv-id check takes the first token before the
  first `;`, so `mx.google.com` with a trailing version still counts.
- 55j04: the look that crosses the hour is added to the tally before it is written, so it is counted in `quiet_looks`
  and `calls`. The first look (no cursor) is never quiet. The tally is **memory only**: a restart loses at most one
  hour's tally. A quiet look that writes no row writes no frame at all. A watch leaves the board through
  `watch_leave` (the met/expired path and the cancelled-wake path of `watch_finish`), which writes the tally first.
  A cancelled watch is only noticed at its next news write, as before (a quiet look returns before `watch_write`).
- u4dvr: cost is one 5-unit read per attachment at each card and each send; a draft with no attachments adds no call.

## Tests, and each planted revert

All new tests pass; plants were run on the full tree, each restored and `touch`ed.

| Test | Plant | What failed |
|---|---|---|
| `watch::tests_mail_tail::a_forged_from_of_a_trusted_sender_only_notices` (key) | `mail_trust` ignores `unverified` | `no header: no task` (left 1, right 0) |
| `watch::mail_auth::tests::authentication_results_are_read_as_gmail_wrote_them` | `aligned` accepts anything | `!passes(dkim=pass header.d=evilexample.test)` |
| `the_owners_own_draft_and_sent_mail_are_no_match` | label guard off, q terms still sent | passes (the q terms cover it) |
| same | label guard off **and** fake ignores the q terms | `no news` (watch.met written) |
| same | q terms dropped, guard on | the `q == "(from:example.test) -in:drafts -in:sent"` assertion |
| `a_quiet_watch_writes_one_row_an_hour`, `a_poll_with_no_new_mail_makes_one_call` | quiet rows written as before | poll test: `a quiet look writes no frame` (a frame was written); the hour test fails too |
| `a_watch_that_ends_writes_its_tally` | flush on leave dropped | `tally` empty (`[]`) |
| `render::preview::tests::the_discord_send_card_bolds_only_recipient_flags` | replace on every line | subject/body assertion |
| `google::tests_mail::a_same_name_same_size_attachment_swap_is_refused` | hash dropped from the digest | send succeeded (`unwrap_err` on `Ok`) |

`a_draft_attachment_read_failure_refuses_the_send`: the fake answers 500 to `messages/*/attachments/*`; the send is
refused with Gmail's message and nothing is sent (about 2 s, the client's retries). `an_unchanged_draft_with_an_attachment_sends` and
`an_html_only_change_after_the_card_is_refused` stay green.

Existing tests edited, and why each still asserts what it did:
- `a_trusted_senders_mail_starts_a_held_task`, `a_mail_with_no_address_beside_a_trusted_one_only_notices`,
  `history_poll_resumes_from_the_last_history_id`: the trusted fixture message now carries Gmail's pass header
  (`GMAIL_PASS`, via `MsgSpec::header`), so each still meets its own condition (a task starts; a message with no
  address still makes the look a notice; the cursor resumes and the match is met). The third never read trust, so its
  header changes no assertion.
- `a_poll_with_no_new_mail_makes_one_call`: the frame assertion went from `frames + 1` to `frames` (a quiet look now
  writes no frame, a stricter assertion of the new behaviour), and its row check now asserts no `history.list` row in
  the ledger and reads the tally row (class `read`, `quiet_looks` 1, `calls` 1, no address or subject).
- `tests_watch.rs`'s `look` helper: the call only (`watch_seen` takes the wake id).

## Runs

- `cargo nextest run -p theseus-core -p theseus-google -p theseus-discord -E 'test(/watch::/) | test(/google::/) | package(theseus-google) | test(/render::preview/)'`: 188 passed, 0 failed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (`look` was split into `read_one` for the line limit).
- `cargo fmt --check`: clean.
- `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: empty output.
- No run under load was asked of this row.
- One unrelated flake seen once: `google::tests_morning::morning_note_lists_todays_events` failed on the first full
  run and also at the unmodified base (it depends on the clock); it passed on later runs.
- The VM's disk filled once (incremental build caches: `target/debug/incremental`, about 11 GB); I deleted only those
  and test binaries under the repository's `target/`, and built with `CARGO_INCREMENTAL=0` after.

## For the maintainer (live check, verbatim)

With the daemon on the branch's build, in Gmail save a draft to an invented test address with a small attachment and an
HTML body; ask the agent to read the draft and send it; expect exactly one send card listing the attachment and, on
approval, one send (Gmail's Sent shows it once, the ledger shows one `gmail.users.drafts.send` row with `outcome: ok`).
Then save a second such draft, let the card show, replace the attachment in Gmail with another file of the same name
and size, approve: expect a refusal naming the change and no send. Then: a mail watch with `then: task` and a
`trusted_senders` address you control; mail from it starts a held task; the same From sent through a server without
DKIM for that domain only notices.

## Follow-ups

- `crates/theseus-core/config/theseus.example.toml`, the `trusted_senders` comment: it should say "mail Gmail
  authenticated from these addresses". Not edited (other rows edit the template).
- Docs: spec Part III should record these five (a mail watch trusts only Gmail-authenticated mail; drafts and sent
  mail are no match; quiet looks are tallied, memory only, about 24 `google.called` rows a day; the card's `(new)`
  bold; the attachment hash in the send digest). `docs/status.md` likewise. `crates/theseus-core/AGENTS.md` has no
  mail-watch paragraph to update.
- A cancelled quiet watch stays on the board until its next news write (unchanged behaviour); its tally is written then.

## The gate

Interim: the first gate run passed door, keel, fmt, shape, features, clippy, clippy-probe and cockpit, then its test build
ran out of disk (a second profile beside mine). It is being rerun on a cleared `target/`; the result follows in a later
report commit.
