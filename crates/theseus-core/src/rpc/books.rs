//! `books.list` and `books.page` (theseus-civ0; `crate::books`): the
//! imported episodes by the book their import hinted, read only.
//!
//! - **Reads, off the serving workers.** Each runs on the blocking pool: a
//!   few rows of the index's terms, and a record per episode a page shows.
//! - **For every surface and place** (2026-11-22). `session_id`, when given,
//!   must name a session; nothing is withheld for its place or the surface.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::books::{BooksListParams, BooksPageParams};
use theseus_protocol::{error_code, method};

use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::books;

impl Core {
    pub(super) async fn rpc_books(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let out = match name {
            method::BOOKS_LIST => {
                let p: BooksListParams = if params.is_null() {
                    BooksListParams::default()
                } else {
                    parse(params)?
                };
                self.books_session(p.session_id.as_deref())?;
                let core = self.clone();
                serde_json::to_value(blocking(move || books::list(&core.store)).await?)
            }
            method::BOOKS_PAGE => {
                let p: BooksPageParams = parse(params)?;
                self.books_session(p.session_id.as_deref())?;
                let q = books::Query::of(&p, true)
                    .map_err(|why| RpcFailure::new(error_code::INVALID_PARAMS, why))?;
                let core = self.clone();
                serde_json::to_value(blocking(move || books::page(&core.store, &q)).await?)
            }
            // The exchange ledger's two (issue 44), routed with these reads.
            other if super::exchanges::OWN.contains(&other) => {
                return self.rpc_exchanges(other, params, conn).await
            }
            other => {
                return Err(RpcFailure::new(
                    error_code::METHOD_NOT_FOUND,
                    format!("unknown method {other:?}"),
                ))
            }
        };
        out.map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// The session a question names, if it names one, must exist.
    fn books_session(&self, session_id: Option<&str>) -> Result<(), RpcFailure> {
        match session_id {
            Some(sid) => self.session_exists(sid),
            None => Ok(()),
        }
    }
}

/// Run `f` on the blocking pool; its error is the method's.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, RpcFailure> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, format!("{e:#}")))
}
