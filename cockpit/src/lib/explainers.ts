// What each panel is for (theseus-onxwh): the words behind the "?" at a panel's top right, in one place, keyed by the
// panel's `help` id (`<Panel help="bridge.pulse">`). Each says the Goal (the question the panel answers), How to read it
// (the axes, units, colours, and what one point, bar or row is) and Why it matters (what to do when it looks bad). A new
// panel adds an entry here; `test/explainers.test.ts` fails a panel whose id has none, and an entry no panel uses.
// Pure: no imports.

export interface Explainer { goal: string; read: string; why: string }

export const PARTS = [['goal', 'Goal'], ['read', 'How to read it'], ['why', 'Why it matters']] as const

const e = (goal: string, read: string, why: string): Explainer => ({ goal, read, why })

export const EXPLAINERS: Record<string, Explainer> = {
  // ---- the Ship
  'ship.watch': e(
    'Answer the six questions an operator asks first: what is working, what waits for me, what is slow, what has it cost today, what went wrong, and what happened while I was away.',
    'Each plate is one question with its number. “Show” (or the keys 1 to 6) lights just those ships on the chart; “since you last looked” counts what happened while this browser was away, and can replay it on the time machine. Minimize slides the whole panel away; the tab on the right edge brings it back.',
    'It is the quickest way to see that something needs you. A waiting approval, a slow job or a spend beyond the day’s usual is a reason to click through to that session before it costs more.'),
  'ship.depth': e(
    'Say how close the camera is to the work, and move between the four depths.',
    'Fleet is every place and session; Ship is one session with its turns and tool calls; Turn is one turn’s model calls; Call is one call’s data. The lit dot is where you are. Scrolling the chart moves it too.',
    'When a ship looks wrong at fleet depth, go down a depth at a time to find the turn and the call that caused it, instead of searching the ledger.'),

  // ---- the Agenda (theseus-p17fd)
  'agenda.week': e(
    'Show the week ahead on one strip: what is due, what runs, what is being watched and what starts, with the week just gone for the routines.',
    'Seven columns, a day each (d zooms to one). Behind, your meetings in each calendar’s colour, a sensitive one hatched with a lock (⇧R hides its titles). Right: Theseus’s marks (● reminder, ○ check-in, │ run due, ✓ ✗ runs, ▷ a start, ⚑ milestone). Left: thin bars for what ran. ◆ is a clash; the cyan line is now. A click opens its verbs; drag a reminder or start to move it.',
    'It is the one place to see every horizon at once. A crowded hour, a run that failed, or a reminder at the wrong time is cheaper to fix here than after it fires.'),
  'agenda.routines': e(
    'Show each routine with its schedule, its last run and what it costs, so you can tell a routine that earns its keep from one that does not.',
    'One row a routine: when it runs next, how its last run ended and what it cost, its 30-day record, the month spent against its limit, and a line of the cost of each of its last runs. The arrow opens the last 30 runs.',
    'A routine that fails, or whose cost line climbs, is spending your money for nothing. Pause it, or open its last run to see why.'),
  'agenda.watches': e(
    'Show what Theseus is waiting on in the world: each watch, its condition and the last value it saw.',
    'One row a watch: what it checks, the condition it waits for, the last value and when it was seen, how many checks it has made and when it gives up. A red mark says why its last checks failed.',
    'A watch that has failed three times in a row is checking nothing. Cancel it or fix what it looks at.'),
  'agenda.projects': e(
    'Show the projects Theseus is carrying, how far along each is and what is due next.',
    'One row a project: its goal, a bar of steps done over steps in all, milestones done, the next milestone with its date, anything overdue and when its review is.',
    'An overdue milestone is the first sign a project is slipping. The next date tells you what to look at before it does.'),
  'agenda.wakes': e(
    'Show the wake-ups Theseus has set for itself and for you, with a way to pause or cancel each.',
    'One row per wake: the note it will act on, its short id, the session or task it belongs to, its state, when it is due and how long from now, and a series when it repeats. Pause holds it; Cancel removes it.',
    'A wake that never fires, or one that fires too often, explains a session that is silent when it should work, or busy when it should rest.'),

  // ---- the Threads (theseus-5i8i)
  'threads.list': e(
    'Show each live thread as one group, with what is open under it and what waits on you.',
    'A header is the thread: its title (a lock when the owner has sealed it), its state, whether its cache is warm or cold, how many things need you, then its last activity, context size and dollars today. Under it are its open plans, steps, questions and wakes; click one to open its session. A seal waiting on you is the red line. A paused thread keeps its count but leaves the board’s.',
    'A thread that needs you is where to go first. A cold thread with a large context costs more to wake than a warm one, so a message there is worth batching.'),
  'threads.thread': e(
    'Say what one thread is, where it stands, and where it came from.',
    'The title is the thread’s; its goal, topics and standing follow. The size of its context and today’s dollars come from its session. “Under”, “Split from” and “Merged into” link to the threads it came from or went into.',
    'It tells you whether a thread is cheap to continue, and which thread to open when one has been split or merged.'),
  'threads.card': e(
    'Show the thread’s card: where it stands, in a few lines.',
    'Each line is one fact the thread keeps. The card is written by the thread’s own work and by you; the model reads it at the head of the thread’s messages.',
    'If the card is wrong the thread will start from the wrong place. Edit it with `theseus thread edit`.'),
  'threads.workingset': e(
    'Show what the thread touches: roots, repos, branches, references, files and hosts.',
    'Each line is one kind of thing and the names the thread holds. Nothing is guessed: these are the record’s own.',
    'The router and the context read the working set to decide what belongs to this thread; a missing repo here is a message sent to the wrong thread.'),
  'threads.work': e(
    'List the thread’s open plans, steps, questions and wakes.',
    'The mark is its attention: ● needs you, ◐ working, ○ ready. Click one to open its conversation.',
    'A question here is waiting on you; a step or a task that has failed stays until someone looks.'),
  'threads.turns': e(
    'Show the last few messages of the thread’s session.',
    'Oldest first, newest last: what you said and what Theseus answered, with when.',
    'A quick way to see where the thread stopped without opening its session.'),

  // ---- the Bridge
  'bridge.latency': e(
    'Show how long the model takes to answer, call by call.',
    'Across is time; up is milliseconds on a log scale (each gridline is ten times the one below). A ring is a call’s first token (how soon it began to answer) and a dot its total time; the colour is the model. The two hairlines are the range’s medians.',
    'A cluster that drifts up, or a model far above its usual line, means the provider is slow or the prompts have grown. Slow first tokens with ordinary totals point at the provider; long totals at long answers.'),
  'bridge.flow': e(
    'Show where the money went, by provider, model and session.',
    'A flow from left to right: each provider splits into its models, each model into the sessions that spent on it. A band’s width is its dollars in the chosen range. Click a session to open it.',
    'A wide band into one session is where to look first when the bill is higher than expected: open it to see which turns spent, and whether the model was the one you meant.'),
  'bridge.start': e(
    'Show how long the daemon’s last start took, phase by phase.',
    'Each bar is a start phase, measured from the process starting, on a log axis of milliseconds. Phases before serving are in the accent; later ones are grey. The lines mark the budget and the moment the daemon began answering.',
    'The daemon promises to answer within a fixed budget after it starts. A phase that grew is where startup work crept in front of serving.'),
  'bridge.turns': e(
    'Show every finished turn: when it ran, how long it took, and what it cost.',
    'Across is time; up is the turn’s duration on a log axis; a dot’s area is its cost. A triangle is a turn that failed. Click a dot to open its session.',
    'Big dots high up are the slow, expensive turns. A run of triangles means a failing tool or provider; open them to read the error.'),
  'bridge.tools': e(
    'Show how each tool is doing: how often it is called, how often it works, and how long it takes.',
    'One row per tool: its calls, the share that succeeded, its median and 95th-percentile time, and a bar of its total time on one shared scale. A failure or denial is named in words with a mark.',
    'A tool with a falling success share or a long 95th percentile is slowing every turn that uses it. A denial means the gate refused it: check Policy before assuming a bug.'),
  'bridge.context': e(
    'Show which sessions carry the biggest prompts.',
    'One bar per session: its latest estimated prompt size in tokens, on one scale, largest first. The small line beside it is its size at each compile; a drop is a recompile that shrank it. Click to open the session’s context.',
    'A big prompt costs more on every call and leaves less room for the work. A session near its window needs compacting or a fresh start.'),
  'bridge.mix': e(
    'Show what kinds of tokens the newest model calls used.',
    'Each column is one of the newest 48 calls, oldest to newest. Its height is its tokens, stacked by kind: input, cache reads, cache writes and output, in the same colours as the money river.',
    'Cache reads are the cheap tokens. A column that is mostly fresh input where there used to be cache reads means the cache missed, and the call cost more than it should.'),
  'bridge.pulse': e(
    'Show the rhythm of everything the daemon records.',
    'One row per family of ledger event, all on the same time axis. Each column is a time slice and its height is the number of rows, scaled for that row alone so a quiet family’s rhythm still shows. The cursor reads every family at once.',
    'A flat row where there was activity, or a burst in the fault family, tells you when something started or stopped. Use the time to choose a range or open the Ledger.'),
  'bridge.fleet': e(
    'Give the state of every execution and the sessions active most recently.',
    'The bar is the executions by state, each segment’s width its share, with the words and counts below. The list is the six most recently active sessions with their state. Click one to open it.',
    'Executions waiting or failed are the ones that need a person. If the count of running turns is at the ceiling, new turns queue.'),
  'bridge.fuel': e(
    'Show how much room each provider’s rate limits leave.',
    'A meter per provider for requests and for tokens: the fill is what is left of the limit, from the newest call’s response headers. It turns amber, then red, as headroom shrinks.',
    'A provider near its limit will start to refuse or slow calls. Spread the work out, or lower the load, before it does.'),

  // ---- the Fleet
  'fleet.list': e(
    'List every session or execution, so you can find one and open it.',
    'One row per session (or per execution, with the switch above): its state, title, place (where it came from: the CLI, a direct message, a channel), cost and last activity. Sort or filter at the top; click a row to open its deck. Executions show their queue, budget and a cancel.',
    'This is where to find the session that is stuck, silent or spending. Cancelling an execution here stops a turn at once.'),
  'fleet.graph': e(
    'Show who started whom, and who reports to whom.',
    'Each node is a conversation or a task (an agent Theseus started); an arrow runs from the one that started the other. Click a node to open its conversation.',
    'It shows why something is running: a task nobody remembers starting has a parent here. Follow the arrows up to the conversation that owns it.'),

  // ---- Actions
  'actions.approvals': e(
    'Collect every tool call waiting for your decision.',
    'One card per call: the session, the tool, what it would do and why it asked. Approve or decline.',
    'The session is paused until you answer. A long list means work is blocked on you.'),
  'actions.calls': e(
    'Follow each tool call through its life.',
    'One row per call, newest first: planned, then authorized, dispatched and settled, with its tool, its session and its outcome. Pick an execution to narrow the list to its calls.',
    'A call stuck before “settled” may have been lost in a crash or may still be running. A denied or failed call explains why a turn went the way it did.'),
  'actions.wakes': e(
    'Point to the wake-ups Theseus has set, which moved to the Agenda page.',
    'One line: how many wakes are set and when the next is due. Click it to open the Agenda, where each wake keeps every field it had here and its Cancel.',
    'A wake that never fires, or one that fires too often, explains a session that is silent when it should work, or busy when it should rest.'),
  'actions.postures': e(
    'Show how each tool is treated by the gate, and tighten it.',
    'One row per tool with its posture: open (runs), notify (runs and tells you) or approve (asks first). Tighten makes it ask more; untighten undoes your tightening.',
    'Tightening a tool is the fast way to make Theseus ask before something risky. The floor, the things that always ask, cannot be loosened here.'),
  'actions.families': e(
    'Show which families of tools each kind of place loads, and which it leaves for the model to ask for.',
    'One tab per kind of place (the CLI, a direct message, a channel, a task, a headless run). Each family is loaded or deferred, with the reason (a default for the place, used in a share of its turns, or unused for days) and its use in the window: turns that used it, searches that found one of its tools, and misses, a call to a tool that was not loaded.',
    'Every request reads the tools it loads, so a family nobody uses costs tokens on every call. A miss is a deferred family someone needed: it is what moves a family into the loaded set.'),
  'actions.tasks': e(
    'Show the plans and steps Theseus is tracking.',
    'One row per plan or step: its state, owner, claim and version, with a waiting change and its card where there is one.',
    'A plan or step that is claimed but stale is work nobody is doing. A waiting change to its goal needs your answer.'),
  'ops.board': e(
    'Show every live piece of work by what it needs: your answer, its work, a pause or a queue, a time today, or its report.',
    'Five columns: Needs you (the longest waiting first), Working, Paused·queued, Scheduled (deferred starts, wakes and routine runs due today; its counter reads “scheduled today”) and Done (today, then older, fifty at a time). A card is a plan, a step, a task, a conversation, a job or a wake, with its progress line, its place (where it came from) and the model it runs on. A step not yet started is to do and has no column. j and k move along a column, h and l across.',
    'It is the one place that says whether a piece of work is still going. A task waiting on a long command is working, and a paused one is held until you resume it.'),
  'ops.graph': e(
    'Show how the work hangs together: which thread holds which plans and steps, and which task is carrying out which.',
    'Each thread is a swimlane with its head card, and everything it started hangs in it; Home is the lane for work that belongs to no thread. A plan is a card with a fold and “n of m” done; a step is a card with a checkbox glyph; a task is a badge strip on the step or plan it carries out; a job is a pill. A colour says the state: needs you, working, to do, paused·queued, scheduled today, at rest or done.',
    'It shows why something is running and what it is part of. A plan stuck at 2 of 5 points at the step that holds it, and a task nobody remembers starting sits on the card it belongs to.'),
  'ops.words': e(
    'Say what the page’s words mean: conversation, thread, plan, step, task and job.',
    'A conversation is where you talk with Theseus in one place (a DM, the CLI, a channel, the cockpit), one turn at a time. A thread is a named line of work with its own conversation. A plan is work broken into parts, with steps and smaller plans in it. A step is one piece of work, checked off when done. A task is an agent Theseus starts to carry out a step or a plan on its own, with its own budget. A job is a command run for a turn or a task, such as a build or a test run.',
    'A task is only ever the running agent: what is stored and shown as work to do is a plan or a step. Also: a turn is one run of the model from a message to its reply, an ask is anything waiting on your answer, and a wake is a time Theseus starts work by itself.'),
  'ops.queue': e(
    'List everything that waits on you, in one place, the longest waiting first.',
    'Each card is one thing to answer: an approval, a budget, a change to a step’s goal, a question, a failure, a seal, or a message waiting for its thread. Its glyph and word say which; the bar under its top says how much of its time is left. Tab moves to the next; Enter lays out the whole card; y, n, 1 to 6, r and d answer the one under the cursor.',
    'A question that waits holds its work. A new one slides in where its age puts it, so the cursor never jumps away from what you are reading.'),
  'ops.inspector': e(
    'Show the selection whole: its facts, its questions and its verbs.',
    'A plan, step, task, job or conversation shows every field the Board has for it, its questions answered in place, and its verbs (pause, stop, cancel). A thread shows its goal, card, working set, open work and recent turns with its verbs. A queue item shows its full question.',
    'It is where you act on one thing without leaving the page: every verb is one call to the daemon, as the same act from any other surface.'),
  'tasks.new': e(
    'Start a task from the page: an agent that carries out a piece of work on its own.',
    'A brief, an optional title and budget, and the conversation it goes under (your Home when none is chosen). It starts the task at once, with no model turn, and scrolls to its row.',
    'A task works on its own with its own budget and reports back to where it came from, so a long piece of work does not hold a conversation.'),
  'actions.taskgraph': e(
    'Show the plans and steps as a tree, and as a graph.',
    'The tree nests each plan or step under the one it belongs to. The graph view draws the same plans and steps as nodes and arrows; click a node for its detail. It shows the present only.',
    'It makes a stuck chain visible: a step waiting on another shows as an arrow into the one that holds it.'),

  // ---- Boundaries and Policy
  'bounds.gate': e(
    'Show the approvals waiting and the rules you have tightened.',
    'The waiting approvals with their buttons, and below them every tightening (“ask like this in future”) with its undo.',
    'The gate decides what runs without asking. Review the tightenings now and then: an old one can ask for things you no longer care about.'),
  'bounds.sandbox': e(
    'Show the jobs running in the sandbox, and what finished ones left behind.',
    'Live jobs list their command and how long they have run. Finished jobs show what they left in scratch; cancelled jobs show the verdict that stopped them.',
    'The sandbox is the hard wall around a job. A job that stays long, or leaves a lot behind, is worth a look.'),
  'bounds.broker': e(
    'Show which program or tool is handed which secret.',
    'One row per grant: who it goes to, the secret’s name (never its value), how it is handed over, and how many times it was used. Withholdings are listed too.',
    'A secret in more places than you expect widens what a mistake can reach. Remove a grant here from the config.'),
  'bounds.places': e(
    'Show each place Theseus reads and speaks in, and what it may do there.',
    'One row per place. “Private: everything” means only you can read it, so every tool is allowed; “shared: public tools” means others can see it, so only public tools run. A warning names anyone else who can view a private channel; a ceiling narrows what the place may do.',
    'The place rule keeps private material out of shared rooms. A private place others can view, or a ceiling that is too loose, is the first thing to fix.'),
  'bounds.egress': e(
    'Show which hosts sandboxed jobs reached, and which they were refused.',
    'The counters give connections, bytes up and down, and refusals since the start. Below, one row per host a job reached (connections and bytes) or was refused, with the job and the reason.',
    'Sandboxed jobs may reach only the hosts allowed to them. A refusal you did not expect means a job tried to go somewhere it should not, and a host you did not expect among the reached ones is worth a question.'),
  'policy.tightenings': e(
    'List the tightenings: “make actions like this ask in the future”.',
    'One row per tightening with the tool and the condition it applies to, and an undo.',
    'Each one makes Theseus ask where it would have run. Undo one when it is only adding noise.'),

  // ---- the Ledger
  'ledger.kinds': e(
    'Show what kinds of rows the record holds, and how many of each.',
    'A treemap: each tile is a kind of row, its area the number of rows, grouped under its family’s header. Click a tile to filter the list to that kind.',
    'A kind that suddenly dominates means something is looping or logging heavily.'),
  'ledger.overtime': e(
    'Show when the rows were recorded.',
    'Across is time, in slices of a ninetieth of the record; a column’s height is the number of rows, with the faults drawn on top in the fault colour. Drag across it to filter the list to that time.',
    'Bursts and gaps show when the daemon was busy, stopped or failing. Brush a burst to read exactly what happened.'),
  'ledger.rows': e(
    'List the ledger’s rows, the append-only record of everything Theseus did.',
    'One row per record, oldest to newest: its position, time, kind and a summary. Filter by kind, session or text; “follow” keeps the newest in view. Click a row to read it whole.',
    'The ledger is the truth: when a view and the record disagree, the record wins. Search it to answer “what exactly happened?”'),
  'ledger.row': e(
    'Read one ledger row whole, as it was recorded.',
    'The row’s fields and its raw payload.',
    'It is the evidence behind a summary line: use it to check what was really recorded.'),
  'ledger.nodes': e(
    'List what was said and done, node by node.',
    'One row per node of the store, newest first: its session, its kind and a preview. Click one to read it.',
    'Nodes are the conversation and the tool results themselves. Read them when the ledger says a turn happened and you want to see what was said.'),
  'ledger.node': e(
    'Read one node whole, as stored.',
    'The node’s fields and its body. It is read only.',
    'It is the exact text the model saw or produced, with nothing summarised.'),

  // ---- Money and Economics
  'money.river': e(
    'Show where every dollar went.',
    'A flow from left to right: sessions into the profiles and models they ran on, into the kinds of tokens they bought (input, cache reads, cache writes at five minutes and at an hour, output), into the sea, the total spent. A band’s width is its dollars. Sessions past the top few fold into “others”.',
    'It shows at once which session, model and token kind is expensive. Wide cache-write or fresh-input bands where cache reads should be point at a prompt that keeps changing.'),
  'money.pace': e(
    'Show how fast money is being spent now.',
    'The dial is dollars an hour, on a scale that grows with the pace (a dollar, ten, a hundred). The columns below are the last hour in five-minute slices, each one’s dollars.',
    'A pace far above your norm means a runaway loop or an expensive model. The budgets and the daily ceiling will stop it, but the dial tells you first.'),
  'money.budgets': e(
    'Show every spending limit, and how much of it is left.',
    'One row per budget: where its limit comes from, what is held for calls in flight, what is left, and what it has spent over its life. Tasks sit under their session with their carve. Below: the burn per hour, recent resets, the judge’s day and the AWS hands.',
    'A budget near its limit makes that session ask or stop. Raise it here before it blocks work you want done.'),
  'money.questions': e(
    'Collect the budget questions waiting for you.',
    'One card per question: which session or task reached its limit and how much more it needs. Answer with the card’s buttons.',
    'The session holds at its limit until you answer, so each is work waiting on you.'),
  'economics.spend': e(
    'Show spend over time, model by model.',
    'Two small charts on one time axis, each on its own scale: dollars, and tokens, with a column per time slice stacked by model in each model’s colour.',
    'A model that grows to dominate the stack is where the money is going. Compare with latency to judge whether it is earning it.'),
  'economics.kinds': e(
    'Show what kind of tokens the money buys.',
    'The total, then one bar split by token kind: input, cache reads, cache writes, output. Each kind’s share is its width.',
    'Output and fresh input are the expensive kinds. A large cache-write share means prompts are rewritten rather than reused.'),
  'economics.tree': e(
    'Show spend by provider, then model, then session.',
    'Horizontal bars on one scale: each provider’s total, its models’ bars in their colours, and under each model its sessions.',
    'It answers “who spent it?” down to a session. Start at the longest bar.'),
  'economics.sessions': e(
    'Show what each session cost.',
    'One horizontal bar per session on one scale, stacked by model. The title says when only the top sessions are drawn; the table lists them all.',
    'The long bars are the sessions to review. A session that cost a lot for little is a candidate to retire or cap.'),
  'economics.latency': e(
    'Show how fast each model answers.',
    'A strip per model on one log axis of time: a ring above the line is each call’s first token, a dot its total.',
    'A model that is both slow and expensive is the first to replace. A wide strip means its speed is unpredictable.'),
  'economics.caching': e(
    'Show how well the provider’s prompt cache is paying, profile by profile.',
    'One row per profile: of its input, the share read from the cache, the tokens written to it, and the dollars saved at the catalog’s prices, net of what the writes cost over plain input. A profile whose writes cost more than its reads saved says so with a mark and a word.',
    'A low read share, or a net loss, means prompts change early or the cache expires between turns, so you are paying to write what is never read.'),

  // ---- Speed
  'speed.start': e(
    'Show how long the last start took, phase by phase.',
    'Each bar is a phase from process start on a log axis of milliseconds; the lines mark the budget and the moment the daemon began answering.',
    'Starting fast is a promise. A phase over its line is startup work that should have moved after serving.'),
  'speed.starts': e(
    'Show how long every start has taken.',
    'Across is time; up is the time to start answering, on a log axis. A dot within the budget is in the ok colour; a triangle over it is in the fault colour.',
    'A rising run of starts means the daemon is getting slower to come up, usually with the size of its history.'),
  'speed.overhead': e(
    'Show the harness’s own time in each turn.',
    'One column per traced turn: the turn’s time less its model calls and tool runs, split into the disk commits and the rest. Taller is slower.',
    'Time the harness adds is time every turn pays. A tall column is a turn where Theseus itself, not the model, was the wait.'),
  'speed.commits': e(
    'Show how long writes to the record take.',
    'A histogram: each column is a range of commit times in milliseconds (the last one is “500 and over”), its height how many commits fell in it. One fsync (a disk sync) a frame, as the panel’s heading says.',
    'Every write waits for the disk. A heavy right tail means a slow or busy disk, which slows every turn.'),
  'speed.benches': e(
    'Show whether each of the speed promises has held, run after run.',
    'One small chart per bench (start, clean stop, crash and restart, binary swap, a plain turn, frames a plain turn (its writes)): each run’s 95th percentile, with the gate’s limit and the README’s budget as lines; a failed run is a triangle.',
    'A run near or over its line means a change made something slower. The gate refuses it, so this shows how close to the edge each promise runs.'),

  // ---- Benchmarks
  'bench.yourwork': e(
    'Say which model your own turns need, measured on your own past work.',
    'One row per model and effort your turns ran at: how many turns and what they cost, the turns you checked (you corrected their routing), and, once the nightly sampler has replayed some one level down the routing table, how many a judge called the same or better, with the lowest match rate that holds at the stated confidence and the dollars moving the row down would have saved. The last line is the replays’ own cost against its day cap.',
    'It is a yardstick, not a switch: nothing here moves routing. Read it before you cheapen a profile, and when a row is expensive and keeps matching one level down, ask whether that work needs the dearer model.'),
  'bench.frontier': e(
    'Show where Theseus sits among the harnesses it was measured against.',
    'Each point is a harness in one run on one task set, on the two properties you pick (each with its better direction). The points joined by the line are the frontier: none beats them on both. Dimmed points are beaten. Click a point for its report.',
    'A harness on the frontier is not beaten on those two measures. Use it to see the trade: more accuracy for more cost or time.'),
  'bench.runs': e(
    'List every benchmark run that was published, newest first.',
    'One row per run with its date and headline numbers. Click one to read its report whole.',
    'It is the record behind the frontier: open a run to see how a number was measured.'),
  'bench.differs': e(
    'Say what differs between the harnesses compared on this page.',
    'A note on the comparison: the settings, models and tools that differ between the harnesses.',
    'A comparison is fair only when you know what was held equal. Read this before reading the chart as a ranking.'),
  'benchrun.report': e(
    'Read a benchmark report whole.',
    'The report’s sections, tables and figures. Each figure has a chart and a table toggle; the table is the figure’s own numbers.',
    'It is the evidence for the frontier’s points, with how the run was done.'),
  'benchrun.data': e(
    'Show the data file a report was made from.',
    'The run’s machine-readable results as published, shown whole.',
    'Use it to check a figure in the report, or to reuse the numbers.'),
  'benchrun.trials': e(
    'List the run’s trials, one row each.',
    'One row per trial: the harness, the task, its outcome and cost.',
    'A single surprising row often explains a surprising average.'),

  // ---- Judgment
  'judgment.main': e(
    'Show how Jev, the judge inside the loop, is doing, check by check.',
    'One row per check (a named, versioned judgment such as loop.v1 or classify.v4): its level (shadow = Jev answers but nothing acts on it; live = Theseus acts on the answer) and version, its calls and cost, and the 50th and 95th-percentile time per class of work. Under the ontology check’s row, one line counts its judgments, the ones that acted (filed a category or attached a project), the ones that only asked, and the writes you took back, with the undo rate over the acts.',
    'A check in shadow is being tested before it can act. Slow or costly judgments are paid for on every turn they run in.'),
  'judgment.log': e(
    'List the judgments Jev made, so you can find and read one.',
    'One row per judgment: its check, conversation, question and result, newest first. Filter by check, conversation or time; “N of M+” says more matched than were counted.',
    'It is the record of Jev’s calls. Read the ones you disagree with and label them.'),
  'judgment.one': e(
    'Read one judgment whole: what was asked, and how sure Jev was.',
    'The judgment’s state as fields, and each answer as a probability bar; buttons let you label it right or wrong.',
    'Your labels teach the learning loop which calls were right. A confident wrong answer is the most useful to mark.'),
  'judgment.ladder': e(
    'See how far each of Jev’s checks is trusted (watching only, or acting), and move it up or back.',
    'One row per check (e.g. loop.v1 decides when to nudge an unfinished turn; classify.v4 sorts each message). Its badge says its level: shadow = Jev answers but nothing acts on it; live = Theseus acts on the answer. The rules under it are its safety limits (e.g. how many labels a day). Promote moves it one level up; Roll back moves it down at once.',
    'A check only starts steering Theseus once it has proved itself in shadow. If one starts making bad calls, roll it back immediately; its earlier level is restored.'),
  'judgment.corrections': e(
    'Show your corrections of how messages are routed to a model.',
    'One row per correction: the message it was made on and the profile you steered it to.',
    'Corrections steer similar messages next time. Remove one that is no longer right.'),
  'judgment.versions': e(
    'Show the versions the learning loop wrote for each check.',
    'One lineage per check; pick any two versions to see what differs. Buttons promote or reject one.',
    'The loop proposes changes to its own judgment; none acts until you promote it.'),
  'judgment.learning': e(
    'Show what the learning loop found.',
    'A report by date: what it labelled, learned and proposed. Pick a date for the report. The ontology check’s block has a line of its own for that day: its judgments, acts, confirms and undos, and the undo rate per kind.',
    'It is where to review the loop’s work before it changes how a check judges.'),
  'judgment.notices': e(
    'Show the notices Jev sent after a call it was sure was risky.',
    'One row per notice: when, the tool, and the reasons with Jev’s probability; the session is a link. The pill says whether notices are on, paused or off.',
    'A notice is Jev saying an open call looked risky, and you can label it. Too many, or too few, say the judgment needs tuning.'),

  // ---- Systems
  'systems.daemon': e(
    'Show the daemon’s own state in one place.',
    'Name, version and protocol; uptime; sessions and turns; the model and its profile; the ledger’s rows; provider errors; total spend; the catalog version; the turns held against the admission ceiling; and the tokens used, split into input, cache and output.',
    'Read it first when something seems wrong: is this the build you think it is, how long has it been up, and is anything held or erroring?'),
  'systems.kernel': e(
    'Show the durable kernel’s counts.',
    'Whether it is accepting turns or held; the admission ceiling and the turns held; executions and actions by state; completions that matched no action; lingering wrappers; the per-session spend limit; and the steps of kernel startup with their times.',
    'Held turns mean the ceiling is full and new ones queue. Quarantined completions are results that matched nothing and are never guessed into place: a count above zero is worth looking at.'),
  'systems.laststart': e(
    'Show the last start’s phases.',
    'Each bar is a start phase measured from process start, on a log axis of milliseconds. Phases before serving are in the accent; later ones are grey.',
    'The daemon promises to answer fast after it starts. A long phase is startup work that should have moved after serving.'),
  'systems.config': e(
    'Show which config the daemon is running on, and whether it has taken the latest.',
    'The source (a file or a vault note) and its reference, the state, what the start was served from, when it was confirmed, and any restart onto a changed note.',
    'If the vault’s note did not load, the daemon keeps serving from its copy. “held” here says so, so you know an edit has not taken.'),
  'systems.secrets': e(
    'Show whether the daemon has the secrets it needs, by name only.',
    'The state, the method and rounds, and one pill per secret: green is ready, amber still resolving, red failed with the reason below. Values are never shown.',
    'Whatever needs a failed secret waits and never runs without it. A red pill is the usual cause of a provider that will not answer.'),
  'systems.children': e(
    'Show the processes the daemon started and whether they are cleaned up.',
    'Whether it is the subreaper; wrappers running and lingering; orphans and owned children; zombies; and how many wrappers and orphans have been reaped.',
    'Zombies or lingering wrappers that keep growing mean jobs are not being cleaned up, and will eventually crowd the machine.'),
  'systems.broker': e(
    'Show which secret is handed to which program or tool.',
    'One row per grant: who receives it, how (kind and variable), the secret’s name, its posture (confirm asks first), and how many times it has been used.',
    'A secret handed to more places than you expect widens what a mistake could reach. Grants come from the config.'),
  'systems.binding': e(
    'Show a chat binding’s connection and traffic.',
    'Its state; the bot and the servers it is in; when it connected and its latency; messages in, out and edits; interactions, ignored and errors; the outbox’s pending, sent and refused; each bound place with who may drive it; and the recent traffic.',
    'A binding that is down means nothing reaches you from that place. Errors, a growing outbox, or a place that is not mention-only where it should be are the things to check.'),
  'systems.approval': e(
    'Say where an approval can be answered, and show the recent ones.',
    'The rule: an answer counts only from a private place. Then the recent approval rows, and any tool tightened at run time with who did it and when.',
    'An approval asked where nobody private can answer waits for ever. Refused rows mean someone answered from a place that does not count.'),
  'systems.contextfiles': e(
    'Show the files that make up what every session is told first.',
    'The system-level files every session gets, and the persona in play with its files, or the personas defined when none is in play.',
    'They are put in front of the model on every turn, so a large one costs tokens every time and a wrong one changes every answer.'),
  'systems.profiles': e(
    'Show the model profiles, and which is live.',
    'One row per profile: its provider and model, its largest output, and whether it has a system prompt. The live one is marked; “make live” switches to another.',
    'New turns use the live profile, so switching it changes cost and quality at once.'),
  'systems.catalog': e(
    'Show every model Theseus knows, with its limits and prices.',
    'One row per model: its provider, context window and largest output, then US dollars per million tokens for input, output and the cache (read, write, and write for an hour), and whether it thinks. The table scrolls sideways when the card is narrow.',
    'Cost estimates and the money gate use these prices. A wrong price means a wrong budget.'),
  'systems.web': e(
    'Show whom the web UI has turned away.',
    'The refusals since the daemon started, by kind, whether the development origin is open and how many pages it served.',
    'An open development origin lets another page talk to the daemon: close it when you are not developing.'),
  'systems.binary': e(
    'Warn when jobs can write the daemon’s own program.',
    'A loud card appears only when the program file is writable by a job.',
    'If a job can replace the binary, the next start runs whatever it wrote. Fix the permission.'),
  'systems.disk': e(
    'Show whether the disk has room, and what the last sweep did.',
    'Free space against the warning and the floor below which new jobs are refused; and what the last sweep of the jobs’ raw output removed and kept, by reason.',
    'A full disk stops new work. Make room before it reaches the floor.'),
  'systems.extensions': e(
    'Show the extensions the model wrote and you approved.',
    'One row per extension with its manifest, digest, files, tests, who approved it, its calls and errors, and Revoke.',
    'Extensions run code the model wrote. Revoke any you do not recognise.'),
  'systems.hands': e(
    'Show the AWS hands, the workers it runs there.',
    'One row per group and a cell per hand coloured by state, with its cost against its cap.',
    'A hand near its cap, or one in a fault colour, is spending or failing out of sight.'),
  'systems.rpc': e(
    'Query the daemon by hand.',
    'Pick a read-only method, give its parameters as JSON and browse the answer.',
    'It is the same protocol the CLI uses, for checking what a view is built on.'),
  'systems.selfchanges': e(
    'Show what Theseus changed about itself.',
    'The kill switch’s state with a Halt button, then the newest changes with what, why, the numbers and an undo.',
    'Self-changes are the riskiest thing Theseus does. Halt stops them; only the owner can resume.'),
  'systems.startphases': e(
    'Show what each start phase found, and how much of the start nothing names.',
    'Serving time after the process started, any time between phases that no phase names, then one row per phase: when it began, how long it took and what it found.',
    'A slow start with an unnamed gap has an unnamed cause. Phases marked “after serving” do not delay answering.'),
  'systems.aws': e(
    'Show each AWS account Theseus can reach and whether it is safe to use.',
    'Per account: its state, who the account’s STS says it is, when it was checked, its region, requests and failures, its hands and hourly spend against the cap, and its durability (what is shipped off the machine).',
    'An account’s key is checked after serving: until it is, no call of that account signs. A failing durability line means the store is not being shipped off the machine.'),
  'systems.crash': e(
    'Say whether the store came through the last crash.',
    'A line for the history check, and for any crash file found. It is plain text, loud when a frame was corrupt.',
    'After a crash this says whether the record is intact, which is what everything else rests on.'),

  // ---- Ontology and People
  'ontology.kinds': e(
    'Say what kinds of classification Theseus keeps about its sessions.',
    'One row per kind: its name, its basis (given, or worked out), who assigns it, how many a session can have, its parent, its precedence, its rule and version, who added it, and a description. The table scrolls sideways when it is wide.',
    'Kinds are the dimensions the ontology sorts sessions by. A kind’s rule and precedence decide which category wins when more than one fits.'),
  'ontology.categories': e(
    'Show the category tree, and how many sessions are in each category.',
    'A tree: each category with its count of sessions, the origin that made it and, for a category a machine made, its confidence; at a large import’s size it starts folded at the roots. The origin buttons keep what one origin made, with the rows above it dimmed so a match shows where it hangs; unknown is the categories the ledger has no row for. Click one to read or edit its guidance beside it. The selected category and the origins picked are kept in the address.',
    'A session’s next compile takes guidance from the categories it is in. An overgrown tree, or a session in the wrong category, changes what it is told.'),
  'ontology.guidance': e(
    'Read, and change, the guidance a category gives to the sessions in it.',
    'The category’s guidance text. Saving shows exactly what will be sent and asks you first; the write is judged by the core like any other.',
    'Guidance is put in front of the model for sessions in this category. Changing it changes how all of them behave, from their next compile.'),
  'ontology.topicadd': e(
    'Add a new topic to the ontology.',
    'Give a name and where in the tree it goes. It shows what it will send and asks first.',
    'A topic is a place for sessions and notes to be filed. Add one when the existing topics keep fitting badly.'),
  'ontology.personas': e(
    'List the personas: who can answer a message, and the state of each one’s soul.',
    'One row per persona: its name, emoji and card, the default mark, and its soul as live, awaiting its soul, or over its cap with its tokens. Edit its soul opens a box that replaces the file (the old one is kept beside it); make default sets the one Jev’s “none” engages; retire takes it away. Each asks first.',
    'A persona’s soul sits in every DM’s cached persona layer, so it is kept small. Only you, from a private place, can write one, and a soul over the cap is refused with its count.'),
  'ontology.personaadd': e(
    'Add a persona by its key.',
    'Give a key, and optionally a name, an emoji, a card and a soul. With no soul the persona awaits one: it is listed and never picked, and you are asked for the soul once.',
    'Jev picks a persona per message from the cards of those with a soul. A key may not be none or prior: they are the question’s own options.'),
  'ontology.kindadd': e(
    'Add a new kind of classification to the ontology, such as expertise or culture.',
    'Give a name (a lowercase letter, then letters, digits, _ or -), and say who may assign it, how many a session holds, where it nests, its precedence and rule, and up to five examples. A fault shows under its field, in the words the daemon would refuse it in. The confirm shows the exact params it will send.',
    'A kind is a dimension the ontology sorts sessions by, and Jev can only file under the kinds the table lists. A new one is interpreted, so it needs no code; it cannot be deleted, only retired once nothing holds it.'),
  'ontology.growth': e(
    'Show how fast the ontology grows, and who is growing it.',
    'A column a day for 90 days, stacked by the origin that made the categories (you, the import, Jev, the sweep, the dream), a line for kinds added, and a ring on a day when a write made then was later taken back. The tip gives each day’s counts.',
    'Jev, the sweep and the dream write here on their own. A chart that climbs with rings on it says the machines are making categories you then undo.'),
  'ontology.minted': e(
    'List what Jev, the sweep and the dream made, with a way to take each back.',
    'One row per category a machine made, newest first: its origin, its confidence, the session it filed, when, and the judgment that made it (a link). Undo sends ontology.undo for that write after showing you its words; a taken-back category is struck through. A write that made several categories is undone whole.',
    'An undo restores the earlier member lists and retires the category, and is labelled wrong in the learning ledger, which is how the check learns what you did not want.'),
  'ontology.people': e(
    'List the people Theseus knows, so a message can be traced to a person.',
    'One row per person with their handles (the names they use in each place). Search by handle; click a person for their page.',
    'Who a message is from decides whose guidance and history apply. Two rows for one person split what Theseus knows about them.'),
  'ontology.person': e(
    'Read one person’s page: their handles, their sessions, and what they do.',
    'Their handles, the sessions they are in, and a role description of what they do and own. Merge folds another person’s entry into this one.',
    'Merge duplicates here, so their sessions and handles are one person’s. The description holds role facts, never opinions.'),
  'ontology.personadd': e(
    'Add a person to Theseus’s list of people.',
    'A name and a handle. It shows what it will send and asks first.',
    'A person needs an entry before their sessions can be grouped under them.'),
  'ontology.proposals': e(
    'Review the people and topics Jev proposes from the text it has read.',
    'One row per proposed person (or topic) with one accept and one reject; select-all accepts everything shown. A note says how many are hidden by your exclusions.',
    'Nothing is added to the ontology until you accept it.'),

  // ---- Context and Books
  'context.filters': e(
    'Narrow the imported episodes to the ones you mean.',
    'Search text, then groups to pick from: source, place, sensitivity, book hint and import, each value with how many episodes it holds; the month and topic you pick show as well. “Clear all” resets.',
    'The import can hold many thousands of episodes. Filters, kept in the address, let you share the exact view you are looking at.'),
  'context.episodes': e(
    'List the imported episodes, so you can read what Theseus can recall from the past.',
    'One row per episode: title, source, when it happened and its topics. Sort by newest, oldest or longest; the veil hides personal and confidential text until you open it; a page at a time. Click a row to open it.',
    'Recall reads these episodes as testimony. Check here what is in the import, and that private text is veiled.'),
  'context.topics': e(
    'Show the topics the imported episodes fall under.',
    'Each topic with how many episodes it holds, as a tree. Click a topic to filter the list.',
    'It shows what the past is about, and where much of it is unsorted.'),
  'context.asof': e(
    'Show when the imported episodes happened.',
    'Across is months; each column’s height is the episodes that began that month, on a square-root scale so small months stay visible (linear is in the header). 2y, 5y and all set the window. A dashed line is a long run of empty months. Episodes dated before 2000 are almost certainly bad import timestamps: they are counted in the undated chip and the table, not drawn. Click a month to filter the list to it.',
    'A gap is a period the import did not cover, so recall has nothing from then. The undated chip keeps the oldest episodes findable: click it to list them.'),
  'context.episode': e(
    'Read one imported episode: where it came from, what it says and how it was labelled.',
    'Its provenance, its summary and its messages, a page at a time. Personal or partner-confidential text is veiled until you open it.',
    'It is the evidence behind a recall: check that what Theseus remembers is what was said.'),
  'context.standing': e(
    'Show what the owner has told Theseus to hold to, and let them take it back.',
    'One group a kind: orders are followed, preferences honoured, facts known. Each record has its scope (everywhere, a session, a place, or a surface such as the terminal), when it began, where it came from, and how many compiles of its scope carried it. Revoke ends it at the next turn.',
    'These ride every prompt of their scope, so a stale one costs tokens and shapes every answer; a surface’s records ride only that surface’s turns.'),
  'context.card': e(
    'Show what Theseus proposes to add to the owner’s card from their own history, and let them answer.',
    'Each proposal is one question: a short fact or preference, how many of the imported episodes it rests on, and Keep, Edit or Not this. Keep writes a standing record; Not this is remembered, so those sources are never proposed again; an unanswered one becomes Not this after a while. At most a few dozen wait at once, and the job that makes them has a daily spend line.',
    'Nothing is added without an answer, so the card holds what the owner chose; what they decline is a label for the next proposals.'),
  'context.books': e(
    'Say honestly what state the books are in.',
    'A note: the books (dictionary, encyclopedia, cookbook, standing procedures, diary, casebook, register) are not compiled yet, and how many episodes carry the import’s guess at a book.',
    'Until they are compiled, recall reads the imported episodes themselves, not a distilled book.'),
  'context.hints': e(
    'Show how many episodes the import hinted at each book.',
    'One bar per book, its width the number of episodes; the table says what each book holds. Click a bar to list its episodes.',
    'The hint is the import’s guess, not a compiled book. A large “unsorted” share means the import could not place them.'),
  'context.pick': e(
    'Pick a session whose turn you want to look into.',
    'A list of sessions; choosing one opens its turns.',
    'It is where to start when you ask why a turn answered as it did.'),
  'context.turn': e(
    'Show what one turn put in front of the model, and why.',
    'The turn’s request in its parts with their tokens against the model’s window, how each was compiled, and why each recalled note was recalled.',
    'If the model missed something it should have known, this says whether it was in front of it or never recalled.'),
  'context.anatomy': e(
    'Show how much of the model’s window a turn used, and on what.',
    'One bar split into the request’s parts, each part’s width its tokens, against the model’s window. A tip names each part.',
    'A part that is too large crowds out the rest. The biggest segment is the first to cut.'),
  'context.parts': e(
    'List every part of the turn’s request.',
    'One row per part (the system block, guidance, tools, recall, messages) with its tokens and what it was.',
    'It names exactly what the model saw, and what each part cost.'),
  'context.recall': e(
    'Show what memory brought into this turn, and why.',
    'One card per recall: the notes it found, and the reason each matched.',
    'A poor recall brings in irrelevant notes. The reason tells you whether the query or the notes are at fault.'),
  'context.ask': e(
    'Ask the memory index a question, as recall would.',
    'Type a query; the answer lists the notes the index returns for it, with their scores.',
    'It lets you test what Theseus would remember before it matters.'),
  'books.chart': e(
    'Show the imported books and how many episodes each holds.',
    'One bar per book, its width the share of episodes, with the first-to-last span of each; the table has the same rows. Click a book to list its episodes.',
    'It shows which book is large and which is thin, and how far back each reaches.'),
  'books.book': e(
    'List one book’s episodes, newest first.',
    'One row per episode; filter by topic, source and place from the book’s own facets; a page at a time.',
    'It lets you read a book from its newest end, and find an episode by where it came from.'),
  'books.episode': e(
    'Read one imported episode in its book.',
    'Its summary, labels and messages.',
    'It is the source for what the book says, so you can check a fact against what was said.'),

  // ---- the session deck
  'deck.transcript': e(
    'Show the session’s conversation as it happens, turn by turn.',
    'Each turn with its messages, tool calls and results, in order; the transcript streams live and a turn opens in the inspector.',
    'It is the first place to look when a session did something you did not expect.'),
  'deck.draft': e(
    'Write the first message of a new session.',
    'Type the message and send it; the session opens with it.',
    'A new session has its own history and budget, so a fresh task starts clean.'),
}

export const explainerOf = (id: string): Explainer | undefined => EXPLAINERS[id]
