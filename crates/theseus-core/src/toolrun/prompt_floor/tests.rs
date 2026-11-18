use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::config::PersonaConfig;
use crate::personas::Book;

fn cfg(state: &Path, files: &[&str], souls: &[(&str, &str)]) -> crate::Config {
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.context.files = files.iter().map(|f| f.to_string().into()).collect();
    for (key, path) in souls {
        cfg.personas.insert(
            key.to_string(),
            PersonaConfig {
                files: vec![path.to_string().into()],
                ..Default::default()
            },
        );
    }
    cfg
}

fn at(p: &str) -> PathBuf {
    PathBuf::from(p)
}

/// The files `[context] files` names, a copy beside one, and everything in the
/// two homes of a soul are held from the start; a neighbour is not.
#[test]
fn the_config_and_the_layout_are_held_from_the_start() {
    let state = Path::new("/nonexistent/prompt-floor/state");
    let c = cfg(state, &["/nonexistent/prompt-floor/ctx/CONTEXT.md"], &[]);
    let f = PromptFloor::of(&c, state);
    for held in [
        "/nonexistent/prompt-floor/ctx/CONTEXT.md",
        "/nonexistent/prompt-floor/ctx/CONTEXT.md.1794074842741.bak",
        "/nonexistent/prompt-floor/ctx/CONTEXT.md.new",
        "/nonexistent/prompt-floor/state/personas/warm/PERSONA.md",
        "/nonexistent/prompt-floor/state/personas/warm/PERSONA.md.17.bak",
        "/nonexistent/prompt-floor/state/persona/theseus.md",
        "/nonexistent/prompt-floor/state/persona/theseus.md.1794074842741.bak",
    ] {
        assert!(f.holds(&at(held)), "{held} is held");
    }
    for free in [
        "/nonexistent/prompt-floor/ctx/OTHER.md",
        "/nonexistent/prompt-floor/ctx/CONTEXT.md.d/x",
        "/nonexistent/prompt-floor/ctx/CONTEXTS.md",
        "/nonexistent/prompt-floor/ctx/CONTEXT.mdx",
        "/nonexistent/prompt-floor/state/personality/x",
        "/nonexistent/prompt-floor/state/store/x",
    ] {
        assert!(!f.holds(&at(free)), "{free} is not held");
    }
}

/// A soul the ontology puts anywhere is held once the book names it, with a
/// copy beside it; a book not seen before is read, the one already known is
/// not read again; a soul that moved is held where it is now.
#[test]
fn a_soul_anywhere_is_held_as_the_book_changes() {
    let state = Path::new("/nonexistent/prompt-floor/state");
    let none = cfg(state, &[], &[]);
    let f = PromptFloor::of(&none, state);
    let soul = "/nonexistent/prompt-floor/away/pilot.md";
    assert!(!f.holds(&at(soul)), "not before the book names it");

    let one = cfg(state, &[], &[("pilot", soul)]);
    let book = Arc::new(Book::of_config(&one));
    assert_eq!(f.refresh(&one, state, &book), vec![at(soul)]);
    assert!(f.holds(&at(soul)) && f.holds(&at(&format!("{soul}.9.bak"))));
    assert!(
        f.refresh(&one, state, &book).is_empty(),
        "the same book is not read again"
    );

    let moved = "/nonexistent/prompt-floor/elsewhere/pilot.md";
    let two = cfg(state, &[], &[("pilot", moved)]);
    let next = Arc::new(Book::of_config(&two));
    assert_eq!(f.refresh(&two, state, &next), vec![at(moved)]);
    assert!(f.holds(&at(moved)) && f.holds(&at(&format!("{moved}.new"))));
    assert!(
        !f.holds(&at(soul)),
        "a file no persona names is not prompt text"
    );
}

/// What a tool's own context gets: the files and homes, for the tools that read
/// the floor themselves.
#[test]
fn the_tools_context_gets_every_held_path() {
    let state = Path::new("/nonexistent/prompt-floor/state");
    let c = cfg(state, &["/nonexistent/prompt-floor/ctx/CONTEXT.md"], &[]);
    let f = PromptFloor::of(&c, state);
    let paths = f.paths();
    assert!(paths.contains(&at("/nonexistent/prompt-floor/ctx/CONTEXT.md")));
    assert!(paths.contains(&at("/nonexistent/prompt-floor/state/personas")));
    assert!(paths.contains(&at("/nonexistent/prompt-floor/state/persona")));
}
