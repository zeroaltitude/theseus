//! An IAM policy document as the narrowing reads it, and the containment
//! proof: a candidate's allows are never wider than the current ones, on any
//! statement.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::glob::covers;

/// A statement's effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Allow,
    Deny,
}

/// One statement. `Not*` forms are kept whole and flagged: the proof does
/// not read them, so a document that allows through one cannot be narrowed.
#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub sid: Option<String>,
    pub effect: Effect,
    pub actions: Vec<String>,
    pub resources: Vec<String>,
    pub condition: Option<Value>,
    /// The statement as written, when it uses `NotAction` or `NotResource`.
    pub raw_not: Option<Value>,
}

/// A policy document: its statements, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Doc {
    pub stmts: Vec<Stmt>,
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

impl Doc {
    /// A document from JSON, or from the percent-encoded text IAM's query
    /// protocol returns.
    pub fn parse(text: &str) -> Result<Doc, String> {
        let decoded;
        let text = if text.trim_start().starts_with('%') {
            decoded = percent_decode(text);
            decoded.as_str()
        } else {
            text
        };
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        Doc::from_value(&v)
    }

    pub fn from_value(v: &Value) -> Result<Doc, String> {
        let st = match &v["Statement"] {
            Value::Array(a) => a.clone(),
            o @ Value::Object(_) => vec![o.clone()],
            _ => return Err("the document has no Statement".into()),
        };
        let mut stmts = Vec::new();
        for s in &st {
            let effect = match s["Effect"].as_str() {
                Some("Allow") => Effect::Allow,
                Some("Deny") => Effect::Deny,
                _ => return Err("a statement has no Effect".into()),
            };
            let not = s.get("NotAction").is_some() || s.get("NotResource").is_some();
            stmts.push(Stmt {
                sid: s["Sid"].as_str().map(String::from),
                effect,
                actions: strings(&s["Action"]),
                resources: strings(&s["Resource"]),
                condition: s.get("Condition").filter(|c| !c.is_null()).cloned(),
                raw_not: not.then(|| s.clone()),
            });
        }
        Ok(Doc { stmts })
    }

    pub fn to_value(&self) -> Value {
        let st: Vec<Value> = self
            .stmts
            .iter()
            .map(|s| {
                if let Some(raw) = &s.raw_not {
                    return raw.clone();
                }
                let mut o = json!({
                    "Effect": match s.effect { Effect::Allow => "Allow", Effect::Deny => "Deny" },
                    "Action": s.actions,
                    "Resource": s.resources,
                });
                if let Some(sid) = &s.sid {
                    o["Sid"] = json!(sid);
                }
                if let Some(c) = &s.condition {
                    o["Condition"] = c.clone();
                }
                o
            })
            .collect();
        json!({"Version": "2012-10-17", "Statement": st})
    }

    pub fn allows(&self) -> impl Iterator<Item = &Stmt> {
        self.stmts.iter().filter(|s| s.effect == Effect::Allow)
    }

    pub fn denies(&self) -> impl Iterator<Item = &Stmt> {
        self.stmts.iter().filter(|s| s.effect == Effect::Deny)
    }

    /// Every distinct action pattern the allows name.
    pub fn allowed_actions(&self) -> BTreeSet<String> {
        self.allows()
            .flat_map(|s| s.actions.iter().cloned())
            .collect()
    }

    /// The first allow statement covering `action`, with no `Not*` form.
    pub fn covering(&self, action: &str) -> Option<&Stmt> {
        self.allows()
            .find(|s| s.raw_not.is_none() && s.actions.iter().any(|a| covers(a, action, true)))
    }

    /// Whether the document allows `action`, by any of its allows.
    pub fn allows_action(&self, action: &str) -> bool {
        self.covering(action).is_some()
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether any allow of `d` covers `action` (a test simulator's reading).
#[cfg(test)]
pub fn covers_action(d: &Doc, action: &str) -> bool {
    d.allows_action(action)
}

/// Why `candidate` is wider than `current`, when it is: every (action,
/// resource) pair of a candidate allow must sit inside one current allow
/// whose condition is the candidate's own (or none). Empty means contained.
/// A current allow with a `Not*` form is not read, so a candidate allow it
/// alone could cover is reported.
pub fn wider(candidate: &Doc, current: &Doc) -> Vec<String> {
    let mut out = Vec::new();
    for c in candidate.allows() {
        if c.raw_not.is_some() {
            out.push(format!(
                "statement {} uses NotAction or NotResource, which the proof does not read",
                c.sid.as_deref().unwrap_or("?")
            ));
            continue;
        }
        for a in &c.actions {
            for r in &c.resources {
                let inside = current.allows().any(|s| {
                    s.raw_not.is_none()
                        && s.actions.iter().any(|x| covers(x, a, true))
                        && s.resources.iter().any(|x| covers(x, r, false))
                        && (s.condition.is_none() || s.condition == c.condition)
                });
                if !inside {
                    out.push(format!(
                        "{a} on {r} is not inside any statement the current policy allows"
                    ));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(v: Value) -> Doc {
        Doc::from_value(&v).unwrap()
    }

    #[test]
    fn a_document_round_trips_and_decodes_iams_encoding() {
        let v = json!({"Version": "2012-10-17", "Statement": [
            {"Sid": "A", "Effect": "Allow", "Action": ["s3:GetObject"], "Resource": ["*"]},
            {"Effect": "Deny", "Action": ["iam:*"], "Resource": ["*"],
             "Condition": {"Bool": {"x": "true"}}}]});
        assert_eq!(doc(v.clone()).to_value(), v);
        let enc = "%7B%22Statement%22%3A%7B%22Effect%22%3A%22Allow%22%2C%22Action%22%3A%22s3%3A%2A%22%2C%22Resource%22%3A%22%2A%22%7D%7D";
        let d = Doc::parse(enc).unwrap();
        assert_eq!(d.stmts[0].actions, ["s3:*"]);
    }

    #[test]
    fn a_narrower_document_is_contained_and_a_wider_one_is_named() {
        let cur = doc(json!({"Statement": [
            {"Effect": "Allow", "Action": ["s3:*"], "Resource": ["arn:aws:s3:::b/*"]}]}));
        let inside = doc(json!({"Statement": [
            {"Effect": "Allow", "Action": ["s3:GetObject"], "Resource": ["arn:aws:s3:::b/hands/*"]}]}));
        assert!(wider(&inside, &cur).is_empty());
        let resource = doc(json!({"Statement": [
            {"Effect": "Allow", "Action": ["s3:GetObject"], "Resource": ["*"]}]}));
        assert_eq!(wider(&resource, &cur).len(), 1);
        let action = doc(json!({"Statement": [
            {"Effect": "Allow", "Action": ["ec2:Describe*"], "Resource": ["arn:aws:s3:::b/x"]}]}));
        assert_eq!(wider(&action, &cur).len(), 1);
    }

    #[test]
    fn a_condition_the_current_statement_has_must_be_kept() {
        let cur = doc(json!({"Statement": [{"Effect": "Allow", "Action": ["s3:*"],
            "Resource": ["*"], "Condition": {"StringEquals": {"aws:RequestedRegion": "us-west-2"}}}]}));
        let dropped = doc(json!({"Statement": [
            {"Effect": "Allow", "Action": ["s3:GetObject"], "Resource": ["*"]}]}));
        assert_eq!(wider(&dropped, &cur).len(), 1);
        let kept = doc(
            json!({"Statement": [{"Effect": "Allow", "Action": ["s3:GetObject"],
            "Resource": ["*"], "Condition": {"StringEquals": {"aws:RequestedRegion": "us-west-2"}}}]}),
        );
        assert!(wider(&kept, &cur).is_empty());
    }

    #[test]
    fn a_not_action_statement_is_never_read_as_covering() {
        let cur = doc(json!({"Statement": [
            {"Effect": "Allow", "NotAction": ["iam:*"], "Resource": ["*"]}]}));
        let c = doc(json!({"Statement": [
            {"Effect": "Allow", "Action": ["s3:GetObject"], "Resource": ["*"]}]}));
        assert_eq!(wider(&c, &cur).len(), 1);
    }
}
