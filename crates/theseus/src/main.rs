//! `theseus`: the CLI. A thin client that speaks the protocol to `theseusd`
//! over its Unix socket, or spawns `theseusd --stdio` and talks over pipes.
//! Built for shells: prompt from an argument or stdin, streamed reply on
//! stdout, diagnostics on stderr, `--json` for machines, meaningful exit codes.
//!
//! Exit codes: 0 ok · 1 server/provider error · 2 usage · 3 cannot connect;
//! `ask` adds how its turn ended (`theseus_client::outcome`, theseus-n88g.2).
//!
//! `run` matches the subcommand; each one is a function in `cmd.rs`, and
//! what it prints is `render.rs`'s (theseus-0g4, finding 11). Since
//! theseus-7yx (step 10a) the connection (`client`) and the renderers
//! (`render`) are this package's library, which the TUI shares, and
//! `print.rs` writes the lines they return.

mod agenda;
mod answer;
mod board;
mod budgets;
mod card_cmd;
mod census;
mod chat_cmd;
mod cite;
mod cmd;
mod content_cmd;
mod digest;
mod extend;
mod follow;
mod fork;
mod google_cmd;
mod harness;
mod help_cmd;
mod herdr;
mod herdr_answer;
#[cfg(test)]
mod herdr_contract_tests;
mod herdr_keys;
mod herdr_main;
mod herdr_setup;
mod herdr_slots;
mod herdr_sync;
mod herdr_up;
mod import;
mod import_sync;
mod interactive;
mod jobout;
mod judge_correct;
mod judge_prove;
mod judge_runs;
mod keys;
mod ledger_verify;
mod mcp;
mod mode_cmd;
mod notices_cmd;
mod onto_cmd;
mod ontology;
mod open;
mod ops_cmd;
mod own_bench;
mod packs;
mod pages;
mod persona_cmd;
mod policy_explain;
mod postmortem;
mod prefix_cmd;
mod print;
mod project;
mod prompt;
mod remind;
mod retract;
mod retry;
mod rewind_cmd;
mod routine;
mod self_cmd;
mod standing_cmd;
mod status;
mod status_notify;
mod status_progress;
mod task_lines;
mod task_start;
mod tasks_forecast;
mod tasks_tree;
mod thread;
mod thread_import;
mod vestige;
mod voice_cmd;
mod watch_cmd;
mod why;
mod work;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use herdr_sync::HerdrCmd;
use theseus_client::Conn;

const AFTER_HELP: &str = "\
Quick start:
  export OP_SERVICE_ACCOUNT_TOKEN=...        1Password, the recommended home of the config's secrets
  theseusd &                                 start the server (or run it in the foreground)
  theseus health                             is it up, which profile is live, token totals
  theseus ask \"Say hello.\"                  one turn, streamed
  echo \"Summarize: ...\" | theseus ask --json  pipelines
  theseus ask -s <session> \"...\"            continue a session (its whole history is the context)
  theseus ask --attach notes.txt \"...\"      send a file with the prompt, as a Discord attachment is sent
  theseus                                    on a terminal: a conversation in this directory, inline (same as `theseus chat`)
  theseus tui                                every session in one terminal: what needs you, answered inline
  theseus history [session]                  a session's transcript: messages, tool calls, results
  theseus watch [session]                    follow a session live (turns started anywhere)
  theseus status [--short] [--watch]         what needs you, what works, what failed: a screen, a prompt segment, a status bar
  theseus tasks                              background tasks: state, spend, what each waits on
  theseus answer <id> <option> [--note ..]   answer a question waiting for you by one of its options (no id: list them)
  theseus stop <session>                     halt a session's running turn and jobs, as /stop does; the conversation goes on
  theseus digest [--since 2h]                what finished, failed or was answered since you last looked, in a few lines
  theseus --spawn ask \"...\"                 no daemon: spawn theseusd on stdio for one turn and what it left for
                                             later (a job's late result, a wake: --follow-for), then stop it cleanly

Web UI:      http://127.0.0.1:7433/  (while theseusd runs)
Exit codes:  0 ok · 1 server or provider error · 2 usage · 3 cannot connect
             ask: 5 spend limit · 6 waits for approval · 7 refused · 8 cut by a limit · 9 stopped
`theseus help all` lists every command.";

/// `theseus ask --help`'s exit codes: how the turn ended,
/// for a script or a benchmark's harness that runs it headless.
const ASK_EXIT_CODES: &str = "\
Exit codes, by how the turn ended, the last one under --spawn's follow (`--json` gives its stop_reason
in full):
  0  done: the model ended its turn
  1  failed: a provider's or a tool's fault, or the daemon's error
  2  usage · 3 cannot connect or spawn theseusd
  5  the session reached its spend limit; the turn waits for the operator to reset it
  6  a call waits for the operator's approval (`theseus answer`), which a headless run cannot give
  7  the model refused
  8  a limit ended the turn before the model did: the loop cap, the output limit, or the context window
  9  stopped: by an operator (/stop, `theseus stop`), or, under --spawn, by a SIGINT or SIGTERM, which
     stops the turn as /stop does, with what it spent, before the spawned daemon's clean stop; during
     the follow, it stops the session's later work (its jobs, a later turn) the same way
  130 or 143  under --spawn, a second SIGINT or SIGTERM ended the run before the turn stopped";

#[derive(Parser, Debug)]
#[command(
    name = "theseus",
    version,
    about = "Theseus CLI: a thin client for the Theseus server (theseusd), built for shells and pipelines.",
    long_about = "Theseus CLI.\n\nA thin client that speaks the Theseus protocol (JSON-RPC over newline-delimited JSON) \
to a running theseusd over its Unix socket, or spawns one on stdio with --spawn. Prompts come from an \
argument or stdin; replies stream to stdout; diagnostics go to stderr; --json gives one JSON object for machines.",
    after_help = AFTER_HELP,
    args_conflicts_with_subcommands = false,
    disable_help_subcommand = true
)]
struct Cli {
    /// Unix socket of a running theseusd.
    #[arg(
        long,
        env = "THESEUS_SOCKET",
        default_value = "~/.theseus/theseus.sock",
        global = true
    )]
    socket: String,

    /// Spawn `theseusd --stdio` (optionally a path to the binary) instead of connecting to the socket.
    #[arg(long, global = true, num_args = 0..=1, default_missing_value = "theseusd")]
    spawn: Option<String>,

    /// Machine-readable output: the final result as one JSON object on stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Do not stream the reply; print it once at the end.
    #[arg(long, global = true)]
    no_stream: bool,

    /// Print a reply as the text it is, with no markdown drawn and no escape sequence, even on a
    /// terminal (a pipe, or NO_COLOR set, already does).
    #[arg(long, global = true)]
    plain: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// The everyday screen; `help all` lists every command; `help COMMAND` is that command's page.
    #[command(hide = true)]
    Help {
        /// `all`, or a command (and its subcommand).
        topic: Vec<String>,
    },
    /// Send one prompt as one turn and print the reply. Reads stdin when PROMPT is omitted or "-".
    /// The exit code says how the turn ended.
    #[command(after_help = ASK_EXIT_CODES)]
    #[command(display_order = 1)]
    Ask(AskArgs),
    /// Run an MCP server's prompt as a turn's input and print the reply, as `ask` does.
    /// `theseus mcp` lists the prompts. The exit code says how the turn ended.
    #[command(after_help = ASK_EXIT_CODES)]
    #[command(hide = true)]
    Prompt(prompt::PromptArgs),
    /// A session's transcript: messages, tool calls with their gate decisions, results, and
    /// anything waiting for your confirmation. SESSION defaults to the most recently active; it is
    /// an id, or a unique end of one (four characters at least). A handle a reply cited
    /// (`m:a1b2c3`, the footnote's command) shows that message whole instead.
    #[command(display_order = 4)]
    History {
        session: Option<String>,
        /// Only the newest N nodes (default 200); with --after or --before, a page's size.
        #[arg(short, long)]
        n: Option<usize>,
        /// Only nodes after this position, oldest first: 0 for the first page, then the
        /// position the page ends with.
        #[arg(long, value_name = "POSITION")]
        after: Option<u64>,
        /// Only nodes before this position, the newest N of them: a page back.
        #[arg(long, value_name = "POSITION")]
        before: Option<u64>,
        /// Print tool results and long messages in full (default: clipped).
        #[arg(long)]
        full: bool,
    },
    /// What a turn's model was shown, and why each piece was there: the model and profile it ran
    /// on and why routing put it there, each block of the compiled context with its tokens and
    /// its source, each recalled note with its date, source and the reason it was recalled, what
    /// the compile left out and why, and the gate's posture for each tool the turn called and
    /// the layer that set it. TURN is a turn's id, or a unique start or end of one (four
    /// characters at least); it defaults to the session's last. A read: nothing is recompiled
    /// or written. --json prints what the daemon answered. `--short` is how full the context is
    /// instead: the session's last compiled request against the model's window, a line for each
    /// kind of part with its tokens and a bar, and the free space; its argument is the SESSION
    /// (default the most recently active).
    #[command(hide = true)]
    Why {
        /// A turn (a session with `--short`).
        turn: Option<String>,
        /// The session; defaults to the most recently active, as `history` does.
        #[arg(long, value_name = "SESSION", conflicts_with = "short")]
        session: Option<String>,
        /// The context's fill, not the turn's reasons.
        #[arg(long)]
        short: bool,
    },
    /// What the next request holds, counted part by part down to each tool: the system's blocks
    /// and sections, every tool's description, schema and total with its use (calls planned in the
    /// last 30 days across every session, and the last one's date), the messages, and the total
    /// against the window. SESSION is that session's next request; with none, a fresh session's
    /// first request for --place, the place kind (cli, dm, channel, task, headless or harness; default cli). It compiles once and
    /// sends nothing. --exact also asks the provider's token counter (not a model) when a key
    /// resolves. --json prints what the daemon answered.
    #[command(hide = true)]
    Census {
        session: Option<String>,
        #[arg(long, value_name = "KIND")]
        place: Option<String>,
        #[arg(long)]
        exact: bool,
    },
    /// Open a turn in the cockpit's inspector: its flame graph, each loop's request
    /// and thinking, each tool call. `theseus open turn [TURN]`: TURN is a turn's id, or a unique
    /// start or end of one (four characters at least); it defaults to the session's last. Prints
    /// the link (`[cockpit] public_url`) and opens it in a browser where one is found; --print
    /// only prints it.
    #[command(hide = true)]
    Open {
        #[command(subcommand)]
        what: open::What,
    },
    /// Where a node went (node.reach): the compilations and loops of its own session whose
    /// context held it, then its copies in other sessions over derived_from (a task's report in
    /// its parent, a task's brief from the reply that started it), each with theirs. NODE is a
    /// node's id, or its last 6 or more characters, as `theseus history` prints it (msg·a1b2c3)
    /// and the cockpit's Nodes list (Ledger) shows it.
    #[command(hide = true)]
    Reach {
        #[arg(value_name = "NODE")]
        node: String,
        /// How many generations of copies to follow (default 3, at most 16).
        #[arg(long, value_name = "N")]
        generations: Option<u32>,
    },
    /// Rewind a conversation to any moment and take the other road (session.fork): a new session
    /// whose context is NODE's session through NODE, by reference, with its own spend limit and
    /// its own turns; the original is never written to. NODE is a node's id, or its last 6 or
    /// more characters, as `theseus history` prints it (msg·a1b2c3), a message of yours or an
    /// answer that made no tool call. Files and what programs did after that point are not
    /// restored: they ran in the original.
    #[command(hide = true)]
    Fork {
        #[arg(value_name = "NODE")]
        node: String,
        /// The fork's title (default: the original's, plus " (fork)").
        #[arg(long, value_name = "TITLE")]
        title: Option<String>,
        /// The profile its turns run on (default: the original's last).
        #[arg(long, value_name = "PROFILE")]
        profile: Option<String>,
        /// The fork's own spend limit in dollars (default: the session limit).
        #[arg(long, value_name = "USD")]
        budget: Option<f64>,
        /// The session NODE is in, when NODE alone does not name it (default: NODE's own).
        #[arg(long, value_name = "SESSION")]
        session: Option<String>,
    },
    /// Rewind (files.restore, session.fork): put the files Theseus's own edit tools changed, and the
    /// conversation, back to before one of your messages. The files first, each only where it still
    /// holds what the last edit wrote (a file you changed since is listed, never touched; files a
    /// program changed are never restored; one over 1 MiB is not kept), then a new conversation
    /// forked at the node before the message, with the original left as it is. `--to @POSITION`
    /// with `--files-only` undoes a restore. The chat does this on Esc Esc.
    #[command(hide = true)]
    Rewind {
        /// The conversation (default: the one the message is in).
        #[arg(value_name = "SESSION")]
        session: Option<String>,
        /// The message to go back to before: its id, or its last 6 or more characters, as `theseus
        /// history` prints it (msg·a1b2c3), or `@POSITION` to undo a restore.
        #[arg(long, value_name = "NODE")]
        to: String,
        /// Put the files back; the conversation stays where it is.
        #[arg(long, conflicts_with = "conversation_only")]
        files_only: bool,
        /// Fork the conversation at the node before the message; the files stay as they are.
        #[arg(long)]
        conversation_only: bool,
    },
    /// Each place Theseus speaks in, and its class (the place rule): private (the CLI, the web
    /// UI, a DM with you, a guild channel bound `private = true`) gets everything; shared (any
    /// other guild channel) gets its own conversation and the public tools alone.
    #[command(display_order = 17)]
    Places,
    /// Publish into a place (the place rule): one item into a place's conversation, as your
    /// message there, and said in the place: a node by id, a file you can read, or a message
    /// (`--text`). Only you, from a private place (the CLI is one), may. PLACE is as
    /// `theseus places` names it (`#openclaw`).
    #[command(hide = true)]
    Publish {
        /// A node's id, or a file's path (it starts with `/`, `~`, or `.`, or names a file).
        #[arg(value_name = "NODE|FILE", required_unless_present = "text")]
        what: Option<String>,
        #[arg(long, value_name = "PLACE")]
        to: String,
        /// Publish this message instead.
        #[arg(long, value_name = "TEXT", conflicts_with = "what")]
        text: Option<String>,
        /// Your words above it.
        #[arg(long, value_name = "NOTE")]
        note: Option<String>,
    },
    /// Mark one node wrong: recall never serves it again, each live session that
    /// used it gets a correction note at its next turn, the syntheses that cite it go stale, one
    /// card names the tasks to re-run, and the table says every place it went. NODE is a node's
    /// id, the last six or more characters of one, or a recalled item's handle. Only you, from a
    /// private place; refused inside a job. A second retraction writes nothing.
    #[command(hide = true)]
    Retract {
        #[arg(value_name = "NODE|HANDLE")]
        node: String,
        /// Why it is wrong, in a sentence.
        #[arg(value_name = "WHY", required = true)]
        why: Vec<String>,
    },
    /// The ontology: the kinds of context, the categories (a bound place's channel or person,
    /// and the topics you declare) with their guidance, and sessions' memberships. A session's
    /// system block carries the guidance of the categories it is in, from its next recompile.
    #[command(hide = true)]
    Ontology {
        #[command(subcommand)]
        cmd: Option<OntologyCmd>,
    },
    /// Follow a session live: streamed text, tool calls, confirmations, context decisions,
    /// whoever started the turn (web UI, CLI, the harness). SESSION defaults to the most recent;
    /// it is an id, or a unique end of one (four characters at least). An unknown one exits 1.
    #[command(display_order = 5)]
    Watch {
        session: Option<String>,
        /// Show thinking summaries too.
        #[arg(long)]
        thinking: bool,
        /// Every session's executions instead of one session's turns: a snapshot of what needs
        /// you or works, then each change as it happens, with its WAL position, and each question
        /// as it is asked and answered (executions.watch).
        #[arg(long, conflicts_with = "session")]
        all: bool,
        /// Answer from here: a question waiting asks `approve? [y/N/t/note]`, and any other line
        /// is sent to the session as a message. Line mode, so it works in any terminal or pane.
        #[arg(long, conflicts_with = "all")]
        interactive: bool,
        /// Inside a herdr pane (HERDR_ENV=1, HERDR_PANE_ID, HERDR_SOCKET_PATH) the watch reports
        /// the session's state to the pane; this turns that off.
        #[arg(long)]
        no_herdr: bool,
    },
    /// The terminal UI: every session in one sidebar with its task trees, the queue of what
    /// needs you, answered inline, and a session's history with its input line. It runs
    /// `theseus-tui`, found beside this binary or else on PATH, with --socket and ARGS passed
    /// through (`theseus tui --help` is its help).
    #[command(disable_help_flag = true)]
    #[command(display_order = 3)]
    Tui {
        /// Passed to theseus-tui: --notify bell|osc9|osc777|off, --help.
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<String>,
    },
    /// An inline conversation in this directory: a new one, or SESSION's (an id, or a unique end
    /// of one). The same as `theseus` alone on a terminal. It runs `theseus-tui chat`, found
    /// beside this binary or else on PATH.
    #[command(display_order = 2)]
    Chat {
        /// A conversation to go on with.
        session: Option<String>,
        /// Continue this directory's last conversation (`theseus -c`).
        #[arg(short = 'c', long = "continue", conflicts_with_all = ["session", "resume"])]
        cont: bool,
        /// Pick a conversation to continue, searched as you type (`theseus -r`).
        #[arg(short = 'r', long, conflicts_with = "session")]
        resume: bool,
        /// Open a task joined (an id, or a unique end of one), with its conversation behind it:
        /// Esc or /back goes there.
        #[arg(long, value_name = "ID", conflicts_with_all = ["session", "cont", "resume"])]
        task: Option<String>,
        /// With --task: read-only, no input to the task.
        #[arg(long, requires = "task")]
        inspect: bool,
        /// Line mode, as `watch --interactive` with the tasks' events as lines: no footer and no
        /// one-key answers. A pipe, or `TERM=dumb`, is line mode too.
        #[arg(long)]
        plain: bool,
    },
    /// Answer any question waiting for you, with one of its answers: a call, a spend limit, a
    /// task change, an extension, a reminder. `theseus answer 7f3a approve`; alone, it lists what
    /// waits.
    ///
    /// The turn an answer resumes is followed to its end (`--no-wait` returns once the answer is
    /// recorded). Approving a spend limit resets its spend to $0. A question that spends money or
    /// changes AWS is the owner's own: an agent's `theseus answer` (no terminal, or run under an
    /// agent) is refused, and the question waits for the DM's card or your own terminal.
    #[command(display_order = 8)]
    Answer(answer::AnswerArgs),
    /// The toollets: class, backend, what policy does with each, and calls so far.
    #[command(hide = true)]
    Tools {
        /// Also print each tool's description and input schema.
        #[arg(long, short)]
        verbose: bool,
    },
    /// "Should have asked": make a tool ask first from now on, undo it, or list every tool's
    /// posture and what set it. A tightening is stored, never in the config, and only makes a
    /// tool stricter; an undo returns the tool to what the config says.
    #[command(hide = true)]
    Policy {
        #[command(subcommand)]
        cmd: Option<PolicyCmd>,
    },
    /// Theseus's AWS account: `bootstrap` plans its first stacks, read-only, and applies them on
    /// your yes.
    #[command(hide = true)]
    Aws {
        #[command(subcommand)]
        cmd: AwsCmd,
    },
    /// Google Workspace: `auth` is the one-time consent (run it from your own shell), `status`
    /// shows the account and the tools on, `revoke` ends the grant.
    #[command(hide = true)]
    Google {
        #[command(subcommand)]
        cmd: google_cmd::GoogleCmd,
    },
    /// Server health: version, live profile, providers, sessions, turns, provider errors, token totals.
    #[command(display_order = 19)]
    Health,
    /// Sessions: list them with per-session token totals (`--state`: each one's execution, with its
    /// state, turns, outstanding actions and budget), or open one to continue across turns.
    #[command(hide = true)]
    Sessions {
        #[command(flatten)]
        page: pages::PageArgs,
        #[command(subcommand)]
        cmd: Option<SessionsCmd>,
    },
    /// Where the money is: each open session's limit and where it comes from, spent, reserved,
    /// held, available, and lifetime cost, its resets and any budget question, each task under
    /// its parent with its carve, and the totals.
    #[command(hide = true)]
    Budgets,
    /// Tasks (work.watch), one line each under its conversation: a mark, its short id, its title,
    /// how long it has run and what it is doing; under one that asks, the command that answers it;
    /// a footer of counts. `--watch` keeps it current: a line for each task that starts, asks,
    /// reports or fails. `--graph` is every plan and step as a tree and each task's state and spend as before;
    /// `--json` the same list as data.
    #[command(display_order = 7, subcommand_precedence_over_arg = true)]
    Tasks {
        #[command(flatten)]
        args: tasks_tree::TasksArgs,
        #[command(subcommand)]
        cmd: Option<tasks_forecast::TasksCmd>,
    },
    /// Task: `task start BRIEF` starts a task from your word with no model turn: now,
    /// or `--at fri 9:00` / `--in-time 2h`, under your Home conversation or `--in SESSION`, with
    /// `--budget`, `--profile`, and `--quote-file` text it reads verbatim. Yours alone (refused
    /// inside a job). `task show ID` prints one task whole, its report and the commands that act
    /// on it; `task pause ID` and `task resume ID` hold and release a running one. `task rehome ID HOME`
    /// sends a task's report to another conversation or `thread:<slug>`; `task reschedule ID --at
    /// WHEN|--in-time SPAN` moves a deferred task's start. `tasks` lists them.
    #[command(hide = true)]
    Task {
        #[command(subcommand)]
        cmd: task_start::TaskCmd,
    },
    /// Wakes: the turns conversations asked for at a time with wake.at that have not run yet,
    /// soonest first, with session, due time, and note. `wake list` is the same; `wake pause|resume|
    /// snooze|cancel ID` act on one (a reminder that fired is answered with `answer ID done|snooze`).
    #[command(display_order = 14)]
    Wake {
        #[command(subcommand)]
        cmd: Option<remind::WakeCmd>,
    },
    /// Cancel a task and its jobs, or a pending wake, by what its id names (its last six characters
    /// are enough, as `theseus tasks` and `theseus wake` show it); `wake cancel ID` is the same for a
    /// wake. `stop SESSION` stops a conversation.
    #[command(hide = true)]
    Cancel {
        #[arg(value_name = "ID")]
        name: String,
    },
    /// Remind the owner: WHEN and what to say, with no model turn. It is posted as it is when it
    /// is due, in the Discord DM when one is bound and to every surface that watches, with Done
    /// and Snooze (`theseus answer ID snooze`). With no `--session` it lives in your Home conversation.
    #[command(display_order = 12)]
    Remind(remind::RemindArgs),
    /// What needs you, what works, and what failed, in one screen: a header of counts and a row
    /// for each task (what it asks, and the command that answers). `--short` is a line for a
    /// prompt or a status bar, `--watch` keeps it current from the push (`--tab`: the terminal
    /// tab's progress as well). One read, within 100 ms of connecting.
    #[command(display_order = 6)]
    Status(status::StatusArgs),
    /// A running job's last lines, once: its size and the last 20 lines (work.peek), scrubbed.
    /// JOB is its id (`act_…`), or a unique end of it as `theseus tasks` prints it.
    #[command(hide = true)]
    Peek(jobout::PeekArgs),
    /// Follow a running job's output live (action.output): its last lines, then each line as it is
    /// printed, until the job ends. Ctrl-C detaches and the job goes on; a stream that fell behind
    /// says so and reads the job again.
    #[command(hide = true)]
    Attach(jobout::AttachArgs),
    /// The future in one view: Now, Today, This week, Later, Recurring (routines and repeating
    /// reminders), Waiting on the world (watches) and Projects, with the verbs that act on them.
    /// `--watch` keeps it current.
    #[command(display_order = 10)]
    Agenda(agenda::AgendaArgs),
    /// What finished, failed, was answered or woke since you last looked, in a few lines
    /// (work.digest). `--since 2h` names a time; the default is since the last time a terminal
    /// client showed you the board, and with none yet, the last 24 hours. `--session` is one
    /// conversation's or task's own timeline.
    #[command(display_order = 11)]
    Digest(digest::DigestArgs),
    /// An incident's timeline, from the ledger: what ran, waited, failed and cost, what was asked
    /// of the owner and answered, and every restart of the daemon, as markdown (`--json`: the
    /// same facts as data). Read only. The default window is the last 12 hours; `--session` or
    /// `--task` narrow it to one conversation or task and what it started.
    #[command(hide = true)]
    Postmortem(postmortem::PostmortemArgs),
    /// Wait until a session needs you, settles, or ends, then print its state and its questions
    /// (session.wait). The daemon owns the wait, so nothing polls, and a wait already satisfied
    /// answers at once. SESSION is its id, or at least its last four characters. Exit codes: 0
    /// reached, 4 timed out, and 1, 2, and 3 as for every command.
    #[command(hide = true)]
    Wait {
        #[arg(value_name = "SESSION", required_unless_present = "any")]
        session: Option<String>,
        /// Any session, not one: return once anything needs you (`--until blocked`, which this
        /// implies), over executions.watch. `until theseus wait --any; do :; done`.
        #[arg(long, conflicts_with_all = ["session", "after"])]
        any: bool,
        /// blocked (it needs you), settled (nothing runs or is queued for it: it needs you, is
        /// ready, or ended; a job or a child task still runs), or terminal (a task ended).
        #[arg(long, default_value = "settled", default_value_if("any", "true", "blocked"),
              value_parser = ["blocked", "settled", "terminal"])]
        until: String,
        /// Only a change after this WAL position counts (one `theseus watch --all` printed, or
        /// a previous wait's).
        #[arg(long, value_name = "POSITION")]
        after: Option<u64>,
        /// How long to wait: 90s, 10m, 2h (default 10m, at most 24h).
        #[arg(long, value_name = "DURATION")]
        timeout: Option<String>,
    },
    /// Dismiss a failed task: it stops needing you on every surface, and its record stays. ID is
    /// the task's id, or at least its last four characters.
    #[command(hide = true)]
    Dismiss {
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Try a failed task again: a new task opens from the same brief and pieces under the same
    /// parent, with no more budget than the failed one had, and the failed one is dismissed.
    #[command(hide = true)]
    Retry(retry::RetryArgs),
    /// Halt what a session is doing, as Discord's `/stop` does: its running turn, its jobs, and
    /// what waits on you. The conversation goes on: the next `ask -s` continues it, and its tasks
    /// and wakes go on too (`theseus cancel <id>` stops one). SESSION is its id, or at least its
    /// last four characters. With `--all` (`--tasks`) it also cancels every task under the
    /// conversation, a task's own tasks and jobs first; its wakes stay.
    #[command(display_order = 9)]
    Stop {
        #[arg(value_name = "SESSION")]
        session: String,
        /// Also cancel every task under the conversation (an owner's act; refused inside a job).
        #[arg(long, visible_alias = "tasks")]
        all: bool,
    },
    /// A session's content labels: what entered it (a tool result, a page, a pasted file) and was
    /// judged to carry. `injection` stops its calls that act; `secret`, `confidential` and
    /// `personal` keep it off a China-origin model. `theseus content SESSION` lists them;
    /// `theseus content remove SESSION FLAG` (or `--all`) removes one, which is yours alone to do.
    /// SESSION is its id, or at least its last four characters.
    #[command(hide = true, subcommand_precedence_over_arg = true)]
    Content(content_cmd::ContentArgs),
    /// A conversation's mode, as Shift+Tab sets it: `ask`, `edits` (file edits in this project run
    /// with a notice), `plan` (read-only until you approve a plan) or `auto` (what only its posture
    /// made wait runs with a notice). With no MODE, says the current one and the plan's checklist.
    #[command(hide = true)]
    Mode(mode_cmd::ModeArgs),
    /// Model profiles: list, or switch the live one (`theseus profile use glm`).
    #[command(display_order = 18)]
    Profile {
        #[command(subcommand)]
        cmd: Option<ProfileCmd>,
    },
    /// Recent ledger rows (every turn, loop, provider call, state change, error).
    #[command(hide = true)]
    Ledger {
        /// How many rows.
        #[arg(short, long, default_value_t = 20)]
        n: usize,
        /// Only rows of this kind, e.g. turn.ended, provider.call, provider.error.
        #[arg(short, long)]
        kind: Option<String>,
        #[arg(short, long)]
        session: Option<String>,
        #[command(subcommand)]
        cmd: Option<ledger_verify::LedgerCmd>,
    },
    /// herdr, the terminal workspace manager: `theseus herdr sync` gives each session that needs
    /// you or is working a pane in herdr, running `theseus watch --interactive`.
    #[command(hide = true)]
    Herdr {
        #[command(subcommand)]
        cmd: herdr_sync::HerdrCmd,
    },
    /// Recall: `memory search <QUERY>` runs recall's pipeline for a query, writing nothing;
    /// `memory recalled <SESSION>` shows what each of its turns would have recalled, in shadow.
    #[command(hide = true)]
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },
    /// Jev's judgments: `judge log` lists the recent ones, each with its pack, session,
    /// answers, and cost; `judge show <id>` shows one whole.
    #[command(hide = true)]
    Judge {
        #[command(subcommand)]
        cmd: JudgeCmd,
    },
    /// The ladder: each Jev pack's rung, share and why, its rollback rules and last rows;
    /// `packs promote <pack> --canary <share> | --live` and `packs rollback <pack>` move one.
    #[command(hide = true)]
    Packs {
        #[command(subcommand)]
        cmd: Option<packs::PacksCmd>,
    },
    /// The operator's past history: `import openclaw <file>...` brings episode
    /// files in as imported sessions, closed and private; `import erase --tag <tag>` and
    /// `import list`.
    #[command(hide = true)]
    Import {
        #[command(subcommand)]
        cmd: Option<import::ImportCmd>,
    },
    /// The MCP servers the config attaches: each server's state, and its tools with their
    /// postures and classes; `mcp restart <NAME>` starts one again, a failed one included.
    #[command(hide = true)]
    Mcp {
        #[command(subcommand)]
        cmd: Option<mcp::McpCmd>,
    },
    /// Extensions: `extend list` shows each proposal with its state, its frozen
    /// digest, its tools and tests, and the question `theseus answer` answers, and each loaded
    /// one; `extend revoke <NAME>` stops a loaded one and drops its tools.
    #[command(hide = true)]
    Extend {
        #[command(subcommand)]
        cmd: extend::ExtendCmd,
    },
    /// Self-improvement: `self log` shows what Theseus changed about itself,
    /// `self halt [WHY]` stops all self-directed work at once, `self resume` (the owner's) releases.
    #[command(name = "self")]
    #[command(hide = true)]
    SelfImprove {
        #[command(subcommand)]
        cmd: self_cmd::SelfCmd,
    },
    /// The persona as data: `persona` shows every persona (name, emoji, voice,
    /// initiative, files with digest and edit time, the compiles that carried it, its places);
    /// `persona edit [NAME] [--name|--emoji|--card|--tts-voice|--avatar|--initiative VALUE]` sets
    /// short fields, or, with none, writes its soul (as do `persona soul KEY`, `persona set KEY`,
    /// `persona add KEY`, `persona retire KEY` and `persona default KEY`); `persona history` lists
    /// the `persona.changed` rows; `persona voice [audition]` is the speaking voice, and `persona rate
    /// up|down [ID]` your 👍 or 👎 on the latest message Theseus sent first.
    #[command(hide = true)]
    Persona {
        #[command(subcommand)]
        cmd: Option<persona_cmd::PersonaCmd>,
    },
    /// The owner's standing orders, preferences and facts: kept in every prompt of
    /// their scope until you revoke them. `standing` lists them; `standing add [--kind order|
    /// preference|fact] [--scope everywhere|place:T|session:ID | --surface NAME] TEXT` and `standing revoke ID` are
    /// yours alone (refused inside a job).
    #[command(hide = true)]
    Standing {
        #[command(subcommand)]
        cmd: Option<standing_cmd::StandingCmd>,
    },
    /// The owner card, proposed from your imported history: `card` lists what
    /// waits as questions (Keep, Edit, Not this), what you kept and declined, and the spend;
    /// `card propose [--dry-run]` runs the job (under `[card] day_usd`); `card keep|edit|decline ID`
    /// answers one (`--scope` says where a kept one holds); `card scope ID SCOPE` moves a kept
    /// one's record; `card done` stops the nightly run. Nothing is added without your answer; the
    /// writes are yours alone (refused inside a job).
    #[command(hide = true)]
    Card {
        #[command(subcommand)]
        cmd: Option<card_cmd::CardCmd>,
    },
    /// Watches: wait on the world with no model. `watch-for` lists them; `watch-for add
    /// TITLE --run 'gh pr view 123 --json state -q .state' --until 'equals MERGED' [--every 5m]
    /// [--for 3d] [--then notice|turn|task]` checks on a timer until the condition holds, then tells
    /// you; `watch-for rm ID` ends one. Adding one is yours alone (refused inside a job).
    #[command(hide = true, name = "watch-for")]
    WatchFor {
        #[command(subcommand)]
        cmd: Option<watch_cmd::WatchesCmd>,
    },
    /// Threads: lines of work on virtual places `thread:<slug>`, each with a session
    /// of its own that runs beside the others. `thread` lists them; `thread new|show|edit|pause|
    /// resume|done|reopen`; address one with `ask --thread SLUG`. The writes are yours alone
    /// (refused inside a job).
    #[command(display_order = 16)]
    Thread {
        #[command(subcommand)]
        cmd: Option<thread::ThreadCmd>,
    },
    /// Routines: recurring work, each run a fresh task with its own budget, from a
    /// month of its own. `routine` lists them; `routine add SCHEDULE BRIEF`, `runs`, `pause`,
    /// `resume`, `run`, `update`, `rm`. The writes are yours alone (refused inside a job).
    #[command(display_order = 13)]
    Routine {
        #[command(subcommand)]
        cmd: Option<routine::RoutineCmd>,
    },
    /// Models and what they cost: `model bench [--week]` is your own benchmark (which model your own
    /// turns need, measured by replaying them one rung down the routing table), `model replay SESSION
    /// TURN -P PROFILE` sends a past turn's first request to another profile (yours alone), `model
    /// prefix` is the prefix library's picks and what they saved, and `model catalog` the models'
    /// windows and prices.
    #[command(hide = true)]
    Model {
        #[command(subcommand)]
        cmd: own_bench::ModelCmd,
    },
    /// Projects (also `projects`): long-term work with a goal, dated milestones that
    /// remind you, a weekly review, and a slice in every session attached to it. `project` lists
    /// them (a bar of milestones done, the next one and its date, the review); `project show ID` (the tree with dates), `project new TITLE --milestone
    /// TITLE@WHEN`, `project attach SESSION ID`, `project detach SESSION`. The writes are yours alone
    /// (refused inside a job).
    #[command(alias = "projects")]
    #[command(display_order = 15)]
    Project {
        #[command(subcommand)]
        cmd: Option<project::ProjectCmd>,
    },
    /// Send a raw JSON-RPC request (e.g. `rpc health`, `rpc turn.submit '{"input":"hi"}'`); notifications echo to stderr.
    #[command(hide = true)]
    Rpc {
        method: String,
        params: Option<String>,
    },
    /// Ask the server to stop cleanly (removes its socket).
    #[command(hide = true)]
    Shutdown,
}

#[derive(Args, Debug)]
struct AskArgs {
    prompt: Option<String>,
    /// Continue an existing session instead of opening a new one.
    #[arg(long, short)]
    session: Option<String>,
    /// Run in this thread's session, opened at its first message: the slug
    /// `theseus thread list` shows.
    #[arg(long, value_name = "SLUG", conflicts_with = "session")]
    thread: Option<String>,
    /// Profile for this turn (default: the live profile).
    #[arg(long = "profile", short = 'P')]
    profile: Option<String>,
    /// Raw provider override for this turn (a configured provider name, e.g. anthropic, zai).
    #[arg(long, short)]
    provider: Option<String>,
    /// Model id for this turn (e.g. claude-sonnet-5, glm-5.3-flash).
    #[arg(long, short)]
    model: Option<String>,
    /// After the reply, print the turn's timing tree (turn > loops > provider/tools) to stderr.
    #[arg(long)]
    trace: bool,
    /// Show the model's thinking summaries on stderr as they stream.
    #[arg(long)]
    thinking: bool,
    /// Send a file with the prompt (repeatable), as a Discord attachment is sent: a text
    /// file's text, labeled with its name; an image (PNG, JPEG, GIF, WebP); a PDF, which the
    /// model reads page by page; or any other file up to 32 MiB, kept for the model.
    #[arg(long = "attach", value_name = "FILE")]
    attach: Vec<PathBuf>,
    /// The directory the session works in: its tools' default directory and the base of their
    /// relative paths (default: this shell's current directory, for a new session). With
    /// --session, moves that session there; without, the session keeps its own.
    #[arg(long, value_name = "DIR")]
    dir: Option<PathBuf>,
    /// Under --spawn: after the turn, keep the daemon up while what it left for later can still
    /// come back (a job's late result, a wake due within the bound) and print each turn that
    /// comes, for at most this long (30m, 90s, 2h). 0 follows nothing.
    #[arg(long, value_name = "DURATION", default_value = follow::FOLLOW_FOR, value_parser = follow::parse_follow_for)]
    follow_for: std::time::Duration,
    /// The threadbench's clock for the router, in milliseconds: a daemon with `[threads]
    /// bench_clock` routes the message as typed at that time.
    #[arg(long, value_name = "MS", hide = true)]
    route_at_ms: Option<u64>,
    /// The turn's time, in seconds: the model is told the time it has left, and a turn with no
    /// passing check near its end is asked to stop and report (default: none).
    #[arg(long, value_name = "SECS")]
    deadline: Option<u64>,
}

#[derive(Subcommand, Debug)]
enum PolicyCmd {
    /// Every tool's posture now and what set it, with the tightenings (default).
    List,
    /// TOOL asks first from now on, on every surface (e.g. `theseus policy tighten proc.run`).
    Tighten {
        tool: String,
        /// The call whose notice prompted it (a correlation id from `theseus ledger -k
        /// tool.notified`), kept with the tightening as a labeled example.
        #[arg(long = "call", value_name = "CORRELATION_ID")]
        call: Option<String>,
    },
    /// Undo a tightening: TOOL goes back to what the config says. It loosens, so it counts only
    /// where an approval would.
    Untighten { tool: String },
    /// Why a call waits: each tool's result, layer by layer in the gate's order (the place and
    /// its ceiling, the class, the config's posture, a tightening, a granted secret, the
    /// place's floor, T1's hold), and the conditions that depend on the call. For SESSION's
    /// place, or for the CLI and every bound place; with --tool, that tool in full.
    Explain {
        #[arg(long, value_name = "SESSION")]
        session: Option<String>,
        /// A tool's canonical name (`proc.run`, `mcp:<server>/<tool>`).
        #[arg(long)]
        tool: Option<String>,
    },
    /// Trust SESSION again: it no longer holds the external text it read (a web page, a
    /// search), so its calls that act run at their postures again. It loosens, so it counts
    /// only where an approval would. SESSION is its id, or at least its last four characters.
    Trust {
        #[arg(value_name = "SESSION")]
        session: String,
    },
    /// Dismiss the notice of SESSION's hold on external text: under `[policy] external_text =
    /// "notify"` the hold has told you, and nothing waits on it. The hold and its record stay;
    /// a later hold tells again. A hold that waits for you (ask mode) is answered or trusted
    /// instead. Yours alone to run. SESSION is its id, or at least its last four characters.
    Dismiss {
        #[arg(value_name = "SESSION")]
        session: String,
    },
}

#[derive(Subcommand, Debug, Default)]
enum OntologyCmd {
    /// The kinds table: where each kind's memberships come from, how many a session holds, its
    /// precedence, and its rule (default).
    #[default]
    Kinds,
    /// The category tree, with each category's guidance.
    Categories,
    /// Declare a topic.
    Topic {
        #[command(subcommand)]
        cmd: TopicCmd,
    },
    /// Take back what Jev wrote: WRITE is the judgment's id (`filed:` and the mint card name it).
    /// The member lists come back as they were, a category it minted is retired, and the pack's
    /// ledger counts the write as wrong. Only you, from a private place.
    Undo { write: String },
    /// Add a kind to the kinds table, or take one away. Only you, from a private place.
    Kind {
        #[command(subcommand)]
        cmd: onto_cmd::KindCmd,
    },
    /// Set CATEGORY's guidance (its id, or a topic's or a person's name): TEXT, or stdin with
    /// `-` or nothing. Empty text takes it away. Only you, from a private place (the CLI is
    /// one), may. A person's guidance is one line of role facts, never an evaluation of them.
    Guide {
        category: String,
        #[arg(value_name = "TEXT|-")]
        text: Option<String>,
    },
    /// A session's memberships; with changes, `+CATEGORY` adds it and `-CATEGORY` takes it out.
    /// They apply at the session's next recompile (`theseus sessions recompile`).
    Member {
        session: String,
        #[arg(allow_hyphen_values = true, value_name = "+CATEGORY|-CATEGORY")]
        changes: Vec<String>,
    },
    /// Jev's unanswered proposals, newest first: a topic a session is not in, or a new topic,
    /// with Jev's confidence and band; people one row per person, with its sessions and bands,
    /// a bare first name inside the one person whose name holds it.
    Proposals {
        #[arg(long, value_name = "SESSION")]
        session: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
        /// Only proposals at this confidence or more; a person's row by its best (0 to 1).
        #[arg(long)]
        min_confidence: Option<f64>,
        /// Each person's proposal on its own line, not a row per person.
        #[arg(long)]
        each: bool,
    },
    /// Accept a proposal: the session joins the topic (yours, `operator`), and the judgment is
    /// labelled. For a new topic, --topic names it: an existing topic, or a new one (--desc).
    /// --person NAME accepts a person's row, every proposal of it. Without either, every
    /// unanswered proposal of --kind at --min-confidence or more, people a row at a time.
    Accept {
        judgment: Option<String>,
        /// A person's row, by its name or key (`theseus ontology proposals`): all its proposals.
        #[arg(long, conflicts_with = "judgment")]
        person: Option<String>,
        /// Accept as this person (a held person's name or id, or the new person's name): a bare
        /// first name's, or an ambiguous row's pick.
        #[arg(long = "as", value_name = "PERSON")]
        as_person: Option<String>,
        /// With no JUDGMENT: only proposals of this kind (`topic` or `person`).
        #[arg(long, conflicts_with_all = ["judgment", "person"])]
        kind: Option<String>,
        /// With no JUDGMENT: only proposals at this confidence or more (0 to 1); a person's row
        /// by its best.
        #[arg(long, conflicts_with_all = ["judgment", "person"])]
        min_confidence: Option<f64>,
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        desc: Option<String>,
        #[arg(long, conflicts_with = "person")]
        note: Option<String>,
    },
    /// People: declare one with its handles, or merge two who are one.
    Person {
        #[command(subcommand)]
        cmd: PersonCmd,
    },
    /// Never propose again: each JUDGMENT's proposal is rejected and what it proposes (the topic's
    /// name, the person's name, or for a new topic with no name, that session) is kept as a never.
    /// --list shows the nevers; --undo KEY takes one back.
    Never(crate::onto_cmd::NeverArgs),
    /// Reject a proposal: the judgment is labelled, and nothing else changes. --person NAME
    /// rejects a person's row, every proposal of it.
    Reject {
        #[arg(required_unless_present = "person")]
        judgment: Option<String>,
        #[arg(long, conflicts_with = "judgment")]
        person: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum PersonCmd {
    /// A new person, NAME, with their handles. A handle another person holds exactly (not a
    /// name) is that person: the new handles join them, and no second person is made. A
    /// person's description and guidance hold role facts (what they do and own, their projects
    /// and channels, how work flows between them and you), never an evaluation of them.
    Add {
        name: String,
        /// `discord:<id>`, `slack:<id>`, `email:<addr>`, or `name:<display name>`; repeat it.
        #[arg(long = "handle", value_name = "KIND:ID")]
        handles: Vec<String>,
        /// Their role, in a line or two: what they do and own (never an evaluation).
        #[arg(long)]
        desc: Option<String>,
    },
    /// Merge A into B: B keeps its id and takes A's handles, memberships and guidance. A DM's
    /// person always survives. `--undo` undoes A's newest merge, from its ledger row.
    Merge {
        a: String,
        #[arg(required_unless_present = "undo")]
        b: Option<String>,
        #[arg(long)]
        undo: bool,
    },
}

#[derive(Subcommand, Debug)]
enum TopicCmd {
    /// A new topic, NAME; its id is made from it.
    Add {
        name: String,
        /// The topic it nests under: its id or its name.
        #[arg(long)]
        parent: Option<String>,
        /// What it is about, in a line or two.
        #[arg(long)]
        desc: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum AwsCmd {
    /// The account's foundation, posture, and relay stacks (AWS design §5): the plan, read-only
    /// (no change set is made), then, on a terminal, a question whether to apply it. A stack
    /// that is as planned shows no change, so a second plan after the apply shows none.
    Bootstrap {
        /// The account, when several are bound.
        #[arg(long)]
        account: Option<String>,
        /// The address the alerts topic emails (a new foundation only); SNS mails it a
        /// confirmation link once: do not open it, but give its token to
        /// `theseus aws confirm-alerts`.
        #[arg(long, value_name = "ADDRESS")]
        alert_email: Option<String>,
        /// The trail's encryption: aws-managed (SSE-S3, the default) or customer (its own KMS
        /// key, about $1 a month).
        #[arg(long, value_name = "KIND")]
        trail_key: Option<String>,
        /// Apply the plan whose digest this is, as a plan printed it, with no question.
        #[arg(long, value_name = "DIGEST")]
        apply: Option<String>,
        /// Show the plan and stop: never ask.
        #[arg(long, conflicts_with = "apply")]
        plan_only: bool,
    },
    /// Confirm the alerts topic's email subscription so that only the account can unsubscribe
    /// it. Do not open the link in SNS's confirmation email (an opened link lets any alert's
    /// unsubscribe link, or a mail scanner that follows it, remove the subscription): copy the
    /// link's address, or its Token=… value, and give it here. With no TOKEN it is read from
    /// stdin, which keeps it out of the shell's history. The token is never printed.
    ConfirmAlerts {
        /// The token, or the whole confirmation link (default: read from stdin).
        #[arg(value_name = "TOKEN")]
        token: Option<String>,
        /// The account, when several are bound.
        #[arg(long)]
        account: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum IndexCmd {
    /// The index and its tender: state, nodes and chunks, how far behind the store it is, its
    /// process, and its vectors.
    Status,
    /// Drop the index and build it again from the WAL. The daemon watches it and writes its
    /// start, progress and end, so `theseus tasks` and `theseus status` show it as the daemon's
    /// own work. The owner's act, from their own shell.
    Rebuild,
    /// The best chunks for QUERY, each with its node, session, WAL position, and the sources that
    /// ranked it (bm25, entity, vector), fused as recall fuses them, vectors weighing most.
    Search {
        #[arg(required = true, value_name = "QUERY")]
        query: Vec<String>,
        /// How many hits (at most 100).
        #[arg(short, long, default_value_t = 10)]
        k: usize,
        /// Only nodes written before this WAL position.
        #[arg(long, value_name = "POSITION")]
        as_of: Option<u64>,
        /// Rank by these sources alone, comma-separated: `bm25,entity` finds the words you remember,
        /// as written (default: every source the index has).
        #[arg(long, value_delimiter = ',', value_name = "SOURCES")]
        sources: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum MemoryCmd {
    /// The index: `memory index status` says how far the index tender has read the store and what it
    /// holds; `memory index search <QUERY>` finds what was said, run, and read, by its words and
    /// names (BM25, exact entities, and vectors once the model's files are installed).
    Index {
        #[command(subcommand)]
        cmd: IndexCmd,
    },
    /// Recall's pipeline for QUERY, writing nothing: what the index offers, what the pack would
    /// admit with each source's rank, and why each other hit is dropped (its place first).
    Search {
        #[arg(required = true, value_name = "QUERY")]
        query: Vec<String>,
        /// As a turn in this session's place would recall (default: a private place's, as the
        /// CLI is).
        #[arg(long, value_name = "SESSION")]
        session: Option<String>,
        /// How many hits to ask the index for (at most 100).
        #[arg(short, long, default_value_t = 40)]
        k: usize,
        /// The arm whose sources and science rank it (default `baseline`): `+retention` shows
        /// each item's retrievability, stability, difficulty and last review; `+activation`
        /// what spreading activation reached and added; `+synthesis` admits the checked
        /// syntheses.
        #[arg(long, value_parser = ["bm25", "baseline", "+retention", "+activation", "+synthesis"])]
        arm: Option<String>,
        /// Only nodes written before this WAL position are candidates (as a turn's recall at
        /// that position would see them).
        #[arg(long, value_name = "POSITION")]
        as_of: Option<u64>,
    },
    /// Consolidation now: clusters of notes recall admits together become cited syntheses,
    /// checked by Jev. The operator's, refused inside a job.
    Consolidate {
        /// List the clusters it would synthesize, and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// What each of SESSION's turns recalled (canary, live) or would have (shadow), newest last.
    Recalled {
        session: String,
        /// The newest this many (at most 200).
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: usize,
    },
    /// Label NODE for recall: `wrong` or `stale` keeps it out of every session's recall from the
    /// next turn on, `useful` lets it back, `should_have` says recall missed it. The operator's,
    /// refused inside a job.
    Label {
        node: String,
        #[arg(value_parser = ["useful", "wrong", "stale", "should_have"])]
        label: String,
        /// The recall that offered it.
        #[arg(long, value_name = "RECALL")]
        recall: Option<String>,
        /// Why, in a few words.
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum JudgeCmd {
    /// How long the batch's requests took, by local day: p50 and p95, and the requests that
    /// carried `onto.v1`. If the batch's p95 grows by more than 50 ms once it joined, set
    /// `[ontology] request = "own"`.
    Latency(onto_cmd::LatencyArgs),
    /// The recent judgments, oldest first: time, pack and rung, session, answers (each with its
    /// band), cost, and how long Jev took; a failed or skipped one says why.
    Log {
        /// How many (the newest).
        #[arg(short, long, default_value_t = 20)]
        n: usize,
        /// Only this session's.
        #[arg(long, value_name = "SESSION")]
        session: Option<String>,
        /// Only this pack's (`loop.v1`, or `loop` for every version).
        #[arg(long, value_name = "PACK")]
        pack: Option<String>,
    },
    /// One judgment: its pack, rung, outcome, timing, and cost; the state Jev was sent, as
    /// fields; and each answer with its probabilities and band.
    Show {
        /// The judgment's id (`jdg_…`), as `judge log --json` or a trace's `judge` mark names it.
        id: String,
    },
    /// Label a judgment, for the learning ledger: right or wrong, or what the right answer was.
    /// The operator's alone: refused inside a Theseus job, and from a shared place.
    Label {
        /// The judgment's id (`jdg_…`).
        id: String,
        /// The label. On the whole judgment: right, wrong, noise, or useful. On a question
        /// (`--question`): a yes-or-no question's true or false, a choice's option
        /// (`progressing`) or `not:<option>`, a score's level, or right or wrong. A content label's
        /// judgment (`theseus content SESSION` names it) is a choice of `yes` or `no`, on the flag's
        /// question (`--question secret`): `no` says the flag should not have been raised.
        label: String,
        /// The question it labels (`work_state`); none labels the whole judgment.
        #[arg(long, short)]
        question: Option<String>,
        /// Why, in a few words.
        #[arg(long)]
        note: Option<String>,
    },
    /// The learning report: per pack and question, calls, labels, precision and recall,
    /// calibration, agreement with the baseline, bands, cost, latency, and the frozen holdout.
    /// Run now, or `--date` reads a stored one.
    Report {
        /// One pack's (`loop.v1`, or `loop` for every version).
        #[arg(long, value_name = "PACK")]
        pack: Option<String>,
        /// The stored report of a local day (`2026-10-04`).
        #[arg(long)]
        date: Option<String>,
    },
    /// Replay a candidate pack version over the incumbent's recorded judgments, and print both
    /// side by side. Real Jev calls, inside `[judge] replay_limit_usd`; the candidate never acts.
    /// The operator's alone: refused inside a Theseus job, and from a shared place.
    Replay(judge_runs::ReplayArgs),
    /// Have a model profile answer a pack's questions over a seeded sample of its answered
    /// judgments, as audit labels (weight 0.5), inside `[judge] audit_limit_usd`. The operator's
    /// alone: refused inside a Theseus job, and from a shared place.
    Audit(judge_runs::AuditArgs),
    /// Rebuild a pack's judged points from the recorded history since a local day, and judge
    /// them in shadow, once each. It sends your history to Jev, so it runs only under your
    /// consent (`[judge] backfill_consent = true`). The operator's alone.
    Backfill(judge_runs::BackfillArgs),
    /// Run the learning loop for one pack now: your labels on its train split rewrite its
    /// wording as a new version, a replay checks it, and the numbers place it (or hold it).
    /// The operator's alone.
    Learn(judge_runs::LearnArgs),
    /// Correct a turn's routing: it should have run on a profile, a task class, `stronger`, or
    /// `cheaper`. Labels its route judgment as yours and moves the session there from its next
    /// turn; a later message close to it runs there too. The operator's alone: refused inside a
    /// Theseus job, and from a shared place.
    Correct(judge_correct::CorrectArgs),
    /// The live correction layer: each correction that steers a close message, newest first.
    Corrections,
    /// The prove: loop.v1's canary against its control, from the ledger's finished tasks, at
    /// equal total spend, per task and per dollar; "insufficient" with its counts until each arm
    /// has its labeled tasks. A read.
    Prove(judge_prove::ProveArgs),
}

#[derive(Subcommand, Debug)]
enum ProfileCmd {
    /// List profiles; `*` marks the live one and where the choice came from (default).
    List,
    /// Make NAME the live profile (persists across restarts).
    Use { name: String },
}

#[derive(Subcommand, Debug)]
enum SessionsCmd {
    /// List the newest 50 sessions with turns and tokens in/out (default); `--before` pages
    /// back, `--all` lists every one.
    List(pages::PageArgs),
    /// One session's execution in full: what it needs from you, what it waits on, its questions,
    /// its budget, its pending wakes, and its last ledger rows. ID is an execution's or a
    /// session's id, or at least the last four characters of either.
    Explain { id: String },
    /// Open a session and print its id; pass it to `ask -s` to keep turns together.
    Open {
        /// Human label shown in listings.
        #[arg(long)]
        label: Option<String>,
    },
    /// Ask for a recompile on the session's next turn: `fresh` keeps only the current exchange,
    /// `transcript` keeps everything with thinking stripped.
    Recompile {
        session: String,
        #[arg(long, default_value = "fresh", value_parser = ["fresh", "transcript"])]
        strategy: String,
    },
    /// Retire a session by hand: it leaves the cockpit's default (live) view and its place's next
    /// message starts a successor. Nothing is deleted; `reopen` brings it back. The operator's own.
    Retire { session: String },
    /// Reopen a retired session (by hand, superseded, or empty). A superseded one keeps its links,
    /// and its place stays on its successor. The operator's own.
    Reopen { session: String },
    /// The stop nudge for one session: `off` keeps its turns from being nudged on after a reply that
    /// says more work follows; `on` gives it back to `[judge] stop_nudge`.
    Nudge {
        session: String,
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Hold a session's turns to one model profile from its next turn (`sessions model S glm`), or
    /// let the hold go (`--off`). The floor may refuse a profile for the session's data. The
    /// operator's own.
    Model {
        session: String,
        /// The profile to hold it to.
        #[arg(required_unless_present = "off", conflicts_with = "off")]
        profile: Option<String>,
        /// Let the hold go: the session's turns route as the default does.
        #[arg(long)]
        off: bool,
    },
}

fn main() {
    // Rust ignores SIGPIPE, so a closed pipe (`theseus history | head`) would
    // panic in println!. Restore the default: exit quietly like other tools.
    #[cfg(unix)]
    // SAFETY: called once at startup before any other thread writes to stdout.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    // Bare `theseus` on a terminal opens a conversation here; a pipe, a
    // script, or any argument keeps today's usage text and exit 2.
    if let Some(code) = chat_cmd::bare() {
        std::process::exit(code);
    }
    let cli = Cli::parse();
    if let Cmd::Help { topic } = &cli.cmd {
        std::process::exit(help_cmd::run(
            &topic.iter().map(String::as_str).collect::<Vec<_>>(),
        ));
    }
    theseus_client::render::style::set_plain(cli.plain);
    // `status --short` runs on every prompt draw: one connection and one
    // request, so one thread, not a pool.
    let short = matches!(&cli.cmd, Cmd::Status(a) if a.is_short_once());
    let mut runtime = if short {
        tokio::runtime::Builder::new_current_thread()
    } else {
        tokio::runtime::Builder::new_multi_thread()
    };
    let runtime = runtime
        .enable_all()
        .build()
        .expect("the CLI's runtime starts");
    // `theseus tui` and `theseus chat` connect nothing: they become `theseus-tui` (10f).
    let result = match chat_cmd::exec_tui(&cli.cmd, &cli.socket, cli.spawn.is_some()) {
        Some(r) => r,
        None => runtime.block_on(run_or_own(cli)),
    };
    let code = match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("theseus: {e:#}");
            theseus_client::outcome::exit_code(&e)
        }
    };
    std::process::exit(code);
}

/// `theseus wait`: a session's wait, or with none named, the first of any.
async fn wait_session_or_any(
    c: &mut Conn,
    json: bool,
    session: Option<String>,
    until: String,
    after: Option<u64>,
    timeout: Option<String>,
) -> Result<()> {
    match session {
        Some(session) => cmd::wait(c, json, session, until, after, timeout).await,
        None => wait_any(c, json, &until, timeout.as_deref()).await,
    }
}

/// `theseus wait --any`, its timeout read as a duration.
async fn wait_any(c: &mut Conn, json: bool, until: &str, timeout: Option<&str>) -> Result<()> {
    let ms = timeout.map(cmd::parse_duration_ms).transpose()?;
    status::wait_any(c, json, until, ms).await
}

/// `theseus wait`: one arm of `run` (clippy holds it to 100 lines).
async fn wait_cmd(c: &mut Conn, json: bool, cmd: Cmd) -> Result<()> {
    let Cmd::Wait {
        session,
        until,
        after,
        timeout,
        ..
    } = cmd
    else {
        unreachable!("only wait comes here")
    };
    wait_session_or_any(c, json, session, until, after, timeout).await
}

/// `theseus agenda`, `card`, `digest` and `mode`: the horizon,
/// the owner card's proposals, the speaking voice, what happened while you were away, and a conversation's mode.
async fn long_cmd(c: &mut Conn, json: bool, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Agenda(a) => agenda::run(c, json, a).await,
        Cmd::Card { cmd } => card_cmd::run(c, json, cmd.unwrap_or_default()).await,
        Cmd::Digest(a) => digest::run(c, json, a).await,
        Cmd::Mode(a) => mode_cmd::run(c, json, a).await,
        _ => unreachable!("only agenda, card, digest and mode come here"),
    }
}

/// `theseus routine` and `theseus project`: recurring work and long-term work.
async fn plans_cmd(c: &mut Conn, json: bool, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Routine { cmd } => routine::run(c, json, cmd.unwrap_or_default()).await,
        Cmd::Project { cmd } => project::run(c, json, cmd.unwrap_or_default()).await,
        _ => unreachable!("only routine and project come here"),
    }
}

/// `theseus ledger`: a check of the chain, or the rows.
async fn ledger_cmd(c: &mut Conn, json: bool, l: Cmd) -> Result<()> {
    match l {
        Cmd::Ledger { cmd: Some(cmd), .. } => ledger_verify::run(c, json, cmd).await,
        Cmd::Ledger {
            n, kind, session, ..
        } => cmd::ledger(c, json, n, kind, session).await,
        _ => unreachable!("only the ledger comes here"),
    }
}

/// `theseus herdr answer|plugin|setup|popup|link` connect nothing for themselves.
async fn run_or_own(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Herdr { ref cmd } if cmd.is_own() => herdr_keys::run_own(&cli.socket, cmd).await,
        // `theseus herdr up` connects for itself.
        Cmd::Herdr {
            cmd: HerdrCmd::Up(a),
        } => herdr_up::run(&cli.socket, cli.spawn.as_deref(), cli.json, a).await,
        // Line mode: the terminal's chat was not wanted (`chat_cmd::exec_tui`).
        Cmd::Chat { .. } => chat_cmd::line_mode(cli).await,
        _ => run(cli).await,
    }
}

#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
async fn run(cli: Cli) -> Result<()> {
    // `theseus status` connects within 100 ms, and `--short` stays silent when it cannot.
    if let Cmd::Status(a) = &cli.cmd {
        return status::run(&cli.socket, cli.spawn.as_deref(), cli.json, a).await;
    }
    // `theseus postmortem --from-rows FILE` reports from kept rows and asks no daemon.
    if let Cmd::Postmortem(a) = cli.cmd {
        return postmortem::go(&cli.socket, cli.spawn.as_deref(), cli.json, a).await;
    }
    let mut conn = connect(&cli).await?;
    let (c, json, spawned) = (&mut conn, cli.json, cli.spawn.is_some());
    let result = match cli.cmd {
        Cmd::Ask(a) => cmd::ask(c, json, cli.no_stream, a, spawned).await,
        Cmd::Prompt(a) => prompt::run(c, json, cli.no_stream, a, spawned).await,
        Cmd::History {
            session,
            n,
            after,
            before,
            full,
        } => cmd::history(c, json, session, (n, after, before), full).await,
        Cmd::Why {
            turn,
            session,
            short,
        } => match short {
            true => why::context(c, json, turn).await,
            false => why::why(c, json, session, turn).await,
        },
        cmd @ Cmd::Census { .. } => census::cmd(c, json, cmd).await,
        Cmd::Open { what } => open::open(c, json, what).await,
        Cmd::Reach { node, generations } => cmd::reach(c, json, node, generations).await,
        cmd @ Cmd::Fork { .. } => fork::cmd(c, json, cmd).await,
        cmd @ Cmd::Rewind { .. } => rewind_cmd::cmd(c, json, cmd).await,
        Cmd::Places => cmd::places(c, json).await,
        Cmd::Retract { node, why } => retract::run(c, json, node, why).await,
        Cmd::Publish {
            what,
            to,
            text,
            note,
        } => cmd::publish(c, json, what, to, text, note).await,
        Cmd::Watch { all: true, .. } => cmd::watch_all(c, json).await,
        Cmd::Watch {
            session,
            thinking,
            interactive,
            no_herdr,
            ..
        } => interactive::watch(c, json, session, thinking, interactive, no_herdr).await,
        Cmd::Tools { verbose } => cmd::tools(c, json, verbose).await,
        Cmd::Policy { cmd } => cmd::policy(c, json, cmd.unwrap_or(PolicyCmd::List)).await,
        Cmd::Aws { cmd } => cmd::aws(c, json, cmd).await,
        Cmd::Google { cmd } => google_cmd::run(c, json, cmd).await,
        Cmd::Ontology { cmd } => ontology::ontology(c, json, cmd.unwrap_or_default()).await,
        Cmd::Health => cmd::health(c, json).await,
        Cmd::Sessions { page, cmd } => cmd::sessions(c, json, page, cmd).await,
        Cmd::Content(a) => content_cmd::run(c, json, a).await,
        Cmd::Budgets => budgets::budgets(c, json).await,
        Cmd::Tasks { args, cmd } => tasks_forecast::dispatch(c, json, args, cmd).await,
        Cmd::Task { cmd } => task_start::run(c, json, cmd).await,
        Cmd::Cancel { name } => cmd::cancel(c, json, name).await,
        Cmd::Remind(a) => remind::remind(c, json, a).await,
        Cmd::Answer(a) => answer::answer(c, json, a).await,
        Cmd::Wake { cmd } => remind::wake(c, json, cmd.unwrap_or_default()).await,
        Cmd::Stop { session, all } => cmd::stop(c, json, session, all).await,
        cmd @ (Cmd::Dismiss { .. } | Cmd::Retry(_)) => retry::run(c, json, cmd).await,
        w @ Cmd::Wait { .. } => wait_cmd(c, json, w).await,
        cmd @ (Cmd::Peek(_) | Cmd::Attach(_)) => jobout::board(c, json, cmd).await,
        cmd @ (Cmd::Agenda(_) | Cmd::Digest(_) | Cmd::Mode(_)) => long_cmd(c, json, cmd).await,
        Cmd::Help { .. }
        | Cmd::Status(_)
        | Cmd::Postmortem(_)
        | Cmd::Tui { .. }
        | Cmd::Chat { .. } => {
            unreachable!("above")
        }
        cmd @ Cmd::Card { .. } => long_cmd(c, json, cmd).await,
        Cmd::Profile { cmd } => cmd::profile(c, json, cmd.unwrap_or(ProfileCmd::List)).await,
        l @ Cmd::Ledger { .. } => ledger_cmd(c, json, l).await,
        Cmd::Herdr { cmd } => herdr_sync::run(c, json, cmd, (!spawned).then(|| &*cli.socket)).await,
        Cmd::Memory { cmd } => cmd::memory(c, json, cmd).await,
        Cmd::Judge { cmd } => cmd::judge(c, json, cmd).await,
        Cmd::Packs { cmd } => packs::run(c, json, cmd.unwrap_or(packs::PacksCmd::List)).await,
        Cmd::Import { cmd } => import::run(c, json, cmd.unwrap_or(import::ImportCmd::List)).await,
        Cmd::Mcp { cmd } => mcp::run(c, json, cmd).await,
        Cmd::Extend { cmd } => extend::run(c, json, cmd).await,
        Cmd::SelfImprove { cmd } => self_cmd::run(c, json, cmd).await,
        Cmd::Persona { cmd } => persona_cmd::run(c, json, cmd.unwrap_or_default()).await,
        Cmd::Standing { cmd } => standing_cmd::run(c, json, cmd.unwrap_or_default()).await,
        Cmd::WatchFor { cmd } => watch_cmd::go(c, json, cmd).await,
        Cmd::Thread { cmd } => thread::run(c, json, cmd.unwrap_or_default()).await,
        cmd @ (Cmd::Routine { .. } | Cmd::Project { .. }) => plans_cmd(c, json, cmd).await,
        Cmd::Model { cmd } => own_bench::run(c, json, cmd).await,
        Cmd::Rpc { method, params } => cmd::rpc(c, json, method, params).await,
        Cmd::Shutdown => cmd::shutdown(c, json).await,
    };
    close(conn).await;
    result
}

/// The daemon's connection: one run's own (`--spawn`), followed for its bound when it asks
///, or the socket's.
async fn connect(cli: &Cli) -> Result<Conn> {
    Ok(match (&cli.spawn, &cli.cmd) {
        (Some(bin), Cmd::Ask(a)) => Conn::spawn_with(bin, &follow::daemon_args(a.follow_for))?,
        (Some(bin), _) => Conn::spawn(bin)?,
        (None, _) => Conn::socket(&cli.socket).await?,
    })
}

/// A spawned daemon stops cleanly, not by a kill, so the next open of its
/// store replays nothing. A daemon that exits as its stop
/// is written breaks the pipe: that write fails instead of ending this
/// process, and the default comes back for what is printed after.
async fn close(conn: Conn) {
    #[cfg(unix)]
    // SAFETY: as in `main`: nothing else changes the disposition, and no
    // other thread writes to a pipe meanwhile.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let closed = conn.close().await;
    #[cfg(unix)]
    // SAFETY: as above.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(e) = closed {
        eprintln!("theseus: {e:#}");
    }
}
