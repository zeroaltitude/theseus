# Share this exact message to this place (issue 44, step 3b, the simple shape)

> **Superseded on 2026-11-22: anything goes.** Nothing private is withheld from a shared place any more, so
> there is nothing for the card to release; its verb is gone and an old release record still decodes. What
> follows is the record of the card as it was built.

The owner's decision 2 (`context-point-of-view.md` §15): in company, Theseus is shown nothing he said privately. This
note describes the one way something private crosses: **the owner shares one exact message of his to one place,
by his own act.** Nothing does it for him.

## What is built

- **The record** (`theseus-core`'s `release.rs`): `release.id.<id>`, indexed by `release.to.<place>\u{1f}<node>`.
  It names ONE message of his (its node, and a digest of its words as he was shown them), the place it is from, the
  ONE place it is released to, the surface, and the time. Only that message's words are ever shown: not the reply
  to it, not another person's steer in the same exchange, not a tool's stub, not anything added later.
- **His act alone.** `exchange.offer` shows him exactly the words a release would share and their digest;
  `exchange.release` (message, digest, place) writes the record; `exchange.releases` lists them; `exchange.withdraw`
  ends one. All four answer only his terminal or the web UI, in person (`places::owner_in_person_of`, asked before
  anything is read): an agent's process, the MCP server, a chat transport and an unnamed connection are refused.
  CLI: `theseus ledger release-offer <message>`, `ledger release <message> --digest <d> --to <place>`,
  `ledger releases`, `ledger withdraw <id>`.
- **One check, at the release and at every read** (`release::piece`), failing closed: a message whose owner's mark
  says it is his, of a private place's exchange; its session neither sealed nor seal-pending as seen by no one
  (`Core::ladder_hidden`, with no thread's own exception, so a reader inside that very thread is no exception);
  never moved into any thread, whatever that thread's seal. A read also needs the words to match the digest.
  Any move record counts, the one an unsplit or an unclaim leaves when it puts the message back too: a message that
  was ever moved is shared again only by a new message.
- **Once.** Grants and withdrawals are serialized: one message is released to one place by one record, however
  many requests race; a second release of it there is refused, withdrawn or not. A withdrawal is a second record
  (`release.end.<id>`); nothing is rewritten. What was already shown or sent cannot be unsent, and the withdrawal
  says so. An unknown or withdrawn id writes nothing.
- **The reader** is the history tool in the named place alone. Only a bare first call there (no cursor or token,
  no other place, thing, or words) lists its releases: newest first, at most ten, with how many more, counted in
  the page's lines. Every later page, a narrowed call, and a look the turn's allowance did not grant list none, so
  what is listed hangs on no position in history; every release still opens by its handle. Each line is
  the message's words, cut as a line is, labelled "released here by the owner from another conversation", and its
  handle (`rel_…`); opening the handle gives the words in full under the same label, and counts as an open. The
  view's slots and the transcript do not carry a released message.

## Not built, and not planned until the owner asks

The automatic offer: a judge deciding that a message in company leans on something private, a look through his
private conversation, and a card in his direct conversation offering to release what it found. It was built in
shadow on this branch and deleted: nothing asks a judge, nothing searches his private conversation on a company
message, and nothing is recorded about one. If it is wanted later, it would end in the same release record above,
made by his press.
