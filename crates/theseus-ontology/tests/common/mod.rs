//! The fixture the golden renders and the property tests share: Kestrel's
//! server, its general channel, two people, and four topics, with guidance
//! on all but one.
#![allow(dead_code)]

use theseus_ontology::{Category, CategoryId, Guidance, Membership, Ontology, Origin, Record};

pub const GUILD: &str = "guild:100000000000000001";
pub const GENERAL: &str = "channel:200000000000000001";
pub const ADA: &str = "person:300000000000000001";
pub const SAM: &str = "person:300000000000000002";
/// 2026-09-21, as the memberships' as-of.
pub const AS_OF: u64 = 1_790_000_000_000;

pub fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

pub fn category(i: &str, name: &str, parent: Option<&str>) -> Category {
    Category {
        id: id(i),
        name: name.into(),
        parent: parent.map(id),
        description: String::new(),
        added_by: "ada".into(),
    }
}

pub fn set_guidance(o: &mut Ontology, i: &str, text: &str) {
    let version = o.guidance(&id(i)).map_or(1, |g| g.version + 1);
    o.put(
        Record::Guidance(Guidance::new(id(i), text, version, "ada")),
        Origin::Operator,
    )
    .unwrap();
}

pub fn kestrel() -> Ontology {
    let mut o = Ontology::seeded();
    let given = [
        (GUILD, "Kestrel", None),
        (GENERAL, "general", Some(GUILD)),
        (ADA, "Ada", None),
        (SAM, "Sam", None),
    ];
    for (i, name, parent) in given {
        o.put(
            Record::Category(category(i, name, parent)),
            Origin::Transport,
        )
        .unwrap();
    }
    let topics = [
        ("topic:theseus", "theseus", None),
        ("topic:rust-harness", "rust-harness", Some("topic:theseus")),
        ("topic:web", "web", Some("topic:theseus")),
        ("topic:cooking", "cooking", None),
    ];
    for (i, name, parent) in topics {
        o.put(
            Record::Category(category(i, name, parent)),
            Origin::Operator,
        )
        .unwrap();
    }
    set_guidance(
        &mut o,
        GUILD,
        "This is Kestrel's server. Keep work talk professional, and never paste secrets here.",
    );
    set_guidance(
        &mut o,
        GENERAL,
        "The general channel is shared with the whole team: answer briefly, and move long work \
         to a thread.",
    );
    set_guidance(
        &mut o,
        ADA,
        "The owner. Approves the work, and likes short answers.",
    );
    set_guidance(
        &mut o,
        SAM,
        "A teammate. Explain the context that may be missing.",
    );
    set_guidance(&mut o, "topic:theseus", "A draft, superseded below.");
    set_guidance(
        &mut o,
        "topic:theseus",
        "Theseus is Ada's Rust agent harness. Its spec, The Ship of Theseus, holds the design \
         and the as-built record.",
    );
    // Pasted with CRLF line ends and a trailing newline: stored trimmed, with LF.
    set_guidance(
        &mut o,
        "topic:rust-harness",
        "Run the gate before every commit:\r\n\r\n    scripts/gate.sh && git commit -S\r\n\r\n\
         Never hand-merge Cargo.lock.\r\n",
    );
    set_guidance(&mut o, "topic:cooking", "Metric units.");
    o
}

/// A session in Kestrel's general channel, with Ada and Sam listed, and two
/// topics: given in a jumble, as a place and a list may give them.
pub fn guild_channel_session() -> Vec<Membership> {
    vec![
        Membership::operator(id("topic:rust-harness"), AS_OF + 2),
        Membership::given(id(SAM), AS_OF),
        Membership::given(id(GENERAL), AS_OF),
        Membership::operator(id("topic:cooking"), AS_OF + 1),
        Membership::given(id(ADA), AS_OF),
        Membership::given(id(GUILD), AS_OF),
    ]
}

/// Ada's DM, with one topic.
pub fn dm_session() -> Vec<Membership> {
    vec![
        Membership::operator(id("topic:rust-harness"), AS_OF + 2),
        Membership::given(id(ADA), AS_OF),
    ]
}
