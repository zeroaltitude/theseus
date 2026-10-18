//! The help a user reads first: no command's one-line purpose carries an internal issue or milestone id, and
//! `docs/cli.md` names every top-level command that `theseus help` lists.

use std::process::Command;

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// `theseus <path> --help`'s text; `tui` and `chat` hand theirs to `theseus-tui`, which may not be built beside
/// this binary, and have no subcommands to walk.
fn help(path: &[String]) -> String {
    let out = Command::new(THESEUS)
        .args(path)
        .arg("--help")
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

/// A help page's `Commands:` rows as (name, one-line purpose).
fn commands(text: &str) -> Vec<(String, String)> {
    text.lines()
        .skip_while(|l| *l != "Commands:")
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| {
            let (name, about) = l.trim_start().split_once(' ')?;
            Some((name.to_string(), about.trim().to_string()))
        })
        .collect()
}

/// Every command at every depth, with its path and one-line purpose.
fn walk(path: &mut Vec<String>, found: &mut Vec<(String, String)>) {
    for (name, about) in commands(&help(path)) {
        if name == "help" {
            continue;
        }
        path.push(name);
        found.push((path.join(" "), about));
        walk(path, found);
        path.pop();
    }
}

/// Whether `text` holds `theseus-` and a 4 or 5 character issue id (`theseus-x9swu`, `theseus-slvl.6`), or a
/// milestone mark (`M6`, `M7 43a`). The binaries `theseus-tui` and `theseus-judge` are not ids.
fn internal_id(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for (at, _) in text.match_indices("theseus-") {
        let id: String = text[at + 8..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if (4..=5).contains(&id.len()) && id != "judge" {
            return Some(format!("theseus-{id}"));
        }
    }
    for (at, _) in text.match_indices('M') {
        let before = at.checked_sub(1).map(|i| bytes[i]);
        let digit = bytes.get(at + 1).is_some_and(u8::is_ascii_digit);
        if digit && !before.is_some_and(|b| b.is_ascii_alphanumeric()) {
            return Some(text[at..].chars().take(4).collect());
        }
    }
    None
}

#[test]
fn no_commands_one_line_help_carries_an_internal_id() {
    let mut found = Vec::new();
    walk(&mut Vec::new(), &mut found);
    assert!(found.len() > 150, "walked only {} commands", found.len());
    let bad: Vec<_> = found
        .iter()
        .filter_map(|(path, about)| internal_id(about).map(|id| format!("{path}: {id}")))
        .collect();
    assert!(bad.is_empty(), "internal ids in one-line help: {bad:#?}");
}

#[test]
fn the_cli_guide_names_every_top_level_command() {
    let guide =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/cli.md")).unwrap();
    let top = commands(&help(&[]));
    assert!(top.len() > 60, "found only {} commands", top.len());
    let missing: Vec<_> = top
        .iter()
        .filter(|(name, _)| !guide.contains(&format!("- **`{name}`**")))
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(missing.is_empty(), "docs/cli.md lacks: {missing:?}");
}

#[test]
fn the_id_finder_finds_the_ids_it_looks_for() {
    assert_eq!(
        internal_id("Open a turn (theseus-x9swu): its flame"),
        Some("theseus-x9swu".into())
    );
    assert_eq!(
        internal_id("Cards (theseus-slvl.6): lists"),
        Some("theseus-slvl".into())
    );
    assert!(internal_id("The index (M6): status").is_some());
    assert!(internal_id("Extensions (M7 43a, 43b)").is_some());
    assert_eq!(
        internal_id("Run `theseus-tui` or theseus-judge, in an MX or HTML"),
        None
    );
}
