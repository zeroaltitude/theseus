//! `theseus extend list` (M7 43a, 43b): the loaded extensions, then the
//! proposals, newest first, each with its state, its frozen digest, its
//! tools and tests, and the question that asks the operator (`theseus
//! confirm <id>`). `theseus extend revoke <name>` (43b) stops a loaded one,
//! the operator's from their own shell.

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;
use theseus_client::Conn;
use theseus_protocol::extend::{
    ExtendInfo, ExtendListResult, ExtendLoadedInfo, ExtensionRevokeParams, ExtensionRevokeResult,
};
use theseus_protocol::method;

#[derive(Subcommand, Debug)]
pub enum ExtendCmd {
    /// Every loaded extension, and every proposal: its state, digest, tools, tests, and question.
    List,
    /// Revoke a loaded extension: its server stops and its tools are gone from the next turn.
    /// The frozen copy stays on disk.
    Revoke {
        /// The extension's name, as `theseus extend list` shows it.
        name: String,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ExtendCmd) -> Result<()> {
    if let ExtendCmd::Revoke { name } = cmd {
        let p = ExtensionRevokeParams {
            name,
            ..Default::default()
        };
        let v = conn
            .request(method::EXTENSION_REVOKE, serde_json::to_value(&p)?)
            .await?;
        if json {
            println!("{}", serde_json::to_string(&v)?);
            return Ok(());
        }
        let r: ExtensionRevokeResult = serde_json::from_value(v)?;
        println!(
            "Revoked {} {}: its server stopped, and {} gone from the next turn. The frozen copy \
             stays at {}.",
            r.name,
            &r.digest[..r.digest.len().min(6)],
            match r.tools.as_slice() {
                [] => "no tool is".to_string(),
                t => format!("{} are", t.join(", ")),
            },
            r.frozen
        );
        return Ok(());
    }
    let v = conn.request(method::EXTEND_LIST, Value::Null).await?;
    if json {
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    }
    let l: ExtendListResult = serde_json::from_value(v)?;
    for e in &l.loaded {
        for line in loaded_lines(e) {
            println!("{line}");
        }
    }
    if l.extensions.is_empty() {
        println!("No extension has been proposed.");
    }
    for e in &l.extensions {
        for line in lines(e) {
            println!("{line}");
        }
    }
    Ok(())
}

/// One loaded extension, in lines.
pub fn loaded_lines(e: &ExtendLoadedInfo) -> Vec<String> {
    let short = &e.digest[..e.digest.len().min(6)];
    let network = match e.network.as_slice() {
        [] => "no network".to_string(),
        hosts => format!("network to {}", hosts.join(", ")),
    };
    let mut out = vec![format!(
        "{} {short}  loaded as {}  {}  {network}",
        e.name, e.server, e.state
    )];
    out.push(format!("  tools: {}", e.tools.join(", ")));
    out.push(format!(
        "  acked by {} through {} at {}; {} calls, {} errors",
        e.acked_by,
        e.acked_via,
        theseus_client::render::fmt_time(e.acked_at_ms),
        e.calls,
        e.errors
    ));
    if let Some(old) = &e.replaced {
        out.push(format!("  replaced {}", &old[..old.len().min(6)]));
    }
    if let Some(why) = &e.last_error {
        out.push(format!("  last error: {why}"));
    }
    out.push(format!("  revoke: theseus extend revoke {}", e.name));
    out
}

/// One proposal, in lines.
pub fn lines(e: &ExtendInfo) -> Vec<String> {
    let short = &e.digest[..e.digest.len().min(6)];
    let network = match e.network.as_slice() {
        [] => "no network".to_string(),
        hosts => format!("network to {}", hosts.join(", ")),
    };
    let mut out = vec![format!(
        "{} {short}  {}  {} of {} tests passed  {network}",
        e.name, e.state, e.passed, e.tests
    )];
    out.push(format!("  {}", e.description));
    out.push(format!(
        "  command: {}  (frozen at {})",
        e.command.join(" "),
        e.frozen
    ));
    if !e.tools.is_empty() {
        out.push(format!("  tools: {}", e.tools.join(", ")));
    }
    if let Some(why) = &e.error {
        out.push(format!("  did not come up in L1: {why}"));
    }
    match (e.state.as_str(), &e.question, &e.answered_by) {
        ("proposed", Some(q), _) => {
            out.push(format!("  waiting: theseus confirm {q} (or --decline)"))
        }
        (_, _, Some(by)) => out.push(format!("  {} by {by}", e.state)),
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waiting_proposal_says_how_to_answer_it() {
        let e = ExtendInfo {
            name: "wordcount".into(),
            digest: "3f2a1c9e".into(),
            state: "proposed".into(),
            description: "Counts words.".into(),
            command: vec!["python3".into(), "server.py".into()],
            frozen: "/state/extensions/wordcount/3f2a1c9e".into(),
            tools: vec!["count".into()],
            passed: 3,
            tests: 3,
            question: Some("act_q1".into()),
            ..Default::default()
        };
        assert_eq!(
            lines(&e),
            [
                "wordcount 3f2a1c  proposed  3 of 3 tests passed  no network",
                "  Counts words.",
                "  command: python3 server.py  (frozen at /state/extensions/wordcount/3f2a1c9e)",
                "  tools: count",
                "  waiting: theseus confirm act_q1 (or --decline)",
            ]
        );
    }
}
