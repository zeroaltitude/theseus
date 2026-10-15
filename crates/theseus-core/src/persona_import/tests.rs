use std::path::{Path, PathBuf};

use theseus_protocol::{PersonaImportParams, StandingKind};

use super::run::{check_name, digest, import};
use super::{card, feedback, split, Manifest};

fn fixture(agent: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agents")
        .join(agent)
}

fn names() -> Vec<String> {
    crate::config::PersonaSettings::default().import_files
}

fn params(agent: &str, name: &str, dry: bool) -> PersonaImportParams {
    PersonaImportParams {
        from: fixture(agent).display().to_string(),
        name: name.into(),
        agent: None,
        dry_run: dry,
    }
}

fn run(state: &Path, agent: &str, name: &str, dry: bool) -> theseus_protocol::PersonaImportResult {
    import(state, &params(agent, name, dry), &names(), 64 * 1024, 1_000)
        .unwrap()
        .0
}

/// The carried files are the workspace's bytes: CRLF, curly quotes, a missing
/// trailing newline and all. The importer never reformats.
#[test]
fn a_carried_file_is_the_workspaces_bytes() {
    let state = tempfile::tempdir().unwrap();
    let r = run(state.path(), "wren", "wren", false);
    for rel in [
        "SOUL.md",
        "IDENTITY.md",
        "USER.md",
        "AGENTS.md",
        "MEMORY.md",
    ] {
        let a = std::fs::read(fixture("wren").join(rel)).unwrap();
        let b = std::fs::read(Path::new(&r.dir).join(rel)).unwrap();
        assert_eq!(a, b, "{rel} was changed in the copy");
    }
    let soul = std::fs::read(Path::new(&r.dir).join("SOUL.md")).unwrap();
    assert!(soul.windows(2).any(|w| w == b"\r\n") && !soul.ends_with(b"\n"));
    // The curated notes ride along and are not compiled.
    assert!(r
        .files
        .iter()
        .any(|f| f.rel == "memory/lessons.md" && f.note));
    assert_eq!(r.context_paths.len(), 5, "{:?}", r.context_paths);
    assert!(r.context_paths.iter().all(|p| !p.contains("/memory/")));
    assert_eq!(r.version, 1);
}

/// A dry run says everything and writes nothing.
#[test]
fn a_dry_run_writes_nothing() {
    let state = tempfile::tempdir().unwrap();
    let r = run(state.path(), "wren", "wren", true);
    assert!(r.dry_run && !r.files.is_empty() && r.card.is_some());
    assert!(!state.path().join("personas").exists());
}

/// A second import of the same bytes is no new version; changed bytes are
/// version 2, and the files they replace are kept aside whole.
#[test]
fn a_later_import_is_a_new_version_and_the_old_bytes_are_kept() {
    let state = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    for f in ["SOUL.md", "IDENTITY.md"] {
        std::fs::copy(fixture("wren").join(f), ws.path().join(f)).unwrap();
    }
    let p = PersonaImportParams {
        from: ws.path().display().to_string(),
        name: "wren".into(),
        ..Default::default()
    };
    let one = import(state.path(), &p, &names(), 64 * 1024, 1).unwrap().0;
    assert_eq!(one.version, 1);
    let again = import(state.path(), &p, &names(), 64 * 1024, 2).unwrap().0;
    assert!(again.unchanged && again.version == 1);
    let old = std::fs::read(Path::new(&one.dir).join("SOUL.md")).unwrap();
    std::fs::write(ws.path().join("SOUL.md"), b"# Soul\n\nA new line.\n").unwrap();
    let two = import(state.path(), &p, &names(), 64 * 1024, 3).unwrap().0;
    assert_eq!(two.version, 2);
    assert!(two.files.iter().any(|f| f.rel == "SOUL.md" && f.changed));
    assert_eq!(
        std::fs::read(Path::new(&one.dir).join(".versions/1/SOUL.md")).unwrap(),
        old
    );
    assert_eq!(
        std::fs::read(Path::new(&one.dir).join("SOUL.md")).unwrap(),
        b"# Soul\n\nA new line.\n"
    );
    assert_eq!(Manifest::read(Path::new(&one.dir)).unwrap().version, 2);
}

/// An over-limit file is cut at headings and the parts joined are the file.
#[test]
fn an_over_limit_file_is_split_verbatim_at_its_headings() {
    let mut text = String::from("# Top\r\n\r\nintro\r\n");
    for i in 0..40 {
        text.push_str(&format!(
            "\r\n## Section {i}\r\n\r\n{}\r\n",
            "words ".repeat(30)
        ));
    }
    let parts = split::parts(&text, 1000);
    assert!(parts.len() > 3);
    assert_eq!(parts.concat(), text, "the parts are not the file");
    for p in &parts {
        assert!(p.len() <= 1000, "{}", p.len());
    }
    // Every part but the first begins at a heading.
    for p in &parts[1..] {
        assert!(
            p.starts_with("\r\n## ") || p.starts_with("## "),
            "{:?}",
            &p[..20]
        );
    }
    // One line longer than the limit is cut at a character boundary.
    let long = "é".repeat(700);
    let parts = split::parts(&long, 1000);
    assert_eq!(parts.concat(), long);
    assert!(parts.iter().all(|p| p.len() <= 1000));
}

#[test]
fn the_importer_splits_a_big_file_and_reports_it() {
    let state = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::fs::copy(
        fixture("wren").join("IDENTITY.md"),
        ws.path().join("IDENTITY.md"),
    )
    .unwrap();
    let mut big = String::from("# Rules\n");
    for i in 0..60 {
        big.push_str(&format!("\n## Rule {i}\n\n{}\n", "text ".repeat(300)));
    }
    std::fs::write(ws.path().join("AGENTS.md"), &big).unwrap();
    let p = PersonaImportParams {
        from: ws.path().display().to_string(),
        name: "big".into(),
        ..Default::default()
    };
    let r = import(state.path(), &p, &names(), 64 * 1024, 1).unwrap().0;
    assert_eq!(r.over_limit.len(), 1);
    assert_eq!(r.over_limit[0].rel, "AGENTS.md");
    let parts: Vec<_> = r.files.iter().filter(|f| f.part_of.is_some()).collect();
    assert_eq!(parts.len() as u32, r.over_limit[0].parts);
    assert!(parts.iter().all(|f| f.bytes <= 64 * 1024));
    let joined: Vec<u8> = parts
        .iter()
        .flat_map(|f| std::fs::read(&f.path).unwrap())
        .collect();
    assert_eq!(joined, big.as_bytes());
}

/// The draft is the identity's role line and the agents file's headings,
/// the same every time, and the importer says it is a draft.
#[test]
fn the_card_draft_is_deterministic_and_marked_a_draft() {
    let id = std::fs::read_to_string(fixture("wren").join("IDENTITY.md")).unwrap();
    let ag = std::fs::read_to_string(fixture("wren").join("AGENTS.md")).unwrap();
    let a = card::draft(Some(&id), Some(&ag)).unwrap();
    assert_eq!(a, card::draft(Some(&id), Some(&ag)).unwrap());
    assert_eq!(
        a,
        "Tidal ecology field notes and sailing logistics for the Harbor Survey. \
         Areas: Operating notes; Tide tables; Species logs; Weather windows."
    );
    assert!(!a.contains("not a heading"), "a fenced line is no heading");
    assert!(card::draft(None, None).is_none());
    assert_eq!(
        card::role_line("Role: Bird counts and migration timing\n").unwrap(),
        "Bird counts and migration timing"
    );
    let state = tempfile::tempdir().unwrap();
    let r = run(state.path(), "wren", "wren", false);
    assert_eq!(r.card.as_deref(), Some(a.as_str()));
    let m = Manifest::read(Path::new(&r.dir)).unwrap();
    assert!(m.card_is_draft(Some(&a)), "the config's card is the draft");
    assert!(!m.card_is_draft(Some("Edited by the owner.")));
}

/// Feedback becomes proposals; the importer makes no live record (it has no
/// way to: it returns the proposals and the daemon files them).
#[test]
fn feedback_items_become_proposals_by_shape() {
    let state = tempfile::tempdir().unwrap();
    let (r, props) = import(
        state.path(),
        &params("wren", "wren", false),
        &names(),
        64 * 1024,
        1,
    )
    .unwrap();
    let texts: Vec<_> = props.iter().map(|p| (p.kind, p.text.as_str())).collect();
    assert!(
        texts.contains(&(
            StandingKind::Order,
            "Never pad an answer with a recap of the question."
        )),
        "{texts:?}"
    );
    assert!(texts.contains(&(
        StandingKind::Order,
        "Always give tide heights in metres, not feet."
    )));
    assert!(texts.contains(&(
        StandingKind::Preference,
        "Prefer one clear table over three paragraphs of prose when comparing sites."
    )));
    assert_eq!(props.len(), 3, "{texts:?}");
    assert!(!texts.iter().any(|(_, t)| t.contains("not feedback")));
    assert_eq!(r.proposals.len(), 3);
    // The ids are stable.
    let again = import(
        state.path(),
        &params("wren", "wren", true),
        &names(),
        64 * 1024,
        2,
    )
    .unwrap()
    .1;
    assert_eq!(again, props);
    // Another persona's proposal of the same words is its own.
    assert_ne!(
        feedback::id_of("wren", StandingKind::Order, "x"),
        feedback::id_of("finch", StandingKind::Order, "x")
    );
}

#[test]
fn feedback_too_long_to_be_a_record_is_counted_not_cut() {
    let text = format!("---\ntype: feedback\n---\n\n{}\n", "word ".repeat(300));
    let f = feedback::of_note(&text);
    assert!(f.items.is_empty() && f.too_long == 1);
}

#[test]
fn a_bad_name_or_an_empty_workspace_is_refused() {
    assert!(check_name("Wren").is_err() && check_name("a/b").is_err() && check_name("").is_err());
    assert!(check_name("wren-2_x").is_ok());
    let state = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    let p = PersonaImportParams {
        from: empty.path().display().to_string(),
        name: "x".into(),
        ..Default::default()
    };
    let e = import(state.path(), &p, &names(), 64 * 1024, 1).unwrap_err();
    assert!(format!("{e:#}").contains("none of the persona files"));
    assert_eq!(digest(b"a").len(), 16);
}
