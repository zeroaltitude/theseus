//! The extraction (theseus-wy7y): the request a cheap profile answers with
//! a session's candidate people through one tool, and the answer read back.
//! The instructions are short: the tool's schema and its field descriptions
//! carry the contract (the repository's minimal-prompting rule). Every line
//! is scrubbed before it is sent, as every outbound judgment's state is.

use std::collections::HashMap;

use serde_json::{json, Value};
use theseus_judge::builders::PersonCandidate;

use super::{fold, Line, CANDIDATES};
use crate::provider::ProviderRequest;

/// The one tool the extractor answers through.
pub const TOOL: &str = "propose_people";
/// The answer's tokens at most: a dozen candidates, each a line or two.
pub const MAX_TOKENS: u32 = 1500;

/// What the extractor is told, beside the tool.
pub const INSTRUCTIONS: &str = "List the people in this session's text with the propose_people \
tool. People only: not agents, assistants, bots, products, codenames, programs or companies. A \
role line says what the person does or owns (role, projects, channels), never an evaluation of \
them; no line at all is better than a guessed one.";

/// The tool's schema: the contract.
pub fn tool() -> Value {
    json!({
        "name": TOOL,
        "description": "The people this session's text names or quotes.",
        "input_schema": {
            "type": "object",
            "properties": {
                "people": {
                    "type": "array",
                    "description": "One entry per person; none when the text names no person.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "The person's name as the text writes it."},
                            "handles": {"type": "array", "items": {"type": "string"},
                                "description": "Handles the text gives for them, as kind:value: slack:<user id>, discord:<user id>, email:<address>. Empty when the text gives none."},
                            "role_line": {"type": "string",
                                "description": "What they do or own (role, projects, channels), from the text. Empty when the text does not say. Never a judgment of them."},
                            "evidence": {"type": "array", "items": {"type": "string"},
                                "description": "The labels (L1, L2, ...) of the lines that name them."}
                        },
                        "required": ["name", "handles", "role_line", "evidence"]
                    }
                }
            },
            "required": ["people"]
        }
    })
}

/// The request: the session's title and its lines, each labelled `L<n>`
/// with its author, scrubbed by `scrub`.
pub fn request(
    model: &str,
    title: &str,
    lines: &[Line],
    scrub: &dyn Fn(&str) -> String,
) -> ProviderRequest {
    let body: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("L{} {}: {}", i + 1, scrub(&l.author), scrub(&l.text)))
        .collect();
    let content = format!("Session: {}\n\n{}", scrub(title.trim()), body.join("\n"));
    let mut extra = std::collections::BTreeMap::new();
    extra.insert(
        "tool_choice".to_string(),
        json!({"type": "tool", "name": TOOL}),
    );
    ProviderRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: vec![json!({"type": "text", "text": INSTRUCTIONS})],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": content}]})],
        tools: vec![tool()],
        thinking: None,
        output_config: None,
        cache_control: None,
        betas: Vec::new(),
        extra,
        image_tokens: 0,
    }
}

/// The candidates of an answer's `propose_people` call: each with a name,
/// its handles those that read as handles, its evidence the lines' texts
/// (by label) and its node ids; one per folded name, at most
/// [`CANDIDATES`]. `None` when the answer holds no such call.
pub fn parse(content: &[Value], lines: &[Line]) -> Option<Vec<(PersonCandidate, Vec<String>)>> {
    let input = content
        .iter()
        .find(|b| b["type"] == "tool_use" && b["name"] == TOOL)?
        .get("input")?;
    let people = input["people"].as_array()?;
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut out = Vec::new();
    for p in people {
        let name: String = p["name"]
            .as_str()
            .unwrap_or("")
            .trim()
            .chars()
            .take(120)
            .collect();
        if name.is_empty() || seen.insert(fold(&name), ()).is_some() {
            continue;
        }
        let handles: Vec<String> = p["handles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|h| theseus_ontology::handle(h.as_str()?).ok())
            .filter(|h| !theseus_ontology::person::is_name(h))
            .take(8)
            .collect();
        let role_line: String = p["role_line"]
            .as_str()
            .unwrap_or("")
            .trim()
            .chars()
            .take(300)
            .collect();
        let mut evidence = Vec::new();
        let mut nodes = Vec::new();
        for label in p["evidence"].as_array().into_iter().flatten() {
            let n = label.as_str().and_then(|s| {
                s.trim()
                    .trim_start_matches(['L', 'l'])
                    .parse::<usize>()
                    .ok()
            });
            if let Some(l) = n.and_then(|n| lines.get(n.checked_sub(1)?)) {
                if !nodes.contains(&l.node) {
                    nodes.push(l.node.clone());
                    evidence.push(format!("{}: {}", l.author, l.text));
                }
            }
        }
        out.push((
            PersonCandidate {
                name,
                handles,
                role_line,
                evidence,
            },
            nodes,
        ));
        if out.len() == CANDIDATES {
            break;
        }
    }
    Some(out)
}
