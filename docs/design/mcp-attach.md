# Attaching an MCP server by asking (issue 22, line 4)

The owner names a server in a private place, a command to run or an address, and Theseus attaches it: tried once in
the sandbox with no secret, shown on one card, and on the owner's yes written to the settings by the daemon and
loaded live, so its tools are offered from the next turn with no restart. Taking it away is the same road. It is
built on `config_change` (`docs/design/config-change.md`): the same wall, the same candidate, the same question
(`config.ack`), the same write with the previous copy kept. Code: `crates/theseus-core/src/config_change/mcp_attach*`
and `crates/theseus-core/src/mcp/attached.rs`.

## What the owner says, and sees

- "Attach the word counter: run `wc-server --lines`." Theseus calls `mcp.attach` with `name` and `command` (the program
  and each argument as its own list entry; there is no shell line), or `url` for a server over HTTP, and optionally
  `read` (the tools the owner says are read-only), `env` (variable to the NAME of a `[secrets]` entry), `auth_secret`,
  `sandbox` and `egress`.
- A server over HTTP asked for with no key whose trial is refused with HTTP 401 wants a sign-in (issue 55): at an
  `https` address it is attached as one that signs in (`oauth = {}`), and the card says why and that the owner runs
  `theseus mcp login NAME` after the yes; at a plaintext address it is written as asked, and the card says it will
  not connect (a server that signs in must be `https`). A client id or scopes, for a server that needs them, are
  settings keys under `oauth`.
- One card: the server's name; exactly what will run (the argument list as a JSON list, so an argument with a space
  or a `;` stays one argument), and whether it runs in the sandbox or OUTSIDE it; each tool the trial listed, with its
  description cut to one line and marked as the server's own words; which tools are read-only (only those the owner
  named: a server's hints never loosen a tool); which secrets it is given, by name, and that their values come from the
  vault; which hosts it may reach; that its results are outside text; whether the trial listed its tools; that its
  tools are offered from the next message with no restart; and that attaching needs the second proof as well as the
  yes.
- A secret the settings do not have (or that the running daemon has not loaded since it started) makes no card: no
  yes could attach the server, so the owner is told which entry is missing and how to add it (a change to `[secrets]`
  holding a vault reference, which is itself a sensitive change), and nothing is asked. The trial still ran, without it.
- `mcp.detach` with a name: one card saying what stops and when. A server attached by asking stops at once. One the
  daemon started from the settings file at its start stops at a restart, and the card says so beforehand and uses
  `config_change`'s restart and its way back.
- `theseus mcp` and health show which servers were attached by asking, who approved and when.

## The order, and what each step may do

1. **The wall** (`config_change/wall.rs`): the owner, alone in a private place, in their own session, as `config.propose`.
   A job, a task, the self loop, an MCP client and a shared place are refused, in words that name `mcp.attach`. A
   message that came through an agent's process (`peer_proc`) is refused too, and so is a turn that continues it after
   an approval or a wait (the session keeps the agent behind its last input); the card is answered only by the owner in
   person (`rpc/mcp_card.rs`), checked after `judge_act`, which stays as it was.
2. **The second proof** (`proof_seam.rs`): attaching can give a server secrets and a network, so it is sensitive. Until
   `owner_proof` is built in the tool refuses, before anything starts, with the seam's words. The yes asks the seam
   again (`answer.rs`).
3. **The trial** (`McpBoard::trial`): the server is started once as `try-<name>` in L1 whatever the owner asked it to
   run in, with `egress` only if it will run in L1, and no secret: its `env` and `auth_secret` are empty and the
   connection is given no environment and no bearer. An HTTP server is asked for its list without its key. A trial that
   fails is shown. It is a task the owner's cancel reaches, and it ends at 30 s.
4. **The candidate** (`edit.rs`): one new table, `[mcp.servers.<name>]`, in the file's own text, parsed back and
   loaded exactly as a start loads it. The card is built from the table AS PARSED FROM THE CANDIDATE, never from the
   model's words. The settings that really change must be that table and nothing else.
5. **The yes** (`answer.rs`, the same `config.ack`): the file is re-read and must be the one the candidate was built
   on; the kept candidate must be the one the card names; the changed settings must still be only the server's table
   (added for an attach, removed for a detach); the second proof; then the daemon's own write, with the previous copy
   kept, and `config.changed`. The answer's frame also carries `mcp.attached`, the record of who attached which.
6. **Live** (`mcp_attach/live.rs`): the server is loaded as the file now holds it, under its own name, with the
   stored list the trial found, so its tools are offered at once; only the tools the card showed are offered until the
   next start, which offers whatever a configured server offers. A detach unloads it and stops its process group.

## What this cannot be made to do

- Start a server with a secret, or outside L1, before the yes: the trial's configuration has neither (`tests` assert
  on what the board's connect was given).
- Show a card that differs from what is written: the card reads the parsed candidate, the yes checks the candidate by
  its digest, and the settings that change are re-checked at the yes.
- Make a tool read-only that the owner did not name.
- Let a server's own words pass as the card's: a tool's name is shown whole and a description is its first line, each
  an escaped one-line literal (control, invisible, direction-changing and line-separating characters escaped) after a
  fixed label, under "descriptions are the server's own words"; a name too long to show whole makes no card.
- Carry a secret's value: a table names a `[secrets]` entry, the trial is given nothing, and no turn, card, log or row
  has a value to carry; an address with a name or key before an `@` is refused (the key is named in `auth_secret`).

## Limits, said plainly

- A server the daemon started from the file stops only at a restart; only one attached by asking in this run unloads live.
- After a restart a server attached by asking is a configured one and offers every tool it lists, as any configured
  server does; `mcp.tools_changed` says what changed.
- A server's tool list kept for the live load is in memory only; a daemon that restarts between the card and the yes
  loads the server and lists it again.
- The trial of an HTTP server is a real request to the address the owner named, before the yes; it carries no key.
