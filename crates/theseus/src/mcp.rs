//! `theseus mcp` (M7 36b): the MCP servers the config attaches, each with
//! its state and its tools (their postures, classes, and the servers' own
//! hints); `theseus mcp restart <name>` starts one again, a failed one
//! included.

use anyhow::Result;
use clap::Subcommand;
use serde_json::{json, Value};
use theseus_client::render;
use theseus_client::Conn;
use theseus_protocol::mcp::{McpListResult, McpRestartResult};
use theseus_protocol::method;

#[derive(Subcommand, Debug)]
pub enum McpCmd {
    /// Start a server again now, a failed one included, its crashes forgotten.
    Restart {
        #[arg(value_name = "NAME")]
        name: String,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: Option<McpCmd>) -> Result<()> {
    let v = match &cmd {
        None => conn.request(method::MCP_LIST, Value::Null).await?,
        Some(McpCmd::Restart { name }) => {
            conn.request(method::MCP_RESTART, json!({ "name": name }))
                .await?
        }
    };
    if json {
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    }
    match cmd {
        None => {
            let l: McpListResult = serde_json::from_value(v)?;
            for line in render::mcp_lines(&l) {
                println!("{line}");
            }
        }
        Some(McpCmd::Restart { .. }) => {
            let r: McpRestartResult = serde_json::from_value(v)?;
            println!("MCP server {} restarting (it was {}).", r.name, r.was);
        }
    }
    Ok(())
}
