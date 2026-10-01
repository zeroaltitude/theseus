//! The value tree the evaluator walks: an API call's JSON input, or a template's properties once their
//! intrinsic functions are resolved as far as a static reading can take them.

use serde_json::Value;

/// Marks the part of a string a static reading could not know (an `Fn::Sub` of an unset parameter,
/// say). A string holding it is a maybe to every test.
pub(crate) const MARK: char = '\u{1}';

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    /// A number, as written.
    Num(String),
    Str(String),
    List(Vec<Node>),
    /// A mapping, in its order.
    Map(Vec<(String, Node)>),
    /// A value no static reading can know (an import, a parameter with no value). Every test asks
    /// about it as a maybe.
    Unresolved(String),
    /// A resource of the same template, by logical id: in this account by construction.
    OwnRef(String),
    /// One of several values, by a condition a static reading cannot decide (`Fn::If`).
    Either(Vec<Node>),
    /// An `Either` branch that removes its member (`AWS::NoValue`).
    Absent,
}

impl Node {
    pub fn from_json(v: &Value) -> Node {
        match v {
            Value::Null => Node::Null,
            Value::Bool(b) => Node::Bool(*b),
            Value::Number(n) => Node::Num(n.to_string()),
            Value::String(s) => Node::Str(s.clone()),
            Value::Array(a) => Node::List(a.iter().map(Node::from_json).collect()),
            Value::Object(o) => Node::Map(
                o.iter()
                    .map(|(k, v)| (k.clone(), Node::from_json(v)))
                    .collect(),
            ),
        }
    }

    /// A member of a mapping, by a name compared without case.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(m) => m
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Back to JSON, for `Fn::ToJsonString` and for messages. What is unknown becomes a marked string.
    pub fn to_json(&self) -> Value {
        match self {
            Node::Null | Node::Absent => Value::Null,
            Node::Bool(b) => Value::Bool(*b),
            Node::Num(n) => n
                .parse::<serde_json::Number>()
                .map(Value::Number)
                .unwrap_or_else(|_| Value::String(n.clone())),
            Node::Str(s) => Value::String(s.clone()),
            Node::List(l) => Value::Array(l.iter().map(Node::to_json).collect()),
            Node::Map(m) => {
                Value::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
            }
            Node::OwnRef(id) => Value::String(id.clone()),
            Node::Unresolved(why) => Value::String(format!("{MARK}{why}{MARK}")),
            Node::Either(_) => Value::String(format!("{MARK}one of several values{MARK}")),
        }
    }

    /// A short rendering for a hit's detail.
    pub fn brief(&self) -> String {
        match self {
            Node::Str(s) => s.replace(MARK, "?"),
            Node::Num(n) => n.clone(),
            Node::Bool(b) => b.to_string(),
            Node::Null | Node::Absent => "null".into(),
            Node::OwnRef(id) => format!("this stack's {id}"),
            Node::Unresolved(why) => format!("unresolved ({why})"),
            Node::Either(_) => "one of several values".into(),
            Node::List(_) | Node::Map(_) => {
                let s = self.to_json().to_string().replace(MARK, "?");
                if s.chars().count() > 80 {
                    format!("{}…", s.chars().take(80).collect::<String>())
                } else {
                    s
                }
            }
        }
    }
}

pub(crate) fn has_mark(s: &str) -> bool {
    s.contains(MARK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members_are_found_without_case() {
        let n = Node::from_json(&serde_json::json!({"AuthType": "NONE"}));
        assert_eq!(n.get("authtype").and_then(Node::as_str), Some("NONE"));
        assert_eq!(n.get("missing"), None);
    }
}
