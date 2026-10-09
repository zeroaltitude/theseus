//! The index's lines (roadmap row 51): a tender as the children line and
//! `theseus index status` say it, health's `index:` line, `theseus index
//! status`, and `theseus index search`'s hits. Apart from `render.rs`, whose
//! length the shape budget caps (`scripts/long-files.txt`).

use super::{fmt_bytes, plural, push, Line, Tag};

/// `index tender running (pid 4242, 1 restart)`, `index tender in backoff
/// (exit 1, next wait 2 s, 3 restarts)`: a tender, as the children line and
/// `theseus index status` say it (roadmap row 51).
pub fn tender_words(t: &theseus_protocol::TenderStatus) -> String {
    let mut parts = Vec::new();
    if let Some(pid) = t.pid {
        parts.push(format!("pid {pid}"));
    }
    if t.adopted {
        parts.push("taken over after a restart".into());
    }
    if t.state == "backoff" {
        if let Some(e) = &t.last_exit {
            parts.push(e.clone());
        }
        parts.push(format!("next wait {} s", t.backoff_ms / 1000));
    }
    parts.push(plural(t.restarts, "restart", "restarts"));
    let state = match t.state.as_str() {
        "backoff" => "in backoff",
        s => s,
    };
    format!("{} tender {state} ({})", t.name, parts.join(", "))
}

/// Health's `index:` line, and `theseus index status`'s first (roadmap row
/// 51): its state, what answers, what it holds, and how far behind the WAL it
/// is; or why there is none.
pub fn index_line(h: &theseus_protocol::index::IndexHealth) -> String {
    let why = h
        .why
        .as_deref()
        .map(|w| format!(" · {w}"))
        .unwrap_or_default();
    let Some(s) = &h.status else {
        return format!("index: {}{why}", h.state);
    };
    let mut line = format!(
        "index: {} · {} · {} in {} · through position {} · {} behind",
        h.state,
        s.mode,
        plural(s.nodes, "node", "nodes"),
        plural(s.documents, "chunk", "chunks"),
        s.position,
        fmt_bytes(s.lag.bytes)
    );
    if s.lag.bytes > 0 && s.lag.ms > 0 {
        line.push_str(&format!(" for {} ms", s.lag.ms));
    }
    if let Some(b) = &s.backfill {
        line.push_str(&format!(
            " · backfill {} of {}",
            fmt_bytes(b.done_bytes),
            fmt_bytes(b.total_bytes)
        ));
    }
    line.push_str(&why);
    line
}

/// `theseus index status` (roadmap row 51): the index, its tender, its
/// cursor and counts, and its vectors.
pub fn index_status_lines(h: &theseus_protocol::index::IndexHealth) -> Vec<Line> {
    let mut out = Vec::new();
    let o = &mut out;
    let tag = match h.state.as_str() {
        "down" | "stalled" => Tag::Bad,
        "ready" | "off" => Tag::Plain,
        _ => Tag::Warn,
    };
    push(o, tag, &index_line(h));
    if let Some(t) = &h.tender {
        let mut line = format!("tender: {}", tender_words(t));
        if let Some(b) = &t.binary {
            line.push_str(&format!(" · {b}"));
        }
        if t.state != "running" && t.state != "backoff" {
            if let Some(w) = &t.why {
                line.push_str(&format!(" · {w}"));
            }
        }
        push(o, Tag::Plain, &line);
    }
    let Some(s) = &h.status else {
        return out;
    };
    push(
        o,
        Tag::Plain,
        &format!(
            "cursor: segment {} offset {} · {} · {} read · {} indexed, {} skipped, {} undecodable · {} · rss {}",
            s.segment,
            s.offset,
            plural(s.commits, "commit", "commits"),
            plural(s.records_read, "record", "records"),
            plural(s.nodes_indexed, "node", "nodes"),
            s.nodes_skipped,
            s.undecodable,
            plural(s.rebuilds, "rebuild", "rebuilds"),
            fmt_bytes(s.rss_bytes)
        ),
    );
    if let Some(v) = &s.vectors {
        let mut line = format!(
            "vectors: {} · {} of {} chunks · {} pending",
            v.model, v.vectors, v.chunks, v.pending
        );
        if let Some(st) = &v.stamp {
            line.push_str(&format!(" · {}", st.model));
        }
        if let Some(r) = &v.reembed {
            line.push_str(&format!(" · re-embedding {} of {}", r.done, r.total));
        }
        push(o, Tag::Plain, &line);
        if let Some(e) = &v.last_error {
            push(o, Tag::Bad, &format!("vectors' last error: {e}"));
        }
    }
    if let Some(w) = &s.waiting {
        push(o, Tag::Dim, &format!("waiting: {w}"));
    }
    if let Some(e) = &s.last_error {
        push(o, Tag::Bad, &format!("last error: {e}"));
    }
    out
}

/// `theseus index search` (roadmap row 51): a line on the search, then each
/// hit, its node and where it is, its sources' ranks, and its text, cut at
/// 160 characters on one line.
pub fn index_hits_lines(q: &str, r: &theseus_protocol::index::IndexQueryResult) -> Vec<Line> {
    let mut out = Vec::new();
    let o = &mut out;
    let t = &r.timings;
    push(
        o,
        Tag::Dim,
        &format!(
            "{} for {q:?} · through position {} · {} behind · {:.1} ms (bm25 {:.1}, entity {:.1}, embed {:.1}, vector {:.1})",
            plural(r.hits.len() as u64, "hit", "hits"),
            r.indexed_through,
            fmt_bytes(r.lag.bytes),
            t.total_ms,
            t.bm25_ms,
            t.entity_ms,
            t.embed_ms,
            t.vector_ms
        ),
    );
    for (source, why) in &r.skipped {
        push(o, Tag::Warn, &format!("{source} skipped: {why}"));
    }
    for (i, h) in r.hits.iter().enumerate() {
        let sources: Vec<String> = h
            .sources
            .iter()
            .map(|(s, sr)| format!("{s} #{}", sr.rank))
            .collect();
        let mut head = format!(
            "{:>2}. {} · {} · {} @{} · {}#{} · {} · fused {:.4}",
            i + 1,
            fmt_date(h.time_ms),
            h.kind,
            h.session_id,
            h.position,
            h.node_id,
            h.chunk,
            sources.join(", "),
            h.fused
        );
        if let Some(tool) = &h.tool {
            head.push_str(&format!(" · {tool}"));
        }
        if h.external {
            head.push_str(" · external");
        }
        if !h.entities_matched.is_empty() {
            head.push_str(&format!(" · entities {}", h.entities_matched.join(" ")));
        }
        push(o, Tag::Plain, &head);
        let one_line = h.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut text: String = one_line.chars().take(160).collect();
        if one_line.chars().count() > 160 {
            text.push('…');
        }
        o.push(Line::new(Tag::Plain, format!("    {text}")));
    }
    out
}

/// `2026-03-14 02:12`: a hit's time (an imported message's own, a native
/// node's creation), on this machine's clock as the CLI's other times are
/// (theseus-w9qv, theseus-0n1v).
pub fn fmt_date(unix_ms: u64) -> String {
    super::time::fmt_date(unix_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use theseus_protocol::index::{IndexHit, IndexLag, IndexQueryResult};

    fn hit(time_ms: u64) -> IndexHit {
        IndexHit {
            node_id: "nod_tide".into(),
            chunk: 0,
            session_id: "ses_ep00".into(),
            position: 42,
            kind: "imported".into(),
            origin: "import".into(),
            author: None,
            place: None,
            tool: None,
            time_ms,
            external: false,
            text: "the harbour's tide tables".into(),
            entities_matched: Vec::new(),
            sources: BTreeMap::new(),
            fused: 0.5,
        }
    }

    /// A hit's line names its date (theseus-w9qv): the index gave it, and the
    /// search's reader needs it to tell March from September.
    #[test]
    fn each_hit_says_its_date() {
        // 2026-03-14 09:12:30 UTC, written at the tests' UTC−7.
        let march = 1_773_479_550_000;
        assert_eq!(fmt_date(march), "2026-03-14 02:12");
        assert_eq!(fmt_date(0), "1969-12-31 17:00");
        // A leap day, and the last minute of a year.
        assert_eq!(fmt_date(1_709_164_800_000), "2024-02-28 17:00");
        assert_eq!(fmt_date(1_798_761_599_000), "2026-12-31 16:59");
        let r = IndexQueryResult {
            hits: vec![hit(march)],
            indexed_through: 42,
            lag: IndexLag { bytes: 0, ms: 0 },
            timings: Default::default(),
            skipped: BTreeMap::new(),
            weights: BTreeMap::new(),
        };
        let lines = index_hits_lines("tide", &r);
        let head = &lines
            .iter()
            .find(|l| l.text.contains("nod_tide"))
            .unwrap()
            .text;
        assert!(
            head.starts_with(" 1. 2026-03-14 02:12 · imported · ses_ep00 @42"),
            "{head}"
        );
    }
}
