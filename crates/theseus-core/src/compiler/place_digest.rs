//! The place digest (theseus-98bz.9): what a turn in a thread is shown of
//! the place's other live threads when Jev's `scope` answer (classify.v4)
//! reads its message as asking across everything going on.
//!
//! - **One line a thread**, labelled `from thread «title»`: its goal, its
//!   card's last line, and the card's dated lines, cut to `line_chars`.
//! - **A sealed or seal-pending thread is a count** ("1 embargoed thread (not
//!   shown)"): no byte of its title, goal, card or working set is read here.
//! - **Per turn, in the tail.** The turn attaches it after the request's last
//!   cache mark (`task_graph::view::attach_text`), so every cached byte is the
//!   same with and without it, and no compilation keeps it.

use theseus_protocol::threads::ThreadInfo;

/// A digest, rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    pub text: String,
    /// The threads it has a line for.
    pub shown: usize,
    /// The sealed or seal-pending threads it only counts.
    pub embargoed: usize,
}

/// Whether a thread's matter stays out of every other thread.
pub(crate) fn held(t: &ThreadInfo) -> bool {
    t.sealed.is_some() || t.seal_pending.is_some()
}

/// Whether a card line carries a date (`2026-11-19`).
fn dated(line: &str) -> bool {
    line.as_bytes().windows(10).any(|w| {
        w.iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        })
    })
}

/// `text` cut to `max` characters, an ellipsis marking the cut.
fn cut(text: &str, max: usize) -> String {
    match text.chars().count() > max {
        true => {
            let mut s: String = text.chars().take(max.saturating_sub(1)).collect();
            s.push('\u{2026}');
            s
        }
        false => text.to_string(),
    }
}

/// One thread's line.
fn line(t: &ThreadInfo, max: usize) -> String {
    let mut parts = Vec::new();
    let goal = t.goal.lines().next().unwrap_or_default().trim();
    if !goal.is_empty() {
        parts.push(format!("goal: {goal}"));
    }
    if let Some((last, before)) = t.card.split_last() {
        parts.push(format!("latest: {}", last.trim()));
        let dates: Vec<&str> = before
            .iter()
            .map(|l| l.trim())
            .filter(|l| dated(l))
            .collect();
        if !dates.is_empty() {
            parts.push(format!("dated: {}", dates.join("; ")));
        }
    }
    let head = format!("- from thread \u{ab}{}\u{bb}", t.title);
    let body = match parts.is_empty() {
        true => head,
        false => format!("{head}: {}", parts.join(" | ")),
    };
    cut(&body, max)
}

/// The digest of `live` (the place's live threads, newest first) for a turn
/// in the thread `here`: none when no other thread is live.
pub fn render(live: &[&ThreadInfo], here: &str, line_chars: usize) -> Option<Digest> {
    let others: Vec<&ThreadInfo> = live.iter().copied().filter(|t| t.slug != here).collect();
    let embargoed = others.iter().filter(|t| held(t)).count();
    let lines: Vec<String> = others
        .iter()
        .filter(|t| !held(t))
        .map(|t| line(t, line_chars))
        .collect();
    if lines.is_empty() && embargoed == 0 {
        return None;
    }
    let mut text =
        String::from("[Place digest: the other live threads of this place, as they stand now.");
    for l in &lines {
        text.push('\n');
        text.push_str(l);
    }
    match embargoed {
        0 => {}
        1 => text.push_str("\n1 embargoed thread (not shown)."),
        n => text.push_str(&format!("\n{n} embargoed threads (not shown).")),
    }
    text.push(']');
    Some(Digest {
        text,
        shown: lines.len(),
        embargoed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::threads::{Seal, WorkingSet};

    fn thread(slug: &str, title: &str, card: &[&str]) -> ThreadInfo {
        ThreadInfo {
            slug: slug.into(),
            title: title.into(),
            goal: format!("Finish {title} this week.\nA second line, left out."),
            card: card.iter().map(|s| (*s).to_string()).collect(),
            working_set: WorkingSet {
                hosts: vec![format!("{slug}-stage-01.lan")],
                ..Default::default()
            },
            ..serde_json::from_value(serde_json::json!({"slug": slug, "title": title})).unwrap()
        }
    }

    #[test]
    fn a_line_holds_the_goal_the_latest_and_the_dated_lines() {
        let t = thread(
            "pr-round",
            "the tarnlib PR",
            &[
                "merge by 2026-11-19",
                "comment 2 answered",
                "all four comments answered",
            ],
        );
        let d = render(&[&t], "home", 280).unwrap();
        assert_eq!(
            d.text,
            "[Place digest: the other live threads of this place, as they stand now.\n\
             - from thread \u{ab}the tarnlib PR\u{bb}: goal: Finish the tarnlib PR this week. | \
             latest: all four comments answered | dated: merge by 2026-11-19]"
        );
        assert_eq!((d.shown, d.embargoed), (1, 0));
    }

    #[test]
    fn each_line_is_cut_to_its_cap() {
        let long = "x".repeat(900);
        let t = thread("pipeline", "the datatidy cleanup", &[&long]);
        let d = render(&[&t], "home", 120).unwrap();
        let line = d.text.lines().nth(1).unwrap().trim_end_matches(']');
        assert_eq!(line.chars().count(), 120);
        assert!(line.ends_with('\u{2026}'));
    }

    /// Five other threads, every field long, at the default cap: within 400
    /// tokens, as the tail's estimate counts them.
    #[test]
    fn five_threads_fit_in_400_tokens() {
        let long = "a long line of what happened ".repeat(20);
        let card = [long.as_str(), "due 2026-11-19 at the latest", long.as_str()];
        let five: Vec<ThreadInfo> = (0..5)
            .map(|i| {
                let mut t = thread(&format!("t{i}"), &format!("thread number {i}"), &card);
                t.goal = long.clone();
                t
            })
            .collect();
        let live: Vec<&ThreadInfo> = five.iter().collect();
        let cap = crate::config::ThreadsConfig::default().place_digest_line_chars;
        let d = render(&live, "home", cap).unwrap();
        assert_eq!(d.shown, 5);
        let tokens = (d.text.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN);
        assert!(tokens <= 400, "{tokens} tokens");
    }

    #[test]
    fn the_asking_thread_is_left_out_and_alone_there_is_no_digest() {
        let t = thread("pr-round", "the tarnlib PR", &[]);
        assert_eq!(render(&[&t], "pr-round", 280), None);
    }

    #[test]
    fn a_held_thread_is_only_counted() {
        let mut sealed = thread("sec-fix", "the ledgerweb fix", &["CVE draft 2026-11-02"]);
        sealed.sealed = Some(Seal {
            by: "the CLI".into(),
            at_ms: 1,
            reason: "an embargoed fix".into(),
        });
        let mut pending = thread(
            "ops",
            "the homesvc maintenance",
            &["window closes 2026-11-13"],
        );
        pending.seal_pending = Some("holds a password".into());
        let d = render(&[&sealed, &pending], "pr-round", 280).unwrap();
        assert_eq!(
            d.text,
            "[Place digest: the other live threads of this place, as they stand now.\n\
             2 embargoed threads (not shown).]"
        );
        assert_eq!((d.shown, d.embargoed), (0, 2));
    }
}
