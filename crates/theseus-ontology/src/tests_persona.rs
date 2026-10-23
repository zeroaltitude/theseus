//! The persona kind (theseus-utfnl): the owner's alone.

use super::*;

const OP: Origin = Origin::Operator;
const MACHINES: [Origin; 3] = [Origin::Jev, Origin::Sweep, Origin::Dream];

fn id(key: &str) -> CategoryId {
    CategoryId::new("persona", key).unwrap()
}

fn card(soul: Option<&str>) -> PersonaCard {
    PersonaCard {
        soul: soul.map(str::to_string),
        ..PersonaCard::default()
    }
}

fn persona(key: &str, soul: Option<&str>) -> Category {
    Category {
        persona: Some(card(soul)),
        ..Category::new(id(key), key, "ada")
    }
}

fn held(keys: &[&str]) -> Ontology {
    let mut o = Ontology::seeded();
    for k in keys {
        o.put(Record::Category(persona(k, Some("/p/soul.md"))), OP)
            .unwrap();
    }
    o
}

#[test]
fn the_persona_seed_is_pinned_and_passes_the_table() {
    let o = Ontology::seeded();
    assert_eq!(o.check_all(), Ok(()));
    let k = o.kind("persona").unwrap();
    assert_eq!(k.basis, Basis::Interpreted);
    assert_eq!(k.assigned_by, [Origin::Operator, Origin::Import]);
    assert_eq!(k.per_session, PerSession::AtMost(1));
    assert_eq!((k.parent.as_deref(), k.precedence), (None, 100));
}

#[test]
fn no_machine_origin_writes_a_persona() {
    let mut o = held(&["ada"]);
    o.put(Record::Category(persona("bo", Some("/p/b.md"))), OP)
        .unwrap();
    for by in MACHINES {
        let tag = format!("{by}:x");
        // A new persona.
        let new = Category {
            added_by: tag.clone(),
            ..persona("cy", Some("/p/c.md"))
        };
        assert!(o.check(&Record::Category(new), by).is_err(), "{by} new");
        // An edit of the owner's.
        let edit = Category {
            added_by: tag.clone(),
            name: "Ada!".into(),
            ..persona("ada", Some("/p/soul.md"))
        };
        assert!(o.check(&Record::Category(edit), by).is_err(), "{by} edit");
        // A merge.
        let merge = Category {
            merged_into: Some(id("bo")),
            ..persona("ada", Some("/p/soul.md"))
        };
        assert!(o.check(&Record::Category(merge), by).is_err(), "{by} merge");
        // A retire.
        let retire = persona("ada", Some("/p/soul.md")).retired(9);
        assert!(o.check(&Record::Category(retire), by).is_err(), "{by} retire");
        // Guidance, new text or empty.
        for text in ["Be brisk.", ""] {
            let g = Guidance::new(id("ada"), text, 1, &tag);
            assert!(o.check(&Record::Guidance(g), by).is_err(), "{by} guide");
        }
        // A membership.
        let m = Membership {
            category: id("ada"),
            origin: by,
            confidence: Some(0.9),
            as_of_ms: 1,
        };
        let list = MemberList {
            session: "ses_1".into(),
            kind: "persona".into(),
            members: vec![m],
        };
        assert!(o.check(&Record::Members(list), by).is_err(), "{by} list");
        // A kind row naming itself.
        let mut row = o.kind("persona").unwrap().clone();
        row.version += 1;
        row.assigned_by.push(by);
        row.added_by = tag;
        assert!(o.check(&Record::Kind(row), by).is_err(), "{by} kind");
    }
}

#[test]
fn the_persona_kind_row_is_pinned() {
    let o = Ontology::seeded();
    let base = o.kind("persona").unwrap().clone();
    let edit = |f: &dyn Fn(&mut Kind)| {
        let mut k = base.clone();
        k.version += 1;
        k.added_by = "operator:ada".into();
        f(&mut k);
        o.check(&Record::Kind(k), OP)
    };
    for by in MACHINES {
        assert!(edit(&|k| k.assigned_by.push(by)).is_err(), "{by}");
    }
    assert!(edit(&|k| k.assigned_by = vec![Origin::Operator]).is_err());
    assert!(edit(&|k| k.per_session = PerSession::Many).is_err());
    assert!(edit(&|k| k.basis = Basis::Given).is_err());
    assert!(edit(&|k| k.parent = Some("persona".into())).is_err());
    // Precedence and description stay the owner's to change.
    assert_eq!(edit(&|k| k.precedence = 95), Ok(()));
    assert_eq!(edit(&|k| k.description = "Who answers.".into()), Ok(()));
}

#[test]
fn a_persona_has_no_memberships() {
    let o = held(&["ada"]);
    for by in [OP, Origin::Import] {
        let list = MemberList {
            session: "ses_1".into(),
            kind: "persona".into(),
            members: vec![Membership::operator(id("ada"), 1)],
        };
        let err = o.check(&Record::Members(list), by).unwrap_err();
        assert!(err.to_string().contains("never a membership"), "{err}");
    }
}

#[test]
fn a_persona_has_no_guidance() {
    let o = held(&["ada"]);
    let g = Guidance::new(id("ada"), "Be brisk.", 1, "operator:ada");
    let err = o.check(&Record::Guidance(g), OP).unwrap_err();
    assert!(err.to_string().contains("theseus persona soul"), "{err}");
}

#[test]
fn one_default_persona() {
    let mut o = held(&["ada", "bo"]);
    let mark = |key: &str| Category {
        persona: Some(PersonaCard {
            default: true,
            ..card(Some("/p/soul.md"))
        }),
        ..Category::new(id(key), key, "ada")
    };
    o.put(Record::Category(mark("ada")), OP).unwrap();
    let err = o.check(&Record::Category(mark("bo")), OP).unwrap_err();
    assert!(matches!(err, Refusal::Duplicate { .. }), "{err}");
    // Re-writing the holder is not a second default.
    o.put(Record::Category(mark("ada")), OP).unwrap();
}

#[test]
fn none_and_prior_are_reserved() {
    let o = Ontology::seeded();
    for key in RESERVED_KEYS {
        let c = persona(key, Some("/p/soul.md"));
        assert!(o.check(&Record::Category(c), OP).is_err(), "{key}");
    }
}

const RESERVED_KEYS: [&str; 2] = ["none", "prior"];

#[test]
fn a_card_belongs_to_a_persona_only() {
    let o = Ontology::seeded();
    let mut c = Category::new(CategoryId::new("topic", "tides").unwrap(), "Tides", "ada");
    c.persona = Some(card(None));
    assert!(o.check(&Record::Category(c), OP).is_err());
    // A persona with no card is refused too.
    let bare = Category::new(id("ada"), "Ada", "ada");
    assert!(o.check(&Record::Category(bare), OP).is_err());
}

#[test]
fn an_awaiting_persona_has_no_soul_digest_and_is_not_public() {
    let o = Ontology::seeded();
    let mut c = persona("ada", None);
    assert_eq!(o.check(&Record::Category(c.clone()), OP), Ok(()));
    c.persona.as_mut().unwrap().soul_public = true;
    assert!(o.check(&Record::Category(c), OP).is_err());
}

#[test]
fn a_persona_is_retired_never_merged() {
    let o = held(&["ada", "bo"]);
    let merge = Category {
        merged_into: Some(id("bo")),
        ..persona("ada", Some("/p/soul.md"))
    };
    assert!(o.check(&Record::Category(merge), OP).is_err());
    let retire = persona("ada", Some("/p/soul.md")).retired(9);
    assert_eq!(o.check(&Record::Category(retire), OP), Ok(()));
}
