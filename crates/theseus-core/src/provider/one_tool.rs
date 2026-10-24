//! A call that must answer through one tool (theseus-yym79). Sonnet 5.5, Opus
//! 5.5 and Fable 5.1 refuse a forced `tool_choice` with a 400, so the tool is
//! forced only where the model's thinking is off in the plain sense
//! (`Catalog::may_force`); everywhere else the model chooses (`auto`) and the
//! tool is strict with a closed schema, so a call it makes is still whole.

use serde_json::{json, Value};

/// How a one-tool call asks: the `thinking` it sends
/// (`Catalog::thinking_off`) and whether it may force its tool.
#[derive(Debug, Clone, PartialEq)]
pub struct Forcing {
    pub thinking: Option<Value>,
    pub force: bool,
}

/// `tool` and the `tool_choice` that goes with it. Forced: the tool as it is
/// and `{"type":"tool"}`. Not forced: `{"type":"auto"}`, and the tool strict
/// with `additionalProperties: false` on every object of its schema.
pub fn one_tool(mut tool: Value, force: bool) -> (Value, Value) {
    if force {
        let choice = json!({"type": "tool", "name": tool["name"]});
        return (tool, choice);
    }
    tool["strict"] = json!(true);
    close(&mut tool["input_schema"]);
    (tool, json!({"type": "auto"}))
}

/// Closes every object schema under `schema`: its own, its properties', and
/// those of the `items` of an array.
fn close(schema: &mut Value) {
    if schema["type"] == "object" {
        schema["additionalProperties"] = json!(false);
    }
    if let Some(props) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        props.values_mut().for_each(close);
    }
    if let Some(items) = schema.get_mut("items") {
        close(items);
    }
}

/// What a one-tool call sends to Haiku 5.5 and to Sonnet 5.5, for the test
/// of each call: forced with thinking `disabled` on the first; `auto` with a
/// strict, closed tool and `between_tools` on the second (theseus-yym79).
#[cfg(test)]
pub fn assert_both_shapes(build: impl Fn(Forcing) -> super::ProviderRequest, tool: &str) {
    let c = crate::catalog::Catalog::builtin();
    let r = build(c.forcing("claude-haiku-5-5"));
    assert_eq!(
        r.extra["tool_choice"],
        json!({"type": "tool", "name": tool})
    );
    assert_eq!(r.thinking, Some(json!({"type": "disabled"})));
    assert_unforced(&build(c.forcing("claude-sonnet-5-5")), tool);
}

/// The Sonnet half: a request that leaves the one tool to `auto`, with
/// thinking `between_tools` and the tool strict and closed. A call site's test
/// through a core on a Sonnet profile holds its request to this.
#[cfg(test)]
pub fn assert_unforced(r: &super::ProviderRequest, tool: &str) {
    assert_eq!(r.extra["tool_choice"], json!({"type": "auto"}));
    assert_eq!(r.thinking, Some(json!({"type": "between_tools"})));
    assert_eq!(r.tools[0]["name"], tool);
    assert_eq!(r.tools[0]["strict"], true);
    assert_eq!(r.tools[0]["input_schema"]["additionalProperties"], false);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({"name": "pick", "description": "d", "input_schema": {
            "type": "object",
            "properties": {
                "list": {"type": "array", "items": {"type": "object",
                    "properties": {"a": {"type": "string"}}, "required": ["a"]}},
                "one": {"type": "string"}
            },
            "required": ["list"]
        }})
    }

    /// Forced, the tool is untouched; not forced, it is strict and closed all
    /// the way down, and `required` stays. (Planted revert: always force.)
    #[test]
    fn a_tool_is_forced_untouched_or_left_to_auto_strict_and_closed() {
        let (tool, choice) = one_tool(sample(), true);
        assert_eq!(tool, sample());
        assert_eq!(choice, json!({"type": "tool", "name": "pick"}));
        let (tool, choice) = one_tool(sample(), false);
        assert_eq!(choice, json!({"type": "auto"}));
        assert_eq!(tool["strict"], true);
        let s = &tool["input_schema"];
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(
            s["properties"]["list"]["items"]["additionalProperties"],
            false
        );
        assert!(s["properties"]["one"].get("additionalProperties").is_none());
        assert_eq!(s["required"], json!(["list"]));
        assert_eq!(s["properties"]["list"]["items"]["required"], json!(["a"]));
    }
}
