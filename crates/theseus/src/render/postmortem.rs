//! `theseus postmortem`'s report (theseus-m2tiu): the facts of an incident's
//! window as data (`Report`, which `--json` prints) and as markdown
//! (`markdown`). The facts come from `postmortem_facts::analyze`, a pure
//! walk over the ledger rows `ledger.window` returns; nothing here reads a
//! clock or a connection, and every time is on this machine's clock.

use serde::Serialize;

use super::time::{fmt_day, fmt_hms, fmt_stamp};

/// One incident report.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub since_ms: u64,
    pub until_ms: u64,
    /// The sessions the window was narrowed to (a session and its tasks).
    pub scope: Vec<String>,
    /// How many ledger rows the report read.
    pub rows: usize,
    pub summary: Summary,
    pub restarts: Vec<Restart>,
    pub timeline: Vec<Line>,
    pub cost: Cost,
    pub asked: Vec<Asked>,
    pub open: Open,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub sessions: Vec<String>,
    pub tasks: Vec<String>,
    pub turns: u32,
    pub turns_failed: u32,
    pub spent_usd: f64,
    pub top_spenders: Vec<CostRow>,
    pub restarts: u32,
    pub asked: u32,
    pub answered: u32,
    pub expired: u32,
    pub first_failure: Option<Line>,
}

/// One line of the timeline: a time, a place in the ledger, and words.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Line {
    pub at_ms: u64,
    /// The ledger row it comes from (`#position`), the last of them for a
    /// fold.
    pub position: u64,
    pub session: Option<String>,
    /// `turn`, `call`, `ask`, `model`, `spend`, `task`, `wake`, `daemon`, or `quiet`.
    pub kind: String,
    pub text: String,
    /// A fold's turns: "… 14 turns, $0.31, nothing notable …".
    #[serde(skip_serializing_if = "is_zero")]
    pub folded: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usd: Option<f64>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Restart {
    pub position: u64,
    /// When the stop before it was written; none when the daemon was killed.
    pub stopped_at_ms: Option<u64>,
    pub crashed: bool,
    pub crash: Option<String>,
    pub started_at_ms: u64,
    pub serving_at_ms: Option<u64>,
    /// From the stop (or the last row a killed daemon wrote) to the start.
    pub down_ms: Option<u64>,
    /// Turns requeued as interrupted: `(session, execution)`.
    pub requeued: Vec<Resumed>,
    pub spool_drained: u64,
    /// Calls the start answered "unknown", by correlation id.
    pub unknown: Vec<String>,
    pub wakes_late: Vec<LateWake>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Resumed {
    pub session: String,
    pub execution: String,
    pub turn: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LateWake {
    pub position: u64,
    pub at_ms: u64,
    pub session: String,
    pub wake: String,
    pub late_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Cost {
    pub total_usd: f64,
    pub by_session: Vec<CostRow>,
    pub by_model: Vec<CostRow>,
    /// Reservations held when the window ends.
    pub held: Vec<Held>,
    pub held_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CostRow {
    pub key: String,
    pub usd: f64,
    pub calls: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Held {
    pub position: u64,
    pub session: String,
    pub correlation_id: String,
    pub tool: String,
    pub usd: f64,
    pub since_ms: u64,
    /// The call's outcome is unknown: its money is held until it is resolved.
    pub unknown: bool,
}

/// A question to the owner.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Asked {
    pub position: u64,
    pub session: String,
    pub correlation_id: String,
    pub tool: String,
    pub reason: String,
    pub asked_at_ms: u64,
    /// `approved`, `declined`, `expired`, or `open`.
    pub outcome: String,
    pub answered_at_ms: Option<u64>,
    pub answered_position: Option<u64>,
    pub by: Option<String>,
    pub via: Option<String>,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Open {
    pub turns: Vec<OpenTurn>,
    pub questions: Vec<Asked>,
    pub wakes: Vec<OpenWake>,
    pub reservations: Vec<Held>,
    pub reserved_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OpenTurn {
    pub position: u64,
    pub session: String,
    pub turn: String,
    pub since_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OpenWake {
    pub position: u64,
    pub session: String,
    pub wake: String,
    pub due_at_ms: u64,
}

// ------------------------------------------------------------------ words

/// A span as people say it: `850 ms`, `14.2 s`, `3 m 05 s`, `2 h 04 m`.
pub fn span(ms: u64) -> String {
    match ms {
        0..=999 => format!("{ms} ms"),
        1_000..=59_999 => format!("{:.1} s", ms as f64 / 1000.0),
        60_000..=3_599_999 => format!("{} m {:02} s", ms / 60_000, ms / 1000 % 60),
        _ => format!("{} h {:02} m", ms / 3_600_000, ms / 60_000 % 60),
    }
}

/// Money to the cent, four decimals under a cent's tenth.
pub fn usd(v: f64) -> String {
    if v != 0.0 && v.abs() < 0.01 {
        format!("${v:.4}")
    } else {
        format!("${v:.2}")
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A line's place in the ledger, which `theseus ledger` and the cockpit read.
fn at(l: &Line) -> String {
    let who = l
        .session
        .as_deref()
        .map(|s| format!(" · {s}"))
        .unwrap_or_default();
    format!("{}{who} · #{}", fmt_hms(l.at_ms), l.position)
}

/// The report as markdown.
pub fn markdown(r: &Report) -> String {
    let mut o = String::new();
    o.push_str(&format!(
        "# Postmortem: {} to {}\n\n",
        fmt_stamp(r.since_ms),
        fmt_stamp(r.until_ms)
    ));
    summary(&mut o, r);
    restarts(&mut o, r);
    timeline(&mut o, r);
    cost(&mut o, r);
    asked(&mut o, r);
    open(&mut o, r);
    o
}

fn summary(o: &mut String, r: &Report) {
    let s = &r.summary;
    o.push_str("## Summary\n\n");
    o.push_str(&format!(
        "- Window {} to {}{}: {}, {}.\n",
        fmt_stamp(r.since_ms),
        fmt_stamp(r.until_ms),
        if r.scope.is_empty() {
            String::new()
        } else {
            format!(" (narrowed to {})", r.scope.join(", "))
        },
        plural(s.sessions.len(), "session", "sessions"),
        plural(s.tasks.len(), "task", "tasks"),
    ));
    o.push_str(&format!(
        "- {} run, {} failed; spent {}{}.\n",
        plural(s.turns as usize, "turn", "turns"),
        s.turns_failed,
        usd(s.spent_usd),
        if s.top_spenders.is_empty() {
            String::new()
        } else {
            format!(
                " (top: {})",
                s.top_spenders
                    .iter()
                    .map(|c| format!("{} {}", c.key, usd(c.usd)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    ));
    o.push_str(&format!(
        "- {}; questions to the owner: {} asked, {} answered, {} expired.\n",
        plural(s.restarts as usize, "daemon restart", "daemon restarts"),
        s.asked,
        s.answered,
        s.expired
    ));
    match &s.first_failure {
        Some(f) => o.push_str(&format!("- First failure: {} — {}.\n", at(f), f.text)),
        None => o.push_str("- No failure in the window.\n"),
    }
    o.push('\n');
}

fn restarts(o: &mut String, r: &Report) {
    o.push_str("## Restarts\n\n");
    if r.restarts.is_empty() {
        o.push_str("The daemon did not restart in this window.\n\n");
        return;
    }
    for x in &r.restarts {
        let stop = match (x.stopped_at_ms, x.crashed) {
            (Some(t), _) => format!("stopped cleanly at {}", fmt_hms(t)),
            (None, true) => "crashed".to_string(),
            (None, false) => "was killed or lost power".to_string(),
        };
        o.push_str(&format!(
            "- The daemon {stop}, started again at {} · #{}{}.\n",
            fmt_hms(x.started_at_ms),
            x.position,
            match x.down_ms {
                Some(d) => format!(" (down {})", span(d)),
                None => String::new(),
            }
        ));
        if let Some(c) = &x.crash {
            o.push_str(&format!("  - Crash: {c}\n"));
        }
        if x.requeued.is_empty() {
            o.push_str("  - No turn was running.\n");
        }
        for t in &x.requeued {
            o.push_str(&format!(
                "  - Turn requeued as interrupted: {}{}.\n",
                t.session,
                t.turn
                    .as_deref()
                    .map(|t| format!(" ({t})"))
                    .unwrap_or_default()
            ));
        }
        if x.spool_drained > 0 {
            o.push_str(&format!(
                "  - {} taken from the spool.\n",
                plural(x.spool_drained as usize, "completion", "completions")
            ));
        }
        for u in &x.unknown {
            o.push_str(&format!("  - Call {u} answered \"unknown\".\n"));
        }
        for w in &x.wakes_late {
            o.push_str(&format!(
                "  - Wake {} of {} ran {} late at {} · #{}.\n",
                w.wake,
                w.session,
                span(w.late_ms),
                fmt_hms(w.at_ms),
                w.position
            ));
        }
    }
    o.push('\n');
}

fn timeline(o: &mut String, r: &Report) {
    o.push_str("## Timeline\n\n");
    if r.timeline.is_empty() {
        o.push_str("Nothing happened in this window.\n\n");
        return;
    }
    let grouped = !r.scope.is_empty();
    let mut groups: Vec<(Option<&str>, Vec<&Line>)> = Vec::new();
    for l in &r.timeline {
        let key = if grouped { l.session.as_deref() } else { None };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(l),
            None => groups.push((key, vec![l])),
        }
    }
    for (key, lines) in groups {
        if let Some(k) = key {
            o.push_str(&format!("### {k}\n\n"));
        } else if grouped {
            o.push_str("### The daemon\n\n");
        }
        let mut day = String::new();
        for l in lines {
            let d = fmt_day(l.at_ms);
            if d != day {
                o.push_str(&format!("**{d}**\n\n"));
                day = d;
            }
            if (l.kind == "quiet" && l.folded > 1) || grouped {
                o.push_str(&format!(
                    "- {} · #{} — {}\n",
                    fmt_hms(l.at_ms),
                    l.position,
                    l.text
                ));
            } else {
                o.push_str(&format!("- {} — {}\n", at(l), l.text));
            }
        }
        o.push('\n');
    }
}

fn cost(o: &mut String, r: &Report) {
    let c = &r.cost;
    o.push_str("## Cost\n\n");
    o.push_str(&format!("Total {}.\n\n", usd(c.total_usd)));
    for (title, rows) in [("By session", &c.by_session), ("By model", &c.by_model)] {
        if rows.is_empty() {
            continue;
        }
        o.push_str(&format!("{title}:\n\n"));
        for x in rows {
            o.push_str(&format!(
                "- {} — {} ({})\n",
                x.key,
                usd(x.usd),
                plural(x.calls as usize, "call", "calls")
            ));
        }
        o.push('\n');
    }
    if c.held.is_empty() {
        o.push_str("No reservation is held at the window's end.\n\n");
    } else {
        o.push_str(&format!(
            "Reservations held at the window's end: {}.\n\n",
            usd(c.held_usd)
        ));
        for h in &c.held {
            o.push_str(&held_line(h));
        }
        o.push('\n');
    }
}

fn held_line(h: &Held) -> String {
    format!(
        "- {} · {} · #{} — {} of `{}` held since {}{}\n",
        fmt_hms(h.since_ms),
        h.session,
        h.position,
        usd(h.usd),
        h.tool,
        fmt_hms(h.since_ms),
        if h.unknown { " (outcome unknown)" } else { "" }
    )
}

fn ask_line(a: &Asked) -> String {
    let answer = match a.outcome.as_str() {
        "expired" => format!(
            "expired unanswered after {}",
            span(a.latency_ms.unwrap_or(0))
        ),
        "open" => "still waiting".to_string(),
        other => format!(
            "{other} by {} from {} after {}",
            a.by.as_deref().unwrap_or("?"),
            a.via.as_deref().unwrap_or("?"),
            span(a.latency_ms.unwrap_or(0))
        ),
    };
    format!(
        "- {} · {} · #{} — `{}`{}: {answer}\n",
        fmt_hms(a.asked_at_ms),
        a.session,
        a.position,
        a.tool,
        if a.reason.is_empty() {
            String::new()
        } else {
            format!(" ({})", a.reason)
        }
    )
}

fn asked(o: &mut String, r: &Report) {
    o.push_str("## Asked and answered\n\n");
    if r.asked.is_empty() {
        o.push_str("Nothing was asked of the owner.\n\n");
        return;
    }
    for a in &r.asked {
        o.push_str(&ask_line(a));
    }
    o.push('\n');
}

fn open(o: &mut String, r: &Report) {
    let x = &r.open;
    o.push_str("## Open at the end\n\n");
    if x.turns.is_empty()
        && x.questions.is_empty()
        && x.wakes.is_empty()
        && x.reservations.is_empty()
    {
        o.push_str("Nothing is open.\n");
        return;
    }
    for t in &x.turns {
        o.push_str(&format!(
            "- Turn running: {} {} since {} · #{}\n",
            t.session,
            t.turn,
            fmt_hms(t.since_ms),
            t.position
        ));
    }
    for q in &x.questions {
        o.push_str(&format!(
            "- Question waiting: {}",
            ask_line(q).trim_start_matches("- ")
        ));
    }
    for w in &x.wakes {
        o.push_str(&format!(
            "- Wake pending: {} {} due {} · #{}\n",
            w.session,
            w.wake,
            fmt_stamp(w.due_at_ms),
            w.position
        ));
    }
    for h in &x.reservations {
        o.push_str(&format!(
            "- Reservation held: {}",
            held_line(h).trim_start_matches("- ")
        ));
    }
}
