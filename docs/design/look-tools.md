# Looking is not asking: Theseus reads itself and its vault's names (issue 26)

The owner's setting for ordinary work is to be told, not asked. A shell command that names Theseus's own program
(`theseusd`) or the vault's program (`op`) is held at every setting, however it is used (`policy.rs`, `runs_program`): a
shell line can hide anything, and the guard was hardened against being talked round. A turn nevertheless reached for
exactly those commands to *look*: at its own profiles and health, and at what the vault holds. Each was a card the owner
had to press, and the vault look could not have worked: the daemon's token is never given to a command a turn runs, so the
vault's program in a shell has no session, by design.

The cure is the one issue 22 used for settings (`config.show`, `config_change/show.rs`): **the floor stays as it is, and
Theseus is given proper ways to look.** Nothing here loosens, narrows or reinterprets the floor, and no allow-list of safe
sub-commands exists.

## The two tools (family `look`, deferred, owner's own private turn)

- **`look.theseus`** (`look/theseus.rs`): the profiles and which is live and why; each provider and whether its key is
  `ready`, `still resolving` or `not resolved`, with where it is read from (`vault`, `environment`, `file`), as health says;
  the daemon's health in a few lines; the build. It never gives a key's value, the vault item a key comes from, or the text
  of a failure.
- **`look.vault`** (`look/vault.rs`, `secrets/look.rs`): the daemon, with its own token and its existing time limit
  (`OP_TIMEOUT`), asks the vault's program who the token is, which vaults it sees, and the items of one vault or of all, and
  joins that with `[secrets]`: which setting reads which item, resolved or not. An item is its id, its title, its category
  and its vault's name, and nothing else; the list is capped (50, at most 100), takes a vault and a part of a title, and
  says its cap and how many it left out.

## The wall, and what it lets through

The wall is `config.show`'s (`config_change/wall.rs`), so the tools are refused to a job, a task, the self-improvement loop,
a session an MCP client opened and a shared place, each in the tool's own words. In the owner's own clean turn a look is
*told* (a notice), not asked. **When the turn has read text from outside since the owner's last message (or its session
carries the injection label), the look is asked first** (`look::ask_if_outside` sets the plan's `authority`, which the gate
asks for at every posture): titles are names, and a turn that was tricked must not list them, or map what Theseus runs on,
unasked. Both tools are in `routine::grant::NEVER`.

## Why no value or note can reach a turn (field names: `vault-use.md`)

1. **The command line is a closed set** (`secrets::look::Verb`): `whoami`, `vault list`, `item list` (and, for field names, `item get` of a listed id: `vault-use.md`) (optionally `--vault
   <id>`), each with `--format json`. No string a model wrote is part of it. A vault is named to the program by the id the
   program itself listed (`VaultId`, letters and digits only), never by what the model typed, so a vault named like a flag
   or a reference is simply not found. A title filter is applied here, to what came back.
2. **The answer is rebuilt from named keys** (`parse_account`, `parse_vaults`, `parse_items`): a field the program lists
   today (an item's username, addresses, tags) or adds tomorrow does not ride through. The account is its host and the kind
   of token, not its identifiers. A reference in `[secrets]` is shown as its vault and item, never its field.
3. **A failure is a class** (`secrets::look::Failed`: no vault session, not started, timed out, refused, unreachable,
   failed, too large, unreadable), never the program's own words, which can name an item.
4. Every name that is shown is one line of at most 120 characters (`clean`).

A test plays the owner's turn with a stub of the vault's program that logs each argument list, and gives the tool hostile
input (a vault that looks like a flag, a reference, a path; a filter that looks like a flag; keys the tool does not take):
every command line that was run is one of the list shapes.

With no token (an install whose secrets come from the environment or from files) the tool says so, runs nothing, and still
lists where each `[secrets]` entry is read from.

## The card

A held shell command that names a floored program says which one, and that a look has a tool
(`needs_you/plain/floor.rs`): the vault's program ("a shell has no vault session by design; `look.vault`"), Theseus's own
program (`look.theseus`, `config.show`), or Theseus's own state named by an argument (`config.show`, `look.theseus`). The
words are `PlainWords::held` (what Discord's card draws as "why it asks") and `PlainWords::why` (what the terminal's card
prints); the cockpit and `theseus answer` print the same. The floor's own reason is read, so the words follow it, and a
test plays each real reason through the gate.

## What the model is told

The descriptions of the two tools, the family's one summary line (always seen) and `proc.run`'s description say that
Theseus's own command line and the vault's program are not the way to look, and that the vault's program in a shell has no
session by design. The family is deferred; the tokens it adds to a request are in `docs/status.md`'s look at issue 26.
