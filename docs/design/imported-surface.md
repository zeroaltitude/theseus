# What reaches an imported session's content

The rule behind `theseus-a7pr`, written down after three separate patches to the same class of defect.
Each patch closed one door; nothing said what the door was for, so the next one was found by reading the
previous fix. This note is the rule, so the fourth finding is the last one found by eye.

## The defect, four times

| Door | Found | State on this base |
|---|---|---|
| `session.history`'s nodes (`text`, `thinking`, `detail`, `cites`) | 2026-11-08 | closed: `history_on(p, conn.surface)` (`rpc/server.rs:326`), `withheld_node` |
| `session.history`'s `session` block — its `title` | 2026-11-08, reviewing the fix | **open**: `methods.rs:1311` builds `session_info` outside the withhold branch |
| `node.list` | 2026-11-08, reviewing the fix | routed with no surface (`rpc/server.rs:333`); out of MCP's reach only because `NODE_LIST` is absent from `mcp_server::ALLOWED` |
| `session.list` / `session.find` with `with_text` (`first_text`, `last_text`, `prompts`, `title`) | 2026-11-09 | **open**: routed through `sessions::OWN` (`rpc/server.rs:318`) with no surface argument |

All four are the same shape: **a read method that returns a session's content, routed to every surface,
with no surface check**. Three of the four were latent rather than designed-safe: they are out of an MCP
client's reach because its method list happens not to name them.

## 1. What is the protected thing

Not "an imported session", and not "a private place". Precisely:

> **The owner's own words, as carried into this daemon from an earlier harness.** Concretely: the text of any
> session whose id `crate::import::is_imported` accepts (the `ses_ep…` prefix, `import/mod.rs:82`), and every
> field of any wire shape that is derived from that text.

Two consequences the design has to carry.

**The place rule cannot be the guard.** `recall.rs:1287` makes every imported session `Place::Private`,
deliberately: an imported episode is the owner's own history whatever place its episode names. That is right
for *where the words may go* and useless for *who is asking*, which is why `memory_lookup::mcp::nodes_of`'s
`place_of(session) != Place::Private` guard (`memory_lookup/mcp.rs:184`) never fires for an imported session.
The two axes are orthogonal and need two predicates:

- **place** — `PlaceClass` / `recall::Place`: may these words be said *there*.
- **surface** — `approval::Surface::reads_private()`: may this *asker* be given them.

`reads_private` is already an allowlist (`Cli | Web`, `approval.rs:67`), so a surface added tomorrow defaults
to withheld. That half is sound and is not revisited here.

**"Derived from that text" is a closed list per wire shape, and it must be checked rather than assumed.**
On `NodeInfo` the text-bearing fields are `text`, `thinking` and `detail`, plus the result's `cites`; that
was checked when `withheld_node` was written. On `SessionInfo` they are `title`, `title_was`, `first_text`,
`last_text` and `prompts`. A field added to either type is a new door, and the type's own test is where that
is caught.

## 2. Where the rule lives

Three places were weighed.

**(a) At the route.** Each dispatch arm takes `conn.surface` and the method withholds. This is what
`session.history` does today. It is the cheapest and it is where the surface is actually in scope — and it
is exactly the shape that produced four separate findings, because a method added without the argument
compiles, runs, and is wrong in silence. *Rejected on its own; kept as the mechanism.*

**(b) At the serialiser.** `SessionInfo` and `NodeInfo` grow a constructor that cannot be called without a
surface. Fail-closed by construction and the strongest of the three — but these types are built at roughly
twenty sites, most of which have no surface in scope at all (the push, the work board, health, the narrative,
the index). Threading a surface to all of them is a large change that would mostly thread `Cli`. *Rejected:
the cost is real and most of it buys nothing.*

**(c) On the type that carries the text.** A `Withheldable<T>` wrapper the wire shape is unwrapped from.
Strictly the best, and the same objection as (b) plus a store-format risk. *Rejected.*

**Chosen: (a), made fail-closed by (d) — a classification every method must carry.** The route stays the
place the surface is read, and the thing that cannot be skipped moves one level up: **every method in
`theseus_protocol::method::ALL` must be classified for the MCP surface, and the classification is walked by a
test.** A method that nobody classified fails the build. The classification is three lists in
`crate::mcp_server`: `ALLOWED` (the server's tools reach it), `JUDGED` (the owner's acts, which their own
judgment refuses), and `NOT_FOR_MCP` (everything else, by name). They partition `method::ALL`.

That does not by itself make `session.list` safe — it makes it *impossible to add a session-reading method
without writing down which of the three it is*, and the act of writing `ALLOWED` is the act at which someone
has to ask "does this return a session's content, and does it take a surface?". That is the question nobody
was asked four times.

The withhold itself is one function per wire shape, used at every door:

- `withheld_node(NodeInfo)` — exists.
- `withheld_session(SessionInfo)` — new: `title`, `title_was`, `first_text`, `last_text`, `prompts`.

A door is closed by taking the surface and mapping the shape through its function. There is no second
mechanism.

## 3. What fail-closed means here

Three bars, in order of how much they are worth.

1. **A new method defaults to refused on `Surface::Mcp`.** True today — `mcp_server::allowed` is an
   allowlist — and now *provably* true: `NOT_FOR_MCP` must name every method `ALLOWED` and `JUDGED` do not,
   so the partition is checked rather than trusted. The test that proves it walks `method::ALL` (229 names on
   this base) and judges the three lists against that walk, in both directions, through
   `crates/complete.rs`'s `check`. A method added to the protocol and left unclassified fails it; a name left
   on a list after its method is removed or renamed fails it too.
2. **A new surface defaults to withheld.** True today: `reads_private` is `Cli | Web`.
3. **A new text-bearing field on `SessionInfo` or `NodeInfo` defaults to withheld.** *Not* true, and not
   solved here. It needs (b) or (c), or a field census on each type. The honest state is that this is the
   one bar of the three that is still an eye-check, and the two withhold functions are where a reviewer
   looks. Noted as open rather than claimed.

The bar that is *not* met by an empty set matters here as much as anywhere: each withhold test asserts the
positive side too — the owner's own surface still gets the text, and a session that is not imported is
unaffected — so a test that loaded nothing cannot pass.

## 4. What breaks

Nothing, on this base, that the owner would notice.

- **`session.list` / `session.find`.** The only callers that pass `with_text` are the CLI's `-c`/`-r` picker
  and the cockpit, both `reads_private()` surfaces; they are unaffected. A Discord-binding or unnamed-surface
  caller loses an imported session's `title`, `first_text`, `last_text` and `prompts` and keeps the row, its
  id, its counts and its state — the same shape `session.history` already answers with. No caller on this
  base asks for those fields from such a surface.
- **`session.history`'s `session.title`.** Discord's in-process binding reads history. For an imported
  session it will now see no title. That is a visible change and it is the point of the finding.
- **`node.list`.** Left as it is by this note: it takes no surface, and closing it means giving it one. It is
  listed in `NOT_FOR_MCP`, so the classification now says in writing that it is refused rather than
  accidentally absent.
- **`memory_get` / `memory_search` over the MCP server.** An owner call, deliberately not decided here: with
  `[mcp_server] memory` on (off by default), `memory_get` returns an imported session's text because
  `place_of` reads it private. Either answer is defensible — the setting is the owner's explicit yes, and the
  episodes are the owner's own words — and the design does not pre-empt it. If the answer is "withhold", the
  mechanism is already written: `nodes_of` gains the `is_imported` check beside its `place_of` one.

## What this note does not do

It does not give `node.list` a surface, and it does not make a new field on `SessionInfo` fail closed. Both
are named above with what they would take. Everything else in the class is either closed or classified.

_Written by Tabitha/Claude, 2026-11-09, for theseus-a7pr and bucket row theseus-3i6d1._
