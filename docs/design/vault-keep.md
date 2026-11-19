# Keeping a new secret in the vault, without it passing through a turn (issue 26)

The owner asks Theseus to keep a secret (a provider's key, say). Its value must reach the vault without passing through a
turn, a card, a record or a log: a value typed in chat goes to the model's provider and into the store, where it cannot be
taken back. So the value is given **in a surface's own form**, as a request of its own to the daemon, which writes it with
its own account. The design note this follows was answered by the owner's side: a new item needs the second proof (a window
may cover it); a replace needs a fresh code every time; an item Theseus's own settings read is replaced only from the owner's
terminal; no detector of key-shaped text, the scrubber is not widened; two cards, keep then use. Whether a durable secret may
travel through Discord's form is the owner's call, still open: **there is no Discord form.**

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
| The owner's terminal | `theseus vault keep` (the card waiting, or `<id>`): a hidden prompt, echo off for the read and restored on drop; a pipe, a capture and a Theseus job's shell are refused | the daemon's own socket (mode 0600) |
| The cockpit | a password field on the card and a "Keep it" button; cleared as it is sent | the web port, as the owner's own page |
| Discord | **none, until the owner decides**: the card says where the value is typed, a press says it again, no form opens (`proof_form.rs`) | |

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
and as a whole: the item is read (`item get`), only the one field (found by its id) is changed or added, and the whole item is
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

## Deferred, not built

The Discord form: one commit when the owner says yes, a modal whose submit sends `vault.keep.answer`. The words of the card and of
`proof_form.rs`'s press change with it. After a keep, the turn is told by the settled card, not woken: the owner asks for the use
(`vault.use`, the second card).
