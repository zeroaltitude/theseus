//! `theseus prompt <server/prompt> [--arg k=v]… [--session <id>]` (M7 36c):
//! run an MCP server's prompt as a turn's input, and stream the reply as
//! `ask` does. The core asks the server for the prompt's messages
//! (`turn.submit { prompt }`); this only names it. `theseus mcp` lists them.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use clap::Args;
use theseus_client::Conn;
use theseus_protocol::mcp::McpPromptRef;
use theseus_protocol::TurnSubmitParams;

#[derive(Args, Debug)]
pub struct PromptArgs {
    /// The prompt, as `theseus mcp` lists it: `<server>/<prompt>`.
    #[arg(value_name = "SERVER/PROMPT")]
    name: String,
    /// One of the prompt's arguments (repeatable): `--arg name=Ada`.
    #[arg(long = "arg", value_name = "K=V")]
    args: Vec<String>,
    /// Continue an existing session instead of opening a new one.
    #[arg(long, short)]
    session: Option<String>,
    /// Profile for this turn (default: the live profile).
    #[arg(long = "profile", short = 'P')]
    profile: Option<String>,
    /// Raw provider override for this turn.
    #[arg(long, short)]
    provider: Option<String>,
    /// Model id for this turn.
    #[arg(long, short)]
    model: Option<String>,
    /// After the reply, print the turn's timing tree to stderr.
    #[arg(long)]
    trace: bool,
    /// Show the model's thinking summaries on stderr as they stream.
    #[arg(long)]
    thinking: bool,
}

/// `server/prompt` as the pair it names. The server's name has no `/`, so the
/// first one divides them.
pub fn parse_name(name: &str) -> Result<(String, String)> {
    match name.split_once('/') {
        Some((s, p)) if !s.is_empty() && !p.is_empty() => Ok((s.into(), p.into())),
        _ => {
            bail!("name the prompt as <server>/<prompt>, as `theseus mcp` lists it (got {name:?})")
        }
    }
}

/// `--arg k=v` pairs: a name, `=`, and a value that may hold more `=`s or be
/// empty. A name given twice is a mistake, not a last-wins.
pub fn parse_args(args: &[String]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for a in args {
        let Some((k, v)) = a.split_once('=').filter(|(k, _)| !k.trim().is_empty()) else {
            bail!("--arg takes name=value (got {a:?})");
        };
        if out.insert(k.trim().to_string(), v.to_string()).is_some() {
            bail!("--arg {} was given twice", k.trim());
        }
    }
    Ok(out)
}

/// The `turn.submit` params that run a prompt.
pub fn params(a: &PromptArgs) -> Result<TurnSubmitParams> {
    let (server, name) = parse_name(&a.name)?;
    Ok(TurnSubmitParams {
        carried: false,
        prompt: Some(McpPromptRef {
            server,
            name,
            arguments: parse_args(&a.args)?,
        }),
        session_id: a.session.clone(),
        input: String::new(),
        profile: a.profile.clone(),
        provider: a.provider.clone(),
        model: a.model.clone(),
        author: None,
        attachments: vec![],
        reply_to: None,
        // Inside a job, its session (theseus-b5cl).
        opened_from: theseus_client::client::job_session(),
    })
}

pub async fn run(
    conn: &mut Conn,
    json: bool,
    no_stream: bool,
    a: PromptArgs,
    spawned: bool,
) -> Result<()> {
    let p = serde_json::to_value(params(&a)?)?;
    crate::cmd::stream_turn(conn, json, no_stream, p, a.thinking, a.trace, spawned).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_prompt_is_named_server_slash_prompt() {
        assert_eq!(
            parse_name("fake/greet").unwrap(),
            ("fake".into(), "greet".into())
        );
        // The first slash divides; a prompt's own name may hold more.
        assert_eq!(
            parse_name("docs/a/b").unwrap(),
            ("docs".into(), "a/b".into())
        );
        for bad in ["greet", "/greet", "fake/", ""] {
            assert!(parse_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn args_are_name_equals_value() {
        let m = parse_args(&args(&["name=Ada", "topic=a=b", "empty="])).unwrap();
        assert_eq!(m["name"], "Ada");
        assert_eq!(m["topic"], "a=b", "a value may hold =");
        assert_eq!(m["empty"], "");
        assert!(parse_args(&args(&["noequals"])).is_err());
        assert!(parse_args(&args(&["=v"])).is_err());
        let twice = parse_args(&args(&["a=1", "a=2"])).unwrap_err().to_string();
        assert!(twice.contains("twice"), "{twice}");
        assert!(parse_args(&[]).unwrap().is_empty());
    }

    #[test]
    fn the_params_carry_the_prompt_and_no_input() {
        let a = PromptArgs {
            name: "fake/greet".into(),
            args: args(&["name=Ada"]),
            session: Some("s1".into()),
            profile: None,
            provider: None,
            model: None,
            trace: false,
            thinking: false,
        };
        let p = params(&a).unwrap();
        let r = p.prompt.unwrap();
        assert_eq!((r.server.as_str(), r.name.as_str()), ("fake", "greet"));
        assert_eq!(r.arguments["name"], "Ada");
        assert_eq!(p.session_id.as_deref(), Some("s1"));
        assert!(p.input.is_empty() && p.attachments.is_empty());
    }
}
