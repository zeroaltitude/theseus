//! A question in plain words (theseus-cudar): who asks, what the call does
//! in a verb and a friendly object, the model's own reason, a risk badge,
//! and what Yes and No do. Worded once, here, from the request and what the
//! core knows around it (`Gloss`), so the Discord card, its settle, `theseus
//! answer` and the TUI all say the same. `ConfirmRequest::plain` carries it.

use std::path::{Path, PathBuf};

use serde_json::Value;
use theseus_protocol::term::words::summarize;
use theseus_protocol::{ConfirmRequest, PlainWords};

use super::words::short;
use crate::toolrun::misroute;

/// What the core knows around a request that the request does not carry.
pub struct Gloss {
    /// The daemon's state dir: its reports, gates and store live under it.
    pub state: PathBuf,
    /// Who asks (`Core::asker_name`).
    pub who: String,
    /// The asking task's id, so a report of its own can be told from another's.
    pub own_task: Option<String>,
    /// The model's words just before the call, when it said any.
    pub model_why: Option<String>,
}

/// The model's reason: the last line it wrote before the call, one line.
pub fn reason_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    let line = line.trim_start_matches(['#', '-', '*', ' ']).trim();
    (!line.is_empty()).then(|| short(line, 160))
}

/// A time of day, `05:12`, from a file's modified time.
fn clock_of(p: &Path) -> Option<String> {
    let ms = std::fs::metadata(p)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let l = crate::wake::local(u64::try_from(ms).ok()?);
    Some(format!("{:02}:{:02}", l.hour, l.minute))
}

fn home_short(p: &Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && s.starts_with(&h) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

/// A path as a person names it. Never an elided path: a long one keeps its
/// end (`words::short`).
pub fn object_of(path: &str, g: &Gloss) -> String {
    let p = crate::config::expand_home(path);
    let rel = p.strip_prefix(&g.state).ok().map(Path::to_path_buf);
    let at = |dir: &str| rel.as_ref().and_then(|r| r.strip_prefix(dir).ok());
    if let Some(name) = at("self/reports").and_then(|r| r.file_name()) {
        let name = name.to_string_lossy();
        let when = clock_of(&p).map_or_else(String::new, |c| format!(" from the {c} run"));
        return match g.own_task.as_deref().filter(|t| name.contains(*t)) {
            Some(_) => format!("its own report{when}"),
            None => format!("another self-repair run's report{when}"),
        };
    }
    if at("self/gates").is_some() {
        let when = clock_of(&p).map_or_else(String::new, |c| format!(" from the {c} run"));
        return format!("a self-repair gate script or log{when}");
    }
    if let Some(r) = at("self/trees") {
        let inner: PathBuf = r.components().skip(1).collect();
        return match inner.as_os_str().is_empty() {
            true => "its working tree".into(),
            false => format!(
                "`{}` in its working tree",
                short(&inner.display().to_string(), 70)
            ),
        };
    }
    if at("store").is_some() {
        return "Theseus's own store (conversation content)".into();
    }
    if at("spool").is_some() {
        return "Theseus's own spool".into();
    }
    if let Some(r) = &rel {
        return match r.as_os_str().is_empty() {
            true => "Theseus's whole state folder".into(),
            false => format!(
                "`{}` in Theseus's state folder",
                short(&r.display().to_string(), 70)
            ),
        };
    }
    let shown = home_short(&p);
    for (dir, name) in [
        ("/.ssh", "your SSH keys"),
        ("/.aws", "your AWS credentials"),
        ("/.gnupg", "your GPG keys"),
        ("/.config/op", "the 1Password CLI's credentials"),
        ("1password-service-token", "a 1Password service token"),
    ] {
        if shown.contains(dir) {
            return format!("{name} (`{}`)", short(&shown, 60));
        }
    }
    format!("`{}`", short(&shown, 90))
}

fn text<'a>(input: &'a Value, k: &str) -> Option<&'a str> {
    input.get(k).and_then(Value::as_str)
}

/// The verb and its object for a call.
pub fn what_of(req: &ConfirmRequest, g: &Gloss) -> String {
    let (tool, input) = (req.tool.as_str(), &req.input);
    let path = text(input, "path").map(|p| object_of(p, g));
    let argv = input.get("argv").and_then(Value::as_array).map(|a| {
        let w: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
        short(&w.join(" "), 110)
    });
    let pat = text(input, "pattern").map(|p| short(p, 60));
    match (tool, path, argv, pat) {
        ("fs.read", Some(p), ..) => format!("read {p}"),
        ("fs.list", Some(p), ..) => format!("list {p}"),
        ("fs.grep", Some(p), _, Some(pat)) => format!("search {p} for `{pat}`"),
        ("fs.glob", Some(p), _, Some(pat)) => format!("find files matching `{pat}` in {p}"),
        ("fs.edit", Some(p), ..) => format!("edit {p}"),
        ("fs.write", Some(p), ..) => format!("write {p}"),
        ("fs.patch", Some(p), ..) => format!("patch {p}"),
        ("git.log" | "git.diff", Some(p), ..) => {
            format!("look at the git {} in {p}", tool.trim_start_matches("git."))
        }
        ("proc.run", _, Some(a), _) => format!("run `{a}`"),
        ("self.evidence", ..) => match text(input, "kind") {
            Some("cause") => format!(
                "read the ledger rows of `{}`",
                short(text(input, "cause").unwrap_or("a cause"), 60)
            ),
            Some("reports") => "read an earlier run's report".into(),
            _ => "read an earlier run's gate files".into(),
        },
        ("http.fetch", ..) => match text(input, "url") {
            Some(u) => format!("fetch `{}`", short(u, 100)),
            None => "fetch a web page".into(),
        },
        _ => short(&summarize(tool, input).replace('`', "'"), 120),
    }
}

/// The risk badge: what the call can do, from the tool and what it holds.
pub fn risk_of(req: &ConfirmRequest) -> &'static str {
    let t = req.tool.as_str();
    let secret = |s: &str| {
        ["/.ssh", "/.aws", "/.gnupg", "/.config/op", "service-token"]
            .iter()
            .any(|k| s.contains(k))
    };
    let named = req.input.to_string();
    if req.floor || secret(&named) {
        "touches secrets"
    } else if t == theseus_kernel::BUDGET_TOOL {
        "spends"
    } else if t.starts_with("http.") || t.starts_with("web.") || t.starts_with("mcp:") {
        "network"
    } else if t.starts_with("proc.") || t.starts_with("term.") {
        "runs a program"
    } else if [
        "fs.read",
        "fs.list",
        "fs.grep",
        "fs.glob",
        "git.log",
        "git.diff",
        "git.status",
        "git.show",
        "self.evidence",
    ]
    .contains(&t)
    {
        "read-only"
    } else {
        "writes"
    }
}

/// The clause a rule gives for asking: the text of the reason's last
/// `— approve (…)`, any `§` reference cut. None when the owner's own posture
/// asks (`enforcement = approve`, a `[policy.…]` setting, a tightening) or the
/// reason says nothing: those need no clause. The one reading of a reason that
/// every surface shares (the Discord card's `why it asks`, the plain words).
pub fn rule_clause(reason: &str) -> Option<String> {
    let (_, rest) = reason.rsplit_once("— approve (")?;
    let rest = rest.trim_end();
    let clause = rest.strip_suffix(')').unwrap_or(rest);
    let words: Vec<&str> = clause
        .split_whitespace()
        .filter(|w| !w.trim_start_matches('(').starts_with('§'))
        .collect();
    let clause = words.join(" ");
    let clause =
        clause.trim_matches(|c: char| c == ',' || c == ';' || c == ':' || c.is_whitespace());
    let posture = ["enforcement = approve", "[policy.", "tightened by"];
    (!clause.is_empty() && !posture.iter().any(|p| clause.starts_with(p)))
        .then(|| clause.to_string())
}

/// Why the gate holds the call, in words: the floor, the thread
/// guard's doubt, the rule's own clause, else the tool's posture.
fn held_of(req: &ConfirmRequest, clause: Option<&str>) -> String {
    if req.floor {
        "it touches Theseus's own state or a secret, which always asks".into()
    } else if clause.is_some_and(misroute::is_question) {
        "it may belong in another thread, so it waits for you".into()
    } else if let Some(c) = clause {
        c.to_string()
    } else {
        format!("`{}` is set to ask before it runs", req.tool)
    }
}

/// The words of an approval question.
pub fn plain(req: &ConfirmRequest, g: &Gloss) -> PlainWords {
    let clause = rule_clause(&req.reason);
    // The guard's question names both threads and the move; any other rule's
    // clause stands in when the model said nothing before the call.
    let why = match &clause {
        Some(c) if misroute::is_question(c) => Some(c.clone()),
        _ => g.model_why.clone().or(clause.clone()),
    };
    PlainWords {
        who: g.who.clone(),
        what: what_of(req, g),
        why: why.unwrap_or_default(),
        held: held_of(req, clause.as_deref()),
        risk: risk_of(req).into(),
        yes: "runs this one call.".into(),
        no: "refuses it; the session is told and carries on without it.".into(),
    }
}

/// Why a call waits (`held_of`), for the card words (`words.rs`).
pub(super) fn held(req: &ConfirmRequest) -> String {
    held_of(req, rule_clause(&req.reason).as_deref())
}

#[cfg(test)]
mod tests;
