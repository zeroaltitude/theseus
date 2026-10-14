//! What a response says of the tool search (theseus-7m7w.2): the searches
//! the model made and the tools they surfaced, read from the response's
//! blocks. On Anthropic's API a search is a `server_tool_use` block (the tool
//! search tool, the query in its input) followed by a `tool_search_tool_result`
//! block whose `tool_reference`s name the tools it found; both ride in the
//! assistant message as the provider returned them and go back unchanged.

use serde_json::Value;

use super::SEARCH_TOOL;

/// One search the provider ran: its query and the tools it surfaced.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub query: String,
    pub tools: Vec<String>,
}

/// The searches in a response's content blocks, in order.
pub fn searches_in(blocks: &[Value]) -> Vec<Found> {
    let mut out: Vec<(String, Found)> = Vec::new();
    for b in blocks {
        match b.get("type").and_then(Value::as_str) {
            Some("server_tool_use")
                if b.get("name").and_then(Value::as_str) == Some(SEARCH_TOOL) =>
            {
                let id = b.get("id").and_then(Value::as_str).unwrap_or("");
                let query = b
                    .pointer("/input/query")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                out.push((
                    id.to_string(),
                    Found {
                        query: query.to_string(),
                        tools: vec![],
                    },
                ));
            }
            Some("tool_search_tool_result") => {
                let id = b.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
                let names: Vec<String> = match b.get("content") {
                    Some(Value::Object(o)) => o
                        .get("tool_references")
                        .and_then(Value::as_array)
                        .map(|a| names_of(a))
                        .unwrap_or_default(),
                    Some(Value::Array(a)) => names_of(a),
                    _ => vec![],
                };
                match out.iter_mut().find(|(i, _)| i == id) {
                    Some((_, f)) => f.tools = names,
                    // A result with no call in this response (the call was in
                    // an earlier message): still a search that surfaced.
                    None => out.push((
                        id.to_string(),
                        Found {
                            query: String::new(),
                            tools: names,
                        },
                    )),
                }
            }
            _ => {}
        }
    }
    out.into_iter().map(|(_, f)| f).collect()
}

fn names_of(a: &[Value]) -> Vec<String> {
    a.iter()
        .filter(|r| r.get("type").and_then(Value::as_str) == Some("tool_reference"))
        .filter_map(|r| r.get("tool_name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_search_and_its_references_are_read_together() {
        let blocks = vec![
            json!({"type": "text", "text": "I will look."}),
            json!({"type": "server_tool_use", "id": "srvtoolu_1", "name": SEARCH_TOOL,
                   "input": {"query": "aws s3 bucket"}}),
            json!({"type": "tool_search_tool_result", "tool_use_id": "srvtoolu_1",
                   "content": {"type": "tool_search_tool_search_result",
                               "tool_references": [
                                   {"type": "tool_reference", "tool_name": "aws_s3_list"},
                                   {"type": "tool_reference", "tool_name": "aws_call"}]}}),
            json!({"type": "tool_use", "id": "toolu_1", "name": "aws_call", "input": {}}),
        ];
        let f = searches_in(&blocks);
        assert_eq!(
            f,
            vec![Found {
                query: "aws s3 bucket".into(),
                tools: vec!["aws_s3_list".into(), "aws_call".into()]
            }]
        );
    }

    #[test]
    fn other_server_tools_and_errors_surface_nothing() {
        let blocks = vec![
            json!({"type": "server_tool_use", "id": "a", "name": "web_search", "input": {"query": "x"}}),
            json!({"type": "server_tool_use", "id": "b", "name": SEARCH_TOOL, "input": {"query": "y"}}),
            json!({"type": "tool_search_tool_result", "tool_use_id": "b",
                   "content": {"type": "tool_search_tool_result_error", "error_code": "unavailable"}}),
        ];
        let f = searches_in(&blocks);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].query, "y");
        assert!(f[0].tools.is_empty());
        assert!(searches_in(&[json!({"type": "text", "text": "hi"})]).is_empty());
    }
}
