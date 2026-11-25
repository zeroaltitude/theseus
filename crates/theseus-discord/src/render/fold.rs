//! A turn's steps folded into one message (issue 56). The owner: "I would
//! like tool calls and notifications to be inside a collapsible widget that
//! defaults to being collapsed." A Discord message has no collapsible part,
//! so a place bound `steps = "folded"` (the default) writes its turn's process
//! messages (`<turn>:L<n>:tools`: the loops' thinking and tool lines,
//! `process.rs`) as ONE message, `<turn>:fold`, closed: one line that says
//! what it holds, from the calls' typed states and tool names and never from
//! a call's argument or result, with a **Show steps** button. Open, it is the
//! very lines those process messages would show, cut whole-line into pages;
//! **Hide steps** closes it again. A place bound `steps = "open"` writes the
//! process messages as before, and nothing here runs.
//!
//! What the fold holds is only what informs: the reply's text, a card, a
//! question, a settle and a failed turn are posts or text parts, which never
//! come here. A call that waits for an answer is counted on the closed line,
//! and its card posts as ever. A "Should I have asked?" control stays on the
//! fold, open or closed, so no button is hidden behind a press.
//!
//! The key begins with the turn's id, so the lane keeps and forgets it with
//! the turn's other keys, and reads it back after a restart from its rows
//! (`courier/recall.rs`). A press carries the turn and the page in its id
//! (`runtime/fold.rs`), so a fold made before a restart opens after it: its
//! lines are read back from the session's nodes, as the renderer's held turns
//! are (`refold.rs`).

use std::collections::{BTreeMap, VecDeque};

use super::process::{Shown, Thought};
use super::step::render_turn;
use super::{Asked, Buttons, Op, Renderer, ToolState, MAX_ASKED, PART_LIMIT};

/// How a fold's key ends: `<turn>:fold`.
pub(crate) const SUFFIX: &str = ":fold";

/// How a process message's key ends (`process.rs`).
const TOOLS: &str = ":tools";

/// The most of a page's lines, in bytes: a process message's own budget
/// (`step.rs`'s `tool_lines`), so a page is never longer than one of them.
const PAGE: usize = PART_LIMIT - 60;

/// What the fold's message carries besides its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldButtons {
    pub turn: String,
    /// Closed (None), or open at this page.
    pub page: Option<usize>,
    pub pages: usize,
    /// The turn's "Should I have asked?" choices, shown open or closed.
    pub menu: Vec<Asked>,
}

/// What one turn's calls were, by their typed name and state, for the
/// closed line.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Tally {
    /// Each family of tool, in the order it first ran, and how many calls.
    kinds: Vec<(Family, usize)>,
    calls: usize,
    /// Each state worth a word, in `STATES` order, and how many calls.
    states: BTreeMap<usize, usize>,
}

/// A family of tools, as the closed line counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Read,
    Search,
    Command,
    Edit,
    Fetch,
    Aws,
    Other,
}

impl Family {
    /// The family of a tool, by its name.
    fn of(tool: &str) -> Self {
        match tool {
            "fs.read" | "fs.list" | "git.diff" | "git.log" | "text.diff" | "job.read"
            | "term.read" => Self::Read,
            "fs.grep" | "fs.glob" | "web.search" => Self::Search,
            "proc.run" | "term.open" | "term.send" | "term.close" | "job.wait" | "job.stop" => {
                Self::Command
            }
            "fs.write" | "fs.edit" | "fs.patch" => Self::Edit,
            "http.fetch" | "web.fetch" => Self::Fetch,
            t if t.starts_with("aws.") => Self::Aws,
            _ => Self::Other,
        }
    }

    fn words(self, n: usize) -> String {
        let (one, many) = match self {
            Self::Read => ("read", "reads"),
            Self::Search => ("search", "searches"),
            Self::Command => ("command", "commands"),
            Self::Edit => ("edit", "edits"),
            Self::Fetch => ("fetch", "fetches"),
            Self::Aws => ("AWS call", "AWS calls"),
            Self::Other => ("other call", "other calls"),
        };
        format!("{n} {}", if n == 1 { one } else { many })
    }
}

/// The states the closed line names, in its order. Done and running calls
/// are only counted as steps.
const STATES: [&str; 7] = [
    "failed",
    "cut short",
    "waiting for approval",
    "in the background",
    "declined",
    "not run",
    "stopped",
];

fn state_of(s: &ToolState) -> Option<usize> {
    match s {
        ToolState::Done { status, .. } if status != "ok" => Some(0),
        ToolState::Cut { .. } => Some(1),
        ToolState::Waiting => Some(2),
        ToolState::Background => Some(3),
        ToolState::Answered {
            approved: false, ..
        } => Some(4),
        ToolState::NotRun => Some(5),
        ToolState::Stopped { .. } => Some(6),
        _ => None,
    }
}

impl Renderer {
    /// What turn `turn`'s calls were, if it holds the turn.
    pub(crate) fn tally(&self, turn: &str) -> Option<Tally> {
        let t = self.turns.iter().rev().find(|t| t.turn_id == turn)?;
        let mut tally = Tally::default();
        for l in t.loops.values().flat_map(|lv| &lv.tools) {
            tally.calls += 1;
            let f = Family::of(&l.tool);
            match tally.kinds.iter_mut().find(|(k, _)| *k == f) {
                Some((_, n)) => *n += 1,
                None => tally.kinds.push((f, 1)),
            }
            if let Some(i) = state_of(&l.state) {
                *tally.states.entry(i).or_default() += 1;
            }
        }
        Some(tally)
    }

    /// The held turns were read back from the session's nodes once
    /// (`refold.rs`): a turn it does not hold now is past what it keeps.
    pub(crate) fn was_rebuilt(&self) -> bool {
        self.rebuilt
    }

    /// Turn `turn`'s process messages as its tool lines make them now, for
    /// a fold this place did not see written (a restart): each loop's lines
    /// and its "Should I have asked?" choices.
    pub(crate) fn tool_messages(&self, turn: &str) -> Vec<Op> {
        let Some(t) = self.turns.iter().rev().find(|t| t.turn_id == turn) else {
            return vec![];
        };
        render_turn(
            t,
            &self.tightened,
            !self.notice_embeds,
            self.exits.as_deref(),
        )
        .into_iter()
        .filter(|m| m.key.ends_with(TOOLS))
        .map(|m| Op::Upsert {
            key: m.key,
            content: m.content,
            buttons: match m.menu {
                Some(menu) if !menu.is_empty() => Buttons::ShouldHaveAsked(menu),
                _ => Buttons::Keep,
            },
        })
        .collect()
    }
}

/// The closed line: the thinking, when the place shows it; then the steps,
/// when it shows its tools, by family; then the states worth a word. Empty
/// when the place shows neither. It begins `-# 💭` when it holds thinking,
/// as a process message that holds thinking does, so its create is as silent
/// as theirs (`policy::of_live`).
pub(crate) fn line(tally: Option<&Tally>, thought: Option<Thought>, s: Shown) -> String {
    let mut bits: Vec<String> = Vec::new();
    match thought.filter(|_| s.thinking) {
        Some(Thought::Thinking) => bits.push("💭 thinking".into()),
        Some(Thought::For(secs)) => bits.push(format!("💭 thought for {secs} s")),
        None => {}
    }
    if let Some(t) = tally.filter(|t| s.tools && t.calls > 0) {
        let kinds: Vec<String> = t.kinds.iter().map(|(f, n)| f.words(*n)).collect();
        let steps = if t.calls == 1 { "step" } else { "steps" };
        bits.push(format!("{} {steps}: {}", t.calls, kinds.join(", ")));
        bits.extend(t.states.iter().map(|(i, n)| format!("{n} {}", STATES[*i])));
    }
    if bits.is_empty() {
        return String::new();
    }
    format!("-# {}", bits.join(" · "))
}

/// A place's folds: each held turn's process messages as they would be
/// written open, and what its one message last showed.
#[derive(Default)]
pub(crate) struct Folds {
    turns: VecDeque<(String, Fold)>,
}

#[derive(Default)]
struct Fold {
    /// Each loop's process message, as the place would write it open.
    loops: BTreeMap<u32, String>,
    /// Each loop's "Should I have asked?" choices.
    menus: BTreeMap<u32, Vec<Asked>>,
    /// Closed (None), or open at this page.
    page: Option<usize>,
    /// What Discord last got.
    shown: Option<(String, FoldButtons)>,
}

impl Fold {
    /// Its lines, open: each loop's message in loop order, line by line.
    fn lines(&self) -> Vec<&str> {
        self.loops.values().flat_map(|c| c.lines()).collect()
    }

    /// The turn's distinct choices, in the order its loops offer them.
    fn menu(&self) -> Vec<Asked> {
        let mut out: Vec<Asked> = Vec::new();
        for a in self.menus.values().flatten() {
            if let Some(o) = out.iter_mut().find(|o| o.tool == a.tool) {
                o.correlation_id.clone_from(&a.correlation_id);
            } else if out.len() < MAX_ASKED {
                out.push(a.clone());
            }
        }
        out
    }

    /// The message as it shows now: `head` closed, or the page it is open
    /// at. Open with no line left to show (its place has hidden what it
    /// held), it shows closed.
    fn render(&self, turn: &str, head: &str) -> Option<(String, FoldButtons)> {
        let menu = self.menu();
        // A turn whose calls the renderer does not count (none here) still
        // says it holds steps: its lines are never left unwritten.
        let head = if head.is_empty() { "-# steps" } else { head };
        let closed = || {
            (!self.loops.is_empty()).then(|| {
                let b = FoldButtons {
                    turn: turn.to_string(),
                    page: None,
                    pages: 1,
                    menu: menu.clone(),
                };
                (head.to_string(), b)
            })
        };
        let Some(page) = self.page else {
            return closed();
        };
        let lines = self.lines();
        let pages = paginate(&lines);
        let p = page.min(pages.len().saturating_sub(1));
        let range = pages.get(p).cloned().unwrap_or(0..0);
        let mut content = lines[range.clone()]
            .iter()
            .map(|l| cut_line(l))
            .collect::<Vec<_>>()
            .join("\n");
        if pages.len() > 1 {
            let more = lines.len() - range.len();
            content.push_str(&format!(
                "\n-# … and {more} more · page {} of {}",
                p + 1,
                pages.len()
            ));
        }
        if content.trim().is_empty() {
            return closed();
        }
        let b = FoldButtons {
            turn: turn.to_string(),
            page: Some(p),
            pages: pages.len(),
            menu,
        };
        Some((content, b))
    }
}

/// A line longer than a page alone, cut to one.
fn cut_line(l: &str) -> String {
    if l.len() <= PAGE {
        return l.to_string();
    }
    let mut end = PAGE - '…'.len_utf8();
    while !l.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &l[..end])
}

/// The pages `lines` make: each as many whole lines as fit in `PAGE` bytes,
/// at least one, in order. None for no lines.
fn paginate(lines: &[&str]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let (mut start, mut len) = (0, 0);
    for (i, l) in lines.iter().enumerate() {
        let add = l.len().min(PAGE) + 1;
        if i > start && len + add > PAGE {
            out.push(start..i);
            (start, len) = (i, 0);
        }
        len += add;
    }
    if start < lines.len() {
        out.push(start..lines.len());
    }
    out
}

/// `<turn>:L<loop>:tools` as its turn and loop.
pub(crate) fn loop_of(key: &str) -> Option<(&str, u32)> {
    let (turn, li) = key.strip_suffix(TOOLS)?.rsplit_once(":L")?;
    Some((turn, li.parse().ok()?))
}

impl Folds {
    /// The turns the renderer holds now: the rest are forgotten.
    pub(crate) fn hold(&mut self, held: &[String]) {
        self.turns.retain(|(t, _)| held.contains(t));
    }

    fn fold_mut(&mut self, turn: &str) -> &mut Fold {
        let i = match self.turns.iter().position(|(t, _)| t == turn) {
            Some(i) => i,
            None => {
                self.turns.push_back((turn.to_string(), Fold::default()));
                self.turns.len() - 1
            }
        };
        &mut self.turns[i].1
    }

    /// Take a process message into its turn's fold.
    fn take_one(&mut self, turn: &str, li: u32, content: String, buttons: &Buttons) {
        let f = self.fold_mut(turn);
        f.loops.insert(li, content);
        match buttons {
            Buttons::ShouldHaveAsked(menu) => {
                f.menus.insert(li, menu.clone());
            }
            Buttons::Clear => {
                f.menus.remove(&li);
            }
            _ => {}
        }
    }

    /// The place's ops, folded: every process message is taken into its
    /// turn's fold, and the fold's one message is written where the turn's
    /// first process message of the batch was, when what it shows changed.
    /// `head` is a turn's closed line. Every other op is as it was.
    pub(crate) fn take(&mut self, ops: Vec<Op>, head: impl Fn(&str) -> String) -> Vec<Op> {
        let mut out: Vec<Result<Op, String>> = Vec::new();
        for op in ops {
            match op {
                Op::Upsert {
                    key,
                    content,
                    buttons,
                } => match loop_of(&key) {
                    Some((turn, li)) => {
                        self.take_one(turn, li, content, &buttons);
                        if !out.iter().any(|o| matches!(o, Err(t) if t == turn)) {
                            out.push(Err(turn.to_string()));
                        }
                    }
                    None => out.push(Ok(Op::Upsert {
                        key,
                        content,
                        buttons,
                    })),
                },
                op => out.push(Ok(op)),
            }
        }
        out.into_iter()
            .filter_map(|o| match o {
                Ok(op) => Some(op),
                Err(turn) => {
                    let h = head(&turn);
                    self.write(&turn, &h)
                }
            })
            .collect()
    }

    /// The fold of `turn` as it shows now, when that changed.
    fn write(&mut self, turn: &str, head: &str) -> Option<Op> {
        self.write_as(turn, head, false)
    }

    /// The same, written `again` though Discord was last sent it (after an
    /// outage, whose writes may not have landed).
    fn write_as(&mut self, turn: &str, head: &str, again: bool) -> Option<Op> {
        let f = self.fold_mut(turn);
        let shown = f.render(turn, head)?;
        f.page = shown.1.page;
        if !again && f.shown.as_ref() == Some(&shown) {
            return None;
        }
        f.shown = Some(shown.clone());
        Some(Op::Upsert {
            key: format!("{turn}{SUFFIX}"),
            content: shown.0,
            buttons: Buttons::Fold(shown.1),
        })
    }

    /// Whether this place holds a fold for `turn`.
    pub(crate) fn holds(&self, turn: &str) -> bool {
        self.turns.iter().any(|(t, _)| t == turn)
    }

    /// A press: `turn`'s fold open at `page`, or closed (None). `seed` gives
    /// its process messages when this place holds no fold for it (a
    /// restart). What it shows now, or None when there is nothing to show.
    pub(crate) fn press(
        &mut self,
        turn: &str,
        page: Option<usize>,
        head: &str,
        seed: impl FnOnce() -> Vec<Op>,
    ) -> Option<(String, Buttons)> {
        if !self.holds(turn) {
            let ops = seed();
            if ops.is_empty() {
                return None;
            }
            for op in ops {
                if let Op::Upsert {
                    key,
                    content,
                    buttons,
                } = op
                {
                    if let Some((t, li)) = loop_of(&key) {
                        self.take_one(t, li, content, &buttons);
                    }
                }
            }
        }
        let f = self.fold_mut(turn);
        let was = f.page;
        f.page = page;
        let Some(shown) = f.render(turn, head) else {
            f.page = was;
            return None;
        };
        f.page = shown.1.page;
        f.shown = Some(shown.clone());
        Some((shown.0, Buttons::Fold(shown.1)))
    }

    /// The turns this place holds a fold of.
    pub(crate) fn turns(&self) -> Vec<String> {
        self.turns.iter().map(|(t, _)| t.clone()).collect()
    }

    /// The loops `turn`'s fold took, in order.
    pub(crate) fn loops(&self, turn: &str) -> Vec<u32> {
        self.turns
            .iter()
            .find(|(t, _)| t == turn)
            .map(|(_, f)| f.loops.keys().copied().collect())
            .unwrap_or_default()
    }

    /// `turn`'s fold made again from `ops`, its loops' process messages as
    /// the place shows them now (`View::set`): each loop it took takes its
    /// message and its choices from them, and none it did not take is
    /// added. What it shows now, when that changed, or `again` even if not.
    pub(crate) fn remake(
        &mut self,
        turn: &str,
        ops: Vec<Op>,
        head: &str,
        again: bool,
    ) -> Option<Op> {
        let f = self.fold_mut(turn);
        for op in ops {
            let Op::Upsert {
                key,
                content,
                buttons,
            } = op
            else {
                continue;
            };
            let Some((_, li)) = loop_of(&key) else {
                continue;
            };
            if let Some(c) = f.loops.get_mut(&li) {
                *c = content;
                match buttons {
                    Buttons::ShouldHaveAsked(menu) => {
                        f.menus.insert(li, menu);
                    }
                    _ => {
                        f.menus.remove(&li);
                    }
                }
            }
        }
        self.write_as(turn, head, again)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(calls: &[(&str, ToolState)]) -> Tally {
        let mut t = Tally::default();
        for (tool, s) in calls {
            t.calls += 1;
            let f = Family::of(tool);
            match t.kinds.iter_mut().find(|(k, _)| *k == f) {
                Some((_, n)) => *n += 1,
                None => t.kinds.push((f, 1)),
            }
            if let Some(i) = state_of(s) {
                *t.states.entry(i).or_default() += 1;
            }
        }
        t
    }

    const BOTH: Shown = Shown {
        tools: true,
        thinking: true,
    };

    fn done() -> ToolState {
        ToolState::Done {
            status: "ok".into(),
            ms: 3,
        }
    }

    /// The closed line counts the calls by family and names the states worth
    /// a word, and says nothing of what a call was given or returned.
    #[test]
    fn the_closed_line_counts_steps_by_family_and_state() {
        let t = tally(&[
            ("fs.read", done()),
            ("fs.read", done()),
            ("fs.grep", done()),
            (
                "proc.run",
                ToolState::Done {
                    status: "error".into(),
                    ms: 9,
                },
            ),
            ("fs.write", ToolState::Waiting),
            ("mcp.tool", done()),
        ]);
        assert_eq!(
            line(Some(&t), None, BOTH),
            "-# 6 steps: 2 reads, 1 search, 1 command, 1 edit, 1 other call · 1 failed · 1 waiting for approval"
        );
        assert_eq!(
            line(
                Some(&tally(&[("fs.read", done())])),
                Some(Thought::For(6)),
                BOTH
            ),
            "-# 💭 thought for 6 s · 1 step: 1 read"
        );
        assert_eq!(
            line(
                Some(&t),
                Some(Thought::Thinking),
                Shown {
                    tools: false,
                    thinking: true
                }
            ),
            "-# 💭 thinking"
        );
        assert_eq!(
            line(
                Some(&t),
                Some(Thought::For(2)),
                Shown {
                    tools: false,
                    thinking: false
                }
            ),
            ""
        );
    }

    fn upsert(key: &str, content: &str) -> Op {
        Op::Upsert {
            key: key.into(),
            content: content.into(),
            buttons: Buttons::Keep,
        }
    }

    /// Two loops' process messages are one fold, written where the first
    /// was, closed; the text part between them is untouched; nothing is
    /// written again when what it shows did not change.
    #[test]
    fn a_turns_process_messages_are_one_closed_fold() {
        let mut f = Folds::default();
        let ops = vec![
            upsert("turn_a:L0:p0", "Reading."),
            upsert("turn_a:L0:tools", "-# ✅ `fs.read` a · 3 ms"),
            upsert("turn_a:L1:tools", "-# ⏳ `fs.read` b"),
        ];
        let out = f.take(ops, |_| "-# 2 steps: 2 reads".into());
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0], upsert("turn_a:L0:p0", "Reading."));
        let Op::Upsert {
            key,
            content,
            buttons: Buttons::Fold(b),
        } = &out[1]
        else {
            panic!("{out:?}")
        };
        assert_eq!(
            (key.as_str(), content.as_str()),
            ("turn_a:fold", "-# 2 steps: 2 reads")
        );
        assert_eq!((b.page, b.pages), (None, 1));
        // The same again writes nothing.
        let again = f.take(
            vec![upsert("turn_a:L1:tools", "-# ⏳ `fs.read` b")],
            |_| "-# 2 steps: 2 reads".into(),
        );
        assert!(again.is_empty(), "{again:?}");
        // Open, it is the loops' lines, in order; closed, the line again.
        let (open, _) = f
            .press("turn_a", Some(0), "-# 2 steps: 2 reads", Vec::new)
            .unwrap();
        assert_eq!(open, "-# ✅ `fs.read` a · 3 ms\n-# ⏳ `fs.read` b");
        let (closed, _) = f
            .press("turn_a", None, "-# 2 steps: 2 reads", Vec::new)
            .unwrap();
        assert_eq!(closed, "-# 2 steps: 2 reads");
    }

    /// Many lines are cut whole-line into pages under a message's limit,
    /// each saying how many more there are; every line shows on one page.
    #[test]
    fn a_long_fold_pages_whole_lines() {
        let mut f = Folds::default();
        let lines: Vec<String> = (0..60)
            .map(|i| {
                format!("-# ✅ `fs.read` /a/rather/long/path/to/a/file/number/{i:03}.txt · 3 ms")
            })
            .collect();
        let ops: Vec<Op> = lines
            .chunks(20)
            .enumerate()
            .map(|(li, c)| upsert(&format!("turn_b:L{li}:tools"), &c.join("\n")))
            .collect();
        f.take(ops, |_| "-# 60 steps: 60 reads".into());
        let (first, b) = f.press("turn_b", Some(0), "x", Vec::new).unwrap();
        let Buttons::Fold(b) = b else { panic!() };
        assert!(b.pages > 1, "{b:?}");
        let mut seen = Vec::new();
        for p in 0..b.pages {
            let (page, _) = f.press("turn_b", Some(p), "x", Vec::new).unwrap();
            assert!(page.len() <= PART_LIMIT, "page {p}: {} bytes", page.len());
            let (body, more) = page.rsplit_once('\n').unwrap();
            assert!(
                more.starts_with("-# … and ")
                    && more.ends_with(&format!("page {} of {}", p + 1, b.pages)),
                "{more}"
            );
            for l in body.lines() {
                assert!(lines.iter().any(|x| x == l), "a whole line: {l}");
                seen.push(l.to_string());
            }
        }
        assert_eq!(seen, lines);
        assert!(first.starts_with(&lines[0]));
        // A page past the last shows the last.
        let (past, _) = f.press("turn_b", Some(99), "x", Vec::new).unwrap();
        assert!(
            past.ends_with(&format!("page {} of {}", b.pages, b.pages)),
            "{past}"
        );
    }

    /// A press on a fold this place does not hold takes its lines from
    /// `seed`; with none to take, it shows nothing.
    #[test]
    fn a_press_after_a_restart_reads_the_lines_from_the_seed() {
        let mut f = Folds::default();
        assert!(f
            .press("turn_c", Some(0), "-# 1 step: 1 read", Vec::new)
            .is_none());
        let (open, _) = f
            .press("turn_c", Some(0), "-# 1 step: 1 read", || {
                vec![upsert("turn_c:L0:tools", "-# ✅ `fs.read` c · 2 ms")]
            })
            .unwrap();
        assert_eq!(open, "-# ✅ `fs.read` c · 2 ms");
    }
}
