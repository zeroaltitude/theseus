# CLOUD_REPORT: gw-mail (theseus-batvg.2), branch `cloud/20261021-gw-mail`

Base: `main` at `1d092ded`, store format 47 (this row stores nothing new, so no bump). One step, one code commit
(`27fe527`), and this report. Started 06:22 UTC, done 08:50 UTC.

## The step: Gmail as five typed tools

### What I found

- G1's pieces were all there (`Google::call`, `finish`, `Source::Mail`, the send counter, the fake, the family, the
  scope map, the template marker). I used them and redefined none.
- **A tool's `plan` is synchronous and reads no network** (`Tool::plan`), so "the plan reads the draft" cannot be
  literal. `aws.stack.apply` gets its digest in its *input*; `gmail.send {draft_id}` has no room for one. The
  approval binds `{tool, args}`, so the digest is not in what the owner approves either.
- The fake's route table matches a path exactly, and Gmail's paths carry ids.
- `Plan` (theseus-protocol `gate.rs`) had no place for a card's data.

### What I changed (`27fe527`)

- `crates/theseus-core/src/google/mail.rs` (new): the five tools, registered by the one `mail::tools` line in
  `Google::tools` (G1's marker comment kept, my line under it).
  - `gmail.search`: one `threads.list`, then one metadata `threads.get` a thread (5 at a time). Rows carry id, date,
    from, subject, snippet, labels, unread, count. Outside text unless every sender is the account; no rows, no mark.
  - `gmail.read {id, save_attachments?}`: a thread, else (404) a message. `id` of the form `draft:<id>` reads a draft
    (see the design choice below). Bodies are plain text, HTML turned to text by `web::html::to_text`, quoted tails
    folded. Attachments are listed (name, type, size) and saved when asked.
  - `gmail.draft`: creates or replaces; `reply_to` sets threadId, In-Reply-To, References, a `Re:` subject, and `to`
    defaults to the original's Reply-To or From.
  - `gmail.send {draft_id}`: see "The send" below.
  - `gmail.label {ids, add, remove, trash}`: system labels by name, user labels through `labels.list`; `trash`
    plans with `Plan.authority`.
  - Several REST calls become the one `google.called` row a result carries: attempts, quota units and ms add, and a
    send's or a trash's row leads (its class and recipient count).
- `crates/theseus-core/src/google/mime.rs` (new, pure): RFC 2822 build (base64 UTF-8 body, CRLF), RFC 2047 words
  both ways (B and Q, never inside a quoted string), threading headers, `Re:`, a header with CR, LF or NUL refused,
  address parsing, a Gmail payload read into text and files, `fold_quoted`, `safe_name`.
- `crates/theseus-core/src/external.rs`: the one exemption, `|| tool == google::mail::DRAFT`, with the doc lines.
  Labels are not exempt.
- `crates/theseus-protocol/src/gate.rs`: `Plan.card: Option<Value>`, `#[serde(skip)]` as `authority` is (so no
  wire change, no TS change, no format bump). The card for G6 to render. Additive; `Plan` is built with
  `..Default::default()` everywhere (the workspace compiled and clippy passed).
- `crates/theseus-google/src/fake/gmail.rs` (new) and `fake.rs` (`pub mod gmail;`, the `mail` field and its
  accessor `Fake::mail()`, and one fall-through call in `answer`, after a test's own routes). The mailbox: threads,
  drafts (it reads the MIME), a sent log, labels, attachments, and `after_send(Reply)` for a send that records and
  then fails on the way back. That is more than "two lines in fake.rs": a pattern route was needed for ids.
- `crates/theseus-core/config/theseus.example.toml`: the five `# "gmail.*"` lines under the `# gw-mail:` marker, which I
  left in place (G1's `the_template_has_a_google_section_and_markers` test still holds, and no other row's merge
  touches my lines). `gmail.send = "approve"` with the comment that it asks whatever the line says. The routine-grant
  example `gog.gmail.search` is now `gmail.search` (line 1186).
- `google/mod.rs`: `mod mail; mod mime;`, `NAMES` now six entries, `mod tests_mail`.
- `config/tests.rs`: the `[policy.tools]` count 60 to 65 (five lines).
- `crates/theseus-google/AGENTS.md`: a line for the fake's mailbox.

### The send (the owner's decision: asks every time, deterministically)

- `gmail.send`'s plan sets `Plan.authority`, so `Policy` asks at every posture. Nothing loosens it: every loosening
  layer (earned grants, a conversation's grants, modes, a routine's grant) begins with `posture_only`, which is false
  for an authority plan.
- **Design choice the owner should hear about (the card's facts are made at draft time).** Because `plan` cannot
  read the network, the facts are made when a draft is made (`gmail.draft`) or read (`gmail.read id "draft:<id>"`):
  the draft as Gmail holds it, a SHA-256 digest of every field the card shows plus the thread, and the card (from, to,
  cc, bcc with each recipient not on the thread and not in Sent flagged `new` by one `messages.list in:sent` each,
  the subject, the first 15 body lines, the attachment names). They are kept in memory (`Mail.shown`). The send's
  plan **pins** the digest of the card it shows (`Shown.pinned`) and carries the card as `Plan.card` and in its
  summary. Its run reads the draft again and refuses, sending nothing, unless the live digest equals the pinned one.
  A draft written or read again clears the pin, so a card is never for a draft other than the one that goes (the
  hole this closes: a `gmail.draft` rewrite landing between the card and the yes). A restart forgets the pins, so a
  send after a restart is refused until the draft is read and a card shown again. A draft made in Gmail itself must be
  read (`draft:<id>`) before it can be sent. Unshown draft: the plan is invalid input and asks the owner nothing.
- Non-repeatable (`Repeat::NonRepeatable`; a 503 after the send is `outcome_unknown` and never retried), counted on
  `max_sends_per_day` (reserved before the call, released when it was refused before it could go, kept when unknown).

### How I proved it

All on this VM, `TZ=America/Phoenix`.

- `cargo nextest run -p theseus-core -E 'test(/tests_mail/)'`: 22 tests, all pass. With the rest of `google`: 61
  pass. The ones the task names:
  `a_send_is_refused_when_the_draft_changed_after_the_plan`,
  `a_send_asks_at_every_posture_and_no_grant_loosens_it`,
  `send_is_non_repeatable_and_reports_outcome_unknown`,
  `the_daily_send_cap_counts_google_call_sends` (cap 2: one `google.call` send, one `gmail.send`, the third refused
  with "today's send cap is reached (2 of 2)", by `gmail.send` and by `google.call`),
  `a_reply_draft_threads_with_in_reply_to_and_references`, `rfc2047_encodes_a_non_ascii_subject`,
  `a_stranger_thread_holds_the_session`, `a_thread_the_account_wrote_alone_is_not_external`,
  `a_draft_runs_under_the_hold_and_a_label_asks`, `html_mail_reads_as_text`,
  `a_saved_attachment_lands_in_the_workspace`, `send_card_flags_new_recipients`,
  `every_mail_tool_is_deferred_and_its_census_row_is_small`.
  Added: `an_unchanged_draft_sends_once_and_counts`, `a_draft_written_again_unpins_the_card`,
  `a_draft_not_shown_cannot_be_planned_until_it_is_read`, `a_header_cannot_be_smuggled_through_a_subject_or_an_address`,
  `a_cards_body_is_its_first_lines`, `search_lists_a_row_a_thread_and_marks_strangers`,
  `label_changes_labels_by_name_and_trash_asks_by_authority`, `a_message_id_reads_when_it_is_no_threads_first`,
  `no_mail_tool_is_in_a_family_that_is_never_hidden`.
- **Planted reverts**, one build, ten plants at once, restored and `touch`ed after; `git status` showed only my
  intended changes. Each named test failed:

  | Plant | Test that failed |
  |---|---|
  | send without comparing the digest (`if false` for the digest check) | `a_send_is_refused_when_the_draft_changed_after_the_plan` |
  | `Plan.authority` dropped from the send's plan | `a_send_asks_at_every_posture_and_no_grant_loosens_it` |
  | `gmail.label` exempted from the hold too | `a_draft_runs_under_the_hold_and_a_label_asks` |
  | the send request made repeatable | `send_is_non_repeatable_and_reports_outcome_unknown` |
  | the send not reserved on the cap | `the_daily_send_cap_counts_google_call_sends` |
  | `In-Reply-To` header not written | `a_reply_draft_threads_with_in_reply_to_and_references` |
  | every recipient "not new" | `send_card_flags_new_recipients` |
  | `safe_name` returns the name as given | `a_saved_attachment_lands_in_the_workspace` |
  | quoted tails not folded | `html_mail_reads_as_text` |
  | `encode_word` never encodes | `rfc2047_encodes_a_non_ascii_subject` |

  12 tests failed in all: the ten above and two dependents (`an_unchanged_draft_sends_once_and_counts`, which counts
  the cap; `a_draft_not_shown_cannot_be_planned_until_it_is_read`, which sends). After the restore: 61 pass.
- Secrets: `a_stranger_thread_holds_the_session` greps every node and ledger row of the session for the access token
  (`ya29.`) and the refresh token, and the `google.called` rows for an address: none. A send's row holds a
  recipient count, never an address (`an_unchanged_draft_sends_once_and_counts`).
- Not run under load (the task did not ask for it).

### The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (log in this session's scratchpad). Phases passed: door,
keel, keel tests, fmt, shape, features, clippy (workspace, all targets, `-D warnings`), clippy-probe, cockpit, test
build, reader rule (15 tests). **Suite: 7,271 tests, 7,237 passed, 34 failed**, all but one the known L1 set:

- 33 L1 failures on this root VM (theseus-pv6i): theseus-sandbox's `contract` tests and `bench spawn_100`, and
  theseusd's `sandbox::*` tests.
- `theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`: not on the known list.
  It failed in the full run (`gone_jobs.rs:331`, "the live wrapper's job runs on": three actions dispatched, one
  expected) and **passed alone, twice** (5.9 s each). It has nothing to do with mail; I read it as the load
  flake the task warns of. Please name it if it fails on your machine.

The gate stopped there ("FAILED in suite"), so I ran what comes after it myself:

- `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`: **no output**, exit 0.
- Generated TypeScript: `theseus-protocol`'s generation test passed in the suite and `git status` shows no change under
  `cockpit/`.
- `cargo deny --offline check`: bans, licences and sources ok; **advisories fail on two RustSec entries** about
  distrusted root certificates ("Certificate verification may accept distrusted intermediates", "Bundled root
  certificates allow trust in distrusted emSign intermediates"). `Cargo.lock` is unchanged by this branch, so the
  entries come from the newer advisory database `cargo deny fetch` just pulled, not from this change.
- The lifecycle, jobs, turn and size benches were not run (`THESEUS_GATE_NO_BENCH=1`; speed is the owner's machine's).
  This change adds nothing on the start path or a plain turn: no tool is registered without `[google]`.

### The live check for the maintainer (needs the owner's Google account; I could not run any of it)

```sh
cargo nextest run -p theseus-core -E 'test(/tests_mail/)'     # 22 pass, with the fake Gmail: no network

# a scratch daemon on a fresh state dir, [google] naming the owner's account and a granted gmail scope
theseus --socket /tmp/gw-mail/sock google status              # state ok; tools_on lists gmail.search ... gmail.label
theseus --socket /tmp/gw-mail/sock ask "search my mail for newer_than:1d, read the newest thread, and draft a reply"
```

Expect: `gmail_search` and `gmail_read` run with a notice and hold the session if a stranger wrote (a later write asks);
`gmail_draft` runs **without** a card even in the held session, and its result prints the card text and the draft id.
Then `ask "send draft <id>"`: a card (every posture, trusted session included) with from, to, cc, bcc (new ones
flagged), subject, the first 15 lines and attachment names, and the summary starts `send draft <id> from ...`. Before
answering, edit the draft in Gmail's web UI and answer yes: the result must say "nothing was sent: the draft changed
after the owner's card showed it", and Sent must be empty. Answer yes on an untouched draft: one message in Sent, and
`theseus google status` shows `sends_today` 1. Check `google.called` rows (`theseus ledger`) hold a recipient count and
no address or subject.

### What is left or uncertain

- **G6 (the cards)**: render `Plan.card` (a JSON object: `draft_id`, `from`, `to`/`cc`/`bcc` as `[{address, new}]`,
  `subject`, `body` (first 15 lines), `body_lines`, `attachments`, `digest`). It is in the plan only, in memory. The
  same facts are in the plan's `summary` as text (`mail::card_text`), so Discord, the CLI and the cockpit show
  something today. How the card reaches `ConfirmRequest` is G6's.
- **The pin is in memory.** A restart between the card and the yes makes the send refuse and ask for a new card.
  That is fail-closed; a durable pin would need the digest in the call's input or a META record.
- `NAMES` in `google/mod.rs` is `[&str; 6]` here and will conflict with the other rows' edits to the same line; so
  will the `[policy.tools]` count in `config/tests.rs` (65 here). The joiner renumbers both.
- The census row test lives in `google/tests_mail.rs`, not `tests_census.rs` (the Google rig is private to the
  `google` module, and four rows adding tests to one file would conflict). It builds a core with deferral on
  (`Parts::for_tests` leaves it off, as `tests_census` also finds) and checks the `cli`, `dm` and `task` places: every
  `gmail_*` is deferred and at most 220 tokens.
- `gmail.label`'s `ids` are thread ids (what a search returns), with a message id accepted when it is no thread's first
  (a 404 falls back to the message). Up to 25 at a time, one call each.
- Attachments are written by the daemon with the process's umask (private under the daemon's 077); I did not use
  `ToolCtx.umask`.
- `gmail.read` with `save_attachments` plans as a **Write** (so the hold and the gate judge the directory); without it,
  a Read.
- `routine/grant.rs` and its tests still use `gog.gmail.search` as a fixture name and in an error sentence. I left
  them (another area); the template's example is fixed.
- Not built, as the task says: contacts, Gmail settings, permanent delete, the `http.fetch` egress predicate.
- The disk allowance here is small: a workspace test build with incremental on filled it twice. I deleted
  `target/debug/incremental` and test binaries under `target/` (never anything outside the repository or `/tmp`),
  and built with `CARGO_INCREMENTAL=0` after.

### Docs that should change (I touched none of them)

- Spec Part III: a G2 item (what it built, how it was proved, the design choice above), and the version line.
- `docs/status.md`: Gmail's five tools, and the roadmap row.
- `docs/privacy.md` (G1's): mail is private; a send needs a card every time; `gmail.draft` is the one write the
  outside-text hold does not stop; a draft's recipients flagged new are looked up in Sent (the owner's own mail
  metadata, never content).
- The Google design, §3.1 row 4 and §3.2, §5: the send is authority, not a posture line (the owner's decision), and
  a routine therefore never sends unattended.
- `crates/theseus-core/AGENTS.md`: a Google mail paragraph (the send's pin, `Plan.card`, the draft exemption).
