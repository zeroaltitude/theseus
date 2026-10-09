//! Recall's lines (M6 step 30a): `theseus memory search` and `theseus memory
//! recalled`, one manifest each: what the index offered, what the pack would
//! admit with each source's rank, and why each other candidate was dropped.

use theseus_protocol::memory::{
    MemoryConsolidateResult, MemoryHealth, MemoryRecallsResult, RecallActivation, RecallManifest,
    RecallOutcomes, RecallRetention,
};

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
    arm_lines(o, m);
    for a in &m.admitted {
        item_lines(o, a);
    }
    drop_lines(o, m);
    out
}

/// The lines between a recall's head and its items: why the rank went
/// without retention's projection (32a), Jev's live rerank (32d), and
/// what spreading activation did (32b).
fn arm_lines(o: &mut Vec<Line>, m: &RecallManifest) {
    if let Some(line) = unranked_line(m) {
        push(o, Tag::Warn, &line);
    }
    if let Some(r) = &m.rerank {
        push(
            o,
            if r.applied { Tag::Plain } else { Tag::Dim },
            &rerank_line(r),
        );
    }
    if let Some(a) = &m.activation {
        let (tag, line) = activation_line(a, m.admitted.len());
        push(o, tag, &line);
    }
}

/// What spreading activation did (32b): its share of what was admitted,
/// or why it did not run.
fn activation_line(a: &RecallActivation, admitted: usize) -> (Tag, String) {
    if a.outcome != "ran" {
        let why = a
            .why
            .as_deref()
            .map(|w| format!(": {w}"))
            .unwrap_or_default();
        return (
            Tag::Dim,
            format!("  activation did not run ({}){why}", a.outcome),
        );
    }
    (
        Tag::Plain,
        format!(
            "  activation ranked {} of the {admitted} admitted, {} found by it alone · from {} \
             reached {} ({} of the index's hits, {} added) · {:.1} ms over {} nodes, {} edges",
            a.admitted,
            a.admitted_added,
            plural(a.seeds, "seed", "seeds"),
            a.reached,
            a.boosted,
            a.added,
            a.took_ms,
            a.nodes,
            a.edges,
        ),
    )
}

/// Health's memory line: the mode and arm, the retention projection (32a)
/// once the arm reads it or a search asked for it, and the adjacency
/// projection (32b) once an arm reads it; then the last turns' recalls by
/// outcome (theseus-w9qv).
pub(super) fn push_health(o: &mut Vec<Line>, h: Option<&MemoryHealth>, now_ms: u64) {
    let Some(h) = h else { return };
    push_memory(o, h);
    if let Some(r) = &h.recalls {
        let (tag, line) = recalls_line(r, now_ms);
        push(o, tag, &line);
    }
}

/// `recall: the last 50: 41 ok, 7 words only, 2 deadline, 0 error · every
/// source last answered 3 min ago` (theseus-w9qv). A warning while any recall
/// found nothing in time, or none was answered.
fn recalls_line(r: &RecallOutcomes, now_ms: u64) -> (Tag, String) {
    let mut line = format!(
        "recall: the last {}: {} ok, {} words only, {} deadline, {} error",
        r.turns, r.ok, r.words_only, r.deadline, r.error
    );
    match r.last_full_ms {
        Some(at) => {
            let secs = now_ms.saturating_sub(at) / 1000;
            let ago = match secs {
                0..120 => format!("{secs} s ago"),
                120..7_200 => format!("{} min ago", secs / 60),
                _ => format!("{} h ago", secs / 3_600),
            };
            line.push_str(&format!(" · every source last answered {ago}"));
        }
        None => line.push_str(" · no answer from every source since the start"),
    }
    let tag = if r.deadline + r.error > 0 || r.ok == 0 {
        Tag::Warn
    } else {
        Tag::Plain
    };
    (tag, line)
}

fn push_memory(o: &mut Vec<Line>, h: &MemoryHealth) {
    let retention = h.arm == "+retention" || !matches!(h.retention.as_str(), "" | "unbuilt");
    if h.mode == "off" && h.adjacency.is_none() && !retention {
        return;
    }
    let mut line = format!("memory: {} · arm {}", h.mode, h.arm);
    let mut tag = Tag::Plain;
    if retention {
        line.push_str(&format!(" · retention {}", h.retention));
        if h.retention != "unbuilt" {
            line.push_str(&format!(
                " · {} ({})",
                plural(h.nodes, "node", "nodes"),
                plural(h.events, "event", "events")
            ));
        }
        if let Some(why) = &h.why {
            tag = Tag::Warn;
            line.push_str(&format!(" · {why}"));
        }
    }
    if let Some(a) = &h.adjacency {
        match a.state.as_str() {
            "built" => line.push_str(&format!(
                " · adjacency {} nodes, {} edges, {} entities, {:.1} MB, through @{}{}",
                a.nodes,
                a.edges,
                a.entities,
                a.bytes as f64 / 1_048_576.0,
                a.through,
                if a.unmapped > 0 {
                    format!(" ({} edges of routes it does not know)", a.unmapped)
                } else {
                    String::new()
                }
            )),
            "failed" => {
                tag = Tag::Warn;
                line.push_str(&format!(
                    " · adjacency failed: {}",
                    a.why.as_deref().unwrap_or("unknown")
                ));
            }
            state => line.push_str(&format!(" · adjacency {state}")),
        }
    }
    push(o, tag, &line);
}

/// Whether Jev's live rerank ordered the pack (32d), or why recall's own
/// order stood.
fn rerank_line(r: &theseus_protocol::memory::RecallRerank) -> String {
    let jdg = r
        .judgment
        .as_deref()
        .map(|j| format!(" ({j})"))
        .unwrap_or_default();
    if r.applied {
        return format!(
            "  in Jev's order{jdg}: rerank.v1 answered after {:.1} ms of the {} ms wait",
            r.waited_ms, r.wait_ms
        );
    }
    let why = match r.why.as_deref() {
        Some("timeout") => format!(
            "Jev had not answered within the {} ms wait; its answer, if it comes, is recorded late",
            r.wait_ms
        ),
        Some("breaker_open") => "rerank's breaker is open".into(),
        Some("budget") => "the judge's day budget is spent".into(),
        Some("nothing_eligible") => "no note passed the filters".into(),
        Some(other) => format!("the rerank fell back: {other}"),
        None => "no answer".into(),
    };
    format!("  in recall's own order{jdg}: {why}")
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
    if let Some(r) = &a.retention {
        push(o, Tag::Dim, &format!("      {}", retention_words(r)));
    }
}

/// Under `+retention` (32a), when the rank went without the projection:
/// why.
fn unranked_line(m: &RecallManifest) -> Option<String> {
    let state = m.retention.as_deref().filter(|s| *s != "ready")?;
    Some(format!(
        "  ranked by the fused score alone: the retention projection is {state}"
    ))
}

/// An item's retention under `+retention` (32a).
fn retention_words(r: &RecallRetention) -> String {
    format!(
        "retention: R {:.3} · stability {:.2} d · difficulty {:.2} · last review {}",
        r.retrievability,
        r.stability,
        r.difficulty,
        super::time::fmt_stamp(r.last_review_ms)
    )
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
        // Under `+retention`, what each shown drop's node holds (a label's
        // effect included).
        for d in m.dropped.iter().filter(|d| &d.reason == reason).take(6) {
            if let Some(r) = &d.retention {
                push(
                    o,
                    Tag::Dim,
                    &format!("      {}#{}: {}", d.node_id, d.chunk, retention_words(r)),
                );
            }
        }
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

/// `theseus memory consolidate`: each cluster's outcome, its sources and
/// text, what was skipped and why, and the day's spend against its limit.
pub fn consolidated_lines(r: &MemoryConsolidateResult) -> Vec<Line> {
    let mut out = Vec::new();
    let o = &mut out;
    let head = if r.dry_run {
        "consolidation, a dry run (nothing written)"
    } else {
        "consolidation"
    };
    push(
        o,
        Tag::Plain,
        &format!(
            "{head}: {} from {} · ${:.4} of ${:.2} spent today",
            plural(r.clusters.len() as u64, "cluster", "clusters"),
            plural(r.recalls, "recall", "recalls"),
            r.spent_today_usd,
            r.limit_usd
        ),
    );
    for c in &r.clusters {
        let id = c.synthesis_id.as_deref().unwrap_or(&c.cluster);
        push(
            o,
            Tag::Plain,
            &format!(
                "  {id} · {} · {} · {} turns · {} · ${:.4}",
                c.outcome,
                plural(c.sources.len() as u64, "source", "sources"),
                c.turns,
                c.profile,
                c.cost_usd
            ),
        );
        push(
            o,
            Tag::Dim,
            &format!("    sources: {}", c.sources.join(", ")),
        );
        if let Some(t) = &c.text {
            push(o, Tag::Plain, &format!("    {t}"));
        }
        if let Some(w) = &c.why {
            push(o, Tag::Warn, &format!("    {w}"));
        }
    }
    for (why, n) in &r.skipped {
        push(o, Tag::Dim, &format!("  skipped {n} for {why}"));
    }
    if let Some(s) = &r.stopped {
        push(o, Tag::Warn, &format!("  stopped: {s}"));
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
        assert!(!all.contains("order"), "no rerank, no line: {all}");
        let reranked = |applied: bool, why: Option<&str>| {
            let m = RecallManifest {
                rerank: Some(theseus_protocol::memory::RecallRerank {
                    judgment: Some("jdg_h1".into()),
                    applied,
                    why: why.map(str::to_string),
                    waited_ms: 115.0,
                    wait_ms: 200,
                }),
                ..m.clone()
            };
            let lines: Vec<String> = recall_lines(&m).into_iter().map(|l| l.text).collect();
            lines.join("\n")
        };
        let all = reranked(true, None);
        assert!(
            all.contains(
                "in Jev's order (jdg_h1): rerank.v1 answered after 115.0 ms of the 200 ms wait"
            ),
            "{all}"
        );
        // Activation's share (32b), and why it did not run.
        let act = |outcome: &str| {
            let m = RecallManifest {
                activation: Some(RecallActivation {
                    outcome: outcome.into(),
                    why: (outcome != "ran").then(|| "the projection is built after serving".into()),
                    seeds: 4,
                    reached: 12,
                    boosted: 2,
                    added: 3,
                    admitted: 2,
                    admitted_added: 1,
                    nodes: 900,
                    edges: 2400,
                    took_ms: 1.25,
                    ..RecallActivation::default()
                }),
                ..m.clone()
            };
            let lines: Vec<String> = recall_lines(&m).into_iter().map(|l| l.text).collect();
            lines.join("\n")
        };
        let all = act("ran");
        assert!(
            all.contains(
                "activation ranked 2 of the 1 admitted, 1 found by it alone · from 4 seeds reached \
                 12 (2 of the index's hits, 3 added) · 1.2 ms over 900 nodes, 2400 edges"
            ),
            "{all}"
        );
        let all = act("building");
        assert!(
            all.contains(
                "activation did not run (building): the projection is built after serving"
            ),
            "{all}"
        );
        let all = reranked(false, Some("timeout"));
        assert!(
            all.contains(
                "in recall's own order (jdg_h1): Jev had not answered within the 200 ms wait"
            ),
            "{all}"
        );
    }

    #[test]
    fn health_names_the_adjacency_projection() {
        let line = |h: &MemoryHealth| {
            let mut o = Vec::new();
            push_health(&mut o, Some(h), 0);
            o.into_iter().map(|l| l.text).collect::<Vec<_>>().join("\n")
        };
        let mut h = MemoryHealth {
            mode: "live".into(),
            arm: "+activation".into(),
            adjacency: Some(theseus_protocol::memory::AdjacencyHealth {
                state: "built".into(),
                nodes: 1200,
                edges: 3400,
                entities: 80,
                bytes: 3 * 1_048_576,
                through: 9876,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            line(&h),
            "memory: live · arm +activation · adjacency 1200 nodes, 3400 edges, 80 entities, \
             3.0 MB, through @9876"
        );
        h.adjacency = Some(theseus_protocol::memory::AdjacencyHealth {
            state: "building".into(),
            ..Default::default()
        });
        assert_eq!(
            line(&h),
            "memory: live · arm +activation · adjacency building"
        );
        let off = MemoryHealth {
            mode: "off".into(),
            arm: "baseline".into(),
            adjacency: None,
            ..Default::default()
        };
        assert_eq!(line(&off), "");
        // Retention's clause (32a), beside it once both are merged.
        let retention = MemoryHealth {
            mode: "live".into(),
            arm: "+retention".into(),
            retention: "ready".into(),
            nodes: 2,
            events: 5,
            ..Default::default()
        };
        assert_eq!(
            line(&retention),
            "memory: live · arm +retention · retention ready · 2 nodes (5 events)"
        );
    }

    /// Health's recall line (theseus-w9qv): the last turns' recalls by
    /// outcome, and when the whole answer last came; a warning while a recall
    /// found nothing in time.
    #[test]
    fn health_counts_recalls_by_outcome() {
        let r = RecallOutcomes {
            window: 50,
            turns: 50,
            ok: 41,
            words_only: 9,
            last_full_ms: Some(1_000_000),
            ..Default::default()
        };
        let h = MemoryHealth {
            mode: "live".into(),
            arm: "baseline".into(),
            recalls: Some(r.clone()),
            ..Default::default()
        };
        let mut o = Vec::new();
        push_health(&mut o, Some(&h), 1_000_000 + 180_000);
        let text: Vec<&str> = o.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            text,
            [
                "memory: live · arm baseline",
                "recall: the last 50: 41 ok, 9 words only, 0 deadline, 0 error · every source \
                 last answered 3 min ago"
            ]
        );
        assert!(matches!(o[1].tag, Tag::Plain));
        let stuck = RecallOutcomes {
            ok: 0,
            words_only: 0,
            deadline: 50,
            last_full_ms: None,
            ..r
        };
        let (tag, line) = recalls_line(&stuck, 0);
        assert!(matches!(tag, Tag::Warn));
        assert_eq!(
            line,
            "recall: the last 50: 0 ok, 0 words only, 50 deadline, 0 error · no answer from \
             every source since the start"
        );
    }
}
