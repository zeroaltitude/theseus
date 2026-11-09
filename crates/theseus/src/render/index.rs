//! The index's lines (roadmap row 51): a tender as the children line and
//! `theseus memory index status` say it, health's `index:` line, `theseus index
//! status`, and `theseus memory index search`'s hits. Apart from `render.rs`, whose
//! length the shape budget caps (`scripts/long-files.txt`).

use super::{fmt_bytes, plural, push, Line, Tag};

/// `index tender running (pid 4242, 1 restart)`, `index tender in backoff
/// (exit 1, next wait 2 s, 3 restarts)`: a tender, as the children line and
/// `theseus memory index status` say it (roadmap row 51).
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

/// Health's `index:` line, and `theseus memory index status`'s first (roadmap row
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
        // The tender's own reason is the whole explanation: "expected, after
        // an upgrade or a request" is the general case, and said of a rebuild
        // whose cause was an index that did not open, or a WAL that was
        // replaced, it would be false. Only a backfill that gave none says it.
        match &b.why {
            Some(why) => line.push_str(&format!(" · why: {why}")),
            None => line.push_str(
                " (expected: the index is rebuilt from the WAL after an upgrade or a request)",
            ),
        }
    }
    // The vectors' backlog and the tender's memory (theseus-agqn).
    if let Some(v) = s.vectors.as_ref().filter(|v| v.model != "off") {
        line.push_str(&format!(
            " · {} waiting for a vector",
            plural(v.pending, "text", "texts")
        ));
    }
    if let Some(g) = s.vectors.as_ref().and_then(|v| v.gate.as_ref()) {
        line.push_str(&gate_words(g));
    }
    if s.rss_bytes > 0 {
        line.push_str(&format!(" · tender rss {}", fmt_bytes(s.rss_bytes)));
    }
    line.push_str(&why);
    line
}

/// A recall's query gate (theseus-d7fy): the pool's size, whether pressure
/// holds it shut now (one thread, one query at a time), and how many queries
/// have waited for it and how many left without their turn.
fn gate_words(g: &theseus_protocol::index::IndexQueryGate) -> String {
    format!(
        " · query gate {} ({}), {} waited, {} expired",
        if g.shut { "shut" } else { "open" },
        if g.shut {
            "1 thread, one at a time".to_string()
        } else {
            plural(u64::from(g.threads), "thread", "threads")
        },
        g.waited,
        g.expired
    )
}

/// `theseus memory index status` (roadmap row 51): the index, its tender, its
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
    // A kept file's read text, indexed beside its node (theseus-lv2u).
    if s.files_indexed + s.files_repeated + s.files_cut + s.files_unread > 0 {
        push(
            o,
            if s.files_unread > 0 {
                Tag::Warn
            } else {
                Tag::Plain
            },
            &format!(
                "files: {} indexed, {} already held, {} cut at the cap, {} unread",
                s.files_indexed, s.files_repeated, s.files_cut, s.files_unread
            ),
        );
    }
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

/// `theseus memory index search` (roadmap row 51): a line on the search, then each
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
            entities: Vec::new(),
            sources: BTreeMap::new(),
            fused: 0.5,
        }
    }

    /// A rebuild's reason is on the health line, in the tender's own words,
    /// and a first build says nothing more.
    #[test]
    fn a_rebuild_says_why_on_the_index_line() {
        use theseus_protocol::index::{IndexBackfill, IndexHealth, IndexStatus};
        let mut s = IndexStatus {
            state: "backfilling".into(),
            ..IndexStatus::default()
        };
        s.backfill = Some(IndexBackfill {
            done_bytes: 0,
            total_bytes: 4096,
            why: Some(
                "this build's index schema 3 -> 4 (a rebuild window of a minute or so, recall read-degraded meanwhile)"
                    .into(),
            ),
        });
        let h = IndexHealth {
            state: "backfilling".into(),
            status: Some(s.clone()),
            ..IndexHealth::default()
        };
        let line = index_line(&h);
        assert!(
            line.contains(
                "why: this build's index schema 3 -> 4 (a rebuild window of a minute or so, recall read-degraded meanwhile)"
            ),
            "{line}"
        );
        s.backfill.as_mut().unwrap().why = None;
        let h = IndexHealth {
            status: Some(s),
            ..h
        };
        assert!(!index_line(&h).contains("rebuilding"));
    }

    /// A rebuild whose reason is known says that reason, and does not also call
    /// itself "expected, after an upgrade or a request" (theseus-la5oc): the
    /// reason is the whole explanation, and for an index that did not open or a
    /// WAL that was replaced the general sentence would be false. One that
    /// gave no reason is the case `a_backfill_is_said_to_be_expected` keeps.
    #[test]
    fn a_rebuild_with_a_reason_does_not_also_call_itself_expected() {
        use theseus_protocol::index::{IndexBackfill, IndexHealth, IndexStatus};
        for why in [
            "this build's index schema 3 -> 4 (a rebuild window of a minute or so, recall read-degraded meanwhile)",
            "its index is missing (its cursor is intact, but there is nothing to resume)",
            "its index did not open: a segment is corrupt",
            "the WAL was rewound or replaced: the cursor's segment is gone",
        ] {
            let h = IndexHealth {
                state: "backfilling".into(),
                status: Some(IndexStatus {
                    backfill: Some(IndexBackfill {
                        done_bytes: 10,
                        total_bytes: 100,
                        why: Some(why.into()),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let line = index_line(&h);
            assert!(
                line.contains("backfill 10 B of 100 B") && line.contains(&format!("why: {why}")),
                "{line}"
            );
            assert!(!line.contains("expected"), "{line}");
        }
    }

    /// A rebuild after an upgrade or a request is expected, and health says so
    /// when the tender gave no reason (theseus-ectzg): a backfill reads as a
    /// window that ends, not a fault.
    #[test]
    fn a_backfill_is_said_to_be_expected() {
        use theseus_protocol::index::{IndexBackfill, IndexHealth, IndexStatus};
        let mut h = IndexHealth {
            state: "backfilling".into(),
            status: Some(IndexStatus {
                backfill: Some(IndexBackfill {
                    done_bytes: 10,
                    total_bytes: 100,
                    why: None,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let line = index_line(&h);
        assert!(line.contains("backfill 10 B of 100 B"), "{line}");
        assert!(line.contains("expected"), "{line}");
        h.status.as_mut().unwrap().backfill = None;
        assert!(!index_line(&h).contains("expected"));
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

    /// Health's line names the query gate (theseus-d7fy): the pool's size
    /// while it is open, one thread and one at a time while it is shut, and
    /// the queries that waited and expired.
    #[test]
    fn the_index_line_names_the_query_gate() {
        use theseus_protocol::index::IndexQueryGate;
        let g = |shut| IndexQueryGate {
            threads: 4,
            shut,
            waited: 7,
            expired: 2,
            one_thread: 9,
        };
        assert_eq!(
            gate_words(&g(false)),
            " · query gate open (4 threads), 7 waited, 2 expired"
        );
        assert_eq!(
            gate_words(&g(true)),
            " · query gate shut (1 thread, one at a time), 7 waited, 2 expired"
        );
    }
}
