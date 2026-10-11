//! The ontology's lines (theseus-8kk.1): `theseus ontology kinds`,
//! `categories`, a session's memberships, and Jev's proposals (28b).

use theseus_protocol::{
    OntologyCategory, OntologyKind, OntologyMembership, OntologyPersonProposals,
    OntologyProposalsResult,
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
        let held = match c.members {
            0 => String::new(),
            1 => "  1 session".into(),
            n => format!("  {n} sessions"),
        };
        out.push(format!("{pad}{} ({}){held}{guide}", c.name, c.id));
        if !c.handles.is_empty() {
            out.push(format!("{pad}  handles: {}", c.handles.join(", ")));
        }
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

/// A person's row (theseus-fvyx): its name, held or new, its sessions and
/// proposals, their confidence range and bands, the first names inside it or
/// the people an ambiguous one may be; then its role line and a few titles.
pub fn person_row_lines(g: &OntologyPersonProposals) -> Vec<String> {
    let n = g.judgments.len();
    let range = match g.confidence_min == g.confidence_max {
        true => format!("{:.2}", g.confidence_max),
        false => format!("{:.2}-{:.2}", g.confidence_min, g.confidence_max),
    };
    let mut head = format!(
        "{} ({})  {} {}, {} {}  {range} {}",
        g.name,
        if g.new {
            "new".to_string()
        } else {
            g.key.clone()
        },
        g.sessions,
        if g.sessions == 1 {
            "session"
        } else {
            "sessions"
        },
        n,
        if n == 1 { "proposal" } else { "proposals" },
        g.bands.join("/")
    );
    if !g.first_names.is_empty() {
        let names: Vec<String> = g.first_names.iter().map(|f| format!("\"{f}\"")).collect();
        head.push_str(&format!("  with {} as {}?", names.join(", "), g.name));
    }
    if !g.ambiguous.is_empty() {
        head.push_str(&format!(
            "  AMBIGUOUS: a word of {} ({}); accept it with --as",
            g.ambiguous.len(),
            g.ambiguous.join(", ")
        ));
    }
    let mut out = vec![head];
    let mut more = Vec::new();
    if !g.handles.is_empty() {
        more.push(g.handles.join(", "));
    }
    if let Some(r) = &g.role_line {
        more.push(r.clone());
    }
    if !g.titles.is_empty() {
        let ts: Vec<String> = g.titles.iter().map(|t| format!("\"{t}\"")).collect();
        more.push(format!("in {}", ts.join(", ")));
    }
    if !more.is_empty() {
        out.push(format!("    {}", more.join("; ")));
    }
    out
}

/// `theseus ontology proposals`: a row per proposed person (with
/// `by_person`), then a line per topic's proposal, newest first, with what
/// answers them.
pub fn ontology_proposals_lines(r: &OntologyProposalsResult) -> Vec<String> {
    let hidden = (r.hidden > 0).then(|| {
        format!(
            "{} hidden by the exclusions (the owner, his agents, the house's names): not listed, never \
             accepted in bulk",
            r.hidden
        )
    });
    if r.proposals.is_empty() && r.people.is_empty() {
        let mut out = vec![
            "No proposals: Jev's categorize.v1 and people.v1 have none unanswered.".to_string(),
        ];
        out.extend(hidden);
        return out;
    }
    let mut out: Vec<String> = Vec::new();
    if !r.people.is_empty() {
        let n: usize = r.people.iter().map(|g| g.judgments.len()).sum();
        out.push(format!(
            "People: {} {}, {n} {}.",
            r.people.len(),
            if r.people.len() == 1 {
                "person"
            } else {
                "people"
            },
            if n == 1 { "proposal" } else { "proposals" }
        ));
        for g in &r.people {
            out.extend(person_row_lines(g));
        }
        if r.people_more > 0 {
            out.push(format!(
                "… and {} more people (--limit shows them)",
                r.people_more
            ));
        }
        out.push(
            "`theseus ontology accept --person NAME` (or `reject --person NAME`) answers a \
             person's every proposal; `--each` lists them one by one."
                .into(),
        );
        if !r.proposals.is_empty() {
            out.push("Topics:".into());
        }
    }
    out.extend(r.proposals.iter().map(|p| {
        let what = match (&p.topic, p.new_topic, &p.person) {
            (_, _, Some(who)) => format!(
                "{} {}{}{}",
                if who.new { "a new person," } else { "person" },
                who.name,
                match who.handles.as_slice() {
                    [] => String::new(),
                    hs => format!(" ({})", hs.join(", ")),
                },
                who.role_line
                    .as_deref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default()
            ),
            (_, true, _) => "a new topic".to_string(),
            (Some(t), _, _) => match &p.topic_name {
                Some(n) => format!("{t} ({n})"),
                None => format!("{t} (gone)"),
            },
            (None, false, _) => "?".to_string(),
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
    }));
    if r.more > 0 {
        out.push(format!("… and {} more (--limit shows them)", r.more));
    }
    out.extend(hidden);
    if !r.proposals.is_empty() {
        out.push(
            "`theseus ontology accept JUDGMENT` (with --topic NAME for a new topic) or `theseus \
             ontology reject JUDGMENT` answers one."
                .into(),
        );
    }
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
            members: 0,
            handles: Vec::new(),
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

    /// theseus-fvyx: a person's row says its sessions, proposals, range and
    /// bands, its first names as the question its accept answers, an
    /// ambiguous one's people, and how a row is answered.
    #[test]
    fn people_are_listed_one_row_per_person() {
        let row = |name: &str, first: &[&str], ambiguous: &[&str]| OntologyPersonProposals {
            key: format!("name:{}", name.to_lowercase()),
            name: name.into(),
            new: true,
            as_person: name.into(),
            judgments: vec!["jdg_1".into(), "jdg_2".into(), "jdg_3".into()],
            sessions: 2,
            confidence_min: 0.62,
            confidence_max: 0.95,
            bands: vec!["act".into(), "confirm".into()],
            handles: vec![],
            role_line: Some("Keeps the tide tables.".into()),
            titles: vec!["Moorings".into()],
            first_names: first.iter().map(|f| f.to_string()).collect(),
            ambiguous: ambiguous.iter().map(|a| a.to_string()).collect(),
            at_ms: 1,
        };
        let r = OntologyProposalsResult {
            people: vec![
                row("Marlo Quill", &["Marlo"], &[]),
                row("Tern", &[], &["Tern Ashby", "Tern Mallow"]),
            ],
            people_more: 4,
            ..Default::default()
        };
        let lines = ontology_proposals_lines(&r);
        assert_eq!(lines[0], "People: 2 people, 6 proposals.");
        assert_eq!(
            lines[1],
            "Marlo Quill (new)  2 sessions, 3 proposals  0.62-0.95 act/confirm  with \"Marlo\" as \
             Marlo Quill?"
        );
        assert_eq!(lines[2], "    Keeps the tide tables.; in \"Moorings\"");
        assert!(
            lines[3]
                .ends_with("AMBIGUOUS: a word of 2 (Tern Ashby, Tern Mallow); accept it with --as"),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.starts_with("… and 4 more people")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("accept --person NAME")),
            "{lines:?}"
        );
        assert!(
            lines.iter().all(|l| !l.contains("accept JUDGMENT")),
            "no topic's line without a topic's proposal"
        );
    }

    #[test]
    fn the_proposals_say_how_many_the_exclusions_hide() {
        let none = OntologyProposalsResult {
            hidden: 3,
            ..Default::default()
        };
        let lines = ontology_proposals_lines(&none);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[1].starts_with("3 hidden by the exclusions"),
            "{lines:?}"
        );
        assert!(
            ontology_proposals_lines(&OntologyProposalsResult::default())
                .iter()
                .all(|l| !l.contains("hidden"))
        );
    }
}
