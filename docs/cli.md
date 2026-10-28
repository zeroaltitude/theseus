# The `theseus` command line

`theseus` is a thin client of the `theseusd` daemon. It speaks the protocol over the daemon's Unix socket (or, with
`--spawn`, over a `theseusd --stdio` it starts for one command), prints the reply on stdout and diagnostics on stderr,
and gives `--json` to machines. This page lists every command, grouped by the job it does; `theseus help` shows the everyday ones and `theseus help all` every one. The crate guide,
[`crates/theseus/AGENTS.md`](../crates/theseus/AGENTS.md), says how each is built; [glossary.md](glossary.md) says what
the words mean.

## Getting help

- `theseus help` is the everyday screen: ask chat tui history watch status tasks answer stop agenda digest remind routine wake project thread places profile health, then the global options and a quick start. `theseus help all` lists every command, hidden ones included, one line each; `theseus help COMMAND` is a command's page. Every other command still parses and runs, and has its `--help`.
- The pattern: everyday actions are top-level verbs (`ask`, `chat`, `answer`, `stop`, `cancel`, `remind`); everything else is a noun group with its verbs under it (`wake pause`, `model replay`, `persona voice`, `memory index`).
- `theseus <command> --help` gives that command's full help; `theseus <command> <subcommand> --help` goes one level down.
- Global options, before or after the command: `--socket PATH` (or `THESEUS_SOCKET`), `--spawn [BINARY]`, `--json`,
  `--no-stream`, `--plain`.
- Exit codes: 0 ok, 1 a server or provider error, 2 usage, 3 cannot connect. `ask` adds 5 spend limit, 6 waits for
  approval, 7 refused, 8 cut by a limit, 9 stopped. `wait` exits 4 when it times out.
- Ids: a session, task, wake, job or node is named by its whole id or by a unique end of it (four characters at least;
  six for a wake or a node).
- Writes that change what Theseus does for the owner (answers, undo of a tightening, trust, publish, the standing
  orders, threads, routines, projects) are refused inside a job, whose shell carries `THESEUS_SESSION`; the operator
  runs them from their own shell.

On a terminal, bare `theseus` (no argument) opens a conversation in this directory, the same as `theseus chat`.

## Talk to Theseus

- **`ask`**: send one prompt as one turn and print the reply; reads stdin when PROMPT is omitted or `-`.
  `theseus ask -s SESSION "and then?"`; `--thread SLUG`, `-P PROFILE`, `--attach FILE`, `--json`.
- **`prompt`**: run an MCP server's prompt as a turn's input and print the reply, as `ask` does.
  `theseus prompt SERVER/PROMPT --arg name=Ada`; `theseus mcp` lists the prompts.
- **`chat`**: an inline conversation in this directory, new or SESSION's; runs `theseus-tui chat`.
  `theseus chat -c` continues this directory's last conversation, `-r` picks one.
- **`tui`**: the terminal UI: every session in one sidebar, what needs you answered inline. Runs `theseus-tui`.
  `theseus tui`.
- **`watch`**: follow a session live (text, tool calls, confirmations, context decisions) whoever started the turn.
  `theseus watch SESSION`, `watch --all`, `watch --interactive`.

## What needs me, and what is running

- **`status`**: what needs you, what works and what failed, in one screen. `theseus status`; `--short` is one line for
  a prompt, `--watch` keeps it current.
- **`tasks`**: tasks, one line each under its conversation, with the command that answers one that asks and a footer of
  counts. `theseus tasks`; `--watch`, `--graph`, `--json`; `theseus tasks forecast` is how often tasks finished inside
  their forecast. To find a task later: `theseus tasks --ended [--since 7d|2026-10-15] [WORDS]` lists the ended ones,
  newest first, one line each with how it ended, when, what it spent and its report's first line; `theseus tasks WORDS`
  searches every task (title, brief and report line, all the words, any case). With nothing open, `theseus tasks` says
  how many ended today.
- **`task`**: start a task from your word with no model turn. `theseus task start "write the changelog" --in-time 2h`.
  `theseus task show ID` prints one task whole (its brief, plan, children, spend, times, its whole report, and the
  commands that act on it now); `theseus task pause ID` holds a running task at its next loop and `theseus task resume ID`
  lets it go on (yours alone, refused inside a job). `theseus task rehome ID HOME` sends its report to another
  conversation or `thread:<slug>`; `theseus task reschedule ID --at WHEN | --in-time SPAN` moves a deferred start;
  `theseus sessions model SESSION PROFILE|--off` holds a session's turns to a profile or lets the hold go. `task` acts on one; `tasks` lists them.
- **`agenda`**: the future in one view: Now, Today, This week, Later, Recurring, Waiting on the world, Projects.
  `theseus agenda --watch`.
- **`digest`**: what finished, failed, was answered or woke since you last looked (an agent's answer is told as the agent's, not
  as yours). `theseus digest --since 2h`. A task that
  reported is named with its report's first line (up to five under one conversation, then the rest by title); a window
  too long to read whole says where it stopped.
- **`peek`**: a running job's last lines, once. `theseus peek JOB`.
- **`attach`**: follow a running job's output live until it ends; Ctrl-C detaches and the job goes on.
  `theseus attach JOB`.
- **`wait`**: block until a session needs you, settles or ends, then print its state and questions; the daemon owns the
  wait. `theseus wait SESSION --until blocked`; `wait --any`.
- **`budgets`**: where the money is: each open session's limit and its source, spent, reserved, held and available, each
  task's carve, and the totals. `theseus budgets`.
- **`sessions`**: list sessions with token totals, or open one to continue. `theseus sessions`; `sessions --state` lists
  each session's execution instead (state, turns, outstanding actions, budget); `sessions explain ID` is one in full;
  `sessions open`, `recompile`, `retire`, `reopen`, `nudge`.

## Answering what waits

- **`answer`**: answer any question waiting for you with one of its own options: a call, a spend limit (approve
  resets its spend to $0), a task change, an extension, a reminder; the turn it resumes is followed (`--no-wait`
  returns once the answer is recorded). With no id, it lists everything waiting. `theseus answer 7f3a approve --note "ok"`;
  `theseus answer 7f3a decline`; `theseus answer` alone lists. The daemon reads who runs it: from your own terminal it is
  your answer; from a process with no terminal, or under an agent (`[policy] agent_processes`), it is recorded and shown as
  that agent's (`answered by an agent (claude)`), and a question that spends money or changes AWS (a spend limit, an
  `aws.*` change) is refused: those wait for your terminal or the DM's card.
- **`dismiss`**: dismiss a failed task: it stops needing you on every surface and its record stays.
  `theseus dismiss ID`.
- **`retry`**: try a failed task again: a new task from the same brief under the same parent, with no more budget than
  the failed one had; the failed one is dismissed. `theseus retry ID`.

## Stopping something

- **`stop`**: halt what a session is doing (its running turn, its jobs, what waits on you), as Discord's `/stop` does.
  The conversation goes on. `theseus stop SESSION`; `--all` also cancels the tasks under it.
- **`cancel`**: cancel a pending wake so nothing fires, or stop a task and its jobs, by what the id names (`wake cancel ID` is the same for a wake).
  `theseus cancel ID`.
- **`shutdown`**: ask the daemon to stop cleanly (removes its socket). `theseus shutdown`.

## Wakes and reminders

A wake is a turn scheduled for a time; a reminder is a wake that posts a note.

- **`wake`**: pending wakes, soonest first, and the verbs on one. `theseus wake` (or `wake list [--session S]`); `wake pause ID [--until WHEN]`, `wake resume ID`, `wake snooze ID [WHEN]`, `wake cancel ID`.
- **`remind`**: a reminder with no model turn: WHEN and what to say. `theseus remind "fri 9:00" "send the invoice"`;
  `--every`, `--days`, `--until`, `--session`.
- A reminder that fired is answered with `theseus answer ID done` or `theseus answer ID snooze [WHEN]` (10 minutes by default); `wake snooze ID [WHEN]` moves a pending wake.
- **`watch-for`**: wait on the world with no model: a command, URL or file checked on a timer until a condition holds,
  then you are told. `theseus watch-for add "PR merged" --run 'gh pr view 123 --json state -q .state' --until 'equals MERGED'`;
  `watch-for rm ID`.
- **`routine`**: recurring work, each run a fresh task with its own budget. `theseus routine add "mon 08:00" "weekly review"`;
  `routine runs|pause|resume|run|update|rm`.

## Inspecting a turn

- **`history`**: a session's transcript: messages, tool calls with their gate decisions, results, anything waiting.
  `theseus history [SESSION]`; a cited handle (`m:a1b2c3`) shows that message whole.
- **`why`**: what a turn's model was shown and why each piece was there: the model and why routing chose it, each
  block of the compiled context, each recalled note, what was left out, the gate's posture per tool.
  `theseus why [TURN]`.
- **`why --short`**: how full the context is: the last compiled request against the model's window, a line per kind of
  part. It is the short form of `why`. `theseus why --short [SESSION]`.
- **`census`**: what the next request holds, counted part by part down to each tool. `theseus census --place cli`;
  `--exact` asks the provider's token counter.
- **`open`**: open a turn in the cockpit's inspector (flame graph, each loop's request and thinking, each tool call).
  `theseus open turn [TURN]`.
- **`inspect`**: which vault secrets a turn's requests carried, by name, per loop, with the model, and whether each was
  substituted before the request left (`[privacy] redact_outbound`). `theseus inspect [SESSION] [TURN]`.
- **`reach`**: where a node went: the compilations that held it and its copies in other sessions. `theseus reach NODE`.
- **`fork`**: a new session from any moment of another, by reference; the original is never written to.
  `theseus fork NODE`.
- **`rewind`**: put the files Theseus's own edit tools changed, and the conversation, back to before one of your
  messages. `theseus rewind --to NODE`.
- **`ledger`**: recent ledger rows (every turn, loop, provider call, state change, error). `theseus ledger -n 20 -k provider.call`;
  `ledger verify`, `ledger head`.
- **`postmortem`**: an incident's timeline from the ledger, as markdown. `theseus postmortem --since 6h`.
- **`mode`**: a conversation's mode, as Shift+Tab sets it: `ask`, `edits`, `plan` or `auto`; with no MODE, say the
  current one. `theseus mode plan`.

## Tools and policy

- **`tools`**: the toollets: class, backend, what policy does with each, calls so far. `theseus tools`.
- **`policy`**: "should have asked": make a tool ask first, undo it, or list every tool's posture and what set it.
  `theseus policy tighten proc.run`; `policy explain`, `trust`.
- **`content`**: a session's content labels: `injection` stops its calls that act, and `secret`, `confidential` and
  `personal` keep it off a China-origin model, until you remove them. `theseus content SESSION` lists them;
  `theseus content remove SESSION FLAG` (or `--all`) removes one, and is yours alone to run.
- **`mcp`**: the MCP servers the config attaches: state, tools, postures. `theseus mcp`; `mcp restart NAME`.
- **`extend`**: extensions: proposals with their state and frozen digest, and the loaded ones. `theseus extend list`;
  `extend revoke NAME`.
- **`aws`**: Theseus's AWS account: `bootstrap` plans its first stacks read-only and applies them on your yes.
- **`google`**: Google Workspace: `auth` is the one-time consent (run it from your own shell), `status` shows the account, the scopes granted and the tools on, `revoke` ends the grant.
  `theseus aws bootstrap`.

## Models

- **`profile`**: model profiles: list, or switch the live one. `theseus profile use glm`.
- **`model`**: models and what they cost, one group:
  - `model catalog`: the model catalog: context windows, output limits, prices per million tokens.
    `theseus model catalog`.
  - `model bench`: your own benchmark: which model your own turns need, measured by replaying them one rung down
    the routing table. `theseus model bench --week`.
  - `model replay`: a past turn: rebuild its first request byte for byte and send it to another profile.
    `theseus model replay SESSION TURN -P sonnet`.
  - `model prefix`: the prefix library: its report, picks, the cache writes they saved and the false fits.
    `theseus model prefix`.
- **`judge`**: Jev's judgments: the recent ones with pack, session, answers and cost. `theseus judge log`;
  `judge show ID`.
- **`packs`**: the ladder: each Jev pack's rung, share and why, its rollback rules and last rows.
  `theseus packs`; `packs promote|rollback`.

## Persona, owner and standing wishes

- **`persona`**: the persona as data: every persona, its voice, files and places. `theseus persona`;
  `persona add|soul|set|retire|default|edit|import|history`.
- **`persona voice`**: the voice a call speaks in; `persona voice audition` makes the greeting in several voices. `theseus persona voice`.
- **`persona rate`**: rate the latest message Theseus sent unprompted. `theseus persona rate up`.
- **`standing`**: the owner's standing orders, preferences and facts, kept in every prompt of their scope.
  `theseus standing add --kind order "no emoji"`; `standing revoke ID`.
- **`card`**: the owner card, proposed from imported history; each proposal is a question to keep, edit or decline.
  `theseus card`; `card propose|keep|edit|decline|scope|done`; `card keep ID --scope surface:terminal` says where a kept
  one holds, and `card scope ID everywhere` moves a kept one's record.

## Memory

- **`memory index`**: how far the index tender has read the store, and a search over what was said, run and read.
  `theseus memory index search "port 7433"`; `memory index status`, `memory index rebuild`.
- **`memory`**: recall: `memory search QUERY` runs recall's pipeline writing nothing; `memory recalled SESSION` shows what
  each turn would have recalled; `memory label`, `memory consolidate`. `theseus memory recalled SESSION`.
- **`ontology`**: the kinds of context, the categories with their guidance, and sessions' memberships; people.
  `theseus ontology categories`.
- **`import`**: bring past history in as imported sessions (closed and private): OpenClaw, Claude Code, Codex.
  `theseus import claude-code`; `import list`, `import erase --tag TAG`.
- **`retract`**: mark one node wrong: recall never serves it again. `theseus retract NODE`.

## Threads, places and projects

- **`thread`**: lines of work on `thread:<slug>`, each with a session of its own. `theseus thread`; `thread new|show|edit|
  pause|resume|done|reopen|switch|seal`; address one with `ask --thread SLUG`.
- **`places`**: each place Theseus speaks in, and its class (private or shared). `theseus places`.
- **`publish`**: put one item (a node, a file, or `--text`) into a place's conversation as your message.
  `theseus publish --to '#general' --text "hello"`.
- **`project`**: long-term work with a goal and dated milestones (also `projects`). `theseus project new "Launch"
  --milestone "ship@fri"`; `project show|attach|detach`.
- **`herdr`**: the herdr terminal workspace manager: `herdr sync` gives each session that needs you or is working a
  pane; `herdr up`, `answer`, `main`, `setup`. See [herdr.md](herdr.md).

## Daemon and admin

- **`health`**: server health: version, live profile, providers, sessions, turns, errors, token totals. `theseus health`.
- **`self`**: self-improvement: what Theseus changed about itself, halt and resume. `theseus self log`; `self halt`; `self patterns [--top N]` (the pattern miner's recurring tool-call sequences, read-only).
- **`rpc`**: send a raw JSON-RPC request. `theseus rpc health`.
- **`help`**: print the command list or a command's help. `theseus help ask`.
