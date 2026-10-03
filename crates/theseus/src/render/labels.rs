//! Labels' lines (M4 19a): `theseus labels`, a session's audience as its
//! compilation was made for it, what its prefix withheld, and each node's
//! label. Apart from `render.rs`, whose length the shape budget caps
//! (`scripts/long-files.txt`).

use theseus_protocol::{Audience, CompilationInfo, InPlay, Integrity, Label, NodeInfo, Readers};

/// A label in a few words, with its badge: `🔒 owner-only`, `👥 for
/// channel discord:7`, `🌐 public`, and `untrusted: http.fetch <url>` when
/// its text came from outside. A node from before labels: `— (before
/// labels: its own session's)`.
pub fn label_words(l: Option<&Label>) -> String {
    let Some(l) = l else {
        return "— (before labels: its own session's)".into();
    };
    let badge = match &l.readers {
        Readers::Owner => "🔒",
        Readers::Public => "🌐",
        Readers::Place(_) | Readers::People(_) => "👥",
    };
    let mut words = format!("{badge} {}", l.readers.describe());
    if l.integrity == Integrity::Untrusted {
        let from = l
            .source
            .as_ref()
            .map_or_else(String::new, |s| format!(": {}", s.what()));
        words.push_str(&format!(", untrusted{from}"));
    }
    words
}

/// The manifest's labels part, read from a compilation as stored.
struct Manifest {
    audience: Option<Audience>,
    readers: Option<Readers>,
    integrity: Option<InPlay>,
    withheld: Vec<theseus_protocol::Withheld>,
}

fn manifest(c: &CompilationInfo) -> Manifest {
    let m = &c.manifest;
    let field = |k: &str| m.get(k).cloned().unwrap_or_default();
    Manifest {
        audience: serde_json::from_value(field("audience")).ok(),
        readers: serde_json::from_value(field("readers")).ok(),
        integrity: serde_json::from_value(field("integrity")).ok(),
        withheld: serde_json::from_value(field("withheld")).unwrap_or_default(),
    }
}

/// `theseus labels SESSION`: who the session's current compilation was made
/// for, what the model may say to whom, what it left out, and each node.
pub fn labels_lines(
    session: &str,
    current: Option<&CompilationInfo>,
    nodes: &[NodeInfo],
) -> Vec<String> {
    let mut out = Vec::new();
    let m = current.map(manifest);
    match (current, &m) {
        (Some(c), Some(m)) => {
            let audience = m.audience.as_ref().map_or_else(
                || "not recorded (a compilation from before labels)".to_string(),
                Audience::describe,
            );
            out.push(format!(
                "session {session} · audience: {audience} · compilation {} ({})",
                c.compilation_id, c.trigger
            ));
            if let Some(r) = &m.readers {
                out.push(format!(
                    "  what the model says may be read: {}",
                    r.describe()
                ));
            }
            if let Some(i) = &m.integrity {
                out.push(format!(
                    "  integrity: {} untrusted node(s) in the prefix; {}",
                    i.untrusted,
                    if i.latched {
                        "the session holds external text"
                    } else {
                        "no hold on external text"
                    }
                ));
            }
            out.push(format!("  withheld from its prefix: {}", m.withheld.len()));
        }
        _ => out.push(format!(
            "session {session} · not compiled yet: its first turn decides its audience"
        )),
    }
    let withheld = |id: &str| {
        m.as_ref()
            .is_some_and(|m| m.withheld.iter().any(|w| w.node_id == id))
    };
    for n in nodes {
        let mark = if withheld(&n.node_id) {
            "  · withheld"
        } else {
            ""
        };
        out.push(format!(
            "  {} {:<17} {}{mark}",
            n.node_id,
            n.kind,
            label_words(n.label.as_ref())
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn node(id: &str, kind: &str, label: Option<Label>) -> NodeInfo {
        let mut n: NodeInfo = serde_json::from_value(serde_json::json!({"node_id": id,
            "kind": kind, "session_id": "ses_a", "position": 1, "at_unix_ms": 1}))
        .unwrap();
        n.label = label;
        n
    }

    #[test]
    fn a_session_shows_its_audience_and_each_nodes_label() {
        let current: CompilationInfo = serde_json::from_value(serde_json::json!({
            "compilation_id": "cmp_b", "session_id": "ses_a", "created_at_ms": 1,
            "trigger": "audience", "strategy": "transcript", "as_of": 9, "includes": 3,
            "manifest": {
                "audience": {"kind": "place", "place": "discord:7", "name": "lab", "viewers": 2},
                "readers": {"place": "discord:7"},
                "integrity": {"latched": false, "untrusted": 0},
                "withheld": [{"node_id": "trs_c", "reason": "owner-only"}]
            }
        }))
        .unwrap();
        let place = Label::trusted(Readers::Place("discord:7".into()));
        let lines = labels_lines(
            "ses_a",
            Some(&current),
            &[
                node("msg_a", "user_message", Some(place)),
                node("trs_c", "tool_result", Some(Label::trusted(Readers::Owner))),
                node("msg_old", "user_message", None),
                node(
                    "msg_d",
                    "user_message",
                    Some(Label::trusted(Readers::People(BTreeSet::from([
                        "discord:42".to_string(),
                    ])))),
                ),
            ],
        );
        assert_eq!(
            lines,
            [
                "session ses_a · audience: #lab (2 people) · compilation cmp_b (audience)",
                "  what the model says may be read: for channel discord:7",
                "  integrity: 0 untrusted node(s) in the prefix; no hold on external text",
                "  withheld from its prefix: 1",
                "  msg_a user_message      👥 for channel discord:7",
                "  trs_c tool_result       🔒 owner-only  · withheld",
                "  msg_old user_message      — (before labels: its own session's)",
                "  msg_d user_message      👥 for discord:42",
            ]
        );
        assert_eq!(
            labels_lines("ses_z", None, &[])[0],
            "session ses_z · not compiled yet: its first turn decides its audience"
        );
    }
}
