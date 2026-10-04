//! Navigation: definition, references, hover, document and workspace
//! symbols, and rename's edit, which is returned and never applied.
//!
//! Each request about a document opens it if it is not open and syncs every
//! open document with the disk first (`docs.rs`), then asks within the
//! default timeout. A request the server did not declare is refused before
//! it is sent ([`Error::NotOffered`]).

use std::path::Path;

use serde_json::{json, Value};

use crate::client::{Client, Error};
use crate::types::{
    self, DocumentSymbols, Hover, Location, Position, SymbolInformation, WorkspaceEdit,
};

impl Client {
    fn offered(&self, provider: &str, what: &str) -> Result<(), Error> {
        if self.capabilities().offers(provider) {
            Ok(())
        } else {
            Err(Error::NotOffered(what.into()))
        }
    }

    async fn at(
        &self,
        method: &str,
        path: &Path,
        pos: Position,
        extra: Value,
    ) -> Result<Value, Error> {
        let uri = self.prepare(path).await?;
        let mut params = json!({ "textDocument": { "uri": uri }, "position": pos });
        if let (Value::Object(p), Value::Object(e)) = (&mut params, extra) {
            p.extend(e);
        }
        let t = self.shared().opts.request_timeout;
        self.request_within(method, params, t).await
    }

    /// Where the symbol at `pos` is defined.
    pub async fn definition(&self, path: &Path, pos: Position) -> Result<Vec<Location>, Error> {
        self.offered("definitionProvider", "definitions")?;
        let v = self
            .at("textDocument/definition", path, pos, Value::Null)
            .await?;
        types::locations(v).map_err(|e| Error::Protocol(format!("an unreadable definition: {e}")))
    }

    /// Every reference to the symbol at `pos`, with its declaration when
    /// `include_declaration`.
    pub async fn references(
        &self,
        path: &Path,
        pos: Position,
        include_declaration: bool,
    ) -> Result<Vec<Location>, Error> {
        self.offered("referencesProvider", "references")?;
        let extra = json!({ "context": { "includeDeclaration": include_declaration } });
        let v = self.at("textDocument/references", path, pos, extra).await?;
        types::locations(v).map_err(|e| Error::Protocol(format!("unreadable references: {e}")))
    }

    /// The hover at `pos`; `None` when the server has nothing there.
    pub async fn hover(&self, path: &Path, pos: Position) -> Result<Option<Hover>, Error> {
        self.offered("hoverProvider", "hover")?;
        let v = self
            .at("textDocument/hover", path, pos, Value::Null)
            .await?;
        if v.is_null() {
            return Ok(None);
        }
        serde_json::from_value(v)
            .map(Some)
            .map_err(|e| Error::Protocol(format!("an unreadable hover: {e}")))
    }

    /// The document's symbols, in the shape the server gives.
    pub async fn document_symbols(&self, path: &Path) -> Result<DocumentSymbols, Error> {
        self.offered("documentSymbolProvider", "document symbols")?;
        let uri = self.prepare(path).await?;
        let t = self.shared().opts.request_timeout;
        let v = self
            .request_within(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": uri } }),
                t,
            )
            .await?;
        DocumentSymbols::from_value(v)
            .map_err(|e| Error::Protocol(format!("unreadable document symbols: {e}")))
    }

    /// The workspace's symbols matching `query`. A malformed entry is
    /// skipped.
    pub async fn workspace_symbols(&self, query: &str) -> Result<Vec<SymbolInformation>, Error> {
        self.offered("workspaceSymbolProvider", "workspace symbols")?;
        let v = self
            .request("workspace/symbol", json!({ "query": query }))
            .await?;
        let Value::Array(items) = v else {
            return Ok(Vec::new());
        };
        Ok(items
            .into_iter()
            .filter_map(|i| serde_json::from_value(i).ok())
            .collect())
    }

    /// Whether the symbol at `pos` can be renamed, and its range; `None`
    /// when the server says it cannot. A server without `prepareProvider`
    /// answers `Ok(None)` here too.
    pub async fn prepare_rename(&self, path: &Path, pos: Position) -> Result<Option<Value>, Error> {
        let prepare = self
            .capabilities()
            .raw
            .pointer("/renameProvider/prepareProvider")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !prepare {
            return Ok(None);
        }
        let v = self
            .at("textDocument/prepareRename", path, pos, Value::Null)
            .await?;
        Ok((!v.is_null()).then_some(v))
    }

    /// The edit that renames the symbol at `pos` to `new_name`, as the
    /// server proposes it. Never applied here.
    pub async fn rename(
        &self,
        path: &Path,
        pos: Position,
        new_name: &str,
    ) -> Result<Option<WorkspaceEdit>, Error> {
        self.offered("renameProvider", "rename")?;
        let v = self
            .at(
                "textDocument/rename",
                path,
                pos,
                json!({ "newName": new_name }),
            )
            .await?;
        if v.is_null() {
            return Ok(None);
        }
        serde_json::from_value(v)
            .map(Some)
            .map_err(|e| Error::Protocol(format!("an unreadable rename edit: {e}")))
    }
}
