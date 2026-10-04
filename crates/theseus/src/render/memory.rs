//! Recall's lines (M6 step 30a): `theseus memory search` and `theseus memory
//! recalled`, one manifest each: what the index offered, what the pack would
//! admit with each source's rank, and why each other candidate was dropped.

use theseus_protocol::memory::{MemoryRecallsResult, RecallManifest};

use super::{plural, push, Line, Tag};

/// The most of an item's text a line shows.
const TEXT_CHARS: usize = 200;

/// One recall: its head, its admitted items with their text when it has it,
/// and its drops by reason with the dropped nodes.
pub fn recall_lines(m: &RecallManifest) -> Vec<Line> {
    let mut out = Vec::new();
    let o = &mut out;
    let mut head = format!(
        "{} · {} · {} · {} · {}",
        m.recall_id, m.mode, m.science, m.place, m.outcome
    );
    if let Some(arm) = &m.arm {
        head.push_str(&format!(" · arm {arm}"));
    }
    if let Some(turn) = &m.turn_id {
        head.push_str(&format!(" · turn {turn}"));
    }
    push(o, Tag::Plain, &head);
    match m.outcome.as_str() {
        "deadline" => {
            push(
                o,
                Tag::Warn,
                &format!(
                    "  the index did not answer within {} ms; the turn went on without recall",
                    m.timings.deadline_ms
                ),
            );
            return out;
        }
        "paused" => {
            push(
                o,
                Tag::Warn,
                &format!(
                    "  paused until the next recompile: {}",
                    m.why.as_deref().unwrap_or("the session's recall cap")
                ),
            );
        }
        "unavailable" => {
            push(
                o,
                Tag::Warn,
                &format!(
                    "  no index answered: {}",
                    m.why.as_deref().unwrap_or("down")
                ),
            );
            return out;
        }
        _ => {}
    }
    let sources: Vec<String> = m.sources.iter().map(|(s, n)| format!("{s} {n}")).collect();
    push(
        o,
        Tag::Dim,
        &format!(
            "  {} ({}) · as of {} · index through {} · {:.1} ms (index {:.1}) · query {} chars",
            plural(m.candidates, "candidate", "candidates"),
            if sources.is_empty() {
                "none".to_string()
            } else {
                sources.join(", ")
            },
            m.as_of.map_or("now".into(), |p| format!("@{p}")),
            m.indexed_through.map_or("?".into(), |p| format!("@{p}")),
            m.timings.total_ms,
            m.timings.index_ms,
            m.query_chars,
        ),
    );
    for (source, why) in &m.skipped {
        push(o, Tag::Warn, &format!("  {source} skipped: {why}"));
    }
    push(
        o,
        Tag::Plain,
        &format!(
            "  {} {} · {} of {} tokens",
            match m.mode.as_str() {
                "canary" | "live" => "admitted",
                _ => "would admit",
            },
            m.admitted.len(),
            m.used_tokens,
            m.budget_tokens
        ),
    );
    for a in &m.admitted {
        item_lines(o, a);
    }
    drop_lines(o, m);
    out
}

/// An admitted item: its rank, where it is, its scores, and its text.
fn item_lines(o: &mut Vec<Line>, a: &theseus_protocol::memory::RecallItem) {
    let ranks: Vec<String> = a
        .sources
        .iter()
        .map(|(s, r)| format!("{s} #{}", r.rank))
        .collect();
    push(
        o,
        Tag::Plain,
        &format!(
            "  {:>2}. {} · {} @{} · {}#{} · fused {:.4}{} · {} tokens",
            a.rank,
            a.kind,
            a.session_id,
            a.position,
            a.node_id,
            a.chunk,
            a.fused,
            if ranks.is_empty() {
                String::new()
            } else {
                format!(" ({})", ranks.join(", "))
            },
            a.tokens,
        ),
    );
    if let Some(text) = &a.text {
        let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut t: String = one_line.chars().take(TEXT_CHARS).collect();
        if one_line.chars().count() > TEXT_CHARS {
            t.push('…');
        }
        o.push(Line::new(Tag::Plain, format!("      {t}")));
    }
}

/// The drops by reason, each with the first of its nodes.
fn drop_lines(o: &mut Vec<Line>, m: &RecallManifest) {
    for (reason, n) in &m.drops {
        let nodes: Vec<String> = m
            .dropped
            .iter()
            .filter(|d| &d.reason == reason)
            .take(6)
            .map(|d| format!("{}#{}", d.node_id, d.chunk))
            .collect();
        let more = if *n as usize > nodes.len() {
            ", …"
        } else {
            ""
        };
        push(
            o,
            Tag::Dim,
            &format!("  dropped {n} for {reason}: {}{more}", nodes.join(", ")),
        );
    }
}

/// `theseus memory recalled <session>`: each recall, oldest first, and how
/// many the session has in all.
pub fn recalls_lines(r: &MemoryRecallsResult) -> Vec<Line> {
    let mut out = Vec::new();
    push(
        &mut out,
        Tag::Dim,
        &format!(
            "{} of {} in session {} (newest last)",
            plural(r.recalls.len() as u64, "recall", "recalls"),
            r.total,
            r.session_id
        ),
    );
    if r.recalls.is_empty() {
        push(
            &mut out,
            Tag::Dim,
            "none yet: recall runs on a turn's first loop with `[memory] mode = \"shadow\"`",
        );
    }
    for m in &r.recalls {
        out.extend(recall_lines(m));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use theseus_protocol::memory::{RecallDrop, RecallItem};

    #[test]
    fn a_recall_says_what_it_would_admit_and_why_it_dropped_the_rest() {
        let m = RecallManifest {
            recall_id: "rcl_a1".into(),
            mode: "shadow".into(),
            science: "baseline@00".into(),
            outcome: "ran".into(),
            place: "private".into(),
            candidates: 3,
            sources: BTreeMap::from([("bm25".into(), 3)]),
            admitted: vec![RecallItem {
                node_id: "msg_1".into(),
                session_id: "ses_b".into(),
                kind: "user_message".into(),
                rank: 1,
                tokens: 9,
                text: Some("the heron nests by the weir".into()),
                ..RecallItem::default()
            }],
            dropped: vec![RecallDrop {
                node_id: "msg_2".into(),
                reason: "place".into(),
                ..RecallDrop::default()
            }],
            drops: BTreeMap::from([("place".into(), 1)]),
            budget_tokens: 1500,
            used_tokens: 9,
            ..RecallManifest::default()
        };
        let text: Vec<String> = recall_lines(&m).into_iter().map(|l| l.text).collect();
        let all = text.join("\n");
        assert!(all.contains("would admit 1 · 9 of 1500 tokens"), "{all}");
        assert!(all.contains("the heron nests by the weir"), "{all}");
        assert!(all.contains("dropped 1 for place: msg_2#0"), "{all}");
    }
}
