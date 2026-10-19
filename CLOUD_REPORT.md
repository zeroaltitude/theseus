# CLOUD_REPORT: needs-you-cards (theseus-suvpe)

Branch `cloud/20261019-needs-you-cards`, based on `5e0bd195`.

## What I found
- The needs-you post was one `Buttons::Keep` message for a single approval, and one nudge for a batch; the CLI hint
  was its only answer path. `asker_name` used the session's title, which for a self task is the backlog objective.
- `ConfirmRequest` already carries what a card needs (`floor`, `external_text`, `budget`, `change`, `tool`, `input`,
  `preview`); the rule that held a call exists only as `reason` text, so the card's "why" is built from those
  structured fields plus the tool's name, never parsed from `reason`. **No protocol type, no store change, no format bump.**
- A press on a needs-you card went through the classic `confirm:` path, whose own edit was skipped only for a
  `carded` question (`Outbox::has_card`); a needs-you post is `pinged`, not `carded`, so I added `Outbox::has_post`
  (carded or pinged) and used it at that one site, so the settle owns the edit.

## What I changed (commits)
- `ce7b25c2` needs-you: every DM question is an answerable card in plain words (theseus-suvpe) — code, tests, the
  Discord crate's AGENTS.md.
- next commit: docs: the core AGENTS.md line.
- the report commit (last).

Core: `[notify] dm_cards` (default true; template + template test), `needs_you/words.rs` (what / why / yes),
`needs_you.rs` (`card_body`, `header_body`, one post per question, `asker_name` from the task record:
"Theseus's self-repair: <title>" for the `self` principal), `Outbox::has_post`.
Discord: `Buttons::Card` (Approve, Decline, trust where offered, or an ask's options; Details last), Details press
(`need:details:<id>`, ephemeral, from `confirm.list` as the presser), `render::question_details`, card text in
`render/ask.rs`, `runtime/reply/answer.rs` (the reply that answers; memory map planned by the lane, landed by the
courier's write hook), `heard()` in `on_message`.

## Design choices for the owner
- **Batch = a header plus one classic message per question** (not one V2 message): one code path that works with
  `live_board` on or off, each card settles on its own through the existing settle, and the pacer's one ping a
  window is the header (the cards post silently).
- Approve/Decline/trust are the classic card's own `confirm:` ids, so a press is the one `action.confirm` path.
- A reply is consumed (never a turn) when it is a short form to a registered message, even from someone the core
  then refuses; the replier is told why in a reply to their message.
- Settle text is the existing "Approved by <who> · <line>"; it does not say "from the CLI" or the time, because
  `Closed` carries neither the surface nor a clock (adding them would add a stored field). Say if you want it.
- The card's "why" for an approval is "<asker> asks: <rule>." where the rule is the floor, held web text, or
  "`<tool>` is set to ask before it runs". The finer "its file is under the state directory" sentence would need a
  structured rule field on `ConfirmRequest` (a stored record, so a format bump); I did not add one.
- After a restart a reply to an older card is an ordinary message (the map is in memory).

## Files in other rows' areas
`runtime.rs` net -1 line (the `on_message` guard line takes `|| self.heard(m)`; `mod reply;` became `pub(crate) mod reply;`,
a `pub(crate) use` line extended); `runtime/earned.rs` +2 lines (one more `else if` in `notice_press`);
`runtime/route.rs` (a field and accessor on `Replies`); `courier.rs` (the one hook line, and the `Buttons::Card` arms in
`asks`, `create`, `edit`); `render.rs` (the `Buttons::Card` variant and one `pub use`). No ceiling raised.

## How I proved it
Environment: 4 cores, disk is tight (I cleared `target/` twice; builds ran with `CARGO_INCREMENTAL=0
CARGO_PROFILE_DEV_DEBUG=0`).
- `cargo fmt --all --check`: clean. `bash scripts/shape.sh`: clean.
- `cargo clippy -p theseus-core -p theseus-discord -p theseus-protocol -p theseus-sim --all-targets -- -D warnings`: clean.
- `TZ=America/Phoenix cargo nextest run --workspace -E 'package(theseus-core) | package(theseus-discord) | package(theseus-protocol)'`:
  4,126 run, 4,124 passed, 2 failed: the spend-line expectation in my own new unit test ("$4.00" vs the narrative's "$4",
  fixed and re-run green), and `tests_task_checkpoint::a_slow_verdict_never_holds_a_loop_and_applies_when_it_comes`,
  a timing test outside this row, which failed once under the full-suite load and passed alone on the base and 3 of
  3 times alone with my change. Name it a flake under load.
- After the fixes: the new and changed tests (`needs_you`, `tests_gateway::cards`, `tests_m3::ask`, the Discord
  `ask`/`reply` tests): 174 run, all passed at the last run before the final one-line test fixes; the spend-line and
  the `dm` count fixes were re-run (`39 run, 38 passed` with only the flake above).
- `python3 scripts/keel-guard.py --base 5e0bd195 --head HEAD --quiet`: empty. (Against the current `origin/main`
  it lists main's newer Google work as erosions, because my base is older; that is not this branch's change.)

### Tests (the brief's names)
- `a_dm_question_is_answered_by_its_cards_approve` (`tests_gateway/cards.rs`, key test)
- `a_batch_is_one_card_per_question` (gateway) and `unprompted_pings_go_one_a_window_and_the_rest_together` (core,
  new shape: 4 posts, header names both, cards silent, answering one settles that card alone)
- `a_reply_answers_the_question_it_replies_to` (`yes` to a card; `approve 2`, `no 1` to the header; bare `yes` lists
  open numbers; `yes, but wait` and a reply to a non-question message are turns) plus pure `runtime::reply::answer::tests`
- `a_reply_from_anyone_else_is_refused` (a collaborator's reply in a shared place)
- `the_card_speaks_plain_words` (core `needs_you::words::tests`, Discord `render::ask::tests`) and
  `a_dm_question_is_a_card_in_plain_words`
- `a_self_tasks_question_names_its_backlog_title`
- `dm_cards_off_is_todays_post` (core: the body equals `body_of`; Discord: no buttons, one nudge)
- `details_shows_the_whole_request_to_the_presser_alone`; `tests_gateway/ask.rs` now expects Details after the options.

### Planted reverts (replay: edit, run the named test, `git checkout` the file, `touch` it)
1. `courier/ask.rs`: `(true, _) => Buttons::Card(q.clone(), choices.clone(), trust),` → `(true, _) => Buttons::Keep,`
   — `tests_gateway::cards::a_dm_question_is_answered_by_its_cards_approve` fails: `left: []`, `right: ["Approve", "Decline", "Details"]`.
2. `runtime/reply.rs`: `self.answers_question(m)` → `false` — `a_reply_answers_the_question_it_replies_to` fails (times out waiting for the answer).
3. `needs_you.rs`: `v.extend(many.iter().enumerate().map(` → `v.extend(many.iter().enumerate().take(0).map(` (no card per question) —
   `a_batch_is_one_card_per_question` and `unprompted_pings_go_one_a_window_and_the_rest_together` fail (`left: 2`, `right: 4`).
4. `needs_you.rs` `asker_name`: `if req.task.is_some() {` → `if false {` — `a_self_tasks_question_names_its_backlog_title` fails: `left: "task 7f3a2c"`.
5. `needs_you/words.rs`: `what: what_of(&req.tool, &req.input),` → `what: req.reason.clone(),` — `the_card_speaks_plain_words` fails.
- Not planted: `a_reply_from_anyone_else_is_refused` (the brief's "answer without the replier's origin"): with no origin the core
  refuses every replier, so that plant fails the owner's reply test, not this one; `dm_cards_off_is_todays_post` (the
  rollback has no single line to plant). Both pass on the code as written.

## Live check for the maintainer
On a scratch daemon (own `--config`, `--socket`, `--state-dir`) bound to a test bot and DM:
1. Start a CLI turn that writes outside the roots; wait 60 s with no screen open. The DM gets one card: three
   lines, **Approve / Decline / Details**. Tap Details (ephemeral: tool, target, rule, id); tap Approve: the call runs
   and the card reads "Approved by … ", buttons gone.
2. Open three questions in one 30-minute window: one header (pings) and three cards ("1 of 3"…, silent). Reply `approve 2`
   and `no 1` to the header; reply `yes` to the third card. Each settles; ✅ on each reply.
3. Reply `yes, but wait` to a card: it is a normal turn. `[notify] dm_cards = false`: the DM gets the one-line posts of before.
4. Config key to add at the install: none needed (default on); to roll back set `[notify] dm_cards = false`.

## Left / uncertain
- Docs the maintainer writes: spec Part III item, `docs/status.md` (the DM question cards), the template comment is in place.
- The settle line has no time or surface (see above). A bare `yes` to a header counts open questions through `confirm.list`
  as the replier, so a surface that may not list them sees none.
