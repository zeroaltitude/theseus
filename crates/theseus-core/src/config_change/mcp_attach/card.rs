//! The cards of `mcp.attach` and `mcp.detach` (issue 22): built here, by the
//! daemon, from the server's table as it will be written (parsed back from the
//! candidate) and from what its trial listed.
//!
//! **Everything is shown whole, as escaped one-line literals, or there is no
//! card.** What will run (each argument of the command, or the address), each
//! environment variable and the secret it names, each host and each read-only
//! tool is a quoted literal in which a quote, a backslash, a line break and
//! anything that is not plainly visible text is escaped, so no value can end its
//! line, begin another or pass for the card's own words. And **no value of the
//! model's or the server's ever begins a line**: each follows a fixed label.
//! What cannot be shown whole within [`fits`]'s limits is refused before a card
//! is made. The one thing cut is a tool's description, which is not what will
//! run: it is cut to one line and says `(cut)`.

use theseus_mcp::types::Tool as Listed;

use crate::config::mcp::McpSandbox;
use crate::config::McpServerConfig;

/// The most tools a card lists whole, name and description, and the most it
/// lists at all (the rest by name alone); a server that lists more is refused.
pub const DESCRIBED: usize = 40;
pub const MAX_TOOLS: usize = 200;
/// The longest a tool's name may be, to be shown whole.
pub const NAME_CHARS: usize = 128;
/// The longest a tool's description is shown, in characters.
pub const DESCRIPTION_CHARS: usize = 120;
/// The longest the trial's own words are shown, in characters.
pub const SAID_CHARS: usize = 200;
/// What a card shows whole, at most: the command, the address, and each list.
pub const COMMAND_CHARS: usize = 1_500;
pub const URL_CHARS: usize = 500;
pub const MAX_ENTRIES: usize = 20;
pub const ENTRY_CHARS: usize = 100;

/// What the trial found: the tools it listed, or why it did not.
pub struct Tried {
    pub tools: Vec<Listed>,
    pub error: Option<String>,
}

/// What a card says.
pub struct Facts<'a> {
    pub name: &'a str,
    pub server: &'a McpServerConfig,
    pub tried: &'a Tried,
    pub outside_text: bool,
    pub reason: &'a str,
    /// The `[secrets]` entries it names that the file does not have.
    pub missing: &'a [String],
    /// Those the file has and this daemon has not loaded (it started before).
    pub unloaded: &'a [String],
    /// What the model said of where it found the command or address: shown
    /// under it as its word, never as a check (`source.rs`).
    pub source: Option<&'a str>,
}

/// A character that shows as nothing, turns the text around it, or breaks a
/// line where the card is shown (the line and paragraph separators).
fn invisible(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{e0000}'..='\u{e007f}')
}

/// `s` as a one-line quoted literal: a quote, a backslash, a line break, and
/// anything invisible or direction-changing escaped, so it can end nothing
/// and begin nothing.
pub fn lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if invisible(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A list of literals: `"a", "b"`.
fn lits<'a>(items: impl IntoIterator<Item = &'a String>) -> String {
    items
        .into_iter()
        .map(|s| lit(s))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The first line of `s`, cut at `max` characters: the literal, and `(cut)`
/// when anything of `s` is left out.
fn cut(s: &str, max: usize) -> String {
    let first = s.lines().next().unwrap_or("").trim();
    let more = s.trim().lines().nth(1).is_some();
    let (shown, long) = crate::scrub::safe::head(first, max);
    match more || long {
        true => format!("{} (cut)", lit(shown)),
        false => lit(shown),
    }
}

/// Whether what will run, and what it is given, can be shown whole: or why
/// not, for the refusal. Called before anything starts.
pub fn fits(s: &McpServerConfig) -> Result<(), String> {
    let too_long = |what: &str, n: usize, max: usize| {
        format!(
            "the {what} is longer than a card can show whole ({n} characters, at most {max}): \
             nothing was tried and nothing was asked"
        )
    };
    let command: usize = s.command.iter().map(|a| lit(a).chars().count() + 2).sum();
    if command > COMMAND_CHARS {
        return Err(too_long("command", command, COMMAND_CHARS));
    }
    if let Some(u) = &s.url {
        let n = lit(u).chars().count();
        if n > URL_CHARS {
            return Err(too_long("address", n, URL_CHARS));
        }
    }
    let lists = [
        ("environment list", s.env.len()),
        ("list of hosts", s.egress.len()),
        ("list of read-only tools", s.read.len()),
    ];
    for (what, n) in lists {
        if n > MAX_ENTRIES {
            return Err(format!(
                "the {what} has {n} entries, more than a card can show whole ({MAX_ENTRIES}): \
                 nothing was tried and nothing was asked"
            ));
        }
    }
    let entries = s
        .env
        .iter()
        .flat_map(|(k, v)| [k, v])
        .chain(s.egress.iter())
        .chain(s.read.iter())
        .chain(s.auth_secret.iter());
    if let Some(e) = entries
        .into_iter()
        .find(|e| lit(e).chars().count() > ENTRY_CHARS)
    {
        return Err(too_long("entry", lit(e).chars().count(), ENTRY_CHARS));
    }
    Ok(())
}

/// What will run or be reached, and where.
fn what_runs(s: &McpServerConfig) -> String {
    match &s.url {
        Some(u) => format!(
            "Address, reached over HTTP (Theseus does not start it; it runs elsewhere): {}\n",
            lit(u)
        ),
        None => {
            let place = match s.sandbox {
                McpSandbox::L1 => {
                    "It runs in Theseus's sandbox, which sees the workspace read-only and hides \
                     Theseus's own files."
                }
                McpSandbox::L0 => {
                    "It runs OUTSIDE the sandbox, as Theseus's own child: it can reach whatever \
                     Theseus itself can."
                }
            };
            format!(
                "{place}\nCommand, one quoted entry per argument, run directly with no shell: [{}]\n",
                lits(&s.command)
            )
        }
    }
}

fn host_of(url: &str) -> &str {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(url)
}

/// Which hosts it may reach.
fn hosts(s: &McpServerConfig) -> String {
    match (&s.url, s.sandbox) {
        (Some(u), _) => format!(
            "Hosts it may reach: {}, the address above.\n",
            lit(host_of(u))
        ),
        (None, McpSandbox::L1) if s.egress.is_empty() => {
            "Hosts it may reach: none, it has no network at all.\n".into()
        }
        (None, McpSandbox::L1) => format!("Hosts it may reach: {}.\n", lits(&s.egress)),
        (None, McpSandbox::L0) => {
            "Hosts it may reach: whatever Theseus itself can reach (it is outside the sandbox).\n"
                .into()
        }
    }
}

/// Which secrets it is given, by name.
fn secrets(s: &McpServerConfig) -> String {
    let mut out = String::new();
    for (var, name) in &s.env {
        out.push_str(&format!(
            "Secret, by name: the variable {} is given the secret {}.\n",
            lit(var),
            lit(name)
        ));
    }
    if let Some(name) = &s.auth_secret {
        out.push_str(&format!(
            "Secret, by name: its key is the secret {}.\n",
            lit(name)
        ));
    }
    match out.is_empty() {
        true => "Secrets: none.\n".into(),
        false => {
            out.push_str(
                "A secret's value comes from the vault when the server starts; it never passes \
                 through this conversation, this card or a log, and the trial ran without it.\n",
            );
            out
        }
    }
}

/// The tools the trial listed: every name whole, the first few with a line of
/// their description.
fn tools(t: &Tried, s: &McpServerConfig) -> Result<String, String> {
    if let Some(e) = &t.error {
        let needs = match s.secrets().next() {
            Some(_) => {
                "Tools: not listed. It needs its secret to start, and the trial gives it none, \
                 or it could not start."
            }
            None => "Tools: not listed. The trial could not list its tools.",
        };
        return Ok(format!(
            "{needs} The trial said: {}\nYou may still say yes; then Theseus offers whatever it \
             lists once it starts.\n",
            cut(e, SAID_CHARS)
        ));
    }
    let n = t.tools.len();
    if n > MAX_TOOLS {
        return Err(format!(
            "the server lists {n} tools, more than a card can show whole ({MAX_TOOLS}): nothing \
             was asked"
        ));
    }
    if let Some(long) = t.tools.iter().find(|x| x.name.chars().count() > NAME_CHARS) {
        return Err(format!(
            "the server lists a tool whose name is longer than {NAME_CHARS} characters ({}), too \
             long to show whole: nothing was asked",
            long.name.chars().count()
        ));
    }
    let mut out =
        format!("Tools it listed in the trial ({n}); descriptions are the server's own words:\n");
    for tool in t.tools.iter().take(DESCRIBED) {
        out.push_str(&format!(
            "- tool {}: {}\n",
            lit(&tool.name),
            cut(
                tool.description.as_deref().unwrap_or("no description"),
                DESCRIPTION_CHARS
            )
        ));
    }
    if n > DESCRIBED {
        let rest: Vec<String> = t.tools[DESCRIBED..]
            .iter()
            .map(|x| x.name.clone())
            .collect();
        out.push_str(&format!("More tools, names only: {}\n", lits(&rest)));
    }
    Ok(out)
}

/// Which tools are read-only: only the ones the owner named.
fn read_only(t: &Tried, s: &McpServerConfig) -> String {
    if s.read.is_empty() {
        return "Read-only: none. Every tool asks first, as any action does.\n".into();
    }
    let listed = |r: &String| t.tools.iter().any(|x| &x.name == r);
    let unknown: Vec<String> = s
        .read
        .iter()
        .filter(|r| !listed(r) && t.error.is_none())
        .cloned()
        .collect();
    let mut out = format!(
        "Read-only: only the tools you named, {}; every other tool asks first. A server's own \
         hints never make a tool read-only.\n",
        lits(&s.read)
    );
    if !unknown.is_empty() {
        out.push_str(&format!(
            "The server did not list {} though you named it read-only: that has no effect.\n",
            lits(&unknown)
        ));
    }
    out
}

fn trial_line(t: &Tried) -> &'static str {
    match &t.error {
        None => {
            "Tried once in the sandbox with no secret and no network beyond what was asked: it \
             started and listed its tools.\n"
        }
        Some(_) => {
            "Tried once in the sandbox with no secret and no network beyond what was asked: it \
             did not list its tools.\n"
        }
    }
}

fn tail(f: &Facts<'_>, out: &mut String) {
    if f.outside_text {
        out.push_str(
            "Take care: this conversation has read text from outside (a page, a search or a \
             server) since your last message, so the request may not be only yours.\n",
        );
    }
    let why = super::super::card::reason_line(f.reason);
    if !why.is_empty() {
        out.push_str(&format!(
            "Theseus's reason, in its own words: {}\n",
            lit(&why)
        ));
    }
}

/// The line that says a server's results are not marked as outside text, when
/// the owner asked for that: on a line of its own.
fn external(s: &McpServerConfig) -> &'static str {
    match s.external {
        true => "Its results are outside text, and Theseus marks them so.\n",
        false => {
            "Its results will NOT be marked as outside text, because external = false was asked \
             for: a page or message that server returns would be read as your own words, and \
             nothing would warn you.\n"
        }
    }
}

/// The question the owner is asked to attach a server.
pub fn attach(f: &Facts<'_>) -> Result<String, String> {
    let mut out = format!("Attach the MCP server {}?\n", lit(f.name));
    out.push_str(&what_runs(f.server));
    if let Some(said) = f.source {
        out.push_str(&super::super::source::line(said));
    }
    out.push_str(&tools(f.tried, f.server)?);
    out.push_str(&read_only(f.tried, f.server));
    out.push_str(&secrets(f.server));
    out.push_str(&hosts(f.server));
    out.push_str(external(f.server));
    out.push_str(trial_line(f.tried));
    out.push_str(
        "Its tools are offered from your next message, with no restart. It can be removed again \
         by asking.\n",
    );
    // A server given a secret or a host needs the code too (issue 57, `needs`).
    let s = f.server;
    if s.secrets().next().is_some() || s.url.is_some() || !s.egress.is_empty() {
        out.push_str(
            "Attaching a server can give it secrets and a network, so this needs a second proof as \
             well as your yes.\n",
        );
    }
    tail(f, &mut out);
    Ok(out)
}

/// What the owner is told when the server needs a secret the settings do not
/// yet have: no yes can attach it, so no question is asked.
pub fn blocked(f: &Facts<'_>) -> Result<String, String> {
    let mut out = format!("Not attached yet: {} needs a secret first.\n", lit(f.name));
    if !f.missing.is_empty() {
        out.push_str(&format!(
            "The settings have no [secrets] entry named {}. Add each from the vault with \
             `vault.use` (a change to [secrets], which needs the second proof too), then ask for \
             the server again. Its value is never typed into this conversation.\n",
            lits(f.missing)
        ));
    }
    if !f.unloaded.is_empty() {
        out.push_str(&format!(
            "The settings have {}, but Theseus has not loaded it since it started: restart \
             Theseus, then ask for the server again.\n",
            lits(f.unloaded)
        ));
    }
    out.push_str(&what_runs(f.server));
    out.push_str(&tools(f.tried, f.server)?);
    out.push_str(&secrets(f.server));
    out.push_str(trial_line(f.tried));
    out.push_str("Nothing was asked of you and nothing was changed.\n");
    tail(f, &mut out);
    Ok(out)
}

/// What a detach puts to the owner: what stops and when.
pub struct Detach<'a> {
    pub name: &'a str,
    pub server: &'a McpServerConfig,
    pub tools: &'a [String],
    pub restart: bool,
    pub wait_secs: u64,
    pub outside_text: bool,
    pub reason: &'a str,
}

pub fn detach(d: &Detach<'_>) -> String {
    let mut out = format!("Remove the MCP server {}?\n", lit(d.name));
    out.push_str(&what_runs(d.server));
    match d.tools.is_empty() {
        true => out.push_str("Tools offered now: none listed.\n"),
        false => out.push_str(&format!("Tools that go: {}.\n", lits(d.tools))),
    }
    match d.restart {
        false => out.push_str("It stops now, and its tools are gone from your next message.\n"),
        true => out.push_str(&format!(
            "Theseus started it from the settings file, so it stops at a restart: Theseus \
             restarts itself to apply this. A restart cuts what is running (a turn, a job, the \
             self-improvement loop's build); it waits up to {} s for a running turn to end \
             first, and if the new settings do not load or Theseus is not healthy afterwards it \
             goes back to the settings it has now.\n",
            d.wait_secs
        )),
    }
    let f = Facts {
        name: d.name,
        server: d.server,
        tried: &Tried {
            tools: vec![],
            error: None,
        },
        outside_text: d.outside_text,
        reason: d.reason,
        missing: &[],
        unloaded: &[],
        source: None,
    };
    tail(&f, &mut out);
    out
}
