//! The ontology's lines (theseus-8kk.1): `theseus ontology kinds`,
//! `categories`, a session's memberships, and Jev's proposals (28b).

use theseus_protocol::{
    OntologyCategory, OntologyKind, OntologyMembership, OntologyProposalsResult,
};

/// The kinds table, a row per kind, by precedence.
pub fn ontology_kinds_lines(kinds: &[OntologyKind]) -> Vec<String> {
    let mut out = vec![format!(
        "{:<10} {:<11} {:<10} {:>4} {:>4}  {:<11} {:<8} assigned by",
        "kind", "basis", "parent", "max", "prec", "rule", "version"
    )];
    for k in kinds {
        out.push(format!(
            "{:<10} {:<11} {:<10} {:>4} {:>4}  {:<11} {:<8} {}",
            k.name,
            k.basis,
            k.parent.as_deref().unwrap_or("-"),
            k.per_session,
            k.precedence,
            k.rule,
            format!("v{}", k.version),
            k.assigned_by.join(", ")
        ));
    }
    out
}

/// The category tree, indented by depth, each with its guidance's version
/// and first line.
pub fn ontology_categories_lines(categories: &[OntologyCategory]) -> Vec<String> {
    if categories.is_empty() {
        return vec![
            "No categories yet: `theseus ontology topic add NAME` declares a topic, and a bound \
             place's channel or person is made when the binding starts."
                .into(),
        ];
    }
    let mut out = Vec::new();
    for c in categories {
        let pad = "  ".repeat(c.depth.saturating_sub(1) as usize);
        let guide = match &c.guidance {
            Some(g) => {
                let first = g.text.lines().next().unwrap_or("");
                let first: String = first.chars().take(60).collect();
                format!("  guidance v{} {}: {first}", g.version, g.digest)
            }
            None => String::new(),
        };
        out.push(format!("{pad}{} ({}){guide}", c.name, c.id));
    }
    out
}

/// A session's memberships, one a line: its category, kind, origin, and
/// as-of.
pub fn ontology_memberships_lines(ms: &[OntologyMembership]) -> Vec<String> {
    if ms.is_empty() {
        return vec!["No memberships.".into()];
    }
    ms.iter()
        .map(|m| {
            format!(
                "{:<28} {:<8} {} as of {}{}",
                m.category,
                m.kind,
                m.origin,
                m.as_of_ms,
                m.confidence
                    .map(|c| format!(" ({c:.2} sure)"))
                    .unwrap_or_default()
            )
        })
        .collect()
}

/// `theseus ontology proposals`: a line per proposal, newest first, with
/// what answers it.
pub fn ontology_proposals_lines(r: &OntologyProposalsResult) -> Vec<String> {
    if r.proposals.is_empty() {
        return vec!["No proposals: Jev's categorize.v1 has none unanswered.".into()];
    }
    let mut out: Vec<String> = r
        .proposals
        .iter()
        .map(|p| {
            let what = match (&p.topic, p.new_topic) {
                (_, true) => "a new topic".to_string(),
                (Some(t), _) => match &p.topic_name {
                    Some(n) => format!("{t} ({n})"),
                    None => format!("{t} (gone)"),
                },
                (None, false) => "?".to_string(),
            };
            format!(
                "{}  {}{} -> {what}  {:.2} {}  {}",
                p.judgment,
                p.session_id,
                p.session_title
                    .as_deref()
                    .map(|t| format!(" \"{t}\""))
                    .unwrap_or_default(),
                p.confidence,
                p.band,
                super::fmt_time(p.at_ms)
            )
        })
        .collect();
    if r.more > 0 {
        out.push(format!("… and {} more (--limit shows them)", r.more));
    }
    out.push(
        "`theseus ontology accept JUDGMENT` (with --topic NAME for a new topic) or `theseus \
         ontology reject JUDGMENT` answers one."
            .into(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_indents_by_depth_and_shows_guidance() {
        let c = |id: &str, name: &str, depth: u32, g: Option<&str>| OntologyCategory {
            id: id.into(),
            kind: "topic".into(),
            name: name.into(),
            parent: None,
            depth,
            description: String::new(),
            added_by: "the CLI".into(),
            guidance: g.map(|t| theseus_protocol::OntologyGuidance {
                category: id.into(),
                text: t.into(),
                version: 2,
                digest: "0123456789abcdef".into(),
                added_by: "the CLI".into(),
            }),
        };
        let lines = ontology_categories_lines(&[
            c(
                "topic:lighthouse",
                "lighthouse",
                1,
                Some("Answer briefly.\nMore."),
            ),
            c("topic:lamps", "lamps", 2, None),
        ]);
        assert_eq!(
            lines,
            vec![
                "lighthouse (topic:lighthouse)  guidance v2 0123456789abcdef: Answer briefly.",
                "  lamps (topic:lamps)",
            ]
        );
    }
}
