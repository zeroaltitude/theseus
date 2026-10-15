//! `theseus herdr setup [--config FILE]` (theseus-pkaq): the herdr config Theseus is built to be
//! used with, printed as a snippet the owner pastes (toasts through herdr, the sidebar sorted by
//! what needs attention, a row of Theseus's own tokens per agent, the keys of the plugin), and, with
//! `--config`, what a given config lacks. It reads and prints; it never writes a config.
//!
//! The snippet is made from the tables below, so what it prints, what `lacks` looks for, and the
//! tokens the reporter sends (`herdr_slots::TOKENS`) have one source each, and a test ties them.

use std::path::Path;

use anyhow::{Context, Result};

/// The sidebar's rows for a Theseus agent: herdr's own state, then Theseus's tokens, which the
/// reporter sends (`$doing` is what the task is doing now, `$cost` its spend, `$model` its model).
pub const ROWS: [&[&str]; 3] = [
    &["state_icon", "agent"],
    &["state_text", "$doing"],
    &["$cost", "$model", "$parent"],
];

/// A key the plugin binds: the key, the plugin's action, and what it does.
pub struct Binding {
    pub key: &'static str,
    pub action: &'static str,
    pub description: &'static str,
}

/// The owner's keys (2026-10-08): answer, next, the TUI, stop, a new task.
pub const KEYS: [Binding; 5] = [
    Binding {
        key: "prefix+a",
        action: "theseus.answer",
        description: "answer the focused task's question",
    },
    Binding {
        key: "prefix+shift+a",
        action: "theseus.next",
        description: "jump to the task that has waited longest",
    },
    Binding {
        key: "prefix+t",
        action: "theseus.tui",
        description: "the Theseus terminal UI",
    },
    Binding {
        key: "prefix+shift+s",
        action: "theseus.stop",
        description: "stop the focused task or session",
    },
    Binding {
        key: "prefix+n",
        action: "theseus.new",
        description: "start a task",
    },
];

/// The `$tokens` the rows name, without the `$`.
pub fn row_tokens() -> Vec<&'static str> {
    ROWS.iter()
        .flat_map(|r| r.iter())
        .filter_map(|t| t.strip_prefix('$'))
        .collect()
}

/// A row as TOML writes it.
fn row(r: &[&str]) -> String {
    let items: Vec<String> = r.iter().map(|t| format!("\"{t}\"")).collect();
    format!("[{}]", items.join(", "))
}

/// The snippet, to paste into herdr's `config.toml`.
pub fn snippet() -> String {
    let mut s = String::new();
    s.push_str("# Theseus in herdr (theseus herdr setup). Paste into herdr's config.toml.\n");
    s.push_str("[ui]\nagent_panel_sort = \"priority\"\nstatus_indicators = \"symbols\"\nsidebar_width = 36\n\n");
    s.push_str("[ui.toast]\ndelivery = \"herdr\"\n\n");
    s.push_str("[ui.sidebar.agents]\n");
    let rows: Vec<String> = ROWS.iter().map(|r| row(r)).collect();
    s.push_str(&format!("rows = [{}]\n", rows.join(", ")));
    for k in &KEYS {
        s.push_str(&format!(
            "\n[[keys.command]]\nkey = \"{}\"\ntype = \"plugin_action\"\ncommand = \"{}\"\ndescription = \"{}\"\n",
            k.key, k.action, k.description
        ));
    }
    s
}

/// What a config lacks, one line each; empty when it has everything. `text` is the config's TOML.
pub fn lacks(text: &str) -> Result<Vec<String>> {
    let cfg: toml::Table = text.parse().context("the config is not valid TOML")?;
    let ui = cfg.get("ui").and_then(|v| v.as_table());
    let get = |k: &str| ui.and_then(|u| u.get(k));
    let mut out = Vec::new();
    let mut want = |ok: bool, line: &str| {
        if !ok {
            out.push(line.to_string());
        }
    };
    want(
        ui.and_then(|u| u.get("toast"))
            .and_then(|t| t.get("delivery"))
            .and_then(|d| d.as_str())
            == Some("herdr"),
        "[ui.toast] delivery = \"herdr\": herdr's toasts name the task that needs you",
    );
    want(
        get("agent_panel_sort").and_then(|v| v.as_str()) == Some("priority"),
        "[ui] agent_panel_sort = \"priority\": what needs you comes first",
    );
    want(
        get("status_indicators").and_then(|v| v.as_str()) == Some("symbols"),
        "[ui] status_indicators = \"symbols\": a state is a symbol, not only a colour",
    );
    want(
        get("sidebar_width")
            .and_then(toml::Value::as_integer)
            .is_some_and(|w| w >= 36),
        "[ui] sidebar_width = 36: room for a task's name and its state",
    );
    let rows: Vec<String> = ui
        .and_then(|u| u.get("sidebar"))
        .and_then(|s| s.get("agents"))
        .and_then(|a| a.get("rows"))
        .and_then(|r| r.as_array())
        .map(|rows| {
            rows.iter()
                .flat_map(|r| r.as_array().into_iter().flatten())
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    for t in row_tokens() {
        want(
            rows.iter().any(|r| *r == format!("${t}")),
            &format!("[ui.sidebar.agents] rows: a row naming ${t}"),
        );
    }
    let bound: Vec<(String, String)> = cfg
        .get("keys")
        .and_then(|k| k.get("command"))
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c.get("key")?.as_str()?.to_string(),
                c.get("command")?.as_str()?.to_string(),
            ))
        })
        .collect();
    for k in &KEYS {
        let on_key = bound.iter().find(|(key, _)| key == k.key);
        match on_key {
            Some((_, cmd)) if cmd == k.action => {}
            Some((_, cmd)) => out.push(format!(
                "[[keys.command]] {} runs `{cmd}` and not {}: {}",
                k.key, k.action, k.description
            )),
            None => out.push(format!(
                "[[keys.command]] {} = {} (type = \"plugin_action\"): {}",
                k.key, k.action, k.description
            )),
        }
    }
    Ok(out)
}

/// `theseus herdr setup [--config FILE]`.
pub fn run(config: Option<&Path>) -> Result<()> {
    let Some(path) = config else {
        print!("{}", snippet());
        return Ok(());
    };
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {} (read only)", path.display()))?;
    let missing = lacks(&text)?;
    if missing.is_empty() {
        println!("{} has everything the snippet has", path.display());
    } else {
        println!("{} lacks:", path.display());
        for m in &missing {
            println!("  {m}");
        }
        println!("`theseus herdr setup` prints the snippet to paste");
    }
    Ok(())
}

#[cfg(test)]
#[path = "herdr_setup_tests.rs"]
mod tests;
