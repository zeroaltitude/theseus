# Theseus does its own work by tools, not by its command line in a shell (issue 28)

A turn that was asked to "use the mythos profile to say hello" ran Theseus's own command line in a shell:
`theseus ask -P mythos "..."`. The model behind the profile answered, and the conversation never showed its words: the
answer was produced inside the shell, came back as terminal text, and the turn's reply said it would run one turn.
Theseus has a command line for a person at a terminal and tools for itself; where a tool is missing it falls back on the
command line, and that fails three ways: what it learns comes back as terminal text, a shell command is judged as a
shell command and not as the act it is, and an answer made inside the shell is not what the conversation shows.

The cure is the one issues 22 and 26 used (`config.show`, `look.*`): **nothing about the floor changes** (a shell command
naming Theseus's own program or the vault's is held as it was, `policy.rs`, `runs_program`), and a turn is given a proper
tool for each thing it has a reason to do. **No tool here gives a turn a power it did not have**; this issue only gives a
better way to use the ones it has. What only the owner may do from their own terminal or by their own press (an approval,
`theseus answer`, the second proof, a held change, `owner`, `vault keep`'s value) is never offered to a turn.

## Part 1: asking a profile, and holding a conversation to one (family `profile`, deferred)

- **`profile.ask`** (`profile_tool/ask.rs`): ONE question to a named profile. The asked model is offered no tool, the
  question is at most 8,000 characters, the answer at most 4,000 tokens (a model that thinks before it speaks spends some
  of it thinking) and the call 100 seconds. The result is the profile's own words, with its model and provider named, and
  says whose they are; a cut answer says it was cut and at what cap.
  - **It is a priced provider call of this turn's**, the way compaction's summary call is: planned in the kernel as a
    `provider.call` action of the asking turn's execution, with its reservation (so the spend limit, the day ceiling, a
    `/stop` and the burn-rate brake all apply), made through the data floor's provider (a profile of restricted origin is
    refused for a session whose labels keep one off, in the floor's own words), and settled at its price (a failed call
    settles as failed, or unknown when the provider may have done the work). The action's `purpose` is `ask_profile`.
    `begin` runs in the turn's own task, where the turn's guard is; `finish` makes the call in the tool's task.
  - **The profile's refusals are the layer's own.** A name that is not a profile, or was renamed, is told in the words
    `session.profile` already uses (`profile_refusal`, `renamed_profile`); a provider's error is told as its own words.
  - **Told, not asked**, at the owner's setting, as a turn's own model call is (its class is a read, as the chat acts'
    are, and it carries no authority). The one exception is the wall's: a turn that has read text from outside since
    the owner's last message is **asked first** (`ask_if_outside`), because a tricked turn must not send words to another
    model, or move the conversation, unasked.
- **`profile.hold`**: this conversation, and no other, goes on in a named profile from its next turn, or is let go
  (`release`). It is what `theseus sessions model` does: it calls `Core::session_set_profile`, which runs
  `judge_act(Act::Session)` and the floor's refusal of a profile for the session's data, and writes the same one frame and
  `session.profile_set` row. It acts **as the owner who spoke**: the message that started the turn (the chat acts'
  `Said`, which `turn.submit` stages), and only when it was the owner's own typed message. A turn no message of the owner's
  started (a wake, a task's report, a job's turn) is refused. The turn that calls it keeps the model it started on.
  `theseus profile use` (the daemon's live profile, for everyone) is NOT offered: that is a setting of Theseus, which
  `config.propose` changes on a card.

**The wall** for both is `config.show`'s (`config_change::wall`): the owner's own turn in a private place only they write
in; refused to a job, a task, the self loop, a session an MCP client opened and a shared place, each in the tool's own
words. Both are in `routine::grant::NEVER`.

## What the model is told, and what it costs

The family is deferred (it adds no tool definition to a request that does not load it) and registered, as the looks are,
only in a daemon started from a config, so no request a test builds carries it. The family's summary line is in
`tools_defer/family.rs`; what loading it by default, and the line, cost against the two budgets is part 3
(`docs/design/look-tools.md` has the same numbers for the looks).

## Played

`config_change/tests_profile.rs`: "use the haiku profile to say hello" is one `profile.ask` call (no shell), the asked
model's request has no tool and one message, its words and model are the result and the turn's reply quotes them, the call
is a `provider.call` action of the turn's execution settled as `ask_profile`; an unknown profile, a bad question, a
task's session are refused and call no model; the hold needs the owner's own message and a real profile, writes the
`session.profile_set` row the command writes, and is let go the same way.
