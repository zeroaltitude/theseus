//! `theseus herdr main` (prefix+m, theseus-m638c): back to the mainline. herdr's `theseus` workspace
//! keeps one `main` tab, whose pane runs `theseus chat` on the home conversation. The first
//! prefix+m makes it; every later one focuses it, so a task's pane is never more than one key
//! from the conversation that started it. The command talks to herdr alone, and nothing stays
//! resident. The task pane's header (`task_header`) names the keys.

use std::path::PathBuf;

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::herdr_up::wire;

/// The pane of the `main` tab, if herdr has one: its `kind` token.
pub fn main_pane(panes: &[Value]) -> Option<&Value> {
    panes
        .iter()
        .find(|p| p["tokens"]["kind"].as_str() == Some(wire::MAIN_KIND))
}

/// The line at the top of a task's pane: whose it is and the two keys that leave it. The keys are
/// read from the plugin's own table, so the words and the bindings cannot differ.
pub fn task_header(short: &str, parent: Option<&str>) -> String {
    let key = |action: &str| {
        crate::herdr_setup::KEYS
            .iter()
            .find(|k| k.action == action)
            .map_or("", |k| k.key)
    };
    let of = parent.map_or(String::new(), |p| format!(" of {p}"));
    format!(
        "── ⚙ task {short}{of} · {} main · {} inspect ──",
        key("theseus.main"),
        key("theseus.inspect")
    )
}

/// `theseus herdr main`: focus the `main` tab, making it the first time.
pub async fn run(socket: &str, herdr_socket: Option<PathBuf>) -> Result<()> {
    let mut h = wire::Herdr::new(crate::herdr_keys::herdr_socket_or_default(herdr_socket)?);
    let listed = h.call("pane.list", json!({})).await?;
    let panes = listed["panes"].as_array().cloned().unwrap_or_default();
    let pane = match main_pane(&panes).and_then(|p| p["pane_id"].as_str()) {
        Some(id) => id.to_string(),
        None => {
            let socket = theseus_client::client::tilde(socket, std::env::var("HOME").ok());
            let exe = std::env::current_exe()?;
            let at = (exe.as_path(), socket.as_str());
            let tab = h
                .open_tab(
                    &mut wire::Space::default(),
                    at,
                    &wire::TabSpec::main(),
                    true,
                )
                .await?;
            if tab.pane_id.is_empty() {
                bail!("herdr made no `main` tab");
            }
            tab.pane_id
        }
    };
    h.call("pane.focus", json!({ "pane_id": pane })).await?;
    println!("focused the main tab ({pane})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn the_header_names_the_parent_and_the_keys() {
        assert_eq!(
            task_header("a1b2c3", Some("Tide notes")),
            "── ⚙ task a1b2c3 of Tide notes · prefix+m main · prefix+i inspect ──"
        );
        assert_eq!(
            task_header("a1b2c3", None),
            "── ⚙ task a1b2c3 · prefix+m main · prefix+i inspect ──"
        );
    }

    #[test]
    fn the_main_tab_runs_chat_as_an_argv() {
        let spec = wire::TabSpec::main();
        let at = wire::Where::Workspace("w1");
        let p = wire::tab_params(&at, Path::new("/bin/theseus"), "/run/t.sock", &spec, true);
        assert_eq!(
            p["root"]["command"],
            json!(["/bin/theseus", "--socket", "/run/t.sock", "chat"])
        );
        let tokens = wire::pane_tokens_params("w1:p2", &spec);
        assert_eq!(tokens["tokens"]["kind"], "main");
    }

    #[test]
    fn the_main_pane_is_the_one_with_the_main_kind() {
        let panes = vec![
            json!({"pane_id": "p1", "tokens": {"kind": "task"}}),
            json!({"pane_id": "p2", "tokens": {"kind": "main"}}),
        ];
        assert_eq!(main_pane(&panes).unwrap()["pane_id"], "p2");
        assert!(main_pane(&panes[..1]).is_none());
    }
}
