//! The place rule's lines (theseus-nbsh): health's `places:` line, and
//! `theseus places`.

use theseus_protocol::{BindingStatus, PlaceCeiling, PlaceClass, PlaceInfo, PlacesHealth};

/// A place's ceiling, in words (step 38a): `floor approve · tools web ·
/// spend ≤ $1.00 · profile glm`, each part it sets.
pub fn ceiling_words(c: &PlaceCeiling) -> String {
    let mut parts = Vec::new();
    if let Some(f) = &c.posture_floor {
        parts.push(format!("floor {f}"));
    }
    if let Some(t) = &c.tools {
        parts.push(match t.is_empty() {
            true => "no tools".to_string(),
            false => format!("tools {}", t.join(",")),
        });
    }
    if let Some(usd) = c.spend_limit_usd {
        parts.push(format!("spend ≤ ${usd:.2}"));
    }
    if let Some(p) = &c.profile {
        parts.push(format!("profile {p}"));
    }
    parts.join(" · ")
}

/// A binding's places counted by guild, as `theseus health` says them (step
/// 38a): `by guild: home 2 (trusted), 314159265358979323 1 · DMs 1`. None
/// when the binding names no guild.
pub fn places_by_guild(b: &BindingStatus) -> Option<String> {
    if b.guilds.is_empty() {
        return None;
    }
    let guilds: Vec<String> = b
        .guilds
        .iter()
        .map(|g| {
            let n = b
                .places
                .iter()
                .filter(|p| p.guild.as_ref() == Some(&g.id))
                .count();
            let trusted = if g.trusted { " (trusted)" } else { "" };
            format!("{} {n}{trusted}", g.name.as_deref().unwrap_or(&g.id))
        })
        .collect();
    let dms = b.places.iter().filter(|p| p.kind == "dm").count();
    Some(format!("by guild: {} · DMs {dms}", guilds.join(", ")))
}

/// A place's name, with what its start-time check found when it is a
/// channel bound private: anyone besides the owner who can view it, or why
/// that could not be read; or, in a trusted guild, that it is (theseus-rdqg).
fn named(p: &PlaceInfo) -> String {
    match (&p.others, &p.unchecked) {
        (Some(o), _) if !o.is_empty() => format!(
            "{} (⚠ bound private, but {} besides the owner can view it: {})",
            p.name,
            match o.len() {
                1 => "1 person".to_string(),
                n => format!("{n} people"),
            },
            o.join(", ")
        ),
        (_, Some(why)) => format!("{} (who can view it is unchecked: {why})", p.name),
        _ if p.trusted_guild => format!("{} (in a trusted guild)", p.name),
        _ => p.name.clone(),
    }
}

/// `named`, with the place's ceiling when it has one: `#pier [tools web]`.
fn named_ceiling(p: &PlaceInfo) -> String {
    match &p.ceiling {
        Some(c) => format!("{} [{}]", named(p), ceiling_words(c)),
        None => named(p),
    }
}

/// What a shared place gets, in words.
fn shared_gets(h: &PlacesHealth) -> String {
    match h.public_paths.is_empty() {
        true => "public tools only".into(),
        false => format!(
            "public tools, and files under {}",
            h.public_paths.join(", ")
        ),
    }
}

fn of(h: &PlacesHealth, class: PlaceClass) -> Vec<String> {
    h.places
        .iter()
        .filter(|p| p.class == class)
        .map(named_ceiling)
        .collect()
}

/// Health's `places:` line: `places: private: CLI, web, DM @zeroaltitude · shared:
/// #openclaw (public tools only)`.
pub fn places_health_line(h: &PlacesHealth) -> String {
    let mut parts = vec![format!(
        "private: {}",
        of(h, PlaceClass::Private).join(", ")
    )];
    let shared = of(h, PlaceClass::Shared);
    if !shared.is_empty() {
        parts.push(format!(
            "shared: {} ({})",
            shared.join(", "),
            shared_gets(h)
        ));
    }
    format!("places: {}", parts.join(" · "))
}

/// Health's `places:` line, a warning while anyone besides the owner can
/// view a channel bound private.
pub(super) fn push_health(o: &mut Vec<super::Line>, h: Option<&PlacesHealth>) {
    let Some(h) = h else {
        return;
    };
    let warn = h
        .places
        .iter()
        .any(|p| p.others.as_ref().is_some_and(|o| !o.is_empty()));
    let tag = if warn {
        super::Tag::Warn
    } else {
        super::Tag::Plain
    };
    super::push(o, tag, &places_health_line(h));
    // What the binding's start found wrong with a place (theseus-ext.11).
    for w in &h.warnings {
        super::push(o, super::Tag::Warn, &format!("  ⚠ {}", w.detail));
    }
}

/// `theseus places`: each place and its class, a line each, then what each
/// class gets.
pub fn places_lines(h: &PlacesHealth) -> Vec<String> {
    let mut lines: Vec<String> = h
        .places
        .iter()
        .map(|p| {
            let guild = p.guild.as_ref().map(|g| format!("  guild {g}"));
            let ceiling = p
                .ceiling
                .as_ref()
                .map(|c| format!("  ceiling: {}", ceiling_words(c)));
            format!(
                "{:<8} {}  {}{}{}",
                p.class.as_str(),
                named(p),
                p.place,
                guild.unwrap_or_default(),
                ceiling.unwrap_or_default()
            )
        })
        .collect();
    lines.extend(h.warnings.iter().map(|w| format!("⚠ {}", w.detail)));
    lines.push(String::new());
    lines.push("private: everything, as the CLI has it.".into());
    lines.push(format!(
        "shared: its own conversation, {}; no proc.run and no AWS; only the context files marked \
         readers = \"public\". Publish something there yourself: `theseus publish NODE|FILE \
         --to PLACE`.",
        shared_gets(h)
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(name: &str, class: PlaceClass) -> PlaceInfo {
        PlaceInfo {
            place: format!("discord:{name}"),
            name: name.into(),
            class,
            others: None,
            unchecked: None,
            trusted_guild: false,
            guild: None,
            ceiling: None,
        }
    }

    /// theseus-rdqg: in a trusted guild a private channel is named as in
    /// one, never warned of, and a channel there bound `private = false` is
    /// shared as any other; `theseus places` says the same.
    #[test]
    fn a_trusted_guilds_channels_are_named_so() {
        let h = PlacesHealth {
            places: vec![
                place("CLI", PlaceClass::Private),
                place("web", PlaceClass::Private),
                PlaceInfo {
                    trusted_guild: true,
                    ..place("#openclaw", PlaceClass::Private)
                },
                place("DM @zeroaltitude", PlaceClass::Private),
                place("#hall", PlaceClass::Shared),
            ],
            public_paths: vec![],
            warnings: vec![],
        };
        assert_eq!(
            places_health_line(&h),
            "places: private: CLI, web, #openclaw (in a trusted guild), DM @zeroaltitude · shared: #hall \
             (public tools only)"
        );
        let mut o = Vec::new();
        push_health(&mut o, Some(&h));
        assert_eq!(o[0].tag, super::super::Tag::Plain, "no warning");
        assert!(places_lines(&h)
            .iter()
            .any(|l| l.starts_with("private  #openclaw (in a trusted guild)  discord:")));
    }

    #[test]
    fn the_places_line_names_each_class() {
        let mut h = PlacesHealth {
            places: vec![
                place("CLI", PlaceClass::Private),
                place("web", PlaceClass::Private),
                place("DM @zeroaltitude", PlaceClass::Private),
                place("#openclaw", PlaceClass::Shared),
            ],
            public_paths: vec![],
            warnings: vec![],
        };
        assert_eq!(
            places_health_line(&h),
            "places: private: CLI, web, DM @zeroaltitude · shared: #openclaw (public tools only)"
        );
        h.places.push(PlaceInfo {
            others: Some(vec!["alice".into()]),
            ..place("#lab", PlaceClass::Private)
        });
        h.public_paths = vec!["~/projects/open".into()];
        assert_eq!(
            places_health_line(&h),
            "places: private: CLI, web, DM @zeroaltitude, #lab (⚠ bound private, but 1 person besides \
             the owner can view it: alice) · shared: #openclaw (public tools, and files under \
             ~/projects/open)"
        );
    }

    /// theseus-ext.11: what the binding's start found wrong with a place is
    /// a warning line each, in health and in `theseus places`.
    #[test]
    fn a_places_warnings_are_lines_of_their_own() {
        let h = PlacesHealth {
            places: vec![place("CLI", PlaceClass::Private)],
            public_paths: vec![],
            warnings: vec![theseus_protocol::PlaceWarning {
                place: "discord:channel:7".into(),
                name: "#lab".into(),
                kind: "unbound".into(),
                detail: "#lab is not bound: its ceiling names profile \"nosuch\"".into(),
            }],
        };
        let mut o = Vec::new();
        push_health(&mut o, Some(&h));
        assert_eq!(o.len(), 2);
        assert_eq!(o[1].tag, super::super::Tag::Warn);
        assert!(places_lines(&h)
            .iter()
            .any(|l| l == "⚠ #lab is not bound: its ceiling names profile \"nosuch\""));
    }
}
