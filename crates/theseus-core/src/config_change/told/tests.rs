use serde_json::json;

use super::*;
use crate::config_change::diff;

fn p(s: &str) -> Vec<String> {
    s.split('.').map(str::to_string).collect()
}

/// What a restart would make live and send: a secret, a key that reads one, an
/// address, a host or a base URL; not an ordinary setting.
#[test]
fn a_secret_an_address_and_a_host_reach_and_the_rest_does_not() {
    for s in [
        "secrets.search_key",
        "providers.lab.api_base",
        "providers.lab.api_key_secret",
        "mcp.servers.docs.url",
        "mcp.servers.docs.auth_secret",
        "mcp.servers.docs.egress",
        "web.proxy",
        "discord.rest_proxy",
        "index.host",
    ] {
        assert!(reaches(&p(s)), "{s}");
    }
    for s in [
        "import.notes.enabled",
        "model.effort",
        "tui.theme",
        "secret",
    ] {
        assert!(!reaches(&p(s)), "{s}");
    }
    assert_eq!(going_live_line(&[]), "");
}

/// The notes are headed, name where the act was asked and the setting that let
/// it run, and say how to undo it or that there is nothing to undo.
#[test]
fn each_note_says_what_was_done_where_and_how_to_undo_it() {
    let args = json!({"restart": true, "asked_by": {"place": "discord:dm:7"}});
    let settings = diff::between(
        "[import.notes]\nenabled = true\n",
        "[import.notes]\nenabled = false\n",
    )
    .unwrap();
    let Note::Undo {
        title,
        by,
        body,
        undo,
    } = change_note(
        &args,
        &settings,
        4,
        ("config.propose", "enforcement = notify"),
        true,
    )
    else {
        panic!("a change's note has its Undo")
    };
    assert_eq!(title, "Changed 1 setting");
    assert_eq!(
        by,
        "asked in discord:dm:7; config.propose ran with no card (enforcement = notify)"
    );
    assert_eq!(
        body,
        "- import.notes.enabled: on → off\n\
         Theseus restarts itself to apply it.\n\
         This conversation had read text from outside since your last message (a page, a search \
         or a server).\n"
    );
    assert_eq!(undo, crate::owner_note::told::Undo::ConfigUndo(4));
    assert_eq!(undo.command(), "ask Theseus to undo settings change #4");
    let undid = json!({"undoes": {"number": 4}, "asked_by": {}});
    let Note::Undo {
        title, by, undo, ..
    } = change_note(&undid, &settings, 5, ("config.undo", "x = open"), false)
    else {
        panic!("an undo's note has its Undo")
    };
    assert_eq!(title, "Undid settings change #4");
    assert_eq!(
        by,
        "asked in a conversation in the terminal or the web UI; config.undo ran with no card \
         (x = open)"
    );
    assert_eq!(undo, crate::owner_note::told::Undo::ConfigUndo(5));
    let r = restart_note(&args, "enforcement = notify");
    assert!(crate::owner_note::is_headed(&r), "{r}");
    assert!(r.contains("nothing to undo"), "{r}");
    for (restart, says) in [(true, "stops at a restart"), (false, "It stopped")] {
        let d = detach_note(
            &json!({"restart": restart}),
            "\"wc\"",
            "enforcement = notify",
        );
        assert!(
            d.starts_with("ℹ️ FYI · Removed the MCP server \"wc\"\n"),
            "{d}"
        );
        assert!(d.contains(says), "{d}");
        assert!(d.contains("attaching a server asks you"), "{d}");
    }
}
