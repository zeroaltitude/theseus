# Keeping a new secret in the vault, without it passing through a turn (issue 26)

**Since issue 54 (the owner, 2026-11-25: "Keeping a secret -- the form to ask for it, yes, the 2fa, no"):** the card a turn's
`vault.keep` puts is still the form where the value is given, and it takes **no second proof**, for a new item, an added field
or a replaced value alike. A card asked before that change keeps the mark it was asked with. Everything else below stands:
the value is typed only where the owner alone sees it, an item Theseus's own settings read is never replaced by a turn, an
agent's process is refused (`keep_wall`), and the owner's terminal `vault.keep` with no card still takes a fresh code. Where
the text below speaks of the card's code, it describes cards asked before issue 54. The terminal and the Discord form read the card's own mark: a card
asked with none asks no code at either, and needs no authenticator app paired (`rpc/vault_keep_waiting.rs`).

The owner asks Theseus to keep a secret (a provider's key, say). Its value must reach the vault without passing through a
turn, a card, a record or a log: a value typed in chat goes to the model's provider and into the store, where it cannot be
taken back. So the value is given **in a surface's own form**, as a request of its own to the daemon, which writes it with
its own account. The design note this follows was answered by the owner's side: a new item needs the second proof (a window
may cover it); a replace needs a fresh code every time; an item Theseus's own settings read is replaced only from the owner's
terminal; no detector of key-shaped text, the scrubber is not widened; two cards, keep then use. Whether a durable secret may
travel through Discord's form was the owner's call; on 2026-11-19 he said yes ("let's add the discord form"): **the form exists, in
his direct messages with Theseus only, and `[vault] keep_by_discord` (true by default) turns it off** (see "The form in the owner's
direct messages" below).

## The tool and its card

`vault.keep` (family `vault`, deferred; the owner's own turn in a private place; the wall, the outside-text hold and
`routine::grant::NEVER` are `vault.use`'s). It takes a vault, an item title, a field label and a purpose, in words:
`deny_unknown_fields`, **no parameter carries a value**, so a steered turn cannot hand one over (a test gives it every key it
could plausibly take). The daemon finds the target in the vault's own lists with its own token (`target.rs`; names reach the
program only as ids it listed) and says which of three it is: a new item, a field added to an item that is there, or an
existing value that would be REPLACED (only a concealed field with no section is replaced). It puts ONE card, the settings
card (`config.ack` with a `keep` marker, as an MCP server's has `mcp`), which says that, where the value is typed, that a value
pasted into chat is already with the provider and in the record, and the second proof it needs. The question's record holds
the target by name and id and never a value, answered, expired or declined.

An item that a `[secrets]` entry or the settings note reads is never replaced by a turn: the tool says where the owner does it.

## Where the value is typed

| Place | How | What carries it |
|---|---|---|
| The owner's terminal | `theseus vault keep` (the card waiting, or `<id>`): a hidden prompt, echo off for the read and restored on drop; a pipe, a capture, a redirected standard error and a Theseus job's shell are refused, by the daemon, in words | the daemon's own socket (mode 0600) |
| The cockpit | a password field on the card and a "Keep it" button; cleared as it is sent | the web port, as the owner's own page |
| Discord | in the owner's direct messages only, a form the card's "Type the value here" button opens (`runtime/keep_form.rs`); anywhere else the card says first where the value is typed and offers only Decline, and no form opens (a press of Approve on a card drawn before issue 29 says where the value is typed, `proof_form.rs`) | Discord's servers, as the card says before the press |

## The card, the terminal and the record (issue 29)

The first use of this on the owner's machine failed at the terminal command, silently, after tests that had driven the
daemon's requests one at a time and never the program. What was built after it:

- **The card offers a decline alone.** A card that takes a value is offered no yes by the daemon (`config_change::confirm_request`
  keeps only the decline in `options`), so no surface can draw an Approve that could only be refused. Its first words are where the
  value is typed. Discord draws a Decline button and no sentence about an Approve (`courier/ask.rs`, `render/settings.rs`); the cockpit
  draws its field.
- **The terminal asks the daemon first, and filters nothing.** `vault.keep.waiting` answers which keep cards wait for the owner's
  terminal and, when none does, why, in words (never asked, expired, declined, answered, kept), from the daemon's own rows; and whether
  anything stops a prompt (no authenticator app enrolled; the account may not write in the vault), before any prompt. Standard input and
  standard error must both be the terminal: the daemon says so, and writes a row.
- **The program says nothing that sounds like a result before the daemon does.** It prints the daemon's heading ("To keep a new item ...:
  type its value."), asks, and ends with one plain line: kept and where, or not kept and why, with the exit status to match. What is
  already waiting on the terminal's input is thrown away before each prompt (`tcflush`), and an empty line asks once more.
- **Every run that reaches the daemon writes a `vault.keep` row** (a card that was put is the question's own `tool.confirm_requested`
  row, which names its target under `keep`): `listed` (what the terminal was told, and its
  class: `none_asked`, `declined`, `expired`, `blocked`, `one`, ...), `stopped` (a value was given and something refused it before the
  vault was asked to write: `blank`, `proof`, `no_authenticator`, `place`, `agent`, `job`, `not_writable`, `vault`, ...), `client_ended`
  (the terminal ended before sending a value: `no_value`, `end_of_input`, `unreadable`, `no_echo`), then the outcomes of a write. Each
  carries the client's process id, as the daemon logs it for the connection, and never a value, its length or its digest.
- **The write permission is the vault's own answer.** `vault list --permission create_items` (a new item) or `edit_items` (a field in an
  item that is there) lists only the vaults the account may write in. It is asked before a card and again at the value, so a permission
  taken away since is found before a code is spent; `look.vault` says for each vault whether Theseus may write there.

Two requests, one value each (`theseus_protocol::vault_keep`; the value is a `SecretText`, which prints as nothing):

- `vault.keep.answer {question_id, value, code?}`: the card's. Judged as every answer is (`judge_act`, the place rule; no
  agent's process), then the value is checked (not blank, at most 4,096 bytes, no NUL), one answer at a time per question,
  then the target is **re-found** by the ids the card recorded: the card was a yes to this item and field, so a vault that
  changed meanwhile writes nothing, and a vault that cannot be asked just now keeps the card waiting. Only then the second
  proof through `proof_gate`, so no code is spent on a write that will not be tried. A plain Approve of the card
  (`action.confirm`, `work.answer`) is refused before the proof gate with the words that say where to type, so no code is
  spent on it.
- `vault.keep {vault, title, field, value, code}`: from the owner's own terminal with no card, the one way to replace an item
  the settings read. The terminal alone (not a job's shell, not an agent's process, not the web port, not Discord, not MCP),
  and always a fresh code.

Neither is in `mcp_server`'s allowed list. A request that does not parse is refused without echoing it (serde's own message
can quote the value it found).

## The write

`OpReader::keep_field` is the path that keeps the Google token: the item as JSON on the program's **stdin**, never argv; read
back with `read`; compared by sha256. A new item is made with `item create -`. An existing one is edited by the ids the vault gave
and as a whole: the item is read with its values (`item get --reveal`: what is read is what is written back), only the one field (found by its id) is changed or added, and the whole item is
piped back to `item edit`, the program's own round trip, so none of its other fields is lost. Its answer is a class and never
the program's words:

| ended | told | the card |
|---|---|---|
| written, read back | "Kept in the vault V, item T, field F: it reads back as written." | approved |
| not written | "Nothing was kept ...: <class>. The vault was not changed." (on a timeout: "The vault may have been changed anyway: check the item before asking again.") | declined, with the reason |
| written, read-back failed | "... reading it back failed (<class>). The item exists or was changed: check it in the vault before relying on it." | approved |
| written, read back differently | "... did NOT read back as written. ... check it" | approved |
| the target changed since the card | "... is not what the card showed any more ... nothing was written. Ask for the keep again." | declined |

A row `vault.keep` records the outcome, the state (`new`, `adds`, `replaces`), the vault, title and field **by name**, who and
from where, and whether the item is one the settings read. Never the value, never its digest.

## A value typed into chat anyway

It is already with the model's provider and in Theseus's record, and nothing here unsays that. The tool's description tells the
model to say so and to ask for the new one by this tool; the card says it too. Nothing detects key-shaped text (acting on what
a person says is a tool or a command, never a match on strings), and there is no tool, setting, vault write or header a pasted
value can be given to: `vault.keep` has no value, and `config.propose` refuses a vault reference written as text.

## What an injected turn could do with it

Ask for a card for a junk target (a card appears; nothing is kept until the owner gives a value, and a card from a turn that read
outside text is asked at the gate first, before the vault is listed). Supply a value (there is no parameter). Retarget after the
owner pressed (the request names only the question, and the target is re-found). Read a value back (nothing returns one).
Replace an item the settings read (refused). Overwrite another item (shown on the card; a fresh code every time).

## The form in the owner's direct messages (issue 29, the owner's yes of 2026-11-19)

- **Where.** A keep card drawn in a direct message with the owner, with `[vault] keep_by_discord` on, swaps its first words
  (`vault_keep::HEAD`) for `HEAD_FORM` and gains a button, "Type the value here", beside Decline (`keep_form::offer`; the needs-you
  post, which always goes to the owner's direct messages, draws the same through `needs_you_body`). Every other drawing of the card (a
  channel, a card drawn by another build whose words differ, the setting off, the `/answer` view) is what it was: a Decline, and the
  first words say where the value is typed. A card the form was not drawn for has no form to press.
- **What the card says before the press**, in one plain line: a value typed in the form goes through Discord's servers on its way to
  Theseus, and the form shows what is typed; the terminal does neither. Discord's text field has no hidden style (`TextInputStyle` is
  Short or Paragraph, checked in the library), so the card does not claim the form hides the text. The terminal and the cockpit are
  still named in the same words.
- **The form.** The press lists the cards as the presser and opens a modal for the one card that matches (a V2 card's id is its last
  six characters, so a press that names two cards opens nothing). One field for the value, labelled "Value for <title>, field <field>"
  (45 characters, clipped with an ellipsis as `proof_form.rs` clips) with the vault named in the description; when the daemon says a
  code is needed (a new item whose window does not cover it, or a replace, which always needs a fresh one) the form has a field of its
  own for it, so a code and a value are never one field. Choice: one form with two fields, not two forms in turn, because a second
  form would leave the value waiting in the binding between them; a wrong code costs the owner retyping the value, which is then
  dropped, and the card keeps waiting.
- **The submit** is `vault.keep.answer {question_id, value, code, author, discord}`: the question from the form's id, the value in a
  `SecretText`, and the presser as the binding read it off the press. The daemon judges it as every press is (`judge_act`: an owner,
  a private place, the place rule; no agent's process), then by `keep_place`: on a transport **a direct message with the owner alone**
  (`places::owner_in_person_of`, the rule a repository's trust takes: a channel bound private is refused, though the other answers
  take it), and only while `keep_by_discord` is on. Then the checks above (open, single use, expiry, blank, 4 KiB, trimmed, the target
  re-found, the second proof only when a write will be tried). The target is the one the question recorded.
- **What is written.** Every attempt writes the rows the terminal's writes: `kept`, `not_written`, `changed` and the like, and
  `stopped` with a class (`place`, `setting_off`, `proof`, `blank`, ...); the binding adds a `discord.confirm` row with the outcome
  and the daemon's refusal and never a field. None holds the value, its length, a prefix or a digest.
- **What is recorded when the gateway cannot read an event** (the review's note 1): the library's own text for a failed read is
  `event=<the whole raw JSON>`, and its source's message (serde's) can quote a fragment, so a form's value could have reached the
  binding's `warn!` line, a `discord.error` row and health's `last_error`. `runtime::receive_words` records the kind in fixed words and
  nothing derived from the event, not its length. The binding's other `board.error` sites format a `twilight_http` error (which can carry
  Discord's response to a request the binding sent: the callback, a followup, a modal's response, an edit) or the daemon's refusal. No
  request the binding sends carries the value (the reply is the daemon's words by name, the modal holds the title, field and vault), and
  Discord does not echo a request's text in an error body, so none of them can carry it; the test sends a malformed form through the fake
  gateway and searches the log at trace level, the rows and health.
- **Where else the form's words are drawn.** The `/answer` view in a direct message draws the same first words and the same button
  (`answer_cmd::answerable` applies `keep_form::offer`). A keep card has no Details button (its buttons are Decline and the form's), so
  there is no Details view of one to explain. A value the daemon would refuse (blank, over 4,096 bytes, a NUL) is found by the form (it counts characters, the daemon bytes) and
  sent as a class and no value (`VaultKeepAnswerParams.refused`): the daemon judges the presser and the place, writes its own `stopped`
  row (`blank`, `not_a_value`) and answers in its own words (`done::refusal_words`), so every attempt leaves the keep path's row. A keep that arrives without the second proof (the window lapsed between the press
  and the submit) is told in the keep card's own words (`vault_keep::proof_missing_words`), never `theseus answer ... approve --code`. A yes or a code given to a keep card from a conversation (the chat's answer tool, a typed
  yes, the plain-text needs-you line) is told the same way: the owner gives the value (`vault_keep::chat_words`, `KEEP_BY_PRESS`,
  `answer_line`).
- **The reply** to the presser is private and is the daemon's words by name (or its refusal); the card is edited to its outcome by
  the daemon's own settle, as the terminal's keep is. The turn is told by the settled card, not woken: the owner asks for the use
  (`vault.use`, the second card).
- **The setting.** `[vault] keep_by_discord`, default true. Off: the card is drawn as above for a channel (no form, no line about
  the chat service), a press says it is off and asks the daemon nothing, and a submit is refused (`setting_off`). It is classified as
  a sensitive table (`config/sensitivity.rs`): a change to it is a card with the second proof.

The proof: `config_change/vault_keep/tests/tests_form.rs` (the daemon, from the Discord surface, against the stand-in vault: the
planted value, every refusal), `runtime/keep_form/tests.rs` (the drawn card and form, with a golden; the binding's one request) and
`crates/theseusd/tests/keep_form.rs` (a real daemon, the fake gateway and the stand-in vault, end to end).

## What the conversation that asked is told (issue 37)

The tool answers at once, with what it asked and where the owner gives the value, and no value. The card then
settles by whichever end: kept (a new item, a field added, a value replaced), refused with its reason, declined,
or expired. Before issue 37 nothing the model reads changed at that point: the settle wrote the answer's row, the
session's narrative and the card's close, and the model's next turn was given only the first result, which says the
card waits. A late result could not carry it (it needs a background placeholder and a queued result, and a bound or
declined question queues none), and a settings card has no such path either.

So the frame that settles a keep card also writes ONE note into the asking session, as a thread's move writes one:
`Your vault.keep card is settled and no longer waits for the owner: <the owner's own words>`, by name and never the
value. The frame is the one that binds or declines the question, so a second answer (refused as already answered), a
replay and a restart add none. The tool's own words no longer quote the card's first lines (where the value is typed
in a place with no form), because the card may be drawn in a place with one.
