//! `theseus sessions` and `theseus executions` print a page, not every one
//! (theseus-7bee): the newest 50, a footer naming how to see more, `--before`
//! to page back, and `--all` for the whole list as it always was. A store
//! with a great many sessions costs the page, never every session.

use anyhow::Result;
use clap::Args;
use serde_json::Value;
use theseus_client::render;
use theseus_client::Conn;
use theseus_protocol::{
    method, ExecutionListParams, ExecutionListResult, SessionListParams, SessionListResult,
};

use crate::cmd::output;
use crate::print;

/// The rows a list prints without `-n`.
pub const PAGE: usize = 50;

/// A list's page: the newest N, a page back, or all of it.
#[derive(Args, Debug, Clone, Default)]
pub struct PageArgs {
    /// Every one, not a page: it reads them all, so with a great many it is slow.
    #[arg(long, conflicts_with_all = ["before", "n"])]
    pub all: bool,
    /// A page back: only those before this cursor, which the footer of a page names.
    #[arg(long, value_name = "CURSOR")]
    pub before: Option<u64>,
    /// How many rows a page holds (default 50, at most 1000).
    #[arg(short, long, value_name = "N")]
    pub n: Option<usize>,
}

impl PageArgs {
    fn size(&self) -> usize {
        self.n.unwrap_or(PAGE)
    }
}

/// `theseus sessions`: the newest page of sessions, or every one under `--all`.
pub async fn sessions(conn: &mut Conn, json: bool, page: &PageArgs) -> Result<()> {
    let v = if page.all {
        conn.request(method::SESSION_LIST, Value::Null).await?
    } else {
        let p = SessionListParams {
            n: Some(page.size()),
            before: page.before,
            ..Default::default()
        };
        conn.request(method::SESSION_LIST, p).await?
    };
    output(json, v, |l: SessionListResult| {
        for s in &l.sessions {
            println!("{}", render::session_row(s).text);
        }
        print::lines(
            &mut std::io::stdout().lock(),
            &render::sessions::page_footer("sessions", l.older),
        )?;
        Ok(())
    })
}

/// `theseus executions`: the newest page of executions, or every one under
/// `--all`.
pub async fn executions(conn: &mut Conn, json: bool, page: &PageArgs) -> Result<()> {
    let v = if page.all {
        conn.request(method::EXECUTION_LIST, Value::Null).await?
    } else {
        let p = ExecutionListParams {
            n: Some(page.size()),
            before: page.before,
            ..Default::default()
        };
        conn.request(method::EXECUTION_LIST, p).await?
    };
    output(json, v, |l: ExecutionListResult| {
        if l.executions.is_empty() {
            println!("no executions");
        }
        for e in &l.executions {
            println!("{}", render::execution_row(e).text);
        }
        print::lines(
            &mut std::io::stdout().lock(),
            &render::sessions::page_footer("executions", l.older),
        )?;
        Ok(())
    })
}
