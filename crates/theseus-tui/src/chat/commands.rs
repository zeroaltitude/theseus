//! The chat's commands: `/help`, `/quit`, and the rows to come. A command is
//! one entry of [`COMMANDS`] (its name, its one line of help, its handler),
//! so a later row adds a line here and its handler in a module of its own,
//! without touching another's entry. `/` in the input line lists them,
//! filtered as the name is typed.

use super::state::{Chat, Effect};

pub struct Command {
    pub name: &'static str,
    /// One line, in the list.
    pub help: &'static str,
    /// Runs with the words after the name.
    pub run: fn(&mut Chat, &str) -> Vec<Effect>,
}

/// Every command, in the order `/` lists them.
pub const COMMANDS: &[Command] = &[
    Command {
        name: "help",
        help: "what you can do here",
        run: help,
    },
    Command {
        name: "history",
        help: "print the earlier turns the reopening folded",
        run: |chat, args| chat.earlier_cmd(args),
    },
    Command {
        name: "quit",
        help: "leave; the conversation goes on",
        run: quit,
    },
    // Debug builds only: the test that proves the terminal comes back after a
    // panic needs one to happen.
    #[cfg(debug_assertions)]
    Command {
        name: "panic",
        help: "debug builds: panic, to prove the terminal is put back",
        run: |_, _| panic!("/panic: a debug build's proof that the terminal comes back"),
    },
];

/// The commands whose names begin with `prefix`, in order.
pub fn matching(prefix: &str) -> Vec<&'static Command> {
    COMMANDS
        .iter()
        .filter(|c| c.name.starts_with(prefix))
        .collect()
}

/// Run the command named `name`; None when there is none.
pub fn run(chat: &mut Chat, name: &str, args: &str) -> Option<Vec<Effect>> {
    let c = COMMANDS.iter().find(|c| c.name == name)?;
    Some((c.run)(chat, args))
}

/// The keys and the commands, as `?` and `/help` print them.
pub fn help_lines() -> Vec<String> {
    let mut out = vec![
        "enter sends · ctrl+j (or \\ then enter) adds a line · ↑/↓ your earlier messages"
            .to_string(),
        "ctrl+o shows a folded paste · ctrl+c clears the line, twice leaves".to_string(),
        "y / n answer a question with the line empty".to_string(),
    ];
    let wide = COMMANDS.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for c in COMMANDS {
        out.push(format!("/{:wide$}  {}", c.name, c.help));
    }
    out
}

fn help(chat: &mut Chat, _args: &str) -> Vec<Effect> {
    chat.gap();
    for l in help_lines() {
        chat.say_plain(super::style::Sty::Dim, &l);
    }
    chat.gap();
    Vec::new()
}

fn quit(chat: &mut Chat, _args: &str) -> Vec<Effect> {
    chat.leave()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_filters_the_list_and_a_name_finds_its_command() {
        assert_eq!(matching("").len(), COMMANDS.len());
        assert_eq!(
            matching("he").iter().map(|c| c.name).collect::<Vec<_>>(),
            ["help"]
        );
        assert!(matching("zz").is_empty());
    }

    #[test]
    fn every_command_is_named_once_and_helped() {
        let mut names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len());
        assert!(COMMANDS
            .iter()
            .all(|c| !c.help.is_empty() && !c.name.contains(' ')));
    }
}
