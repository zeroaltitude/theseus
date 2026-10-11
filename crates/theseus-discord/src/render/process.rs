//! A loop's process message (theseus-l1y1): the message its tool lines go
//! into, `<turn>:L<loop>:tools`, with the loop's thinking at its top. The
//! model's thinking (`model.thinking`; Anthropic's summarized thinking by
//! default, `[profiles.*] thinking_display`) streams there as `-#` lines,
//! and once the loop's text starts (its first text, or the loop's end with
//! none) it folds to one line, `-# 💭 thought for 6 s`. The tool lines below
//! it are the renderer's, as they are without thinking.
//!
//! So thinking makes no message of its own: the process message is made at
//! the loop's first thinking or its first tool line, whichever comes first,
//! and a loop that thinks and calls a tool makes the creates it makes
//! without thinking. A process message that holds thinking goes out silent,
//! its tool lines with it (`holds_thinking`, read by `policy::of_live`): a
//! thinking turn buzzes for its answer alone. Live progress, best
//! effort, never a post; the whole thinking is the session's history. It
//! holds the turns the place's renderer holds (`RECENT_TURNS`, by the same
//! `turn.started`), so the lane keeps and forgets its keys with theirs.

use std::collections::{BTreeMap, VecDeque};

use theseus_protocol::Event;
use tokio::time::Instant;

use super::{Buttons, Op, DISCORD_LIMIT, RECENT_TURNS};

/// The most of a loop's thinking its message shows, in characters, when its
/// tool lines leave room for it.
const SHOWN: usize = 1800;

/// The most of a loop's thinking kept, in characters: what it shows, with
/// room for the whitespace its start trims. The rest is only counted, so a
/// long thinking costs the place no more memory than a short one.
const KEPT: usize = 2 * SHOWN;

/// The thinking's first line while it streams.
const HEAD: &str = "-# 💭 thinking";

/// Room kept for the line that says what was left out.
const MORE_LINE: usize = 40;

/// How a process message's key ends: `<turn>:L<loop>:tools`.
const SUFFIX: &str = ":tools";

/// What a place shows: its tool lines, its thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shown {
    pub tools: bool,
    pub thinking: bool,
}

#[derive(Default)]
pub(crate) struct Process {
    turns: VecDeque<(String, BTreeMap<u32, Loop>)>,
}

#[derive(Default)]
struct Loop {
    /// Its first `KEPT` characters of thinking, how many that is, and how
    /// many more came.
    text: String,
    kept: usize,
    more: usize,
    /// Its first thinking, and, once its text started, how long it thought.
    since: Option<Instant>,
    thought: Option<u64>,
    /// The renderer's tool lines for the loop, as it last gave them.
    tools: Option<String>,
    /// What Discord last got, and whether there is more since.
    shown: Option<String>,
    dirty: bool,
}

impl Loop {
    fn thinks(&self) -> bool {
        !self.text.trim().is_empty()
    }

    /// The message as the place shows it: the thinking (streaming, or
    /// folded), then the tool lines; empty when it shows neither.
    fn content(&self, s: Shown) -> String {
        let tools = if s.tools {
            self.tools.as_deref().unwrap_or("")
        } else {
            ""
        };
        let head = match self.thought {
            _ if !s.thinking || !self.thinks() => String::new(),
            Some(secs) => format!("-# 💭 thought for {secs} s"),
            None => streaming(
                &self.text,
                self.more,
                (DISCORD_LIMIT - 1).saturating_sub(tools.len()),
            ),
        };
        match (head.is_empty(), tools.is_empty()) {
            (_, true) => head,
            (true, false) => tools.to_string(),
            (false, false) => format!("{head}\n{tools}"),
        }
    }
}

impl Process {
    /// One event of the place's session. Thinking's first text shows at
    /// once, the rest at the tick; the fold shows at once.
    pub(crate) fn on_event(&mut self, e: &Event, now: Instant, s: Shown) -> Vec<Op> {
        let Some(turn) = e.turn_id() else {
            return vec![];
        };
        match e {
            Event::TurnStarted(_) => {
                self.turns.push_back((turn.to_string(), BTreeMap::new()));
                while self.turns.len() > RECENT_TURNS {
                    self.turns.pop_front();
                }
                return vec![];
            }
            Event::ModelThinking(d) => {
                let Some(lp) = self.loop_mut(turn, d.loop_index) else {
                    return vec![];
                };
                if lp.thought.is_some() {
                    return vec![];
                }
                let first = !lp.thinks();
                lp.since.get_or_insert(now);
                for c in d.text.chars() {
                    if lp.kept < KEPT {
                        lp.text.push(c);
                        lp.kept += 1;
                    } else {
                        lp.more += 1;
                    }
                }
                lp.dirty = true;
                if !(first && lp.thinks()) {
                    return vec![];
                }
            }
            Event::ModelDelta(d) if !d.text.trim().is_empty() => {
                self.fold(turn, Some(d.loop_index), now)
            }
            Event::ModelAnswered(d) => self.fold(turn, Some(d.loop_index), now),
            Event::LoopEnded(l) => self.fold(turn, Some(l.loop_index), now),
            Event::TurnEnded(_) | Event::TurnFailed(_) => self.fold(turn, None, now),
            _ => return vec![],
        }
        self.tick(s)
    }

    /// Each loop whose message changed since Discord last got it.
    pub(crate) fn tick(&mut self, s: Shown) -> Vec<Op> {
        let mut ops = Vec::new();
        for (turn, loops) in &mut self.turns {
            for (li, lp) in loops.iter_mut().filter(|(_, lp)| lp.dirty) {
                lp.dirty = false;
                let content = lp.content(s);
                if content.is_empty() || lp.shown.as_ref() == Some(&content) {
                    continue;
                }
                lp.shown = Some(content.clone());
                ops.push(Op::Upsert {
                    key: format!("{turn}:L{li}{SUFFIX}"),
                    content,
                    buttons: Buttons::Keep,
                });
            }
        }
        ops
    }

    /// The renderer's ops, as the place shows them: a loop's tool lines go
    /// under its thinking, a place that hides its tools shows its thinking
    /// alone, and a notice embed only with its tools.
    pub(crate) fn fold_ops(&mut self, ops: Vec<Op>, s: Shown) -> Vec<Op> {
        let mut out = Vec::new();
        for op in ops {
            match op {
                Op::Upsert {
                    key,
                    content,
                    buttons,
                } if key.ends_with(SUFFIX) => {
                    let Some(lp) = loop_of(&key).and_then(|(t, li)| self.loop_mut(t, li)) else {
                        if s.tools {
                            out.push(Op::Upsert {
                                key,
                                content,
                                buttons,
                            });
                        }
                        continue;
                    };
                    lp.tools = Some(content);
                    lp.dirty = false;
                    let content = lp.content(s);
                    let buttons = if s.tools { buttons } else { Buttons::Keep };
                    if content.is_empty()
                        || (lp.shown.as_ref() == Some(&content) && buttons == Buttons::Keep)
                    {
                        continue;
                    }
                    lp.shown = Some(content.clone());
                    out.push(Op::Upsert {
                        key,
                        content,
                        buttons,
                    });
                }
                Op::Notice { .. } if !s.tools => {}
                op => out.push(op),
            }
        }
        out
    }

    /// The loop's text started, or it ended (`li`: None, every loop of the
    /// turn): its thinking folds to how long it took.
    fn fold(&mut self, turn: &str, li: Option<u32>, now: Instant) {
        let Some((_, loops)) = self.turns.iter_mut().find(|(t, _)| t == turn) else {
            return;
        };
        for (_, lp) in loops
            .iter_mut()
            .filter(|(i, _)| li.is_none_or(|li| **i == li))
        {
            if let (Some(since), None, true) = (lp.since, lp.thought, lp.thinks()) {
                let ms = now.saturating_duration_since(since).as_millis();
                lp.thought = Some(u64::try_from(ms.div_ceil(1000)).unwrap_or(u64::MAX).max(1));
                lp.dirty = true;
            }
        }
    }

    fn loop_mut(&mut self, turn: &str, li: u32) -> Option<&mut Loop> {
        let (_, loops) = self.turns.iter_mut().find(|(t, _)| t == turn)?;
        Some(loops.entry(li).or_default())
    }
}

/// `<turn>:L<loop>:tools` as its turn and loop.
fn loop_of(key: &str) -> Option<(&str, u32)> {
    let (turn, li) = key.strip_suffix(SUFFIX)?.rsplit_once(":L")?;
    Some((turn, li.parse().ok()?))
}

/// Does a process message hold its loop's thinking, streaming or folded?
/// Its create goes out silent, tool lines and all (`policy::of_live`; the
/// owner's call on the fold, 2026-10-10: a thinking turn buzzes for its
/// answer alone). The thinking is always its top line; a tool line never
/// starts with `-# 💭`.
pub(crate) fn holds_thinking(content: &str) -> bool {
    content.starts_with("-# 💭")
}

/// The thinking as it streams: its header, then its lines as `-#` lines, at
/// most `SHOWN` characters of it and `budget` bytes in all, saying how many
/// characters it left out (`more` came past what was kept). Its backticks
/// and backslashes are escaped, so a code fence in the thinking shows as
/// text and never opens a block over the tool lines below it.
fn streaming(text: &str, more: usize, budget: usize) -> String {
    let text = if more == 0 {
        text.trim()
    } else {
        text.trim_start()
    };
    let total = text.chars().count() + more;
    let room = budget.saturating_sub(MORE_LINE);
    let mut out = HEAD.to_string();
    let (mut used, mut open) = (0, false);
    for c in text.chars() {
        if used >= SHOWN {
            break;
        }
        if c == '\n' || (!open && c.is_whitespace()) {
            open = open && c != '\n';
            used += 1;
            continue;
        }
        let escaped = matches!(c, '`' | '\\');
        let add = c.len_utf8() + usize::from(escaped) + if open { 0 } else { 4 };
        if out.len() + add > room {
            break;
        }
        if !open {
            out.push_str("\n-# ");
            open = true;
        }
        if escaped {
            out.push('\\');
        }
        out.push(c);
        used += 1;
    }
    if total > used {
        out.push_str(&format!("\n-# … ({} more characters)", total - used));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use theseus_protocol::{LoopEnded, ModelDelta, TurnStarted, TurnSubmitResult};

    const BOTH: Shown = Shown {
        tools: true,
        thinking: true,
    };

    fn started(turn: &str) -> Event {
        Event::TurnStarted(TurnStarted {
            turn_id: turn.into(),
            ..Default::default()
        })
    }

    fn delta(turn: &str, li: u32, text: &str) -> ModelDelta {
        ModelDelta {
            turn_id: turn.into(),
            loop_index: li,
            text: text.into(),
        }
    }

    fn tools(turn: &str, li: u32, content: &str) -> Op {
        Op::Upsert {
            key: format!("{turn}:L{li}:tools"),
            content: content.into(),
            buttons: Buttons::Keep,
        }
    }

    fn upserts(ops: &[Op]) -> Vec<(String, String)> {
        ops.iter()
            .filter_map(|o| match o {
                Op::Upsert { key, content, .. } => Some((key.clone(), content.clone())),
                _ => None,
            })
            .collect()
    }

    /// Thinking streams at the top of the loop's process message, its first
    /// text at once and the rest at the tick; the loop's tool line goes under
    /// it; the loop's text folds it to one line, and the tool line stays.
    #[test]
    fn thinking_streams_at_the_top_and_folds_when_the_text_starts() {
        let mut p = Process::default();
        let t0 = Instant::now();
        assert!(p.on_event(&started("turn_a"), t0, BOTH).is_empty());
        let think = |text| Event::ModelThinking(delta("turn_a", 0, text));
        let got = upserts(&p.on_event(&think("The tide chart"), t0, BOTH));
        assert_eq!(
            got,
            [(
                "turn_a:L0:tools".to_string(),
                "-# 💭 thinking\n-# The tide chart".to_string()
            )]
        );
        assert!(holds_thinking(&got[0].1));
        assert!(p.on_event(&think(" is in\n\nwork/."), t0, BOTH).is_empty());
        let got = upserts(&p.tick(BOTH));
        assert_eq!(
            got[0].1,
            "-# 💭 thinking\n-# The tide chart is in\n-# work/."
        );
        assert!(p.tick(BOTH).is_empty(), "nothing changed since");
        // A tool line before the text goes under the thinking.
        let got = upserts(&p.fold_ops(vec![tools("turn_a", 0, "▫️ `fs.read` work/chart")], BOTH));
        assert_eq!(
            got[0].1,
            "-# 💭 thinking\n-# The tide chart is in\n-# work/.\n▫️ `fs.read` work/chart"
        );
        assert!(holds_thinking(&got[0].1), "a tool line under thinking");
        // The text starts 5.2 s after the first thinking: the fold, at once.
        let text = Event::ModelDelta(delta("turn_a", 0, "Reading it."));
        let got = upserts(&p.on_event(&text, t0 + Duration::from_millis(5200), BOTH));
        assert_eq!(got[0].1, "-# 💭 thought for 6 s\n▫️ `fs.read` work/chart");
        // Thinking after the fold changes nothing; the tool line still does.
        assert!(p.on_event(&think("more"), t0, BOTH).is_empty());
        assert!(p.tick(BOTH).is_empty());
        let got = upserts(&p.fold_ops(
            vec![tools("turn_a", 0, "✅ `fs.read` work/chart · 3 ms")],
            BOTH,
        ));
        assert_eq!(
            got[0].1,
            "-# 💭 thought for 6 s\n✅ `fs.read` work/chart · 3 ms"
        );
    }

    /// A loop that ends with no text folds at its end, and a turn's end folds
    /// whatever is left; a loop with no thinking shows its tool lines as
    /// they are, and an unchanged one is not sent again.
    #[test]
    fn a_loop_folds_at_its_end_and_tools_alone_are_as_they_were() {
        let mut p = Process::default();
        let t0 = Instant::now();
        p.on_event(&started("turn_a"), t0, BOTH);
        p.on_event(&Event::ModelThinking(delta("turn_a", 0, "Hmm.")), t0, BOTH);
        let end = Event::LoopEnded(LoopEnded {
            turn_id: "turn_a".into(),
            loop_index: 0,
            ..Default::default()
        });
        let got = upserts(&p.on_event(&end, t0 + Duration::from_millis(300), BOTH));
        assert_eq!(got[0].1, "-# 💭 thought for 1 s");
        assert!(holds_thinking(&got[0].1));
        let line = tools("turn_a", 1, "▫️ `fs.read` a");
        let got = upserts(&p.fold_ops(vec![line.clone()], BOTH));
        assert_eq!(
            got[0].1, "▫️ `fs.read` a",
            "no thinking: the line as it was"
        );
        assert!(p.fold_ops(vec![line], BOTH).is_empty(), "unchanged");
        p.on_event(&Event::ModelThinking(delta("turn_a", 2, "Then.")), t0, BOTH);
        let ended = Event::TurnEnded(TurnSubmitResult {
            turn_id: "turn_a".into(),
            ..Default::default()
        });
        let got = upserts(&p.on_event(&ended, t0 + Duration::from_secs(2), BOTH));
        assert_eq!(
            got,
            [("turn_a:L2:tools".into(), "-# 💭 thought for 2 s".into())]
        );
    }

    /// A place that hides its thinking shows its tool lines as they were;
    /// one that hides its tools shows its thinking alone, and no embed.
    #[test]
    fn each_word_hides_its_own_half() {
        let t0 = Instant::now();
        for s in [
            Shown {
                tools: true,
                thinking: false,
            },
            Shown {
                tools: false,
                thinking: true,
            },
            Shown {
                tools: false,
                thinking: false,
            },
        ] {
            let mut p = Process::default();
            p.on_event(&started("turn_a"), t0, s);
            let thought =
                upserts(&p.on_event(&Event::ModelThinking(delta("turn_a", 0, "Hmm.")), t0, s));
            assert_eq!(thought.len(), usize::from(s.thinking), "{s:?}");
            let got = upserts(&p.fold_ops(vec![tools("turn_a", 0, "▫️ `fs.read` a")], s));
            match (s.tools, s.thinking) {
                (true, _) => assert_eq!(got[0].1, "▫️ `fs.read` a"),
                (false, true) => assert!(got.is_empty(), "the thinking is as it was"),
                (false, false) => assert!(got.is_empty()),
            }
        }
    }

    /// Long thinking is cut to what fits beside the tool lines, within
    /// Discord's 2,000, saying how much it left out; a long one keeps only
    /// its start; and it holds the renderer's recent turns alone.
    #[test]
    fn long_thinking_is_cut_and_says_so_and_old_turns_are_dropped() {
        let mut p = Process::default();
        let t0 = Instant::now();
        p.on_event(&started("turn_a"), t0, BOTH);
        let long = "x".repeat(SHOWN + 25);
        let got = upserts(&p.on_event(&Event::ModelThinking(delta("turn_a", 0, &long)), t0, BOTH));
        assert!(got[0].1.ends_with("\n-# … (25 more characters)"), "{got:?}");
        assert!(got[0].1.chars().count() <= 2000);
        let tide = "é".repeat(KEPT);
        assert!(p
            .on_event(&Event::ModelThinking(delta("turn_a", 0, &tide)), t0, BOTH)
            .is_empty());
        assert_eq!(
            p.turns[0].1[&0].text.chars().count(),
            KEPT,
            "only the start is kept"
        );
        let got = upserts(&p.tick(BOTH));
        assert!(
            got[0]
                .1
                .ends_with(&format!("… ({} more characters)", 25 + KEPT)),
            "{got:?}"
        );
        // Tool lines near the limit leave the thinking what room is left.
        let lines = "▫️ `fs.read` a\n".repeat(90);
        let got = upserts(&p.fold_ops(vec![tools("turn_a", 0, lines.trim_end())], BOTH));
        assert!(got[0].1.len() <= 2000, "{}", got[0].1.len());
        assert!(got[0].1.starts_with("-# 💭 thinking\n-# xxx"));
        assert!(got[0].1.contains(" more characters)\n▫️"));
        assert!(p
            .on_event(
                &Event::ModelThinking(delta("turn_x", 0, "unseen")),
                t0,
                BOTH
            )
            .is_empty());
        for i in 0..RECENT_TURNS {
            p.on_event(&started(&format!("turn_{i}")), t0, BOTH);
        }
        assert_eq!(p.turns.len(), RECENT_TURNS);
        assert!(p.turns.iter().all(|(id, _)| id != "turn_a"));
    }

    /// A tool loop folds when its call is made (`model.answered`), not when
    /// the loop ends after the tool ran: its N is the thinking's own seconds
    /// (a 9 s tool after 1.2 s of thinking says 2 s, never 9).
    #[test]
    fn a_tool_loop_folds_at_its_answer_not_after_its_tool() {
        let mut p = Process::default();
        let t0 = Instant::now();
        p.on_event(&started("turn_a"), t0, BOTH);
        p.on_event(&Event::ModelThinking(delta("turn_a", 0, "Hmm.")), t0, BOTH);
        p.fold_ops(vec![tools("turn_a", 0, "▫️ `proc.run` build")], BOTH);
        let answered = Event::ModelAnswered(delta("turn_a", 0, ""));
        let got = upserts(&p.on_event(&answered, t0 + Duration::from_millis(1200), BOTH));
        assert_eq!(got[0].1, "-# 💭 thought for 2 s\n▫️ `proc.run` build");
        let end = Event::LoopEnded(LoopEnded {
            turn_id: "turn_a".into(),
            loop_index: 0,
            ..Default::default()
        });
        assert!(
            p.on_event(&end, t0 + Duration::from_secs(9), BOTH)
                .is_empty(),
            "the loop's end changes nothing"
        );
    }

    /// A code fence in the thinking shows as text while it streams, so the
    /// tool lines below it stay tool lines, not a code block.
    #[test]
    fn a_fence_in_the_thinking_never_swallows_the_tool_lines() {
        let mut p = Process::default();
        let t0 = Instant::now();
        p.on_event(&started("turn_a"), t0, BOTH);
        let think = "Like this:\n```rust\nfn tide() {}\n```\nThen C:\\work.";
        p.on_event(&Event::ModelThinking(delta("turn_a", 0, think)), t0, BOTH);
        let got = upserts(&p.fold_ops(vec![tools("turn_a", 0, "▫️ `fs.read` a")], BOTH));
        let content = &got[0].1;
        assert!(!content.contains("```"), "{content}");
        assert_eq!(
            content,
            "-# 💭 thinking\n-# Like this:\n-# \\`\\`\\`rust\n-# fn tide() {}\n\
             -# \\`\\`\\`\n-# Then C:\\\\work.\n▫️ `fs.read` a"
        );
    }

    /// A message holds thinking when its top line is the thinking's, with
    /// tool lines under it or not; tool lines alone never do.
    #[test]
    fn a_message_holds_thinking_by_its_top_line_alone() {
        assert!(holds_thinking(
            "-# 💭 thinking\n-# a\n-# … (3 more characters)"
        ));
        assert!(holds_thinking("-# 💭 thought for 2 s"));
        assert!(holds_thinking("-# 💭 thought for 2 s\n▫️ `fs.read` a"));
        assert!(holds_thinking("-# 💭 thinking\n-# a\n▫️ `fs.read` a"));
        assert!(!holds_thinking("-# … 3 earlier call(s)\n▫️ `fs.read` a"));
        assert!(!holds_thinking("▫️ `fs.read` a"));
    }
}
