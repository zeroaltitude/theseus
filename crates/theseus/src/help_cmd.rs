//! `theseus help`: the everyday screen, `help all` for every command, `help COMMAND` for one's page.
//!
//! clap lists a command unless it is hidden, and the commands that are not everyday are hidden (they parse, run and
//! have their `--help`). `help all` is clap's tree read again with `hide` ignored, so a new command cannot be left
//! out of it.

use clap::{Command, CommandFactory};

use crate::Cli;

/// The commands `theseus help` shows, in the order it shows them (the `display_order` of each in `main.rs`).
#[cfg(test)]
pub const EVERYDAY: [&str; 19] = [
    "ask", "chat", "tui", "history", "watch", "status", "tasks", "answer", "stop", "agenda",
    "digest", "remind", "routine", "wake", "project", "thread", "places", "profile", "health",
];

/// What one line of `help all` says of a command: its about's first sentence, without its parentheses, cut at a
/// colon when what comes before is a phrase, and clipped at a word.
fn line(cmd: &Command) -> String {
    let about = cmd.get_about().map(ToString::to_string).unwrap_or_default();
    let (mut plain, mut depth) = (String::new(), 0);
    for c in about.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    let plain = plain
        .replace(" ,", ",")
        .replace(" :", ":")
        .replace("  ", " ");
    let plain = plain.trim();
    let sentence = plain.split(". ").next().unwrap_or("").trim_end_matches('.');
    let first = match sentence.split_once(": ") {
        Some((phrase, _)) if phrase.len() >= 24 => phrase,
        _ => sentence,
    };
    match first.char_indices().nth(72) {
        Some((cut, _)) => format!(
            "{}…",
            first[..cut]
                .rsplit_once(' ')
                .map_or(&first[..cut], |(w, _)| w)
        ),
        None => first.trim_end().to_string(),
    }
}

/// Every command, one line each: the everyday screen's, then the rest, each in its listed order.
pub fn all() -> String {
    let mut root = Cli::command();
    root.build();
    let (everyday, rest): (Vec<&Command>, Vec<&Command>) = {
        let mut subs: Vec<&Command> = root.get_subcommands().collect();
        subs.sort_by_key(|c| c.get_display_order());
        subs.into_iter().partition(|c| !c.is_hide_set())
    };
    let width = everyday
        .iter()
        .chain(&rest)
        .map(|c| c.get_name().len())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (title, group) in [("Everyday:", everyday), ("The rest:", rest)] {
        out.push_str(title);
        out.push('\n');
        let mut group = group;
        if title == "The rest:" {
            group.sort_by_key(|c| c.get_name());
        }
        for c in group {
            out.push_str(&format!("  {:width$}  {}\n", c.get_name(), line(c)));
        }
        out.push('\n');
    }
    out.push_str("`theseus COMMAND --help` is a command's page.\n");
    out
}

/// Print the page `theseus help TOPIC…` names; the exit code (2 for a command that is not one).
pub fn run(topic: &[&str]) -> i32 {
    if topic == ["all"] {
        print!("{}", all());
        return 0;
    }
    let mut cmd = Cli::command();
    cmd.build();
    for name in topic {
        match cmd.find_subcommand_mut(name) {
            Some(sub) => cmd = sub.clone(),
            None => {
                eprintln!("theseus: no command `{name}`; `theseus help all` lists every command");
                return 2;
            }
        }
    }
    let _ = cmd.print_long_help();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names `theseus help` lists, from the page clap renders.
    fn listed(page: &str) -> Vec<String> {
        page.lines()
            .skip_while(|l| *l != "Commands:")
            .skip(1)
            .take_while(|l| l.starts_with("  "))
            .filter(|l| !l.starts_with("   "))
            .filter_map(|l| l.split_whitespace().next().map(str::to_string))
            .collect()
    }

    #[test]
    fn the_everyday_help_lists_exactly_the_everyday_commands() {
        let page = Cli::command().render_help().to_string();
        assert_eq!(listed(&page), EVERYDAY, "{page}");
        assert!(
            page.trim_end()
                .lines()
                .last()
                .is_some_and(|l| l == "`theseus help all` lists every command."),
            "{page}"
        );
    }

    #[test]
    fn help_all_lists_every_command() {
        let page = all();
        let names: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        assert!(names.len() > EVERYDAY.len());
        let have: Vec<String> = page
            .lines()
            .filter(|l| l.starts_with("  ") && !l.starts_with("   "))
            .filter_map(|l| l.split_whitespace().next().map(str::to_string))
            .collect();
        for name in &names {
            assert!(
                have.contains(name),
                "`help all` leaves out `{name}`:\n{page}"
            );
        }
        assert_eq!(have.len(), names.len(), "{page}");
    }

    #[test]
    fn a_hidden_command_still_has_its_page() {
        assert_eq!(run(&["ledger"]), 0);
        assert_eq!(run(&["no-such"]), 2);
    }

    /// Earned autonomy and scoped grants are gone: `policy` has no `earned`,
    /// `revoke` or `grants`, and its help says nothing of either.
    #[test]
    fn policy_has_no_earned_autonomy_or_grants() {
        use clap::Parser;
        for line in [
            "theseus policy earned",
            "theseus policy earned accept ern_00000001",
            "theseus policy earned --propose-now",
            "theseus policy revoke sgr_0a1b2c3d",
            "theseus policy grants",
        ] {
            assert!(
                Cli::try_parse_from(line.split_whitespace()).is_err(),
                "`{line}` still parses"
            );
        }
        let mut root = Cli::command();
        let policy = root.find_subcommand_mut("policy").expect("policy");
        let names: Vec<&str> = policy.get_subcommands().map(|c| c.get_name()).collect();
        assert!(
            !names
                .iter()
                .any(|n| ["earned", "revoke", "grants"].contains(n)),
            "{names:?}"
        );
        let help = policy.render_long_help().to_string();
        assert!(!help.to_lowercase().contains("earned"), "{help}");
    }
}
