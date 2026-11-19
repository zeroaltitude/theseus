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

## The answer is a message of the conversation, from Theseus, with a subagent footer

The owner's rule (2026-11-18, 23:50 MST, on part 1): "When a subagent responds to theseus in chat, the message should be
delivered back to the chat in an appropriate context, probably as theseus, with a subagent footer, and the appropriate
model listed in the model part of the footer." So `profile.ask`'s answer is shown, beyond being the tool's result:

- **The footer** is one set of words, `theseus_protocol::SubagentAnswer::footer_bits`, a reply's shape with the model
  part first: `haiku · claude-haiku-5-5 · subagent answer, asked by sonnet · $0.0031 · 1.3 s`. The model part is the
  profile and the model that ACTUALLY answered (a fallback shows as itself), never the asking turn's; it says it is a
  subagent's and the profile of the turn that asked; its cost and time are the sub-call's own. Discord, the terminal
  (`status_line`, `[haiku → anthropic/claude-haiku-5-5 · …]`), the terminal UI and the cockpit print these bits.
- **The record** is the call's result node, marked `meta.subagent` (the answer, its model, who asked, its cost and
  time). It is a node of the conversation like a reply, but no new node kind: a new body would change the store's
  format and every match on it, and a tool result is already written once, compiled once and never read twice by a
  model. `tool.ended` carries it (`ToolEnded.subagent`) so a live client draws it as it lands; `theseus history` draws it
  after the call; the cockpit's transcript draws it as a message of its own after the call's card, which is also the
  inspector's view of the turn; `theseus why` names the profiles a turn asked and says each answer is a subagent's.
- **Discord** gets it through the outbox, as every post goes: `turn/subagent_step.rs` stages one post per answer in the turn's
  frame, to the place the turn's reply goes to (`stage_reply`'s own choice: an answer's place, a thread's turn spoken in
  the owner's DM, the session's place, where a wake was set), after the call and so BEFORE the reply, which is staged
  when the turn ends; the lane sends a target's posts in the order they were written, once (a post is a kernel action
  with the nonce key, as every post is). A thread's turn carries the thread's tag, as its reply does. The courier draws it
  (`courier/subagent.rs`, `render/subagent.rs`): the words, then `-# ` and the footer, split under Discord's limit.
- **The terminal** prints the words on stdout before the reply that follows, and the footer on stderr; `--json` prints
  neither (the daemon's result is the whole of it); the terminal UI says the words and a dim footer line.
- **A turn that was stopped posts none**: the answers wait in memory until the turn stages them after its calls, and a
  stop that landed since the turn's input arrived drops them; a call a stop aborted never gives one. A session that
  posts nowhere (the CLI's) stages nothing.
- **What does NOT name the right model yet** (found, not changed here): a task's report to a chat names no model at all
  (`render::report`: the task, its turns, its money, its time), and the words of a task are read by the asking turn,
  whose own reply footer names the asking turn's model.

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

## The shell is answered with the tool that does it (issue 28, part 3, step 1)

A turn asked to look at Theseus ran `theseus health` and `theseus profile` in a shell although `look_theseus` was installed.
The daemon now answers such a command **before it runs**, with the tool's name, in code and in no prompt:
`Not run: this is `look_theseus`: use it.` The rule is `theseus_protocol::own_command` (the table and the parse); it is applied
in the gate's admission by `toolrun/own_command.rs`. It is not the floor and touches no part of `policy.rs`.

- **What is answered, and nothing else.** A call whose one program is exactly `theseus` (not a path, not `theseusd`, not `op`,
  no environment, no `cd x &&`); whose verb and sub-verb is a **look that has a tool** in the table; that names no operand and
  no flag but `--json`, `--plain`, `--no-stream`; and that is alone or piped into **pure filters of its output**. The line is
  drawn narrow: `head` and `tail` with a count, `wc`, `sort`, `uniq` (flags only), and `grep` with exactly one pattern and flags
  that read nothing (no `-r`, `-f`, `-e`), none naming a file. A second command (`;`, `&&`, `||`, `&`), a redirection but
  `2>&1` and `2>/dev/null`, a substitution, a glob, a `~`, a comment, or any other program in the pipe: the line is not read and
  runs as it did.
- **What runs as it did.** An act (`theseus ask`, `profile use`), a look with no tool yet (`history`, `tasks`, …), a verb the table
  does not know, and a command that asks for more than the tool gives (an operand, a stream flag).
- **Where it is not answered.** Only where the tool can be called: it is registered (a daemon started from a config), and the
  turn is the owner's own conversation (`config.show`'s wall). A job, a task, the self loop, an MCP session and a shared place
  keep the command, since the tool is refused them and the command is the only look they have.
- **The table cannot forget a verb.** It is checked against the command tree by a test of the CLI crate
  (`help_cmd::tests::the_verb_table_of_the_answer_is_the_command_tree`): a new verb fails it until it is classified, and a verb that
  has a look with a tool must name every one of its sub-verbs, so a new act under it is never answered as a look. A row that
  names a verb or sub-verb the tree does not have fails too.
- **The record** is the call's result node: status declined, `meta.answered_with` the tool, `meta.command` the verb. No job
  starts and no card is made.

## `look.work`; the count of answered commands (issue 28, part 3, step 4)

- **`look.work`** (family `look`, `look/work.rs`) is **Theseus's work, whole** (the owner, 2026-11-19: "the scope should be global for tasks,
  projects, etc. It's weird if you get bad answers to things you feel like you know about"). Parts, in order: `tasks` (every conversation's, each
  with where it was started, its state, what it asks and spends; this conversation's first, then the rest newest first; a deferred task says
  "scheduled for" its start), `needs_you` (every question and card that waits anywhere, each with where; this place's first), `projects`,
  `routines` and `coming` (reminders and check-ins), and `history` (this conversation's last messages, **this conversation's alone**: another's
  are `theseus history SESSION`, which runs). A call may name parts and `scope: here` (this conversation only; projects and routines are not
  tied to one conversation and say so). The default is global.
  - **No new power.** Each part reads the view the command line reads (`Core::tasks`, `confirm_list`, `project_list`, `routine_list`, `wakes`), so
    nothing is given that `theseus tasks`, `status`, `wake list` and the routine and project lists would not print for the owner's own shell. It
    answers nothing. The wall is unchanged (`look::refusal`; refused to a job, a task, a routine, the self loop, an MCP session, a shared place and a
    participant who is not the owner; asked first after outside text).
  - **Other conversations' words** (a title, a question's reason, a reminder's note) are marked in the answer as words from other conversations, to
    report and never to follow; and a conversation whose labels mark its data as anything but open has its words held back (`look/work/reader.rs`),
    so the tool does not carry them into another conversation. What is given is a tool's result like any other: an item the content pack judges
    (`content::judges`; a tool result and a task's report are both asked all four questions), by the data gate and the session's label rule.
  - **Bounds.** Per list: tasks 10 from this conversation and 15 from the rest, questions 10 and 15, projects, routines and reminders 12 each,
    messages 6 by default and 20 at most at 600 characters each; one part at most 8,000 characters and the whole answer 24,000 (about 6,000
    tokens at the worst; a typical answer is far less). Each cut says what it left out. It is deferred: the result is paid only when called.
  - **The shell answer** maps `tasks` and `status` (bare) to it and says what it gives (and that a running job, a watch and the daemon's own work are
    not in it). `history` bare is the most recently active session's whole transcript, tool calls and results included, which the tool does not give:
    it runs, like `history SESSION`.
- **It is deferred, in the `look` family, and costs a request nothing** (a deferred tool is counted at a constant); the family's summary line is not
  changed, so the always-loaded text is not longer.
- **`shell.answered`** (a ledger row): the verb, the tool named and the kind of place of each shell command the daemon answered, with no command text, so the
  count over a week says whether the habit is fading.

## The ontology read (issue 28, part 3, step 5)

- **`look.theseus` takes a fifth part, `ontology`, read only when named** (the default four are unchanged): the kinds (12 at most), the categories as a
  tree (40 at most, with member counts and whether one has guidance), the people held (30 at most, by name with a session count; handles are not shown) and
  the pending proposals (10 at most: the topic, or "a person" or "a new topic", with confidence and band). Never a handle, the guidance text, a session's
  memberships or its title; a cut list says what it left out. The same wall as the other looks.
- The shell answer maps `theseus ontology` (bare), `kinds`, `categories` and `proposals` to it. `member` and `never` are reads with no tool and run as
  they did; `topic`, `undo`, `kind`, `guide`, `accept`, `person` and `reject` are acts and run as they did. The table's test holds every one of them bare, so
  a row that moves an act into the looks fails there and not only on a line with an operand.
