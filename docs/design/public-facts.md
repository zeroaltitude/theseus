# A published fact is looked up and shown, not asked for (issue 26)

Seen once on the owner's machine: with the web tools loaded, asked to set a provider up, Theseus asked the owner where the
provider is served and which protocol it speaks, and looked nothing up. Two things pushed it that way. Its care about where
a secret is sent is right, and stays. But everything it reads says that a turn which read outside text is asked first and
its card is marked, and nothing said that a fact its maker publishes (a model's exact name, a provider's address and
protocol, a server's command) may be looked up and **shown**. So the timid reading won.

## What the model is told

The descriptions of `config.propose`, `vault.use` and `mcp.attach` say that such a fact may be looked up with the web tools
instead of asked for, that it is given with a `source`, that a guess is never put on a card, and that where a secret is
sent is still the owner's to confirm on the card. These three tools are in deferred families (`settings`, `vault`): no
description or summary a person's turn always loads changed, and the family summary lines are as they were. The two
budgets that a growth there could cross (the terminal place's loaded-family budget, and the small-prefix line of
`tests_wait_cost`) are unchanged.

## The `source`

An optional text beside a proposed setting (`config.propose`'s changes, the `also` of `vault.use`), or on `mcp.attach` for
the server's command or address: a page's address and what it says. It is the model's word and nothing more. The daemon
fetched nothing and checked nothing, and the card's line says so (`config_change/source.rs`):

```
- providers.lantern.api_base: not set (its default) → "https://lantern.example.test" (after a restart)
  Theseus's source for this, in its own words (nothing here checked it): "https://docs.lantern.example.test/api says ..."
```

- **One line, cut.** Runs of whitespace (a line break included) become one space; the text is cut at 200 characters, with
  a `…` that says so (the length of the model's reason), before its unseen characters are written out, so a line of them
  is longer. A character that cannot be seen is written as `\u{..}`, a quote or a backslash is escaped as a value's are
  (`diff::escaped`), so the line ends in the card's own closing quote.
- **It begins no line and ends none.** It follows a fixed label, indented, so it can pass for no setting line, no care
  line and no reason; a fence in it is one more character of a line. Discord's markdown escaping and the cockpit's text
  drawing treat it as the rest of the card; a test of each place draws a source and a hostile one.
- **Where it sits.** Under the setting it was given for; a source on a table is under each setting in it. A source on an
  address Theseus sends to (`config/sensitivity.rs`, `ADDRESS_REASON`) lists that setting, with its source, before the
  others: where a secret goes is the first thing the owner reads.
- **What it never replaces.** The setting's own value is on its own line, from the two parsed configs. The line that says a
  turn read outside text, the second proof, the restart and the reason are built from what the daemon knows, as before; a
  source adds lines above them and moves none of them out of the card. A source is never a check: nothing reads it, and
  nothing it says changes what the yes writes.

Every place that draws a settings card draws `PlainWords::detail`, which is this question text whole (Discord, the
cockpit, the terminal and `theseus answer`), so the daemon words the source once for all of them.

## What cannot be proved here

That a real model now looks things up. No fleet box carries a model key; a scripted turn proposes an address with its
source and ends with one card that shows both. The owner sees the rest on their own machine.
