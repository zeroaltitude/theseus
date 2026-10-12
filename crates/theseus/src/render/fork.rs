//! The lines that show a fork (theseus-2qzx, R19): `theseus fork`'s result,
//! the inherited prefix in `theseus history`, and the forks in `theseus
//! reach`. Shared by the CLI and the terminal UI.

use theseus_protocol::{
    short_id as node_short, NodeInfo, NodeReachResult, ReachFork, SessionForkResult,
};

use super::{Line, Tag};

/// A session id as the mockup writes it: its prefix, `…`, its last four.
pub fn short_session(id: &str) -> String {
    let n = id.chars().count();
    let tail: String = id.chars().skip(n.saturating_sub(4)).collect();
    match id.split_once('_') {
        Some((p, _)) if !p.is_empty() => format!("{p}_…{tail}"),
        _ => format!("…{tail}"),
    }
}

/// `166201` as `166,201`.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn size(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        _ => format!("{:.1} KB", bytes as f64 / 1024.0),
    }
}

/// What a fork printed: where it forked and what it made, and what it did
/// not bring back.
pub fn forked_lines(r: &SessionForkResult) -> Vec<Line> {
    vec![
        Line::new(
            Tag::Plain,
            format!(
                "forked {} at {} (position {}) → {} \"{}\" in {} ms, {}",
                short_session(&r.source_session),
                node_short(&r.at_node),
                thousands(r.at_position),
                short_session(&r.session_id),
                r.title,
                r.ms,
                size(r.bytes)
            ),
        ),
        Line::new(Tag::Dim, format!("not restored: {}", r.not_restored)),
    ]
}

/// Where a history's inherited prefix begins and ends: the marks to print
/// before the node at each index (`theseus history` of a fork). A node whose
/// session is not the history's own was inherited from there.
pub fn history_marks(session: &str, nodes: &[NodeInfo]) -> Vec<(usize, Line)> {
    let mut out = Vec::new();
    let mut from: Option<&str> = None;
    for (i, n) in nodes.iter().enumerate() {
        let inherited = n.session_id != session;
        match (from, inherited) {
            (None, true) => {
                from = Some(&n.session_id);
                out.push((
                    i,
                    Line::new(
                        Tag::Dim,
                        format!(
                            "── inherited from {}, by reference (nothing of it is copied)",
                            short_session(&n.session_id)
                        ),
                    ),
                ));
            }
            (Some(_), false) => {
                from = None;
                out.push((
                    i,
                    Line::new(Tag::Dim, "── this session's own turns".to_string()),
                ));
            }
            _ => {}
        }
    }
    out
}

/// The forks of a node, as `theseus reach` prints them: a generation to a
/// line, as a copy's, with what held the node in the fork under it.
pub fn reach_fork_lines(r: &NodeReachResult) -> Vec<Line> {
    r.forks.iter().map(fork_line).collect()
}

fn fork_line(f: &ReachFork) -> Line {
    let held = if f.exposure.compilations.is_empty() && f.exposure.loops == 0 {
        "no context held it".to_string()
    } else {
        format!(
            "{} compilation{}, {} loop{}",
            f.exposure.compilations.len(),
            if f.exposure.compilations.len() == 1 {
                ""
            } else {
                "s"
            },
            f.exposure.loops,
            if f.exposure.loops == 1 { "" } else { "s" },
        )
    };
    Line::new(
        Tag::Plain,
        format!(
            "  generation {} · {} · forked from {} at position {} by reference (compilation {}): {}",
            f.generation,
            f.session_id,
            f.from,
            thousands(f.at_position),
            f.compilation_id,
            held
        ),
    )
}
