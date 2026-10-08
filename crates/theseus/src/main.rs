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

mod budgets;
mod cmd;
mod extend;
mod herdr;
mod herdr_sync;
mod import;
mod interactive;
mod judge_correct;
mod judge_prove;
mod judge_runs;
mod mcp;
mod ontology;
mod packs;
mod policy_explain;
mod print;
mod prompt;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use theseus_client::Conn;

const AFTER_HELP: &str = "\
Quick start:
  export OP_SERVICE_ACCOUNT_TOKEN=...        1Password, the recommended home of the config's secrets
  theseusd &                                 start the server (or run it in the foreground)
  theseus health                             is it up, which profile is live, token totals
  theseus ask \"Say hello.\"                  one turn, streamed
  echo \"Summarize: ...\" | theseus ask --json  pipelines
  theseus profile use glm                    switch the live profile (persists)
  theseus ask -P sonnet \"...\"               one turn under another profile
  theseus ledger -n 20 -k provider.call      what every call cost and how long it took
  theseus ask -s <session> \"...\"            continue a session (its whole history is the context)
  theseus ask --attach notes.txt \"...\"      send a file with the prompt, as a Discord attachment is sent
  theseus history [session]                  a session's transcript: messages, tool calls, results
  theseus watch [session]                    follow a session live (turns started anywhere)
  theseus watch --all                        every session's executions as they change, with what needs you
  theseus tui                                every session in one terminal: what needs you, answered inline
  theseus confirm [id] [--decline]           answer a tool call or a budget question waiting for you (no id: list them)
  theseus budgets                            where the money is: each session's limit and its source, spend, tasks' carves
  theseus tasks                              background tasks (task.create): state, spend, what each waits on
  theseus wakes                              pending wakes (wake.at): session, due time, and note
  theseus reach <node>                       where a node went: the contexts that held it, and its copies in other sessions
  theseus cancel <id>                       stop a task and its jobs, or cancel a wake (its last six characters are enough)
  theseus stop <session>                     halt a session's running turn and jobs, as /stop does; the conversation goes on
  theseus wait <session> --until blocked      return once a session needs you (or settled, or terminal for a task)
  theseus executions explain <id>            one execution in full: what it waits on, its questions, budget, last rows
  theseus tools                              the toollets, their policy, and calls so far
  theseus policy tighten proc.run            should have asked: proc.run asks first from now on (untighten: undo)
  theseus policy explain --tool proc.run     why a call waits: every layer of the gate, for each place (--session: one)
  theseus policy trust <session>             after a session read a web page, its calls that act wait; this trusts it again
  theseus catalog                            models, context windows, and prices
  theseus index search \"port 7433\"           find what was said, run, or read; `index status`: how far the index has read
  theseus memory recalled <session>          what recall would have admitted on each turn, and why it dropped the rest
  theseus --spawn ask \"...\"                 no daemon: spawn theseusd on stdio for one turn, then stop it cleanly
  theseus shutdown

Web UI:      http://127.0.0.1:7433/  (while theseusd runs)
Exit codes:  0 ok · 1 server or provider error · 2 usage · 3 cannot connect
             ask: 5 spend limit · 6 waits for approval · 7 refused · 8 cut by a limit · 9 stopped
More:        theseus <command> --help";

/// `theseus ask --help`'s exit codes (theseus-n88g.2): how the turn ended,
/// for a script or a benchmark's harness that runs it headless.
const ASK_EXIT_CODES: &str = "\
Exit codes, by how the turn ended (`--json` gives its stop_reason in full):
  0  done: the model ended its turn
  1  failed: a provider's or a tool's fault, or the daemon's error
  2  usage · 3 cannot connect or spawn theseusd
  5  the session reached its spend limit; the turn waits for the operator to reset it
  6  a call waits for the operator's approval (`theseus confirm`), which a headless run cannot give
  7  the model refused
  8  a limit ended the turn before the model did: the loop cap, the output limit, or the context window
  9  stopped: by an operator (/stop, `theseus stop`), or, under --spawn, by a SIGINT or SIGTERM, which
     stops the turn as /stop does, with what it spent, before the spawned daemon's clean stop
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
    args_conflicts_with_subcommands = false
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

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Send one prompt as one turn and print the reply. Reads stdin when PROMPT is omitted or "-".
    /// The exit code says how the turn ended.
    #[command(after_help = ASK_EXIT_CODES)]
    Ask(AskArgs),
    /// Run an MCP server's prompt as a turn's input and print the reply, as `ask` does.
    /// `theseus mcp` lists the prompts. The exit code says how the turn ended.
    #[command(after_help = ASK_EXIT_CODES)]
    Prompt(prompt::PromptArgs),
    /// A session's transcript: messages, tool calls with their gate decisions, results, and
    /// anything waiting for your confirmation. SESSION defaults to the most recently active.
    History {
        session: Option<String>,
        /// Only the newest N nodes; with --after or --before, a page's size (default 200).
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
    /// Where a node went (node.reach): the compilations and loops of its own session whose
    /// context held it, then its copies in other sessions over derived_from (a task's report in
    /// its parent, a task's brief from the reply that started it), each with theirs. NODE is a
    /// node's id, or its last 6 or more characters, as `theseus history` prints it (msg·a1b2c3)
    /// and the cockpit's Nodes list (Ledger) shows it.
    Reach {
        #[arg(value_name = "NODE")]
        node: String,
        /// How many generations of copies to follow (default 3, at most 16).
        #[arg(long, value_name = "N")]
        generations: Option<u32>,
    },
    /// Each place Theseus speaks in, and its class (the place rule): private (the CLI, the web
    /// UI, a DM with you, a guild channel bound `private = true`) gets everything; shared (any
    /// other guild channel) gets its own conversation and the public tools alone.
    Places,
    /// Publish into a place (the place rule): one item into a place's conversation, as your
    /// message there, and said in the place: a node by id, a file you can read, or a message
    /// (`--text`). Only you, from a private place (the CLI is one), may. PLACE is as
    /// `theseus places` names it (`#openclaw`).
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
    /// The ontology: the kinds of context, the categories (a bound place's channel or person,
    /// and the topics you declare) with their guidance, and sessions' memberships. A session's
    /// system block carries the guidance of the categories it is in, from its next recompile.
    Ontology {
        #[command(subcommand)]
        cmd: Option<OntologyCmd>,
    },
    /// Follow a session live: streamed text, tool calls, confirmations, context decisions,
    /// whoever started the turn (web UI, CLI, the harness). SESSION defaults to the most recent.
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
    Tui {
        /// Passed to theseus-tui: --notify bell|osc9|osc777|off, --help.
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<String>,
    },
    /// Answer a tool call waiting for your confirmation, or a session at its spend limit
    /// (approve resets its spend to $0), then follow the turn it resumes.
    /// Without an id, list everything waiting.
    Confirm(ConfirmArgs),
    /// The toollets: class, backend, what policy does with each, and calls so far.
    Tools {
        /// Also print each tool's description and input schema.
        #[arg(long, short)]
        verbose: bool,
    },
    /// "Should have asked": make a tool ask first from now on, undo it, or list every tool's
    /// posture and what set it. A tightening is stored, never in the config, and only makes a
    /// tool stricter; an undo returns the tool to what the config says.
    Policy {
        #[command(subcommand)]
        cmd: Option<PolicyCmd>,
    },
    /// Theseus's AWS account: `bootstrap` plans its first stacks, read-only, and applies them on
    /// your yes.
    Aws {
        #[command(subcommand)]
        cmd: AwsCmd,
    },
    /// The model catalog: context windows, output limits, prices per million tokens.
    Catalog,
    /// Server health: version, live profile, providers, sessions, turns, provider errors, token totals.
    Health,
    /// Sessions: list them with per-session token totals, or open one to continue across turns.
    Sessions {
        #[command(subcommand)]
        cmd: Option<SessionsCmd>,
    },
    /// Where the money is: each open session's limit and where it comes from, spent, reserved,
    /// held, available, and lifetime cost, its resets and any budget question, each task under
    /// its parent with its carve, and the totals.
    Budgets,
    /// Executions (one per session): state, turns, outstanding actions, budget; `executions cancel <id>`.
    Executions {
        #[command(subcommand)]
        cmd: Option<ExecutionsCmd>,
    },
    /// Tasks: the background sessions conversations started with task.create, the newest
    /// first, with state, spend of their carved limit, and what each waits on.
    Tasks {
        /// Only the tasks this session started.
        #[arg(long, short)]
        session: Option<String>,
    },
    /// Wakes: the turns conversations asked for at a time with wake.at that have not run yet,
    /// soonest first, with session, due time, and note.
    Wakes {
        /// Only this session's wakes.
        #[arg(long, short)]
        session: Option<String>,
    },
    /// Cancel a pending wake, so nothing fires; or stop a task and its jobs, as `executions
    /// cancel` does, and its place hears it once. ID is its id or its last six characters, as
    /// `theseus wakes` and `theseus tasks` show it.
    Cancel {
        #[arg(value_name = "ID")]
        name: String,
    },
    /// Wait until a session needs you, settles, or ends, then print its state and its questions
    /// (session.wait). The daemon owns the wait, so nothing polls, and a wait already satisfied
    /// answers at once. SESSION is its id, or at least its last four characters. Exit codes: 0
    /// reached, 4 timed out, and 1, 2, and 3 as for every command.
    Wait {
        #[arg(value_name = "SESSION")]
        session: String,
        /// blocked (it needs you), settled (nothing runs or is queued for it: it needs you, is
        /// ready, or ended; a job or a child task still runs), or terminal (a task ended).
        #[arg(long, default_value = "settled", value_parser = ["blocked", "settled", "terminal"])]
        until: String,
        /// Only a change after this WAL position counts (one `theseus watch --all` printed, or
        /// a previous wait's).
        #[arg(long, value_name = "POSITION")]
        after: Option<u64>,
        /// How long to wait: 90s, 10m, 2h (default 10m, at most 24h).
        #[arg(long, value_name = "DURATION")]
        timeout: Option<String>,
    },
    /// Halt what a session is doing, as Discord's `/stop` does: its running turn, its jobs, and
    /// what waits on you. The conversation goes on: the next `ask -s` continues it, and its tasks
    /// and wakes go on too (`theseus cancel <id>` stops one). SESSION is its id, or at least its
    /// last four characters.
    Stop {
        #[arg(value_name = "SESSION")]
        session: String,
    },
    /// Model profiles: list, or switch the live one (`theseus profile use glm`).
    Profile {
        #[command(subcommand)]
        cmd: Option<ProfileCmd>,
    },
    /// Recent ledger rows (every turn, loop, provider call, state change, error).
    Ledger {
        /// How many rows.
        #[arg(short, long, default_value_t = 20)]
        n: usize,
        /// Only rows of this kind, e.g. turn.ended, provider.call, provider.error.
        #[arg(short, long)]
        kind: Option<String>,
        #[arg(short, long)]
        session: Option<String>,
    },
    /// herdr, the terminal workspace manager: `theseus herdr sync` gives each session that needs
    /// you or is working a pane in herdr, running `theseus watch --interactive`.
    Herdr {
        #[command(subcommand)]
        cmd: herdr_sync::HerdrCmd,
    },
    /// The index (M6): `index status` says how far the index tender has read the store and what
    /// it holds; `index search <QUERY>` finds what was said, run, and read, by its words and
    /// names (BM25, exact entities, and vectors once the model's files are installed).
    Index {
        #[command(subcommand)]
        cmd: IndexCmd,
    },
    /// Recall (M6): `memory search <QUERY>` runs recall's pipeline for a query, writing nothing;
    /// `memory recalled <SESSION>` shows what each of its turns would have recalled, in shadow.
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },
    /// Jev's judgments (M5): `judge log` lists the recent ones, each with its pack, session,
    /// answers, and cost; `judge show <id>` shows one whole.
    Judge {
        #[command(subcommand)]
        cmd: JudgeCmd,
    },
    /// The ladder (M5): each Jev pack's mode, share and why, its rollback rules and last rows;
    /// `packs promote <pack> --canary <share> | --live` and `packs rollback <pack>` move one.
    Packs {
        #[command(subcommand)]
        cmd: Option<packs::PacksCmd>,
    },
    /// The operator's past history (theseus-0lrr.6): `import openclaw <file>...` brings episode
    /// files in as imported sessions, closed and private; `import erase --tag <tag>` and
    /// `import list`.
    Import {
        #[command(subcommand)]
        cmd: Option<import::ImportCmd>,
    },
    /// The MCP servers the config attaches (M7): each server's state, and its tools with their
    /// postures and classes; `mcp restart <NAME>` starts one again, a failed one included.
    Mcp {
        #[command(subcommand)]
        cmd: Option<mcp::McpCmd>,
    },
    /// Extensions (M7 43a, 43b): `extend list` shows each proposal with its state, its frozen
    /// digest, its tools and tests, and the question `theseus confirm` answers, and each loaded
    /// one; `extend revoke <NAME>` stops a loaded one and drops its tools.
    Extend {
        #[command(subcommand)]
        cmd: extend::ExtendCmd,
    },
    /// Send a raw JSON-RPC request (e.g. `rpc health`, `rpc turn.submit '{"input":"hi"}'`); notifications echo to stderr.
    Rpc {
        method: String,
        params: Option<String>,
    },
    /// Ask the server to stop cleanly (removes its socket).
    Shutdown,
}

#[derive(Args, Debug)]
struct AskArgs {
    prompt: Option<String>,
    /// Continue an existing session instead of opening a new one.
    #[arg(long, short)]
    session: Option<String>,
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
}

#[derive(Args, Debug)]
struct ConfirmArgs {
    correlation_id: Option<String>,
    /// Approve, which is what an answer does unless --decline says otherwise.
    #[arg(long, conflicts_with = "decline")]
    approve: bool,
    /// Decline instead of approve (the model is told, and carries on without it; a
    /// session at its limit keeps waiting, and its next message asks again).
    #[arg(long, alias = "deny")]
    decline: bool,
    /// A note for the ledger and, on a decline, for the model.
    #[arg(long)]
    note: Option<String>,
    /// Return as soon as the answer is recorded instead of following the resumed turn.
    #[arg(long)]
    no_wait: bool,
    /// Approve, and trust the call's session again: it no longer holds external text, so
    /// its later calls that act run at their postures (as `theseus policy trust` does).
    #[arg(long, conflicts_with = "decline")]
    trust: bool,
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
}

#[derive(Subcommand, Debug)]
enum OntologyCmd {
    /// The kinds table: where each kind's memberships come from, how many a session holds, its
    /// precedence, and its rule (default).
    Kinds,
    /// The category tree, with each category's guidance.
    Categories,
    /// Declare a topic.
    Topic {
        #[command(subcommand)]
        cmd: TopicCmd,
    },
    /// Set CATEGORY's guidance (its id, or a topic's name): TEXT, or stdin with `-` or nothing.
    /// Empty text takes it away. Only you, from a private place (the CLI is one), may.
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
    /// Jev's unanswered proposals (categorize.v1, in shadow), newest first: a topic a session
    /// is not in, or a new topic, with Jev's confidence and band.
    Proposals {
        #[arg(long, value_name = "SESSION")]
        session: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Accept a proposal: the session joins the topic (yours, `operator`), and the judgment is
    /// labelled. For a new topic, --topic names it: an existing topic, or a new one (--desc).
    Accept {
        judgment: String,
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        desc: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Reject a proposal: the judgment is labelled, and nothing else changes.
    Reject {
        judgment: String,
        #[arg(long)]
        note: Option<String>,
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
enum ExecutionsCmd {
    /// List every execution with state, turns, outstanding actions, and budget.
    List,
    /// Cancel an execution: deterministic control path, terminates its jobs.
    Cancel { execution_id: String },
    /// One execution in full: what it needs from you, what it waits on, its questions, its
    /// budget, its pending wakes, and its session's last ledger rows. ID is an execution's or a
    /// session's id, or at least the last four characters of either.
    Explain { id: String },
}

#[derive(Subcommand, Debug)]
enum IndexCmd {
    /// The index and its tender: state, nodes and chunks, how far behind the store it is, its
    /// process, and its vectors.
    Status,
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
    /// The recent judgments, oldest first: time, pack and mode, session, answers (each with its
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
    /// One judgment: its pack, mode, outcome, timing, and cost; the state Jev was sent, as
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
        /// (`progressing`) or `not:<option>`, a score's level, or right or wrong.
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
    /// Correct a turn's routing: it should have run on a profile, a mode, `stronger`, or
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
    /// List sessions with turns and tokens in/out (default).
    List,
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
}

#[tokio::main]
async fn main() {
    // Rust ignores SIGPIPE, so a closed pipe (`theseus history | head`) would
    // panic in println!. Restore the default: exit quietly like other tools.
    #[cfg(unix)]
    // SAFETY: called once at startup before any other thread writes to stdout.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    let code = match run(cli).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("theseus: {e:#}");
            theseus_client::outcome::exit_code(&e)
        }
    };
    std::process::exit(code);
}

#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
async fn run(cli: Cli) -> Result<()> {
    // `theseus tui` connects nothing itself: it becomes `theseus-tui` (10f).
    if let Cmd::Tui { args } = &cli.cmd {
        return cmd::tui(&cli.socket, cli.spawn.is_some(), args);
    }
    let mut conn = match &cli.spawn {
        Some(bin) => Conn::spawn(bin)?,
        None => Conn::socket(&cli.socket).await?,
    };
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
        Cmd::Reach { node, generations } => cmd::reach(c, json, node, generations).await,
        Cmd::Places => cmd::places(c, json).await,
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
        Cmd::Confirm(a) => cmd::confirm(c, json, a).await,
        Cmd::Tools { verbose } => cmd::tools(c, json, verbose).await,
        Cmd::Policy { cmd } => cmd::policy(c, json, cmd.unwrap_or(PolicyCmd::List)).await,
        Cmd::Aws { cmd } => cmd::aws(c, json, cmd).await,
        Cmd::Ontology { cmd } => {
            ontology::ontology(c, json, cmd.unwrap_or(OntologyCmd::Kinds)).await
        }
        Cmd::Catalog => cmd::catalog(c, json).await,
        Cmd::Health => cmd::health(c, json).await,
        Cmd::Sessions { cmd } => cmd::sessions(c, json, cmd.unwrap_or(SessionsCmd::List)).await,
        Cmd::Budgets => budgets::budgets(c, json).await,
        Cmd::Executions { cmd } => {
            cmd::executions(c, json, cmd.unwrap_or(ExecutionsCmd::List)).await
        }
        Cmd::Tasks { session } => cmd::tasks(c, json, session).await,
        Cmd::Wakes { session } => cmd::wakes(c, json, session).await,
        Cmd::Cancel { name } => cmd::cancel(c, json, name).await,
        Cmd::Stop { session } => cmd::stop(c, json, session).await,
        Cmd::Wait {
            session,
            until,
            after,
            timeout,
        } => cmd::wait(c, json, session, until, after, timeout).await,
        Cmd::Profile { cmd } => cmd::profile(c, json, cmd.unwrap_or(ProfileCmd::List)).await,
        Cmd::Ledger { n, kind, session } => cmd::ledger(c, json, n, kind, session).await,
        Cmd::Herdr { cmd } => {
            let socket = cli.spawn.is_none().then_some(cli.socket.as_str());
            herdr_sync::run(c, json, cmd, socket).await
        }
        Cmd::Index { cmd } => cmd::index(c, json, cmd).await,
        Cmd::Memory { cmd } => cmd::memory(c, json, cmd).await,
        Cmd::Judge { cmd } => cmd::judge(c, json, cmd).await,
        Cmd::Packs { cmd } => packs::run(c, json, cmd.unwrap_or(packs::PacksCmd::List)).await,
        Cmd::Import { cmd } => import::run(c, json, cmd.unwrap_or(import::ImportCmd::List)).await,
        Cmd::Mcp { cmd } => mcp::run(c, json, cmd).await,
        Cmd::Extend { cmd } => extend::run(c, json, cmd).await,
        Cmd::Rpc { method, params } => cmd::rpc(c, json, method, params).await,
        Cmd::Shutdown => cmd::shutdown(c, json).await,
        Cmd::Tui { .. } => unreachable!("`theseus tui` execs theseus-tui before connecting"),
    };
    // A spawned daemon stops cleanly, not by a kill, so the next open of its
    // store replays nothing (theseus-n88g.2). A daemon that exits as its stop
    // is written breaks the pipe: that write fails instead of ending this
    // process, and the default comes back for what is printed after.
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
    result
}
