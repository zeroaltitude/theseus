//! MCP's types, as much of them as v1 reads: the handshake, tools, prompts,
//! and content.
//!
//! A server is untrusted, so every read is lenient where leniency loses
//! nothing (a missing list is empty, a null flag is false), and every
//! type keeps the fields it does not name (`other`), so a stored tool list
//! and its digest lose nothing a server sent. Content stays the JSON the
//! server sent, read through [`Content::view`].

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Map, Value};

/// A client's or a server's name and version (`clientInfo`, `serverInfo`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Implementation {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Implementation {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            ..Self::default()
        }
    }
}

/// `tools` or `prompts` in a server's capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListCapability {
    #[serde(
        default,
        rename = "listChanged",
        deserialize_with = "lenient_bool",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub list_changed: bool,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// What a server says it offers. v1 uses tools and prompts; the rest is
/// kept as sent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ListCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<ListCapability>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// The answer to `initialize`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    #[serde(default)]
    pub capabilities: ServerCapabilities,
    #[serde(default)]
    pub server_info: Implementation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// A server's hints about a tool. They are shown, and never loosen
/// anything: the operator's `read` list decides a tool's class (36b).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// One tool, as `tools/list` gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "empty_object_schema")]
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
    /// Everything else the server sent: `_meta`, `icons`, `execution`, …
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Tool {
    /// A tool the server says must be called as a task (2025-11-25's
    /// `execution.taskSupport = "required"`). This client does not declare
    /// tasks, so such a call fails at the server.
    pub fn requires_task(&self) -> bool {
        self.other
            .get("execution")
            .and_then(|e| e.get("taskSupport"))
            .and_then(Value::as_str)
            == Some("required")
    }
}

fn empty_object_schema() -> Value {
    json!({ "type": "object" })
}

/// One argument a prompt takes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient_bool",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub required: bool,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// One prompt, as `prompts/list` gives it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prompt {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(
        default,
        deserialize_with = "lenient_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub arguments: Vec<PromptArgument>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// One message of a prompt (`prompts/get`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: String,
    pub content: Content,
}

/// The answer to `prompts/get`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GetPromptResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub messages: Vec<PromptMessage>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// The answer to `tools/call`. `is_error` is a failure the model reads, not
/// a protocol error.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    #[serde(default, deserialize_with = "lenient_vec")]
    pub content: Vec<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    #[serde(
        default,
        deserialize_with = "lenient_bool",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub is_error: bool,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl CallToolResult {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::text(text)],
            ..Self::default()
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            is_error: true,
            ..Self::text(text)
        }
    }

    /// The result as text, for a model that reads text: text as itself;
    /// links and embedded resources as text with their URIs; images and
    /// audio as a line naming them (36b gives an image to a model with
    /// vision as an image block); `structuredContent` as JSON, when no
    /// text block already carries it (the spec asks a server to send it as
    /// text too).
    pub fn text_for_model(&self) -> String {
        let mut parts: Vec<String> = self.content.iter().map(Content::as_model_text).collect();
        let has_text = self
            .content
            .iter()
            .any(|c| matches!(c.view(), ContentView::Text(_)));
        if let (Some(s), false) = (&self.structured_content, has_text) {
            parts.push(s.to_string());
        }
        parts.join("\n")
    }
}

/// One content block: the JSON a server sent, kept whole, and read through
/// [`Content::view`]. A block of a kind this crate does not know is kept,
/// never dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Content(pub Value);

/// A content block, read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContentView<'a> {
    Text(&'a str),
    Image {
        data: &'a str,
        mime_type: &'a str,
    },
    Audio {
        data: &'a str,
        mime_type: &'a str,
    },
    /// A link to a resource the server holds (`resource_link`).
    ResourceLink {
        uri: &'a str,
        name: &'a str,
        mime_type: Option<&'a str>,
    },
    /// A resource sent whole (`resource`): text, or a base64 blob.
    Resource {
        uri: &'a str,
        mime_type: Option<&'a str>,
        text: Option<&'a str>,
        blob: Option<&'a str>,
    },
    /// Any other kind, or a block missing what its kind needs.
    Unknown(&'a Value),
}

impl Content {
    pub fn text(text: impl Into<String>) -> Self {
        Self(json!({ "type": "text", "text": text.into() }))
    }

    pub fn image(data_base64: impl Into<String>, mime_type: &str) -> Self {
        Self(json!({ "type": "image", "data": data_base64.into(), "mimeType": mime_type }))
    }

    pub fn kind(&self) -> &str {
        self.0.get("type").and_then(Value::as_str).unwrap_or("")
    }

    pub fn view(&self) -> ContentView<'_> {
        let v = &self.0;
        let s = |k: &str| v.get(k).and_then(Value::as_str);
        let unknown = ContentView::Unknown(v);
        match self.kind() {
            "text" => s("text").map_or(unknown, ContentView::Text),
            "image" | "audio" => match (s("data"), s("mimeType")) {
                (Some(data), Some(mime_type)) if self.kind() == "image" => {
                    ContentView::Image { data, mime_type }
                }
                (Some(data), Some(mime_type)) => ContentView::Audio { data, mime_type },
                _ => unknown,
            },
            "resource_link" => match s("uri") {
                Some(uri) => ContentView::ResourceLink {
                    uri,
                    name: s("name").unwrap_or(""),
                    mime_type: s("mimeType"),
                },
                None => unknown,
            },
            "resource" => {
                let r = v.get("resource");
                let rs = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_str);
                match rs("uri") {
                    Some(uri) => ContentView::Resource {
                        uri,
                        mime_type: rs("mimeType"),
                        text: rs("text"),
                        blob: rs("blob"),
                    },
                    None => unknown,
                }
            }
            _ => unknown,
        }
    }

    /// This block as text for a model (see [`CallToolResult::text_for_model`]).
    pub fn as_model_text(&self) -> String {
        match self.view() {
            ContentView::Text(t) => t.to_string(),
            ContentView::Image { data, mime_type } => {
                format!("[image, {mime_type}, {} bytes of base64]", data.len())
            }
            ContentView::Audio { data, mime_type } => {
                format!("[audio, {mime_type}, {} bytes of base64]", data.len())
            }
            ContentView::ResourceLink { uri, name, .. } => {
                format!("[resource link: {name} <{uri}>]")
            }
            ContentView::Resource {
                uri,
                text: Some(text),
                ..
            } => format!("[resource <{uri}>]\n{text}"),
            ContentView::Resource { uri, mime_type, .. } => format!(
                "[resource <{uri}>, {}, binary]",
                mime_type.unwrap_or("no type")
            ),
            ContentView::Unknown(v) => v.to_string(),
        }
    }
}

/// A flag a careless server may send as null.
fn lenient_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(Option::<bool>::deserialize(d)?.unwrap_or(false))
}

/// A list a careless server may send as null.
fn lenient_vec<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_keeps_what_it_does_not_name() {
        let raw = json!({
            "name": "search",
            "description": "Search the index.",
            "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}},
            "annotations": {"readOnlyHint": true, "experimentalHint": 3},
            "icons": [{"src": "data:,"}],
            "execution": {"taskSupport": "required"}
        });
        let t: Tool = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(t.annotations.as_ref().unwrap().read_only_hint, Some(true));
        assert!(t.requires_task());
        assert_eq!(serde_json::to_value(&t).unwrap(), raw);
        // No schema at all: an object.
        let t: Tool = serde_json::from_value(json!({"name": "bare"})).unwrap();
        assert_eq!(t.input_schema, json!({"type": "object"}));
        assert!(!t.requires_task());
    }

    #[test]
    fn results_read_leniently_and_as_text() {
        let r: CallToolResult = serde_json::from_value(json!({
            "content": [
                {"type": "text", "text": "two links:"},
                {"type": "resource_link", "uri": "demo://a", "name": "a"},
                {"type": "resource", "resource": {"uri": "demo://b", "text": "bee"}},
                {"type": "resource", "resource": {"uri": "demo://c", "blob": "AAAA", "mimeType": "image/png"}},
                {"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"},
                {"type": "hologram"}
            ],
            "isError": null
        }))
        .unwrap();
        assert!(!r.is_error);
        assert_eq!(
            r.text_for_model(),
            "two links:\n[resource link: a <demo://a>]\n[resource <demo://b>]\nbee\n\
             [resource <demo://c>, image/png, binary]\n[image, image/png, 12 bytes of base64]\n\
             {\"type\":\"hologram\"}"
        );
        // Structured content alone is given as JSON; beside text, the text
        // already carries it.
        let r: CallToolResult =
            serde_json::from_value(json!({"content": null, "structuredContent": {"sum": 5}}))
                .unwrap();
        assert_eq!(r.text_for_model(), "{\"sum\":5}");
        let r: CallToolResult = serde_json::from_value(json!({
            "content": [{"type": "text", "text": "{\"sum\": 5}"}],
            "structuredContent": {"sum": 5},
            "isError": true
        }))
        .unwrap();
        assert!(r.is_error);
        assert_eq!(r.text_for_model(), "{\"sum\": 5}");
        // A text block with no text is kept, as an unknown block.
        assert!(matches!(
            Content(json!({"type": "text"})).view(),
            ContentView::Unknown(_)
        ));
    }

    #[test]
    fn the_handshake_and_prompts_read_leniently() {
        let r: InitializeResult = serde_json::from_value(json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {"listChanged": null}, "prompts": {"listChanged": true}, "logging": {}},
            "serverInfo": {"name": "fake"}
        }))
        .unwrap();
        assert!(!r.capabilities.tools.as_ref().unwrap().list_changed);
        assert!(r.capabilities.prompts.as_ref().unwrap().list_changed);
        assert!(r.capabilities.other.contains_key("logging"));
        assert_eq!(r.server_info.name, "fake");
        let p: Prompt = serde_json::from_value(json!({
            "name": "greet",
            "arguments": [{"name": "name", "required": true}, {"name": "tone", "required": null}]
        }))
        .unwrap();
        assert!(p.arguments[0].required && !p.arguments[1].required);
        let p: Prompt = serde_json::from_value(json!({"name": "bare", "arguments": null})).unwrap();
        assert!(p.arguments.is_empty());
    }
}
