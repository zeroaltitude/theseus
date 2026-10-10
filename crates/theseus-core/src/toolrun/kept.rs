//! A result's cap (theseus-46v), and what it keeps of what it cuts
//! (theseus-v73m). A job's result (`proc.run`, a batch's step) and a
//! terminal's screen (`term.read`) cut by `[tools] result_max_chars` keep
//! their whole output, scrubbed, in the session's outputs
//! (`crate::outputs`), and the cut line names the file and the lines it left
//! out, counted in the whole output, with the `fs_read` that reads them. A
//! tool with a cap of its own (`fs.read`, `Tool::result_max_chars`) is cut to
//! a contiguous head at its last whole line, never head and tail. Every other
//! result keeps its head and tail, as before.

use theseus_tools::REST_NARROWER;

use super::{cap, ResultNode, ToolRuntime};
use crate::narrative;

/// Where the output in a result's text is, and what it was read from: the
/// output is the text's last `from_end` bytes but its last `after` (a batch's
/// later steps), and `raw`, when it is a job's, the file whose last bytes it
/// is, `unread` bytes not read before them.
#[derive(Debug, Clone, Default)]
pub(super) struct Source {
    pub raw: Option<String>,
    pub unread: u64,
    pub from_end: usize,
    pub after: usize,
}

/// The most lines a cut line's `fs_read` asks for at once: its default.
const READ_LINES: usize = 2000;

impl ToolRuntime {
    /// `scrubbed`, `r`'s text scrubbed, capped: the content, whether it was
    /// cut, and the file it names when the whole output was kept.
    pub(super) fn capped(
        &self,
        session_id: &str,
        r: &ResultNode<'_>,
        scrubbed: &str,
    ) -> (String, bool, Option<String>) {
        let tool = self.tool(r.tool);
        let rest = |left: &str| {
            tool.as_ref()
                .map_or_else(|| REST_NARROWER.into(), |t| t.rest(left))
        };
        if let Some(max) = tool.as_ref().and_then(|t| t.result_max_chars()) {
            let (c, t) = head(scrubbed, max, rest);
            return (c, t, None);
        }
        let max = self.result_max_chars;
        let source = r.kept.clone().or_else(|| {
            (r.tool == crate::term::READ).then(|| Source {
                from_end: r.text.len(),
                ..Source::default()
            })
        });
        let Some(source) = source.filter(|_| scrubbed.chars().count() > max && max >= 64) else {
            let (c, t) = cap(scrubbed, max, rest);
            return (c, t, None);
        };
        let span = self.output_span(&r.text, &source, scrubbed);
        match self.keep(session_id, r.tool_use_id, &source, span.as_ref(), scrubbed) {
            Ok((path, before)) => {
                let (c, t) = cap(scrubbed, max, |left| {
                    // `left` is a part of `scrubbed`: where it starts.
                    let at = left.as_ptr() as usize - scrubbed.as_ptr() as usize;
                    words(&path, span.as_ref(), scrubbed, at, left, before)
                });
                (c, t, Some(path))
            }
            Err(e) => {
                tracing::warn!(error = %e, tool = r.tool, "a capped result's whole output was not kept");
                let (c, t) = cap(scrubbed, max, |left| {
                    format!("its whole output was not kept ({e}): {}", rest(left))
                });
                (c, t, None)
            }
        }
    }

    /// The output's bytes in `scrubbed`: the text around it scrubbed alone,
    /// found at either end. None when the scrub joined them.
    fn output_span(
        &self,
        text: &str,
        s: &Source,
        scrubbed: &str,
    ) -> Option<std::ops::Range<usize>> {
        let start = text.len().checked_sub(s.from_end)?;
        let end = text.len().checked_sub(s.after)?;
        let (pre, post) = (text.get(..start)?, text.get(end..)?);
        let (pre, post) = (self.scrubber.scrub(pre).0, self.scrubber.scrub(post).0);
        let end = scrubbed.len().checked_sub(post.len())?;
        (scrubbed.starts_with(&pre) && scrubbed.ends_with(&post) && pre.len() <= end)
            .then_some(pre.len()..end)
    }

    /// The whole output kept: the job's raw file scrubbed again whole, or
    /// the result's own output. Its path, and the lines before the result's
    /// output begins.
    fn keep(
        &self,
        session_id: &str,
        call: &str,
        s: &Source,
        span: Option<&std::ops::Range<usize>>,
        scrubbed: &str,
    ) -> anyhow::Result<(String, u64)> {
        let to = self
            .outputs
            .path_for(session_id, call)
            .ok_or_else(|| anyhow::anyhow!("no outputs directory"))?;
        let before = match (&s.raw, span) {
            (Some(raw), _) => theseus_store::blocking(|| {
                let scrub = |t: &str| self.scrubber.scrub(t).0;
                let max = self.output_max_bytes.saturating_add(64 * 1024);
                self.outputs
                    .keep_file(std::path::Path::new(raw), &to, &scrub, s.unread, max)
            })?,
            (None, Some(span)) => {
                theseus_store::blocking(|| self.outputs.keep_text(&scrubbed[span.clone()], &to))?;
                0
            }
            (None, None) => anyhow::bail!("its output could not be told from its text"),
        };
        Ok((to.display().to_string(), before))
    }
}

/// The cut line's words for a kept output: the lines `left` (at byte `at`
/// of `text`) holds, counted in the whole output, and the read that returns
/// them; only the file, when the cut is not all output.
fn words(
    path: &str,
    span: Option<&std::ops::Range<usize>>,
    text: &str,
    at: usize,
    left: &str,
    before: u64,
) -> String {
    let lines = |s: &str| s.bytes().filter(|&b| b == b'\n').count() as u64;
    match span.filter(|s| s.start <= at && at + left.len() <= s.end && !left.is_empty()) {
        Some(s) => {
            let first = before + 1 + lines(&text[s.start..at]);
            let last = first + lines(&left[..left.len() - 1]);
            let limit = (last + 1 - first).min(READ_LINES as u64);
            format!(
                "they are lines {first}-{last} of the whole output, kept at {path}; fs_read with \
                 offset={first} and limit={limit} reads them"
            )
        }
        None => format!(
            "the whole output is kept at {path}; fs_read with offset and limit reads it in ranges"
        ),
    }
}

/// `text` cut to at most `max` characters as a contiguous head, at the last
/// whole line that fits, with a line that says what is not shown and how to
/// get it (`rest`).
fn head(text: &str, max: usize, rest: impl FnOnce(&str) -> String) -> (String, bool) {
    let n = text.chars().count();
    if n <= max || max < 1024 {
        return (text.to_string(), false);
    }
    // Room for the line that says what is left.
    let keep = max - 512;
    let mut h = text.char_indices().nth(keep).map_or(text.len(), |(i, _)| i);
    if let Some(i) = text[..h].rfind('\n') {
        h = i + 1;
    }
    let left = &text[h..];
    let size = format!(
        "{} ({})",
        narrative::count(left.lines().count() as u64, "line", "lines"),
        narrative::count(left.chars().count() as u64, "character", "characters")
    );
    (
        format!("{}…[{size} not shown: {}]…\n", &text[..h], rest(left)),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_is_cut_at_a_whole_line_and_says_what_follows() {
        let text: String = (1..=500).map(|i| format!("{i:>6}\trow {i}\n")).collect();
        let (c, cut) = head(&text, 2_000, |left| format!("from {}", &left[..6]));
        assert!(cut);
        assert!(c.chars().count() <= 2_000, "{}", c.chars().count());
        let shown: Vec<&str> = c.lines().collect();
        let last = shown[shown.len() - 2];
        assert!(
            last.ends_with(&format!("row {}", shown.len() - 1)),
            "{last}"
        );
        assert!(
            shown[shown.len() - 1].starts_with(&format!("…[{} lines (", 500 - (shown.len() - 1))),
            "{c}"
        );
        assert!(
            c.ends_with(&format!("not shown: from {:>6}]…\n", shown.len())),
            "{c}"
        );
        assert_eq!(
            head("short", 2_000, |_| unreachable!()),
            ("short".into(), false)
        );
    }

    #[test]
    fn the_cut_line_counts_lines_in_the_whole_output() {
        let text = "[exit code 1]\nl1\nl2\nl3\nl4\nl5\n";
        let span = 14..text.len();
        let at = text.find("l3").unwrap();
        let left = &text[at..text.find("l5").unwrap()];
        assert_eq!(
            words("/s/o.out", Some(&span), text, at, left, 100),
            "they are lines 103-104 of the whole output, kept at /s/o.out; fs_read with \
             offset=103 and limit=2 reads them"
        );
        // A cut that reaches past the output names the file alone.
        assert_eq!(
            words("/s/o.out", Some(&span), text, 3, &text[3..20], 0),
            "the whole output is kept at /s/o.out; fs_read with offset and limit reads it in ranges"
        );
    }
}
