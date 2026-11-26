use serde_json::json;

use super::*;

/// The note is headed, names where the removal was asked and the setting that
/// let it run, and says how to have the server back.
#[test]
fn the_note_says_what_was_removed_where_and_how_to_have_it_back() {
    let d = detach_note(
        &json!({"asked_by": {"place": "discord:dm:7"}}),
        "\"wc\"",
        "enforcement = notify",
    );
    assert!(crate::owner_note::is_headed(&d), "{d}");
    assert_eq!(
        d,
        "ℹ️ FYI · Removed the MCP server \"wc\"\n\
         asked in discord:dm:7; mcp.detach ran with no card (enforcement = notify)\n\
         It stopped, and its tools are gone from your next message.\n\
         To have it back, ask Theseus to attach it again: attaching a server asks you, as it \
         always has."
    );
}
