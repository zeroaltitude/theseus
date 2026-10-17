use super::*;
use crate::herdr_slots::TOKENS;

/// The snippet is TOML, and every `$token` it names is one the reporter sends.
#[test]
fn the_snippet_parses_and_names_only_tokens_the_reporter_sends() {
    let text = snippet();
    let parsed: toml::Table = text.parse().unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(parsed["ui"]["toast"]["delivery"].as_str(), Some("herdr"));
    assert_eq!(parsed["ui"]["agent_panel_sort"].as_str(), Some("priority"));
    assert_eq!(parsed["ui"]["status_indicators"].as_str(), Some("symbols"));
    assert_eq!(parsed["ui"]["sidebar_width"].as_integer(), Some(36));
    let keys = parsed["keys"]["command"].as_array().unwrap();
    assert_eq!(keys.len(), KEYS.len());
    // The tokens, read from the printed TOML and not from the table that made it.
    let mut named = Vec::new();
    for r in parsed["ui"]["sidebar"]["agents"]["rows"]
        .as_array()
        .unwrap()
    {
        for t in r.as_array().unwrap() {
            if let Some(name) = t.as_str().unwrap().strip_prefix('$') {
                named.push(name.to_string());
            }
        }
    }
    assert!(!named.is_empty());
    for n in &named {
        assert!(
            TOKENS.contains(&n.as_str()),
            "${n} is not a token the reporter sends: {TOKENS:?}"
        );
    }
    assert!(row_tokens().iter().all(|t| TOKENS.contains(t)));
}

/// A config that has the snippet lacks nothing; one that has nothing lacks all of it, and the keys
/// it binds to something else are said.
#[test]
fn lacks_lists_what_a_config_does_not_have() {
    assert_eq!(lacks(&snippet()).unwrap(), Vec::<String>::new());
    let all = lacks("").unwrap();
    assert!(all.iter().any(|l| l.contains("toast")), "{all:?}");
    assert!(
        all.iter().any(|l| l.contains("agent_panel_sort")),
        "{all:?}"
    );
    assert!(all.iter().any(|l| l.contains("$doing")), "{all:?}");
    assert_eq!(
        all.iter()
            .filter(|l| l.starts_with("[[keys.command]]"))
            .count(),
        KEYS.len()
    );
    let other = lacks(
        "[ui]\nagent_panel_sort = \"priority\"\n[[keys.command]]\nkey = \"prefix+a\"\ncommand = \"lazygit\"\n",
    )
    .unwrap();
    assert!(
        other
            .iter()
            .any(|l| l.contains("prefix+a runs `lazygit` and not theseus.answer")),
        "{other:?}"
    );
    assert!(
        !other.iter().any(|l| l.contains("agent_panel_sort")),
        "{other:?}"
    );
    assert!(lacks("[ui").is_err());
}

/// The plugin's manifest and the snippet agree: every action a key runs exists as `theseus.<id>`,
/// every popup `popup` can open is a pane, the link handler's pattern is the code's, and the
/// pattern accepts what `parse_link` accepts and refuses what it refuses.
#[test]
fn the_manifest_the_snippet_and_the_code_agree() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contrib/herdr/theseus/herdr-plugin.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let m: toml::Table = text.parse().unwrap();
    assert_eq!(m["id"].as_str(), Some(crate::herdr_keys::PLUGIN_ID));
    assert_eq!(m["min_herdr_version"].as_str(), Some("0.9.0"));
    let actions: Vec<&str> = m["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    for k in &KEYS {
        let id = k.action.strip_prefix("theseus.").unwrap();
        assert!(
            actions.contains(&id),
            "{} runs {} which the manifest lacks",
            k.key,
            k.action
        );
    }
    let panes: Vec<&str> = m["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    for p in crate::herdr_keys::POPUPS {
        assert!(panes.contains(&p), "popup {p} is no pane of the manifest");
    }
    for p in &panes {
        assert!(
            crate::herdr_keys::POPUPS.contains(p),
            "pane {p} cannot be opened"
        );
        let pane = m["panes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"].as_str() == Some(p))
            .unwrap();
        assert_eq!(pane["placement"].as_str(), Some("popup"), "{p}");
    }
    let tui = m["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"].as_str() == Some("tui"))
        .unwrap();
    assert_eq!(
        (tui["width"].as_str(), tui["height"].as_str()),
        (Some("92%"), Some("90%"))
    );
    // The startup hook falls back from `up --boot` to `sync`.
    let hook = m["startup"].as_array().unwrap()[0]["command"]
        .as_array()
        .unwrap();
    let hook = hook.last().unwrap().as_str().unwrap();
    assert!(
        hook.starts_with("theseus herdr up --boot 2>/dev/null || theseus herdr sync"),
        "{hook}"
    );
    // The link handler: the code's pattern, an action that exists.
    let h = &m["link_handlers"].as_array().unwrap()[0];
    assert_eq!(h["pattern"].as_str(), Some(crate::herdr_keys::LINK_PATTERN));
    assert!(actions.contains(&h["action"].as_str().unwrap()));
    // Every action's command is a `sh -c` line, nothing more.
    for a in m["actions"].as_array().unwrap() {
        let c = a["command"].as_array().unwrap();
        assert_eq!((c[0].as_str(), c[1].as_str()), (Some("sh"), Some("-c")));
        assert_eq!(c.len(), 3);
    }
}

/// prefix+i's popup reads the task from the environment the key put there, and opens the chat
/// inspecting it; prefix+m runs the command that focuses the main tab.
#[test]
fn the_inspect_popup_and_the_main_key_run_their_commands() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contrib/herdr/theseus/herdr-plugin.toml");
    let m: toml::Table = std::fs::read_to_string(path).unwrap().parse().unwrap();
    let command = |list: &str, id: &str| {
        m[list]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"].as_str() == Some(id))
            .unwrap()["command"][2]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(
        command("panes", "inspect"),
        "theseus chat --task \"$THESEUS_SESSION_ARG\" --inspect; sleep 1"
    );
    assert!(command("actions", "inspect").contains("herdr popup inspect"));
    assert_eq!(command("actions", "main"), "exec theseus herdr main");
    let keys: Vec<_> = KEYS.iter().map(|k| (k.key, k.action)).collect();
    assert!(keys.contains(&("prefix+m", "theseus.main")));
    assert!(keys.contains(&("prefix+i", "theseus.inspect")));
}
