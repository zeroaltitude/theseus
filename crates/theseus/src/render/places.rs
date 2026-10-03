//! The place rule's lines (theseus-nbsh): health's `places:` line, and
//! `theseus places`.

use theseus_protocol::{PlaceClass, PlaceInfo, PlacesHealth};

/// A place's name, with what its start-time check found when it is a
/// channel bound private: anyone besides the owner who can view it, or why
/// that could not be read.
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
        _ => p.name.clone(),
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
        .map(named)
        .collect()
}

/// Health's `places:` line: `places: private: CLI, web, DM @eddie · shared:
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
}

/// `theseus places`: each place and its class, a line each, then what each
/// class gets.
pub fn places_lines(h: &PlacesHealth) -> Vec<String> {
    let mut lines: Vec<String> = h
        .places
        .iter()
        .map(|p| format!("{:<8} {}  {}", p.class.as_str(), named(p), p.place))
        .collect();
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
        }
    }

    #[test]
    fn the_places_line_names_each_class() {
        let mut h = PlacesHealth {
            places: vec![
                place("CLI", PlaceClass::Private),
                place("web", PlaceClass::Private),
                place("DM @eddie", PlaceClass::Private),
                place("#openclaw", PlaceClass::Shared),
            ],
            public_paths: vec![],
        };
        assert_eq!(
            places_health_line(&h),
            "places: private: CLI, web, DM @eddie · shared: #openclaw (public tools only)"
        );
        h.places.push(PlaceInfo {
            others: Some(vec!["alice".into()]),
            ..place("#lab", PlaceClass::Private)
        });
        h.public_paths = vec!["~/projects/open".into()];
        assert_eq!(
            places_health_line(&h),
            "places: private: CLI, web, DM @eddie, #lab (⚠ bound private, but 1 person besides \
             the owner can view it: alice) · shared: #openclaw (public tools, and files under \
             ~/projects/open)"
        );
    }
}
