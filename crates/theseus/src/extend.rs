//! `theseus extend list` (M7 43a): the proposed extensions, newest first,
//! each with its state, its frozen digest, its tools and tests, and the
//! question that asks the operator (`theseus confirm <id>`).

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;
use theseus_client::Conn;
use theseus_protocol::extend::{ExtendInfo, ExtendListResult};
use theseus_protocol::method;

#[derive(Subcommand, Debug)]
pub enum ExtendCmd {
    /// Every proposal: its state, digest, tools, tests, and question.
    List,
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ExtendCmd) -> Result<()> {
    let ExtendCmd::List = cmd;
    let v = conn.request(method::EXTEND_LIST, Value::Null).await?;
    if json {
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    }
    let l: ExtendListResult = serde_json::from_value(v)?;
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
