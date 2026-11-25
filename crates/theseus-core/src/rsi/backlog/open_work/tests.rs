//! `self.open_work`'s own tests (issue 9). Every name is invented.

use super::*;

const LISTING: &str = r#"[
  {"number": 12, "title": "The beacon  blinks\ttwice", "state": "open",
   "labels": [{"name": "1.0.7"}]},
  {"number": 7, "title": "Tide tables lose a footnote", "state": "open",
   "labels": ["1.0.6"]},
  {"number": 9, "title": "Closed already", "state": "closed"},
  {"number": 10, "title": "A pull request", "state": "open", "pull_request": {"url": "x"}}
]"#;

fn config(file: &std::path::Path, label: &str) -> SelfIssuesConfig {
    SelfIssuesConfig {
        enabled: true,
        repo: "harbourworks/lanterns".into(),
        file: file.to_string_lossy().into_owned(),
        version_label: label.into(),
        ..SelfIssuesConfig::default()
    }
}

fn file_with(dir: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
    let p = dir.path().join("issues.json");
    std::fs::write(&p, body).unwrap();
    p
}

#[test]
fn the_list_names_each_open_issue_with_a_number_and_a_link_and_the_file_s_age() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(&file_with(&dir, LISTING), "");
    let now = theseus_protocol::now_unix_ms();
    let text = listing(&cfg, now).unwrap();
    assert!(text.contains("2 open in harbourworks/lanterns:"), "{text}");
    assert!(
        text.contains(
            "- #7: Tide tables lose a footnote (https://github.com/harbourworks/lanterns/issues/7)"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "- #12: The beacon blinks twice (https://github.com/harbourworks/lanterns/issues/12)"
        ),
        "a title is one line: {text}"
    );
    assert!(
        !text.contains("#9") && !text.contains("#10"),
        "a closed issue and a pull request are not open work: {text}"
    );
    assert!(
        text.starts_with("Theseus has not read the repository's issues since"),
        "{text}"
    );
    assert!(text.contains("(today)"), "{text}");
}

#[test]
fn an_old_file_says_how_old_and_the_version_label_narrows_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(&file_with(&dir, LISTING), "1.0.7");
    let later = theseus_protocol::now_unix_ms() + 3 * 86_400_000;
    let text = listing(&cfg, later).unwrap();
    assert!(text.contains("(3 days ago)"), "{text}");
    assert!(
        text.contains("1 open in harbourworks/lanterns labelled 1.0.7:"),
        "a narrowed list says so: {text}"
    );
    assert!(text.contains("#12") && !text.contains("#7:"), "{text}");
}

#[test]
fn a_missing_file_is_said_and_never_an_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(&dir.path().join("none.json"), "");
    let why = listing(&cfg, 0).unwrap_err();
    assert!(
        why.contains("has not read the repository's issues yet"),
        "{why}"
    );
    assert!(why.contains("do not look for them anywhere else"), "{why}");
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(&file_with(&dir, "[]"), "");
    let text = listing(&cfg, theseus_protocol::now_unix_ms()).unwrap();
    assert!(
        text.contains("No issue of harbourworks/lanterns was open"),
        "{text}"
    );
}

#[test]
fn it_is_offered_whenever_the_source_is_on_whatever_the_loop_mode_is() {
    let mut cfg = crate::Config::example();
    let names = |c: &crate::Config| {
        let mut r = theseus_tools::Registry::new();
        super::register(c, &mut r);
        r.all().map(|t| t.name().to_string()).collect::<Vec<_>>()
    };
    assert!(names(&cfg).is_empty(), "off by default: no catalog moves");
    cfg.self_issues.enabled = true;
    assert_eq!(names(&cfg), vec![OPEN_WORK]);
    cfg.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
    assert!(names(&cfg).contains(&OPEN_WORK.to_string()));
}

#[test]
fn its_description_sends_a_reader_to_the_repository_and_away_from_any_other_tracker() {
    let t = OpenWork {
        cfg: SelfIssuesConfig::default(),
    };
    let d = t.description();
    assert!(
        d.contains("repository's issues") && d.contains("no other tracker applies"),
        "{d}"
    );
    assert_eq!(t.class(), ToolClass::Read);
    assert_eq!(t.input_schema()["additionalProperties"], json!(false));
}

#[test]
fn the_list_runs_oldest_first_and_a_long_title_is_cut_at_a_word() {
    let dir = tempfile::tempdir().unwrap();
    let long = format!("{}extraordinarily", "word ".repeat(23));
    let body = format!(
        r#"[{{"number": 30, "title": "{long} tail", "state": "open"}},
            {{"number": 3, "title": "First", "state": "open"}}]"#
    );
    let cfg = config(&file_with(&dir, &body), "");
    let text = listing(&cfg, theseus_protocol::now_unix_ms()).unwrap();
    assert!(
        text.find("- #3: First").unwrap() < text.find("- #30:").unwrap(),
        "{text}"
    );
    assert!(
        text.contains(&format!("- #30: {}… (", "word ".repeat(23).trim_end())),
        "stops at the last whole word: {text}"
    );
    assert!(!text.contains("extraord"), "{text}");
}

/// The registration is the line `rsi::backlog::tool::register` carries, so
/// this builds the runtime the chat's registry comes from: the tool is in it
/// with the source on (whatever the loop's mode), not with it off, and a
/// shared place is never offered it.
#[test]
fn the_chat_s_registry_holds_it_with_the_source_on_and_a_shared_place_never_does() {
    use crate::places::{offered, PlaceClass};
    use crate::secrets::SecretBoard;
    use crate::toolrun::{build_runtime, InlineLauncher};
    let registry = |on: bool| {
        let mut cfg = crate::Config::example();
        cfg.self_issues.enabled = on;
        let board = SecretBoard::new(cfg.secrets.keys().cloned(), std::time::Instant::now());
        build_runtime(&cfg, None, Arc::default(), Arc::new(InlineLauncher), board).unwrap()
    };
    assert!(registry(false).registry.get(OPEN_WORK).is_none());
    assert!(registry(true).registry.get(OPEN_WORK).is_some());
    assert!(offered(PlaceClass::Private, OPEN_WORK, &[]));
    assert!(!offered(PlaceClass::Shared, OPEN_WORK, &[]));
}

/// A title cut where it is long stops before a value the board knows.
#[test]
fn a_long_title_line_stops_before_a_known_value() {
    use crate::tests_cut_sites::{whole_or_absent, V};
    let board = crate::secrets::SecretBoard::empty();
    board.hold("cut_test", crate::secrets::Secret::new(V.into()));
    let line = one_line(&format!("{}{V} again", "t".repeat(TITLE_CHARS - 10)));
    assert!(line.ends_with('…'), "{line}");
    whole_or_absent("the listed title", &line, V);
}
