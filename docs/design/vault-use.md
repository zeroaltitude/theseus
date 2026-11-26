# Using what the vault holds, by name (issue 26)

Much of Theseus's own configuration is kept in the vault, so the owner expects it to use the vault more widely. It does,
by tools of its own that have the daemon's own wiring to the vault. The rule that holds all of it: **a secret is used by
its name; its value is never asked for and never shown, and it passes through no turn, no card and no record.** The vault's
program in a shell has no session by design (the daemon's token is never given to a command a turn runs), and the floor
that holds such a command is untouched.

## Field names (`look.vault`, `fields_of`)

A setting that reads the vault needs a field's name, and the look listed titles only. `look.vault` takes `fields_of` (an
item's title or id) and gives each field's label, its section if it has one, and a kind from a closed list (`concealed`,
`text`, `email address`, `web address`, `one-time code`, `a note`, ...; a type the program adds tomorrow is `other`).

- One more verb joins the closed set (`secrets::look::Verb`): `item get <id> --format json`, where the id is one the
  program itself listed (`ItemId`, letters and digits only). No string a model wrote is in it. A title that matches two
  items is not guessed between: both ids are given and nothing is run.
- That verb's answer holds the fields' values, beside their labels. It is read by `OpReader::look_item` alone, kept in
  zeroizing memory, and rebuilt by `parse_fields` from `label`, `id`, `type`, `purpose` and `section` only. **The `value`
  key is never read**; a note's text, a url or a field's own reference are not either. A test whose stub program returns
  values (a field, a note, a section's, an odd type's) shows none reaches the result, the record or the ledger.
- The wall, the outside-text hold and the failure classes are the other look's.

## One card to use it (`vault.use`, family `vault`)

The turn names a vault, an item and a field **in words**, a name of its choosing for the secret, and (as `also`, written as
for `config.propose`) the other settings that should read that name: a provider's `api_key_secret`, a program's
`broker.programs.<p>.env.<VAR>`, a server's `auth_secret`. The daemon:

1. lists the vaults with its own token and finds the one named (by id or name). **Where the account cannot see it, the
   result says so, lists what it does see, and says what the owner can do: grant the account that vault, or move the item to
   one it sees. The vault's maker does not let a service account into a personal vault.**
2. finds the item among that vault's items (by id or title; two of one title are refused with both ids) and the field among
   its fields (by label or id; a section when two share a label).
3. **builds the reference itself**, from the names it found, or from their ids where a name is shared with a sibling
   (the vault's program matches names ignoring case) or has a character a reference cannot carry (anything but letters, digits, space, `-`, `_`, `.`, so a `/`, a `?` or a `#` can never make the reference
   mean another thing); and says on the card when ids were used. A field or a section is written only by a label or id
   that it alone answers to, among the fields the reference reaches (a sectionless field's are all the item's fields);
   when neither its label nor its id is, there is no card and the owner is told to rename one in the vault.
4. **reads the reference once, as a start will, and keeps nothing**: the proof that the item and field are there, that the
   account can read them, and that the field holds something. A failure is told by class (`Failed`), never in the program's
   words, and makes no card.
5. **puts the whole change to the owner as ONE card** (`config_change::put_using`, the path every settings change
   takes). There is no use without a card (issue 54, round 4): a new name may be read by a setting that names it by
   default (the voice's key is read by an audition even with voice off), so nothing is written before the owner's yes.
   `vault_use::classify` decides, by a positive list and nothing else, which proof the card asks:
   - **a pure use** (the secret's own entry, new or re-pointed, and only readers on the list, each set to exactly the
     name: the main model's key, an existing provider's key, an existing URL server's `auth_secret`): no second proof
     ("using a secret must not ask for 2fa", the owner, 2026-11-25);
   - **anything else** (an address, a new server or provider, an MCP `env`, any other setting): `config.propose`'s proof
     for the whole change, at the card and at its yes (`config_change::sensitive_of`).

   The card says the item and field as the owner would say them, that the daemon read it just now, the name it is kept
   under, and **what is given it**: every setting set to that name, and every reader whose field names it by default
   (the main model, GitHub, Discord, the web search, the judge, the voice, Theseus's own MCP server, Google, each AWS
   account), each with where it sends the key when the daemon knows (an existing provider's or the main model's address,
   an MCP server's URL, as a scheme and host). When none is found it says a setting that reads this name by default may
   use it, never that nothing does. A new name's card begins "This gives a secret from the vault a name in the
   settings."; a re-point's says what the name was read from until now and which settings already read it. After the yes
   the card's own restart applies it; the new secret resolves at that start.

**A vault reference is never written by a turn.** `config.propose` (and `also`) refuse a change that writes a `[secrets]`
entry that is not `env:` or `file:`, in words that point to `vault.use`. Before this, a proposal could write any string
that parses: it was never checked against the vault, a title with a slash was silently read as another item and section,
and a `?attribute=otp` read the one-time code, not the field the card appeared to name. An undo may put back a reference an
earlier card showed. `also` may not write `[secrets]`: the tool writes that entry, and it is the one the daemon built.

## Who may, and what an injected turn can do

The wall is the settings tools' (`wall.rs`): the owner's own turn in a private place, never a job, a task, the self loop, an
MCP client's session or a shared place. **When the turn has read text from outside since the owner's last message, or
carries the `injection` flag**, `[content_based_security] outside` decides (issue 54): at `"notify"`, the default, the call
goes on and the owner is told once in an FYI note; at `"ask"` it is asked at the gate before the daemon reads anything from
the vault (`vault_use::ask_if_outside`), with Approve and Decline only. The tool is in `routine::grant::NEVER`. At either
setting the use itself is then a card: nothing is written before the owner's yes.

The tool has no field that carries a value, and its description and the family's always-seen summary say that a secret is
used by its name, that its value is never asked for, and that the vault's program in a shell has no session by design.

## Keeping a new secret

Not here. The design for keeping a secret the owner gives (a card's own form, never a chat message) is a note for the
owner's answer first; no code for it exists.
