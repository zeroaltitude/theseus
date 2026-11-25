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

/// What a change to a prompt file says (issue 23): the file, and how many lines
/// it adds and removes; for an undo of change `undoing`, what it puts back. The
/// path is written out so no character in it can hide, and goes between
/// backticks.
pub fn file(path: &str, added: usize, removed: usize, undoing: Option<u32>) -> String {
    let path = super::shown::escape_line(path).0;
    let n = |k: usize, one, many| crate::narrative::count(k as u64, one, many);
    let lines = format!(
        "{} added, {} removed",
        n(added, "line", "lines"),
        n(removed, "line", "lines")
    );
    match undoing {
        Some(n) => format!("undo change #{n}, putting back the prompt file `{path}` ({lines})"),
        None => format!("change the prompt file `{path}` ({lines}); no restart"),
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
    // plain cut: a commit id's first eight characters
    let c8: String = commit.chars().take(8).collect();
    format!("install the build of commit `{c8}` and restart Theseus")
}

/// What a yes from words (the model's answer tool) is told for a settings
/// question: how it is answered instead.
pub const BY_PRESS: &str = "a settings card is answered by its own button, or with `theseus answer <id> approve` \
     in the owner's own terminal: it is never answered in their name from a conversation. Nothing changed";

/// What a yes from words is told for a call the floor holds.
pub const FLOOR_BY_PRESS: &str = "a call held by the floor (it reaches a protected file or program) is approved by its own button, or with \
     `theseus answer <id> approve` in the owner's own terminal: it is never approved in their name from a conversation. Nothing ran";

/// What a yes from an agent's process (on the CLI or the web socket, not typed
/// words) is told for a call the floor holds.
pub const FLOOR_BY_AGENT: &str = "an agent's process cannot approve a call held by the floor (it reaches a protected file or program); \
     the owner approves it by its own button, or with `theseus answer <id> approve` in the owner's own terminal. Nothing ran";

/// What a yes from words is told for the card of `profile.hold`: the owner's yes to it is the
/// owner's word for that hold, so it is their own press.
pub const HOLD_BY_PRESS: &str = "the card that holds a conversation to a profile is answered by its own button, or with \
     `theseus answer <id> approve` in the owner's own terminal: it is never answered in their name from a conversation. Nothing was done";

/// What a yes from an agent's process is told for the card of `profile.hold`.
pub const HOLD_BY_AGENT: &str = "an agent's process cannot answer the card that holds a conversation to a profile; \
     the owner answers it by its own button, or with `theseus answer <id> approve` in the owner's own terminal. Nothing was done";

/// Why a yes from words (the model's answer tool) is refused for the waiting
/// question `q`, or none: a settings question or a call held by the floor is the
/// owner's own press, whatever else it is. Every other kind (a spend limit, a
/// task's question, a question with options, a reminder, an ordinary approval)
/// is answered from chat as it always was.
pub fn by_press_only(q: &theseus_protocol::ConfirmRequest) -> Option<&'static str> {
    if super::vault_keep::is_keep_request(q) {
        // A secret to keep takes a value, not a yes: `BY_PRESS` would send the owner to an Approve it refuses.
        Some(super::vault_keep::KEEP_BY_PRESS)
    } else if q.plain.as_ref().is_some_and(|p| !p.detail.is_empty()) {
        Some(BY_PRESS)
    } else if q.tool == crate::profile_tool::HOLD {
        Some(HOLD_BY_PRESS)
    } else if q.floor {
        Some(FLOOR_BY_PRESS)
    } else {
        None
    }
}

/// Whether `a` is a settings question (a change, an undo, a server, a restart or
/// an install).
pub fn is_settings_action(a: &theseus_kernel::Action) -> bool {
    a.tool == super::ACK || a.tool == crate::rsi::join::root_install::ACK
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
    let file = args["kind"] == super::prompt_file::KIND;
    let keep = super::vault_keep::is_card(args);
    let (risk, yes) = match (args.get("merge").is_some(), args["restart_only"] == true) {
        _ if keep => (
            "stores a secret in Theseus's vault",
            "is not offered: this card takes a value, which you type in your terminal or the cockpit, and the daemon keeps it in the vault with its own account.",
        ),
        (true, _) => (
            "installs a program Theseus runs",
            "installs that build and restarts Theseus.",
        ),
        (_, true) => ("restarts Theseus", "restarts Theseus."),
        _ if file => (
            "changes the text that goes into every later prompt",
            "has the daemon write the file itself; it takes effect with the next message.",
        ),
        _ => (
            "changes Theseus's own settings",
            "has the daemon write these settings itself, and restart if it says so.",
        ),
    };
    Some(PlainWords {
        who: "Theseus".into(),
        what: said,
        why: String::new(),
        held: match keep {
            true => "it stores a secret, which only you give".into(),
            false => "it changes what Theseus is or runs, so only you may allow it".into(),
        },
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

    /// The model is told what waits as the change in words, so a yes from the
    /// owner is judged against what the card says, and never against a tool's
    /// name.
    #[test]
    fn the_answer_tool_names_a_settings_question_by_its_change() {
        let args = json!({
            "question": "Change 1 setting?\n- narrative: off → on (after a restart)\n",
            "settled": "change `narrative` from off to on; Theseus then restarts",
        });
        let q = theseus_protocol::ConfirmRequest {
            correlation_id: "act_00000000deadbeef".into(),
            tool: super::super::ACK.into(),
            plain: plain_of(&args),
            ..Default::default()
        };
        let named = crate::chat_acts::named_question(&q);
        assert_eq!(
            named,
            "`config.ack` — change `narrative` from off to on; Theseus then restarts (…deadbeef)"
        );
    }

    /// Only a settings question and a floor hold are the owner's own press;
    /// every other kind is answered from chat as it always was.
    #[test]
    fn only_settings_and_floor_holds_are_by_press() {
        use theseus_protocol::{BudgetAsk, ConfirmRequest};
        let settings = plain_of(&json!({"question": "Change 1 setting?\n"}));
        let ask = |f: &dyn Fn(&mut ConfirmRequest)| {
            let mut q = ConfirmRequest {
                tool: "fs.write".into(),
                ..Default::default()
            };
            f(&mut q);
            by_press_only(&q)
        };
        assert_eq!(ask(&|q| q.plain = settings.clone()), Some(BY_PRESS));
        assert_eq!(ask(&|q| q.floor = true), Some(FLOOR_BY_PRESS));
        assert_eq!(ask(&|_| {}), None, "an ordinary approval");
        assert_eq!(
            ask(&|q| {
                q.tool = "budget.reset".into();
                q.budget = Some(BudgetAsk::default());
            }),
            None,
            "a spend limit"
        );
        assert_eq!(
            ask(&|q| q.task = Some(Default::default())),
            None,
            "a task's question"
        );
        assert_eq!(
            ask(&|q| q.tool = "owner_ask".into()),
            None,
            "a question with options"
        );
        assert_eq!(ask(&|q| q.tool = "wake.at".into()), None, "a reminder");
    }
}
