//! The LSP types this client reads and writes, by hand, as theseus-mcp's
//! are: the subset the tools need, with `serde_json::Value` for the rest.
//! Field names follow the spec (3.17) through `rename_all = "camelCase"`.
//! What a server sends beyond a type's fields is ignored, and what it leaves
//! out that the spec makes optional defaults.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A place in a document: a 0-based line, and a 0-based column counted in
/// UTF-16 code units (the encoding every server supports; see
/// [`crate::position`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationLink {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_selection_range: Option<Range>,
    pub target_uri: String,
    pub target_range: Range,
    pub target_selection_range: Range,
}

impl LocationLink {
    /// The place a reader is sent to: the target's name, not its whole body.
    pub fn to_location(&self) -> Location {
        Location {
            uri: self.target_uri.clone(),
            range: self.target_selection_range,
        }
    }
}

/// A definition's answer in any of its three shapes, made one list:
/// `Location`, `Location[]`, or `LocationLink[]` (a link's selection range).
pub fn locations(v: Value) -> Result<Vec<Location>, serde_json::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum One {
        Link(LocationLink),
        Loc(Location),
    }
    let to = |o: One| match o {
        One::Link(l) => l.to_location(),
        One::Loc(l) => l,
    };
    match v {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => items
            .into_iter()
            .map(|i| serde_json::from_value(i).map(to))
            .collect(),
        v => Ok(vec![to(serde_json::from_value(v)?)]),
    }
}

/// 1 error, 2 warning, 3 information, 4 hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Severity(pub u8);

impl Severity {
    pub const ERROR: Severity = Severity(1);
    pub const WARNING: Severity = Severity(2);
    pub const INFORMATION: Severity = Severity(3);
    pub const HINT: Severity = Severity(4);

    pub fn name(self) -> &'static str {
        match self.0 {
            1 => "error",
            2 => "warning",
            3 => "information",
            4 => "hint",
            _ => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub range: Range,
    /// The spec leaves it optional; a reader treats a missing one as an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// A number or a string, as the server gives it.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub code: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<u8>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub related_information: Value,
}

impl Diagnostic {
    /// An error, or one with no severity (the spec says the client decides).
    pub fn is_error(&self) -> bool {
        self.severity.is_none_or(|s| s == Severity::ERROR)
    }

    /// The code as text, when it has one.
    pub fn code_text(&self) -> Option<String> {
        match &self.code {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }
}

/// `textDocument/publishDiagnostics`'s params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishDiagnosticsParams {
    pub uri: String,
    #[serde(default)]
    pub version: Option<i32>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

/// `textDocument/diagnostic`'s answer: the full list, or "unchanged since the
/// result id you gave".
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DocumentDiagnosticReport {
    Full {
        #[serde(default, rename = "resultId")]
        result_id: Option<String>,
        #[serde(default)]
        items: Vec<Diagnostic>,
    },
    Unchanged {
        #[serde(rename = "resultId")]
        result_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

/// A document named with the version an edit applies to. `version` is null
/// when the server means the file on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionalVersionedTextDocumentIdentifier {
    pub uri: String,
    #[serde(default)]
    pub version: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentEdit {
    pub text_document: OptionalVersionedTextDocumentIdentifier,
    /// `TextEdit`s; an `AnnotatedTextEdit` reads as one (its annotation id is
    /// dropped).
    pub edits: Vec<TextEdit>,
}

/// One of `documentChanges`: an edit to a document, or a file operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DocumentChange {
    Edit(TextDocumentEdit),
    /// `create`, `rename`, or `delete`, as sent: a rename that moves a file
    /// names it here, and the tool shows it rather than doing it.
    Operation(ResourceOperation),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceOperation {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub options: Value,
}

/// A rename's answer: edits by document (`changes`) or in order with
/// versions and file operations (`documentChanges`; a server uses one when
/// the client says it can read it, which this client does). Never applied
/// here: the caller shows it, and a gated tool applies it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<std::collections::BTreeMap<String, Vec<TextEdit>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_changes: Option<Vec<DocumentChange>>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub change_annotations: Value,
}

impl WorkspaceEdit {
    /// Every text edit, by document in the order given (`documentChanges`
    /// first, then `changes`), with the version each was made against.
    pub fn text_edits(&self) -> Vec<(String, Option<i32>, Vec<TextEdit>)> {
        let mut out = Vec::new();
        for c in self.document_changes.iter().flatten() {
            if let DocumentChange::Edit(e) = c {
                out.push((
                    e.text_document.uri.clone(),
                    e.text_document.version,
                    e.edits.clone(),
                ));
            }
        }
        for (uri, edits) in self.changes.iter().flatten() {
            out.push((uri.clone(), None, edits.clone()));
        }
        out
    }

    /// The file operations it asks for (create, rename, delete).
    pub fn operations(&self) -> Vec<&ResourceOperation> {
        self.document_changes
            .iter()
            .flatten()
            .filter_map(|c| match c {
                DocumentChange::Operation(o) => Some(o),
                DocumentChange::Edit(_) => None,
            })
            .collect()
    }
}

/// A hover's answer, its contents made one text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hover {
    /// `MarkupContent`, a `MarkedString`, or a list of `MarkedString`s.
    pub contents: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

impl Hover {
    /// The contents as one text: markup's value, a marked string's code in a
    /// fence of its language, the parts of a list joined by blank lines.
    pub fn text(&self) -> String {
        fn one(v: &Value) -> String {
            match v {
                Value::String(s) => s.clone(),
                Value::Object(m) => {
                    let value = m.get("value").and_then(Value::as_str).unwrap_or_default();
                    match m.get("language").and_then(Value::as_str) {
                        Some(lang) => format!("```{lang}\n{value}\n```"),
                        None => value.to_string(),
                    }
                }
                Value::Array(items) => items.iter().map(one).collect::<Vec<_>>().join("\n\n"),
                _ => String::new(),
            }
        }
        one(&self.contents)
    }
}

/// The spec's `SymbolKind`: 1 file … 26 type parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SymbolKind(pub u8);

impl SymbolKind {
    pub fn name(self) -> &'static str {
        const NAMES: [&str; 26] = [
            "file",
            "module",
            "namespace",
            "package",
            "class",
            "method",
            "property",
            "field",
            "constructor",
            "enum",
            "interface",
            "function",
            "variable",
            "constant",
            "string",
            "number",
            "boolean",
            "array",
            "object",
            "key",
            "null",
            "enum member",
            "struct",
            "event",
            "operator",
            "type parameter",
        ];
        usize::from(self.0)
            .checked_sub(1)
            .and_then(|i| NAMES.get(i))
            .copied()
            .unwrap_or("symbol")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbol {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<DocumentSymbol>,
}

/// A flat symbol: `textDocument/documentSymbol`'s older answer, and
/// `workspace/symbol`'s. A `WorkspaceSymbol` whose location has no range
/// (the server resolves it later) reads with `range` left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInformation {
    pub name: String,
    pub kind: SymbolKind,
    pub location: SymbolLocation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolLocation {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

/// `textDocument/documentSymbol`'s answer in either shape.
#[derive(Debug, Clone, PartialEq)]
pub enum DocumentSymbols {
    Nested(Vec<DocumentSymbol>),
    Flat(Vec<SymbolInformation>),
}

impl DocumentSymbols {
    pub fn from_value(v: Value) -> Result<Self, serde_json::Error> {
        let items = match v {
            Value::Null => return Ok(DocumentSymbols::Nested(Vec::new())),
            Value::Array(items) => items,
            other => return serde_json::from_value(other).map(DocumentSymbols::Nested),
        };
        // The spec's tell: a flat symbol has a location, a nested one a
        // selection range.
        if items.first().is_some_and(|i| i.get("location").is_some()) {
            serde_json::from_value(Value::Array(items)).map(DocumentSymbols::Flat)
        } else {
            serde_json::from_value(Value::Array(items)).map(DocumentSymbols::Nested)
        }
    }

    /// Every symbol with its depth, outer ones first.
    pub fn flatten(&self) -> Vec<(usize, String, SymbolKind, Range)> {
        fn walk(
            s: &[DocumentSymbol],
            depth: usize,
            out: &mut Vec<(usize, String, SymbolKind, Range)>,
        ) {
            for d in s {
                out.push((depth, d.name.clone(), d.kind, d.selection_range));
                walk(&d.children, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        match self {
            DocumentSymbols::Nested(s) => walk(s, 0, &mut out),
            DocumentSymbols::Flat(s) => out.extend(
                s.iter()
                    .filter_map(|i| Some((0, i.name.clone(), i.kind, i.location.range?))),
            ),
        }
        out
    }
}

/// `initialize`'s answer, its capabilities kept raw: what this client reads
/// from them is in [`crate::client::Capabilities`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    #[serde(default)]
    pub capabilities: Value,
    #[serde(default)]
    pub server_info: Option<ServerInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// `workspace/didChangeWatchedFiles`'s kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(into = "u8")]
pub enum FileChangeType {
    Created,
    Changed,
    Deleted,
}

impl From<FileChangeType> for u8 {
    fn from(t: FileChangeType) -> u8 {
        match t {
            FileChangeType::Created => 1,
            FileChangeType::Changed => 2,
            FileChangeType::Deleted => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn r(l0: u32, c0: u32, l1: u32, c1: u32) -> Value {
        json!({"start": {"line": l0, "character": c0}, "end": {"line": l1, "character": c1}})
    }

    #[test]
    fn a_definition_in_each_of_its_shapes() {
        let one = locations(json!({"uri": "file:///a.py", "range": r(1, 2, 1, 5)})).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].range.start, Position::new(1, 2));
        let link = locations(json!([{
            "originSelectionRange": r(0, 0, 0, 3),
            "targetUri": "file:///b.rs",
            "targetRange": r(4, 0, 9, 1),
            "targetSelectionRange": r(4, 3, 4, 8),
        }]))
        .unwrap();
        assert_eq!(link[0].uri, "file:///b.rs");
        assert_eq!(
            link[0].range.start,
            Position::new(4, 3),
            "the name, not the body"
        );
        assert!(locations(Value::Null).unwrap().is_empty());
    }

    #[test]
    fn a_rename_with_document_changes_and_a_file_operation() {
        let e: WorkspaceEdit = serde_json::from_value(json!({
            "documentChanges": [
                {"textDocument": {"uri": "file:///a.ts", "version": 3},
                 "edits": [{"range": r(0, 9, 0, 12), "newText": "total"}]},
                {"kind": "rename", "oldUri": "file:///a.ts", "newUri": "file:///b.ts"},
                {"textDocument": {"uri": "file:///c.ts", "version": null},
                 "edits": [{"range": r(2, 0, 2, 3), "newText": "total", "annotationId": "x"}]}
            ]
        }))
        .unwrap();
        let edits = e.text_edits();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].1, Some(3));
        assert_eq!(edits[1].1, None);
        assert_eq!(edits[1].2[0].new_text, "total");
        assert_eq!(e.operations()[0].kind, "rename");
        let old: WorkspaceEdit = serde_json::from_value(json!({
            "changes": {"file:///a.py": [{"range": r(0, 0, 0, 1), "newText": "y"}]}
        }))
        .unwrap();
        assert_eq!(old.text_edits()[0].0, "file:///a.py");
    }

    #[test]
    fn hover_text_in_each_shape() {
        let h = |c: Value| {
            Hover {
                contents: c,
                range: None,
            }
            .text()
        };
        assert_eq!(h(json!({"kind": "markdown", "value": "**x**"})), "**x**");
        assert_eq!(
            h(json!({"language": "python", "value": "def f()"})),
            "```python\ndef f()\n```"
        );
        assert_eq!(
            h(json!(["a", {"language": "rust", "value": "fn f()"}])),
            "a\n\n```rust\nfn f()\n```"
        );
    }

    #[test]
    fn symbols_in_either_shape() {
        let nested = DocumentSymbols::from_value(json!([{
            "name": "Ledger", "kind": 5, "range": r(0, 0, 9, 0), "selectionRange": r(0, 6, 0, 12),
            "children": [{"name": "total", "kind": 6, "range": r(1, 4, 3, 0), "selectionRange": r(1, 8, 1, 13)}]
        }]))
        .unwrap();
        let flat = nested.flatten();
        assert_eq!(flat.len(), 2);
        assert_eq!(
            (flat[1].0, flat[1].1.as_str(), flat[1].2.name()),
            (1, "total", "method")
        );
        let f = DocumentSymbols::from_value(json!([{
            "name": "main", "kind": 12, "location": {"uri": "file:///m.rs", "range": r(0, 3, 0, 7)},
            "containerName": "m"
        }]))
        .unwrap();
        assert!(
            matches!(&f, DocumentSymbols::Flat(s) if s[0].container_name.as_deref() == Some("m"))
        );
        let ws: SymbolInformation = serde_json::from_value(
            json!({"name": "x", "kind": 13, "location": {"uri": "file:///x.py"}}),
        )
        .unwrap();
        assert_eq!(ws.location.range, None);
    }

    #[test]
    fn diagnostics_and_pull_reports() {
        let d: Diagnostic = serde_json::from_value(json!({
            "range": r(2, 4, 2, 9), "severity": 1, "code": 2322, "source": "ts", "message": "Type 'string' is not assignable"
        }))
        .unwrap();
        assert!(d.is_error());
        assert_eq!(d.code_text().as_deref(), Some("2322"));
        let full: DocumentDiagnosticReport =
            serde_json::from_value(json!({"kind": "full", "resultId": "7", "items": [d]})).unwrap();
        assert!(
            matches!(full, DocumentDiagnosticReport::Full { result_id: Some(ref id), ref items } if id == "7" && items.len() == 1)
        );
        let same: DocumentDiagnosticReport =
            serde_json::from_value(json!({"kind": "unchanged", "resultId": "7"})).unwrap();
        assert!(matches!(same, DocumentDiagnosticReport::Unchanged { .. }));
        assert_eq!(
            serde_json::to_value(FileChangeType::Deleted).unwrap(),
            json!(3)
        );
    }
}
