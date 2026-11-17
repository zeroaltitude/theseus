//! A settings question in words, once, for every place that shows it (issue 22).
//!
//! The daemon words each settings question (a change, an undo, an attached or
//! removed server, a restart, an install) when it asks, from the same facts the
//! card is built from, and keeps the line in the question's arguments. A
//! surface reads it as the request's plain words (`ConfirmRequest::plain`):
//! `what` is the change in one line, and `detail` is the card whole. Nothing
//! here is the model's: its reason is on the card, shown as its, and never in
//! this line.

use serde_json::Value;
use theseus_protocol::PlainWords;

use super::diff::{self, Setting};
use super::edit;
use crate::config::sensitivity;

/// The most characters the line may take before it names the settings and
/// leaves their values to the card.
pub const ROOM: usize = 1_200;

/// The line a plain restart is settled with.
pub const RESTART: &str = "restart Theseus, with no change to its settings";

/// A value in the line: its words, and "its default" for one not set.
fn value(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "its default".into(),
        v => diff::words(v),
    }
}

/// What a change to `settings` says, each setting with its value before and
/// after; for an undo of change `undoing`, what it puts back. A line that
/// would pass [`ROOM`] names the settings and says the card has each value.
pub fn change(settings: &[Setting], undoing: Option<u32>) -> String {
    let restart = settings.iter().any(|s| sensitivity::needs_restart(&s.path));
    let tail = match restart {
        true => "; Theseus then restarts",
        false => "; no restart",
    };
    let head = match undoing {
        Some(n) => format!("undo change #{n}, putting back"),
        None => "change".into(),
    };
    let n = crate::narrative::count(settings.len() as u64, "setting", "settings");
    let each: Vec<String> = settings
        .iter()
        .map(|s| {
            format!(
                "`{}` from {} to {}",
                edit::dotted(&s.path),
                value(s.before.as_ref()),
                value(s.after.as_ref())
            )
        })
        .collect();
    let whole = match settings.len() {
        1 => format!("{head} {}{tail}", each[0]),
        _ => format!("{head} {n}: {}{tail}", each.join("; ")),
    };
    if whole.chars().count() <= ROOM {
        return whole;
    }
    let names: Vec<String> = settings
        .iter()
        .map(|s| format!("`{}`", edit::dotted(&s.path)))
        .collect();
    let named = format!(
        "{head} {n} ({}); the values are on the card{tail}",
        names.join(", ")
    );
    match named.chars().count() <= ROOM {
        true => named,
        false => format!("{head} {n}; the settings and values are on the card{tail}"),
    }
}

/// What attaching or removing an MCP server says, from the marker the daemon
/// keeps on the question: `op` and the server's `name`.
pub fn mcp(marker: &Value, restart: bool) -> String {
    let name = marker["name"].as_str().unwrap_or("a server");
    let verb = match marker["op"].as_str() {
        Some("detach") => "remove the MCP server",
        _ => "attach the MCP server",
    };
    let then = match restart {
        true => "; Theseus then restarts",
        false => "; no restart",
    };
    format!("{verb} `{name}`{then}")
}

/// What installing a joined change says: the commit, which is all the card
/// and the helper agree on.
pub fn install(commit: &str) -> String {
    let c8: String = commit.chars().take(8).collect();
    format!("install the build of commit `{c8}` and restart Theseus")
}

/// The first line of a card's question, as the line for a question asked
/// before the daemon kept its own (one in flight across an upgrade).
fn first_line(question: &str) -> String {
    let line = question.lines().next().unwrap_or("").trim();
    let line = line.trim_end_matches('?');
    let mut chars = line.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => "change Theseus's settings".into(),
    }
}

/// The plain words of a settings question from its arguments: the line the
/// daemon kept (`settled`), and the card whole (`question`). A question of
/// another kind has none of the two, and gets no words from here.
pub fn plain_of(args: &Value) -> Option<PlainWords> {
    let question = args["question"].as_str()?.to_string();
    let said = args["settled"]
        .as_str()
        .map_or_else(|| first_line(&question), str::to_string);
    let (risk, yes) = match (args.get("merge").is_some(), args["restart_only"] == true) {
        (true, _) => (
            "installs a program Theseus runs",
            "installs that build and restarts Theseus.",
        ),
        (_, true) => ("restarts Theseus", "restarts Theseus."),
        _ => (
            "changes Theseus's own settings",
            "has the daemon write these settings itself, and restart if it says so.",
        ),
    };
    Some(PlainWords {
        who: "Theseus".into(),
        what: said,
        why: String::new(),
        held: "it changes what Theseus is or runs, so only you may allow it".into(),
        risk: risk.into(),
        yes: yes.into(),
        no: "changes nothing.".into(),
        detail: question,
    })
}

/// What `theseus answer` says back after the owner's answer to a settings
/// question: what was approved or declined, in the daemon's own words. None
/// for a question of any other kind.
pub fn echo(a: &theseus_kernel::Action, approved: bool) -> Option<String> {
    if a.tool != super::ACK && a.tool != crate::rsi::join::root_install::ACK {
        return None;
    }
    let p = plain_of(&a.proposal.as_ref()?.args)?;
    let how = if approved { "approved" } else { "declined" };
    Some(format!("{how}: {}", p.what))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn s(path: &str, before: Option<Value>, after: Option<Value>) -> Setting {
        Setting {
            path: edit::parse_path(path).unwrap(),
            before,
            after,
        }
    }

    #[test]
    fn one_setting_says_before_and_after() {
        let line = change(
            &[s(
                "discord.task_channel_cap",
                Some(json!(25)),
                Some(json!(26)),
            )],
            None,
        );
        assert_eq!(
            line,
            "change `discord.task_channel_cap` from 25 to 26; Theseus then restarts"
        );
        let unset = change(&[s("narrative", None, Some(json!(true)))], None);
        assert!(unset.contains("from its default to on"), "{unset}");
    }

    #[test]
    fn several_settings_and_an_undo_say_each_one() {
        let a = [
            s("narrative", Some(json!(false)), Some(json!(true))),
            s("tui.theme", None, Some(json!("dark"))),
        ];
        let line = change(&a, Some(3));
        assert!(
            line.starts_with("undo change #3, putting back 2 settings: "),
            "{line}"
        );
        assert!(
            line.contains("`narrative` from off to on; `tui.theme` from its default to \"dark\"")
        );
    }

    /// A change that would not fit the room names its settings, and says the
    /// card has the values: never a value cut in the middle.
    #[test]
    fn a_long_line_names_the_settings_and_sends_the_reader_to_the_card() {
        let big = "v".repeat(400);
        let many: Vec<Setting> = (0..12)
            .map(|i| s(&format!("memory.k{i}"), Some(json!(big)), Some(json!(big))))
            .collect();
        let line = change(&many, None);
        assert!(line.chars().count() <= ROOM, "{}", line.chars().count());
        assert!(line.contains("`memory.k11`") && line.contains("the values are on the card"));
        assert!(!line.contains(&big[..50]));
    }

    #[test]
    fn the_plain_words_carry_the_card_whole_and_never_the_models_reason() {
        let args = json!({
            "question": "Change 1 setting?\n- narrative: off → on (after a restart)\nTheseus's reason, in its own words: \"because\"\n",
            "settled": "change `narrative` from off to on; Theseus then restarts",
            "reason": "because",
        });
        let p = plain_of(&args).unwrap();
        assert!(
            p.detail.starts_with("Change 1 setting?\n- narrative"),
            "{p:?}"
        );
        assert!(!p.what.contains("because") && p.why.is_empty());
        // A question asked before the line was kept gets its first line.
        let old = plain_of(&json!({"question": "Undo change #2?\nx"})).unwrap();
        assert_eq!(old.what, "undo change #2");
        assert!(plain_of(&json!({})).is_none());
    }

    #[test]
    fn the_other_kinds_say_their_own_thing() {
        assert_eq!(
            mcp(&json!({"op": "attach", "name": "docs"}), false),
            "attach the MCP server `docs`; no restart"
        );
        assert!(mcp(&json!({"op": "detach", "name": "docs"}), true)
            .starts_with("remove the MCP server `docs`"));
        assert_eq!(
            install("0123456789abcdef"),
            "install the build of commit `01234567` and restart Theseus"
        );
        let restart = plain_of(
            &json!({"question": "Restart Theseus?\n", "settled": RESTART, "restart_only": true}),
        )
        .unwrap();
        assert_eq!(restart.risk, "restarts Theseus");
    }
}
