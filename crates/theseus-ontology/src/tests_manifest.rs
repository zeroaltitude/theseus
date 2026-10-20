//! A session in six kinds (theseus-jrf4c): the compile walk composes the
//! interpreted kinds the way it composes the given ones, by precedence, and
//! the manifest records each membership with its origin and confidence. New
//! kinds are rows, not code; this is the golden that says so.

use super::*;

const OP: Origin = Origin::Operator;

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn category(i: &str, name: &str, parent: Option<&str>, by: &str) -> Category {
    Category {
        id: id(i),
        name: name.into(),
        parent: parent.map(id),
        description: String::new(),
        added_by: by.into(),
        retired_ms: None,
        handles: Vec::new(),
        merged_into: None,
    }
}

fn world() -> Ontology {
    let mut o = Ontology::seeded();
    let cats = [
        (
            "guild:100000000000000001",
            "Kestrel",
            None,
            "transport",
            "Server rules.",
        ),
        (
            "channel:200000000000000001",
            "general",
            Some("guild:100000000000000001"),
            "transport",
            "Channel rules.",
        ),
        (
            "topic:ontology",
            "ontology",
            None,
            "ada",
            "Categories first.",
        ),
        (
            "event:release-1",
            "release 1",
            None,
            "ada",
            "Ship by Friday.",
        ),
        (
            "resource:theseus-repo",
            "theseus repo",
            None,
            "ada",
            "Read AGENTS.md first.",
        ),
        (
            "expertise:rust",
            "rust",
            None,
            "ada",
            "Prefer the std library.",
        ),
    ];
    for (i, name, parent, by, guidance) in cats {
        let origin = if by == "transport" {
            Origin::Transport
        } else {
            OP
        };
        o.put(Record::Category(category(i, name, parent, by)), origin)
            .unwrap();
        o.put(Record::Guidance(Guidance::new(id(i), guidance, 1, by)), OP)
            .unwrap();
    }
    o
}

/// The session's memberships, in an order the walk does not read.
fn session() -> Vec<Membership> {
    let jev = |i: &str, c: f32, at| Membership {
        category: id(i),
        origin: Origin::Jev,
        confidence: Some(c),
        as_of_ms: at,
    };
    vec![
        Membership::operator(id("expertise:rust"), 5),
        jev("resource:theseus-repo", 0.75, 4),
        Membership {
            category: id("event:release-1"),
            origin: Origin::Sweep,
            confidence: Some(0.5),
            as_of_ms: 3,
        },
        jev("topic:ontology", 0.75, 2),
        Membership::given(id("channel:200000000000000001"), 1),
        Membership::given(id("guild:100000000000000001"), 1),
    ]
}

/// The manifest, as it is written: precedence order, origin and confidence
/// as each membership came, nothing of the memberships' own order.
#[test]
fn the_manifest_of_a_session_in_six_kinds() {
    let o = world();
    let c = o.compose(&session());
    assert_eq!(c.skipped, vec![]);
    assert_eq!(
        serde_json::to_string(&c.memberships).unwrap(),
        concat!(
            r#"[{"kind":"guild","category":"guild:100000000000000001","origin":"transport","as_of_ms":1},"#,
            r#"{"kind":"channel","category":"channel:200000000000000001","origin":"transport","as_of_ms":1},"#,
            r#"{"kind":"topic","category":"topic:ontology","origin":"jev","confidence":0.75,"as_of_ms":2},"#,
            r#"{"kind":"event","category":"event:release-1","origin":"sweep","confidence":0.5,"as_of_ms":3},"#,
            r#"{"kind":"resource","category":"resource:theseus-repo","origin":"jev","confidence":0.75,"as_of_ms":4},"#,
            r#"{"kind":"expertise","category":"expertise:rust","origin":"operator","as_of_ms":5}]"#
        )
    );
    let headers: Vec<&str> = c
        .sections
        .iter()
        .map(|s| s.lines().next().unwrap())
        .collect();
    assert_eq!(
        headers,
        [
            "# Guidance",
            "# Guidance (guild Kestrel)",
            "# Guidance (channel Kestrel › general)",
            "# Guidance (topic ontology)",
            "# Guidance (event release 1)",
            "# Guidance (resource)",
            "# Guidance (expertise rust)",
        ]
    );
}

/// The rendered bytes depend on which categories are in play, never on who
/// assigned them or how sure: a membership Jev writes where the operator's
/// stood renders the same system block, so the prompt cache survives it.
#[test]
fn origin_and_confidence_change_the_manifest_and_not_the_bytes() {
    let o = world();
    let base = o.compose(&session());
    let mut other = session();
    other.reverse();
    other[2].origin = OP;
    other[2].confidence = None;
    let again = o.compose(&other);
    assert_eq!(base.render(), again.render());
    assert_ne!(base.memberships, again.memberships);
}
