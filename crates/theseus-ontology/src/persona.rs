//! Personas (theseus-utfnl): the `persona` kind. A persona is who answers a
//! message; Jev picks one per message (`engage`), so a persona is never a
//! session's membership and has no guidance: its soul is a file, and the
//! category holds the path and the short fields that name and shape it.
//!
//! The owner alone writes a persona. The kind's row is pinned (its basis,
//! origins and count stay as the seed has them, for every writer), and this
//! module's refusals close the other ways in: a membership list, a guidance
//! record, and a merge.

use serde::{Deserialize, Serialize};

use crate::category::{Category, Guidance, MemberList};
use crate::kind::{seeds, Kind};
use crate::ontology::Ontology;
use crate::refusal::Refusal;

/// The kind personas are.
pub const KIND: &str = "persona";

/// Keys that are route options' ids, so no persona holds them: `none` is the
/// default persona, `prior` the one already engaged.
pub const RESERVED: [&str; 2] = ["none", "prior"];

/// A card's cap, in characters: about 60 tokens with room.
pub const CARD_MAX: usize = 600;

/// How readily a persona speaks first: `quiet`, `normal` or `chatty`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Initiative {
    Quiet,
    #[default]
    Normal,
    Chatty,
}

fn normal(i: &Initiative) -> bool {
    *i == Initiative::Normal
}

/// What a `persona:<key>` category holds beyond its name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonaCard {
    /// The soul file's path as the owner spelled it. None: the persona
    /// awaits its soul, and is listed but never offered to Jev.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soul: Option<String>,
    /// The file entry's `readers = "public"`: a shared place may read it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub soul_public: bool,
    /// The first 16 hex digits of the SHA-256 of the soul, as the owner's
    /// words were written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soul_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    /// About 60 tokens of specialization and background: what Jev's persona
    /// question reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tts_voice: Option<String>,
    /// A path or a URL, for Discord.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(default, skip_serializing_if = "normal")]
    pub initiative: Initiative,
    /// Whether a shared place may be answered by this persona beyond its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shareable: bool,
    /// The persona that answers when Jev picks none: at most one holds it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
}

impl PersonaCard {
    /// Whether the persona has a soul, so it can answer.
    pub fn is_live(&self) -> bool {
        self.soul.is_some()
    }
}

/// A category's card against its own rules: only a persona carries one, a
/// persona's key is not a route option's, and the short fields are spelled
/// as the config spelled them.
pub(crate) fn check_card(c: &Category) -> Result<(), Refusal> {
    let what = |field: &str| format!("persona `{}`'s {field}", c.id);
    if c.kind() != KIND {
        return match c.persona {
            None => Ok(()),
            Some(_) => Err(Refusal::invalid(
                format!("category `{}`'s persona card", c.id),
                format!("only a {KIND} carries one, and this is a {}", c.kind()),
            )),
        };
    }
    if RESERVED.contains(&c.id.local()) {
        return Err(Refusal::invalid(
            format!("the persona key {:?}", c.id.local()),
            "it names a route option (`none` is the default persona, `prior` the one already \
             engaged): pick another key",
        ));
    }
    let Some(p) = &c.persona else {
        return Err(Refusal::invalid(
            format!("persona `{}`", c.id),
            "it has no card: a persona carries one, even an empty one",
        ));
    };
    for (field, v) in [
        ("emoji", &p.emoji),
        ("tts_voice", &p.tts_voice),
        ("avatar", &p.avatar),
        ("card", &p.card),
        ("soul", &p.soul),
        ("soul_digest", &p.soul_digest),
    ] {
        let Some(v) = v else { continue };
        if v.trim().is_empty() {
            return Err(Refusal::invalid(
                what(field),
                "it is empty: leave it out instead",
            ));
        }
        if field != "card" && v.contains('\n') {
            return Err(Refusal::invalid(what(field), "it must be one line"));
        }
    }
    if p.card
        .as_deref()
        .is_some_and(|v| v.chars().count() > CARD_MAX)
    {
        return Err(Refusal::invalid(
            what("card"),
            format!("it is at most {CARD_MAX} characters (about 60 tokens)"),
        ));
    }
    if (p.soul.is_none()) && (p.soul_public || p.soul_digest.is_some()) {
        return Err(Refusal::invalid(
            what("soul"),
            "it is absent, so it has no digest and is not public",
        ));
    }
    Ok(())
}

/// A persona is picked per message, never a session's membership.
pub(crate) fn check_list(l: &MemberList) -> Result<(), Refusal> {
    match l.kind == KIND {
        true => Err(Refusal::Writer {
            why: "a persona is picked per message, never a membership".into(),
        }),
        false => Ok(()),
    }
}

/// A persona has no guidance: its text is its soul file. Refused for every
/// writer, the operator included, so no machine origin has a way in.
pub(crate) fn check_guidance(g: &Guidance) -> Result<(), Refusal> {
    match g.category.kind() == KIND {
        true => Err(Refusal::Writer {
            why: format!(
                "`{}` has no guidance: a persona's text is its soul file (`theseus persona soul \
                 {}`)",
                g.category,
                g.category.local()
            ),
        }),
        false => Ok(()),
    }
}

/// At most one live persona is the default (the one `none` engages).
pub(crate) fn check_default(onto: &Ontology, c: &Category) -> Result<(), Refusal> {
    if !c.persona.as_ref().is_some_and(|p| p.default) {
        return Ok(());
    }
    match onto
        .categories()
        .find(|o| o.id != c.id && o.persona.as_ref().is_some_and(|p| p.default))
    {
        Some(twin) => Err(Refusal::Duplicate {
            why: format!(
                "`{}` is the default persona already: one persona answers when Jev picks none \
                 (`theseus persona default {}` moves it)",
                twin.id,
                c.id.local()
            ),
        }),
        None => Ok(()),
    }
}

/// A persona is never merged into another: its soul is a file, and would be
/// lost.
pub(crate) fn check_merge(c: &Category) -> Result<(), Refusal> {
    match c.kind() == KIND {
        true => Err(Refusal::WrongKind {
            why: format!(
                "`{}` is a persona, and a persona is retired, never merged",
                c.id
            ),
        }),
        false => Ok(()),
    }
}

/// The kind row, pinned for every writer, the operator included: a persona
/// is the owner's alone, so no edit adds a machine origin to `assigned_by`
/// or changes how the kind is held.
pub(crate) fn pinned(k: &Kind) -> Result<(), Refusal> {
    if k.name != KIND {
        return Ok(());
    }
    let seed = seeds().into_iter().find(|s| s.name == KIND);
    match seed {
        Some(s)
            if k.basis == s.basis
                && k.assigned_by == s.assigned_by
                && k.per_session == s.per_session
                && k.parent == s.parent =>
        {
            Ok(())
        }
        _ => Err(Refusal::Given {
            kind: KIND.into(),
            why: "`persona` is the owner's alone: its basis, assigned_by, per_session and parent \
                  stay as the seed has them (interpreted, operator and import, one per session, \
                  no parent). Its precedence and description may change"
                .into(),
        }),
    }
}
