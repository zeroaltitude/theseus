//! The narrative (theseus-5fy). With `narrative = true`, every architectural
//! step of the session/turn/loop/model-call structure says what it did in one
//! plain sentence, filled into a fixed template from the structure itself.
//! No model writes it, so it costs no tokens. Each line goes to every
//! `narrative.watch` subscriber and into a bounded tail in memory, so a tab
//! opened late still shows recent history. Nothing is written to the store:
//! no frames and no fsyncs.
//!
//! Off means off. Every emission goes through [`narrate!`], which tests one
//! flag before it evaluates or formats anything, so a daemon without the key
//! pays one branch per step and keeps nothing.
//!
//! A sentence names the tool and its main resource, and gives sizes, counts,
//! times, and costs. It never carries tool input or output, secrets, or
//! message text beyond a character count.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::Mutex;

use theseus_protocol::{Event, Message, NarrativeLine, NarrativePart};

/// Lines the tail keeps for a subscriber that arrives late.
pub const TAIL: usize = 500;

/// Narrate one line: `narrate!(narrator, Session, Some(session_id), None, "…")`.
/// The format arguments are evaluated only when narration is on.
macro_rules! narrate {
    ($narrator:expr, $part:ident, $session:expr, $turn:expr, $($fmt:tt)+) => {{
        let n: &$crate::narrative::Narrator = &$narrator;
        if n.on() {
            n.line(
                theseus_protocol::NarrativePart::$part,
                $session,
                $turn,
                format!($($fmt)+),
            );
        }
    }};
}
pub(crate) use narrate;

/// Narrate one line in a turn, whose context (`TurnCtx`) names the narrator,
/// the session, and the turn: `narrate_turn!(tc, Tool, "…")`.
macro_rules! narrate_turn {
    ($tc:expr, $part:ident, $($fmt:tt)+) => {{
        let tc = &$tc;
        $crate::narrative::narrate!(tc.narrator, $part, Some(tc.session_id), Some(tc.turn_id), $($fmt)+)
    }};
}
pub(crate) use narrate_turn;

/// The narrative's live channel: the tail and the subscribers.
pub struct Narrator {
    on: bool,
    capacity: usize,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    seq: u64,
    tail: VecDeque<NarrativeLine>,
    subs: Vec<(String, crate::outbound::Outbound)>,
    /// Sessions the narrative has met since the daemon started. The first
    /// turn of any other session with turns behind it is a resume.
    seen: HashSet<String>,
}

impl Narrator {
    pub fn new(on: bool) -> Self {
        Self::with_capacity(on, TAIL)
    }

    pub fn with_capacity(on: bool, capacity: usize) -> Self {
        Self {
            on,
            capacity,
            state: Mutex::new(State::default()),
        }
    }

    /// The one check every emission makes first.
    #[inline]
    pub fn on(&self) -> bool {
        self.on
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Keep one line and send it to every subscriber, dropping those whose
    /// connection closed. Callers go through [`narrate!`], which calls this
    /// only when narration is on.
    pub fn line(
        &self,
        part: NarrativePart,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        text: String,
    ) {
        let mut s = self.state.lock().unwrap();
        s.seq += 1;
        let line = NarrativeLine {
            seq: s.seq,
            at_unix_ms: theseus_protocol::now_unix_ms(),
            part,
            session_id: session_id.map(str::to_string),
            turn_id: turn_id.map(str::to_string),
            text,
        };
        if !s.subs.is_empty() {
            let m = Message::from(Event::NarrativeLine(line.clone()));
            s.subs.retain(|(_, tx)| tx.notify(m.clone(), "narrative"));
        }
        s.tail.push_back(line);
        while s.tail.len() > self.capacity {
            s.tail.pop_front();
        }
    }

    /// Subscribe a connection: the tail, oldest first, now, and every later
    /// line as a notification. `None` when narration is off.
    pub fn watch(
        &self,
        conn: &str,
        tx: impl Into<crate::outbound::Outbound>,
    ) -> Option<Vec<NarrativeLine>> {
        if !self.on {
            return None;
        }
        let mut s = self.state.lock().unwrap();
        s.subs.retain(|(c, _)| c != conn);
        s.subs.push((conn.to_string(), tx.into()));
        Some(s.tail.iter().cloned().collect())
    }

    /// Stop sending to a connection (also when it closes).
    pub fn unwatch(&self, conn: &str) {
        self.state.lock().unwrap().subs.retain(|(c, _)| c != conn);
    }

    pub fn subscribers(&self) -> usize {
        self.state.lock().unwrap().subs.len()
    }

    /// The lines the tail holds, oldest first.
    pub fn tail(&self) -> Vec<NarrativeLine> {
        self.state.lock().unwrap().tail.iter().cloned().collect()
    }

    /// True the first time the narrative meets `session_id` since the daemon
    /// started; it remembers the session.
    pub fn first_sight(&self, session_id: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .seen
            .insert(session_id.to_string())
    }
}

// ---------------------------------------------------------------- the words

/// An id's last six characters after an ellipsis, enough to tell ids apart
/// in a sentence (the line carries the whole session and turn ids).
pub fn short(id: &str) -> String {
    let n = id.chars().count();
    if n <= 7 {
        return id.to_string();
    }
    let i = id.char_indices().nth(n - 6).map_or(0, |(i, _)| i);
    format!("…{}", &id[i..])
}

/// Why the compiler made a new compilation, from its trigger (`a+b` for two).
pub fn trigger_phrase(trigger: &str) -> String {
    let one = |t: &str| match t {
        "new_session" => "the session is new".to_string(),
        "model_changed" => "the model changed".into(),
        "system_changed" => "the system prompt changed".into(),
        "tools_changed" => "the tools changed".into(),
        "overflow" => "the context outgrew the model's window".into(),
        "manual_fresh" => "the operator asked for a fresh start".into(),
        "manual_transcript" => "the operator asked for a transcript recompile".into(),
        other => format!("of {other}"),
    };
    trigger
        .split('+')
        .map(one)
        .collect::<Vec<_>>()
        .join(" and ")
}

/// `1 loop`, `2 loops`.
pub fn count(n: u64, one: &str, many: &str) -> String {
    format!("{} {}", thousands(n), if n == 1 { one } else { many })
}

/// `130,300`.
pub fn thousands(n: u64) -> String {
    let d = n.to_string();
    let mut out = String::with_capacity(d.len() + d.len() / 3);
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Dollars as the web UI shows them; `None` is a price the catalog lacks.
pub fn money(cost: Option<f64>) -> String {
    match cost {
        None => "an unknown cost".into(),
        Some(0.0) => "$0".into(),
        Some(c) if c < 0.01 => format!("${c:.4}"),
        Some(c) => format!("${c:.3}"),
    }
}

/// Micro-dollars as a sentence says them (theseus-0sg): to the cent from a
/// dollar up (`$100`, `$99.48`, `$12.30`), to four decimals below it
/// (`$0.45`, `$0.0045`, `$0.002`), and exactly below a hundredth of a cent.
pub fn dollars(m: theseus_kernel::Micros) -> String {
    let step = match m {
        1_000_000.. => 10_000,
        100.. => 100,
        _ => 1,
    };
    theseus_kernel::usd((m + step / 2) / step * step)
}

/// `3 ms`, `2.3 s`, `4 min 12 s`.
pub fn duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} min {} s", ms / 60_000, ms % 60_000 / 1000)
    }
}

/// `812 bytes`, `8.4 KB`, `1.2 MB`.
pub fn bytes(n: u64) -> String {
    if n < 1024 {
        count(n, "byte", "bytes")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

/// Lines of text, the way an editor counts them.
pub fn lines_in(text: &str) -> u64 {
    if text.is_empty() {
        0
    } else {
        text.lines().count() as u64
    }
}

/// A tool call's main resource: `proc.run cargo test` (the program and its
/// subcommand, never the rest of the argv), or `fs.read src/main.rs` (the
/// first path, relative to the working directory when it is under it).
pub fn subject(
    tool: &str,
    argv: Option<&[String]>,
    first_path: Option<&Path>,
    cwd: &Path,
) -> String {
    if let Some(argv) = argv {
        let program = argv
            .first()
            .map(|a| match Path::new(a).file_name() {
                Some(f) => f.to_string_lossy().into_owned(),
                None => a.clone(),
            })
            .unwrap_or_default();
        let sub = argv.get(1).filter(|a| {
            a.len() <= 24
                && a.starts_with(|c: char| c.is_ascii_lowercase())
                && a.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        });
        return match sub {
            Some(s) => format!("{tool} {program} {s}"),
            None => format!("{tool} {program}"),
        };
    }
    match first_path {
        Some(p) => {
            let shown = match p.strip_prefix(cwd) {
                Ok(rel) if rel.as_os_str().is_empty() => ".".into(),
                Ok(rel) => rel.display().to_string(),
                Err(_) => p.display().to_string(),
            };
            format!("{tool} {shown}")
        }
        None => tool.to_string(),
    }
}

/// Why the gate chose a posture, from its reason: the setting or the rule in
/// the reason's parentheses, with the plan's summary taken off the front and
/// a command's whole argv replaced by "the command". Falls back to `setting`
/// when the reason is not in the gate's shape.
pub fn gate_why(
    reason: &str,
    summary: &str,
    tool: &str,
    posture: &str,
    argv: Option<&[String]>,
    setting: &str,
) -> String {
    let body = reason
        .strip_prefix(summary)
        .and_then(|r| r.strip_prefix(": "))
        .unwrap_or(reason);
    let Some(why) = body
        .strip_prefix(&format!("{tool} — {posture} ("))
        .and_then(|r| r.strip_suffix(')'))
    else {
        return setting.to_string();
    };
    match argv {
        Some(a) if !a.is_empty() => why.replacen(&format!("`{}`", a.join(" ")), "the command", 1),
        _ => why.to_string(),
    }
}

/// What the model's stop reason means for the loop.
pub fn stop_phrase(stop_reason: Option<&str>, tool_uses: usize) -> String {
    match stop_reason {
        Some("tool_use") => format!(
            "it stopped to call {}",
            count(tool_uses as u64, "tool", "tools")
        ),
        Some("end_turn") => "it ended its turn".into(),
        Some("max_tokens") => "it hit its output limit".into(),
        Some("refusal") => "it refused".into(),
        Some("stop_sequence") => "it hit a stop sequence".into(),
        Some("pause_turn") => "it paused its turn".into(),
        Some(other) => format!("it stopped ({other})"),
        None => "it gave no stop reason".into(),
    }
}

/// Why a turn stops, from the Advancer's reason or the turn's stop reason.
pub fn end_phrase(reason: &str) -> String {
    match reason {
        "no_tool_calls" | "end_turn" => "the model ended its turn".into(),
        "max_loops" => "the loop cap is reached".into(),
        "awaiting_confirm" => "a call waits for approval".into(),
        "nothing_new" => "there was nothing new for the model".into(),
        "refusal" => "the model refused".into(),
        "max_tokens" => "the model hit its output limit".into(),
        "budget" => "the session reached its spend limit and asks the operator".into(),
        other => format!("the stop reason is {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::NarrativePart as Part;
    use tokio::sync::mpsc::unbounded_channel;

    fn say(n: &Narrator, text: &str) {
        narrate!(n, Turn, Some("ses_1"), None, "{text}");
    }

    #[test]
    fn off_keeps_nothing_and_refuses_a_watch() {
        let n = Narrator::new(false);
        let mut evaluated = false;
        narrate!(n, Turn, None, None, "{}", {
            evaluated = true;
            "x"
        });
        assert!(!evaluated, "nothing is evaluated when narration is off");
        assert!(n.tail().is_empty());
        let (tx, _rx) = unbounded_channel();
        assert!(n.watch("c", tx).is_none());
        assert_eq!(n.subscribers(), 0);
    }

    /// A subscriber that arrives late gets the tail, which is bounded, then
    /// every new line; a closed one is dropped on the next line.
    #[test]
    fn a_late_subscriber_gets_the_bounded_tail_then_every_line() {
        let n = Narrator::with_capacity(true, 3);
        for i in 1..=5 {
            say(&n, &format!("line {i}"));
        }
        let (tx, mut rx) = unbounded_channel();
        let tail = n.watch("late", tx).unwrap();
        let texts: Vec<&str> = tail.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["line 3", "line 4", "line 5"]);
        assert_eq!(tail.iter().map(|l| l.seq).collect::<Vec<_>>(), [3, 4, 5]);
        assert!(tail
            .iter()
            .all(|l| l.part == Part::Turn && l.session_id.as_deref() == Some("ses_1")));
        say(&n, "line 6");
        let Ok(Message::Notification(m)) = rx.try_recv() else {
            panic!("the new line is sent");
        };
        assert_eq!(m.method, theseus_protocol::notify::NARRATIVE_LINE);
        let line: NarrativeLine = serde_json::from_value(m.params).unwrap();
        assert_eq!((line.seq, line.text.as_str()), (6, "line 6"));
        assert_eq!(n.tail().len(), 3, "the tail stays bounded");
        drop(rx);
        say(&n, "line 7");
        assert_eq!(n.subscribers(), 0, "a closed subscriber is dropped");
        let (tx, _rx) = unbounded_channel();
        n.watch("again", tx).unwrap();
        n.unwatch("again");
        assert_eq!(n.subscribers(), 0);
    }

    #[test]
    fn the_first_sight_of_a_session_is_remembered() {
        let n = Narrator::new(true);
        assert!(n.first_sight("ses_a"));
        assert!(!n.first_sight("ses_a"));
        assert!(n.first_sight("ses_b"));
    }

    #[test]
    fn the_words() {
        assert_eq!(short("turn_0199aabbccddeeff"), "…ddeeff");
        assert_eq!(short("abc"), "abc");
        assert_eq!(
            trigger_phrase("model_changed+tools_changed"),
            "the model changed and the tools changed"
        );
        assert_eq!(thousands(130_300), "130,300");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(thousands(999), "999");
        assert_eq!(count(1, "loop", "loops"), "1 loop");
        assert_eq!(count(1204, "token", "tokens"), "1,204 tokens");
        assert_eq!(money(Some(0.014)), "$0.014");
        assert_eq!(money(Some(0.0042)), "$0.0042");
        assert_eq!(money(Some(0.0)), "$0");
        assert_eq!(money(None), "an unknown cost");
        for (m, s) in [
            (100_000_000, "$100"),
            (99_482_311, "$99.48"),
            (12_300_000, "$12.30"),
            (450_000, "$0.45"),
            (452_311, "$0.4523"),
            (4_521, "$0.0045"),
            (2_000, "$0.002"),
            (42, "$0.000042"),
            (0, "$0"),
        ] {
            assert_eq!(dollars(m), s, "{m}");
        }
        assert_eq!(duration(3), "3 ms");
        assert_eq!(duration(2300), "2.3 s");
        assert_eq!(duration(252_000), "4 min 12 s");
        assert_eq!(bytes(812), "812 bytes");
        assert_eq!(bytes(8602), "8.4 KB");
        assert_eq!(lines_in(""), 0);
        assert_eq!(lines_in("a\nb\n"), 2);
        assert_eq!(
            stop_phrase(Some("tool_use"), 2),
            "it stopped to call 2 tools"
        );
        assert_eq!(end_phrase("no_tool_calls"), "the model ended its turn");
    }

    /// The subject names the program and its subcommand, never the rest of
    /// the argv, and a path relative to the working directory.
    #[test]
    fn a_subject_names_the_resource_and_no_more() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let cwd = Path::new("/w");
        assert_eq!(
            subject(
                "proc.run",
                Some(&argv(&["cargo", "test", "--", "x"])),
                None,
                cwd
            ),
            "proc.run cargo test"
        );
        assert_eq!(
            subject(
                "proc.run",
                Some(&argv(&["/usr/bin/git", "push", "https://t@h/r"])),
                None,
                cwd
            ),
            "proc.run git push"
        );
        assert_eq!(
            subject(
                "proc.run",
                Some(&argv(&["bash", "-c", "echo $SECRET"])),
                None,
                cwd
            ),
            "proc.run bash"
        );
        assert_eq!(
            subject(
                "proc.run",
                Some(&argv(&["curl", "https://x/y?token=z"])),
                None,
                cwd
            ),
            "proc.run curl"
        );
        assert_eq!(
            subject("fs.read", None, Some(Path::new("/w/src/main.rs")), cwd),
            "fs.read src/main.rs"
        );
        assert_eq!(
            subject("fs.list", None, Some(Path::new("/w")), cwd),
            "fs.list ."
        );
        assert_eq!(
            subject("fs.read", None, Some(Path::new("/etc/hosts")), cwd),
            "fs.read /etc/hosts"
        );
        assert_eq!(subject("text.diff", None, None, cwd), "text.diff");
    }

    /// The why is the gate's own rule, without the plan's summary and without
    /// a command's argv.
    #[test]
    fn the_gate_why_drops_the_summary_and_the_argv() {
        let argv: Vec<String> = ["ls", "-la", "/x"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            gate_why(
                "proc.run — notify (enforcement = notify)",
                "run `ls` in /w",
                "proc.run",
                "notify",
                Some(&argv),
                "s"
            ),
            "enforcement = notify"
        );
        assert_eq!(
            gate_why(
                "run `ls -la /x` in /w: proc.run — approve (`ls -la /x` matches the approve list entry `ls`)",
                "run `ls -la /x` in /w",
                "proc.run",
                "approve",
                Some(&argv),
                "s"
            ),
            "the command matches the approve list entry `ls`"
        );
        assert_eq!(
            gate_why(
                "read /etc/hosts: fs.read — approve (/etc/hosts is outside the workspace roots: /w)",
                "read /etc/hosts",
                "fs.read",
                "approve",
                None,
                "s"
            ),
            "/etc/hosts is outside the workspace roots: /w"
        );
        assert_eq!(
            gate_why("something else", "", "fs.read", "open", None, "the setting"),
            "the setting"
        );
    }
}
