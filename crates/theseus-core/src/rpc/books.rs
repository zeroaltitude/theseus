//! `books.list` and `books.page` (theseus-civ0; `crate::books`): the
//! imported episodes by the book their import hinted, read only.
//!
//! - **Reads, off the serving workers.** Each runs on the blocking pool: a
//!   few rows of the index's terms, and a record per episode a page shows.
//! - **For a place.** `session_id` names the session the question is asked
//!   for, as `memory.search`'s does; none is a private place (the CLI, the
//!   web UI). A shared place gets the books' counts and each episode's
//!   times and labels' sensitivity, never its text (`books::WITHHELD`).

use std::sync::Arc;

use serde_json::Value;
use theseus_memory::recall::Place;
use theseus_protocol::books::{BooksListParams, BooksPageParams};
use theseus_protocol::{error_code, method};

use super::server::{parse, RpcFailure};
use super::Core;
use crate::books;

impl Core {
    pub(super) async fn rpc_books(
        self: Arc<Self>,
        name: &str,
        params: Value,
    ) -> Result<Value, RpcFailure> {
        let out = match name {
            method::BOOKS_LIST => {
                let p: BooksListParams = if params.is_null() {
                    BooksListParams::default()
                } else {
                    parse(params)?
                };
                self.books_private(p.session_id.as_deref())?;
                let core = self.clone();
                serde_json::to_value(blocking(move || books::list(&core.store)).await?)
            }
            method::BOOKS_PAGE => {
                let p: BooksPageParams = parse(params)?;
                let private = self.books_private(p.session_id.as_deref())?;
                let q = books::Query::of(&p, private)
                    .map_err(|why| RpcFailure::new(error_code::INVALID_PARAMS, why))?;
                let core = self.clone();
                serde_json::to_value(blocking(move || books::page(&core.store, &q)).await?)
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

    /// Whether a question asked for `session_id`'s place is asked for a
    /// private one; none is.
    fn books_private(&self, session_id: Option<&str>) -> Result<bool, RpcFailure> {
        let Some(sid) = session_id else {
            return Ok(true);
        };
        self.session_exists(sid)?;
        Ok(matches!(self.runner.place_of(sid), Place::Private))
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
