//! Enforcer 1 for stacks: a CloudFormation template's resources checked against the list before
//! `aws.stack.plan` makes its change set (the design's §3.4 and §3.6).
//!
//! Properties are read after their intrinsic functions are resolved as far as a static reading can:
//! parameters (the stack's values, else their defaults), pseudo parameters, mappings, conditions,
//! `Fn::Sub`, `Fn::Join`, and `Fn::GetAtt` of a property the same template sets. What stays unknown
//! (an import, an `Fn::If` on an undecidable condition) is a maybe, and a maybe asks as a hit does.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};

use crate::cidr;
use crate::eval::{Context, Fired, Truth};
use crate::list::{GuardList, Guardrail};
use crate::node::{has_mark, Node, MARK};

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("the template is neither YAML nor JSON: {0}")]
    Syntax(String),
    #[error("the template has no Resources mapping")]
    NoResources,
}

/// Parse a template, YAML or JSON. Short-form tags become their long forms: `!Ref X` is `{Ref: X}`,
/// `!GetAtt A.B` is `{Fn::GetAtt: [A, B]}`, and `!Sub s` is `{Fn::Sub: s}`.
pub fn parse_template(text: &str) -> Result<Node, TemplateError> {
    if text.trim_start().starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
            return Ok(Node::from_json(&v));
        }
    }
    let mut b = Builder::default();
    Parser::new_from_str(text)
        .load(&mut b, false)
        .map_err(|e| TemplateError::Syntax(e.to_string()))?;
    b.root
        .ok_or_else(|| TemplateError::Syntax("the document is empty".into()))
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    anchors: HashMap<usize, Node>,
    root: Option<Node>,
}

struct Frame {
    seq: bool,
    items: Vec<Node>,
    entries: Vec<(String, Node)>,
    key: Option<String>,
    anchor: usize,
    tag: Option<Tag>,
}

impl Frame {
    fn new(seq: bool, anchor: usize, tag: Option<Tag>) -> Frame {
        Frame {
            seq,
            items: Vec::new(),
            entries: Vec::new(),
            key: None,
            anchor,
            tag,
        }
    }
}

impl MarkedEventReceiver for Builder {
    fn on_event(&mut self, ev: Event, _mark: Marker) {
        match ev {
            Event::Scalar(v, style, anchor, tag) => {
                let n = tagged(tag.as_ref(), scalar(v, style, tag.as_ref()));
                self.done(anchor, n);
            }
            Event::SequenceStart(anchor, tag) => self.stack.push(Frame::new(true, anchor, tag)),
            Event::MappingStart(anchor, tag) => self.stack.push(Frame::new(false, anchor, tag)),
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some(f) = self.stack.pop() {
                    let n = if f.seq {
                        Node::List(f.items)
                    } else {
                        Node::Map(f.entries)
                    };
                    self.done(f.anchor, tagged(f.tag.as_ref(), n));
                }
            }
            Event::Alias(id) => {
                let n = self
                    .anchors
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| Node::Unresolved("a YAML alias to nothing".into()));
                self.push(n);
            }
            _ => {}
        }
    }
}

impl Builder {
    fn done(&mut self, anchor: usize, n: Node) {
        if anchor > 0 {
            self.anchors.insert(anchor, n.clone());
        }
        self.push(n);
    }

    fn push(&mut self, n: Node) {
        match self.stack.last_mut() {
            None => {
                if self.root.is_none() {
                    self.root = Some(n);
                }
            }
            Some(f) if f.seq => f.items.push(n),
            Some(f) => match f.key.take() {
                None => f.key = Some(key_text(&n)),
                Some(k) => f.entries.push((k, n)),
            },
        }
    }
}

fn key_text(n: &Node) -> String {
    match n {
        Node::Str(s) | Node::Num(s) => s.clone(),
        Node::Bool(b) => b.to_string(),
        Node::Null => String::new(),
        other => other.to_json().to_string(),
    }
}

fn scalar(v: String, style: TScalarStyle, tag: Option<&Tag>) -> Node {
    if style != TScalarStyle::Plain || tag.is_some() {
        return Node::Str(v);
    }
    match v.as_str() {
        "" | "~" | "null" | "Null" | "NULL" => Node::Null,
        "true" | "True" | "TRUE" => Node::Bool(true),
        "false" | "False" | "FALSE" => Node::Bool(false),
        s if s.parse::<f64>().is_ok()
            && s.chars().all(|c| c.is_ascii_digit() || "+-.eE".contains(c)) =>
        {
            Node::Num(v)
        }
        _ => Node::Str(v),
    }
}

/// CloudFormation's short forms, as their long forms. YAML's own tags (`!!str`) leave the value as it is.
fn tagged(tag: Option<&Tag>, n: Node) -> Node {
    let Some(t) = tag else { return n };
    if t.handle != "!" {
        return n;
    }
    let name = t.suffix.as_str();
    match name {
        "Ref" | "Condition" => Node::Map(vec![(name.to_string(), n)]),
        "GetAtt" => {
            let args = match n {
                Node::Str(s) => match s.split_once('.') {
                    Some((a, b)) => Node::List(vec![Node::Str(a.into()), Node::Str(b.into())]),
                    None => Node::Str(s),
                },
                other => other,
            };
            Node::Map(vec![("Fn::GetAtt".into(), args)])
        }
        _ => Node::Map(vec![(format!("Fn::{name}"), n)]),
    }
}

/// A template's guardrail hits.
#[derive(Debug)]
pub struct Scan<'l> {
    pub hits: Vec<TemplateHit<'l>>,
    /// What the scan could not see, for the plan's diff node: a transform, a loop, a resource with no type.
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub struct TemplateHit<'l> {
    pub guardrail: &'l Guardrail,
    pub logical_id: String,
    pub resource_type: String,
    /// False when the template left the answer open; it asks all the same.
    pub certain: bool,
    pub detail: Option<String>,
}

impl TemplateHit<'_> {
    /// The floor's confirm line: "public ingress: WebSg (AWS::EC2::SecurityGroup), a security group rule
    /// open to … (SecurityGroupIngress[0].CidrIp = 0.0.0.0/0)".
    pub fn confirm(&self) -> String {
        let g = self.guardrail;
        let mut s = format!(
            "{}: {} ({}), {}",
            g.limit.label(),
            self.logical_id,
            self.resource_type,
            g.summary
        );
        if let Some(d) = &self.detail {
            s.push_str(&format!(" ({d})"));
        }
        if !self.certain {
            s.push_str(" [may hit: a value is unresolved]");
        }
        s
    }
}

impl GuardList {
    /// Scan a parsed template. `parameters` are the stack's values; one the stack leaves unset takes its
    /// `Default`.
    pub fn scan(
        &self,
        template: &Node,
        ctx: &Context,
        parameters: &BTreeMap<String, String>,
    ) -> Result<Scan<'_>, TemplateError> {
        let Some(Node::Map(resources)) = template.get("Resources") else {
            return Err(TemplateError::NoResources);
        };
        let r = Resolver::new(template, ctx, parameters);
        let mut scan = Scan {
            hits: Vec::new(),
            notes: Vec::new(),
        };
        if let Some(t) = template.get("Transform") {
            scan.notes.push(format!(
                "the template names a transform ({}): scan the processed template too",
                t.brief()
            ));
        }
        for (id, res) in resources {
            if id.starts_with("Fn::ForEach::") {
                scan.notes.push(format!(
                    "{id}: Fn::ForEach makes resources a static scan cannot see; scan the processed template"
                ));
                continue;
            }
            let Some(ty) = res.get("Type").and_then(Node::as_str) else {
                scan.notes.push(format!("{id}: no Type"));
                continue;
            };
            let (exists, condition) = match res.get("Condition").and_then(Node::as_str) {
                None => (Truth::Yes, None),
                Some(c) => match r.condition(c) {
                    Some(true) => (Truth::Yes, None),
                    Some(false) => continue,
                    None => (Truth::Maybe, Some(c)),
                },
            };
            let props = match res.get("Properties") {
                Some(p) => r.resolve(p, 0),
                None => Node::Map(Vec::new()),
            };
            for g in &self.guardrails {
                for rule in g
                    .template
                    .iter()
                    .filter(|t| t.types.iter().any(|t| t == ty))
                {
                    let fired = match &rule.when {
                        None => Fired {
                            truth: Truth::Yes,
                            detail: None,
                        },
                        Some(w) => w.eval(&props, ctx),
                    }
                    .capped(exists);
                    if fired.truth.hits() {
                        let detail = match (fired.detail, condition) {
                            (d, None) => d,
                            (Some(d), Some(c)) => Some(format!("{d}, if condition {c} holds")),
                            (None, Some(c)) => Some(format!("if condition {c} holds")),
                        };
                        scan.hits.push(TemplateHit {
                            guardrail: g,
                            logical_id: id.clone(),
                            resource_type: ty.to_string(),
                            certain: fired.truth == Truth::Yes,
                            detail,
                        });
                        break;
                    }
                }
            }
        }
        Ok(scan)
    }
}

/// Resolves intrinsic functions as far as a static reading can.
struct Resolver<'t> {
    ctx: &'t Context,
    params: BTreeMap<String, Node>,
    mappings: Option<&'t Node>,
    conditions: Option<&'t Node>,
    resources: Option<&'t Node>,
    memo: RefCell<HashMap<String, Option<bool>>>,
}

const DEPTH: usize = 48;

/// A mapping's member by its exact name: templates are case-sensitive.
fn exact<'n>(n: &'n Node, key: &str) -> Option<&'n Node> {
    match n {
        Node::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

/// A value's text when it is fully known.
fn plain(n: &Node) -> Option<String> {
    match n {
        Node::Str(s) if !has_mark(s) => Some(s.clone()),
        Node::Num(s) => Some(s.clone()),
        Node::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A value inside a string: known text, a same-stack resource's logical id, or a marked unknown.
fn text_of(n: &Node) -> String {
    match n {
        Node::Str(s) | Node::Num(s) => s.clone(),
        Node::Bool(b) => b.to_string(),
        Node::OwnRef(id) => id.clone(),
        Node::Unresolved(why) => format!("{MARK}{why}{MARK}"),
        _ => format!("{MARK}a value that is not text{MARK}"),
    }
}

impl<'t> Resolver<'t> {
    fn new(t: &'t Node, ctx: &'t Context, given: &BTreeMap<String, String>) -> Resolver<'t> {
        let mut params = BTreeMap::new();
        if let Some(Node::Map(ps)) = exact(t, "Parameters") {
            for (name, def) in ps {
                let ty = exact(def, "Type")
                    .and_then(Node::as_str)
                    .unwrap_or("String");
                let value = match given.get(name) {
                    Some(v) => Some(v.clone()),
                    None => exact(def, "Default").and_then(plain),
                };
                let value = match value {
                    Some(v) if ty.starts_with("List<") || ty == "CommaDelimitedList" => Node::List(
                        v.split(',')
                            .map(|s| Node::Str(s.trim().to_string()))
                            .collect(),
                    ),
                    Some(v) => Node::Str(v),
                    None => Node::Unresolved(format!("parameter {name} has no value")),
                };
                params.insert(name.clone(), value);
            }
        }
        Resolver {
            ctx,
            params,
            mappings: exact(t, "Mappings"),
            conditions: exact(t, "Conditions"),
            resources: exact(t, "Resources"),
            memo: RefCell::new(HashMap::new()),
        }
    }

    fn resolve(&self, n: &Node, depth: usize) -> Node {
        if depth > DEPTH {
            return Node::Unresolved("nested too deep".into());
        }
        match n {
            Node::Map(m) if m.len() == 1 && (m[0].0 == "Ref" || m[0].0.starts_with("Fn::")) => {
                self.intrinsic(&m[0].0, &m[0].1, depth + 1)
            }
            Node::Map(m) => Node::Map(
                m.iter()
                    .filter_map(|(k, v)| {
                        let r = self.resolve(v, depth + 1);
                        (r != Node::Absent).then(|| (k.clone(), r))
                    })
                    .collect(),
            ),
            Node::List(l) => Node::List(
                l.iter()
                    .map(|v| self.resolve(v, depth + 1))
                    .filter(|r| *r != Node::Absent)
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn intrinsic(&self, name: &str, arg: &Node, depth: usize) -> Node {
        match name {
            "Ref" => match arg.as_str() {
                Some(s) => self.reference(s),
                None => Node::Unresolved("a Ref to something that is not a name".into()),
            },
            "Fn::GetAtt" => match arg {
                Node::List(l) if l.len() == 2 => match (plain(&l[0]), plain(&l[1])) {
                    (Some(a), Some(b)) => self.get_att(&a, &b, depth),
                    _ => Node::Unresolved("Fn::GetAtt of a computed name".into()),
                },
                Node::Str(s) => match s.split_once('.') {
                    Some((a, b)) => self.get_att(a, b, depth),
                    None => Node::Unresolved(format!("Fn::GetAtt {s}")),
                },
                _ => Node::Unresolved("Fn::GetAtt of an odd shape".into()),
            },
            "Fn::Sub" => self.sub(arg, depth),
            "Fn::Join" => {
                let Node::List(l) = arg else {
                    return Node::Unresolved("Fn::Join of an odd shape".into());
                };
                let (Some(delim), Some(parts)) = (l.first().and_then(plain), l.get(1)) else {
                    return Node::Unresolved("Fn::Join of an odd shape".into());
                };
                match self.resolve(parts, depth) {
                    Node::List(items) => {
                        Node::Str(items.iter().map(text_of).collect::<Vec<_>>().join(&delim))
                    }
                    other => Node::Unresolved(format!("Fn::Join of {}", other.brief())),
                }
            }
            "Fn::Select" => {
                let Node::List(l) = arg else {
                    return Node::Unresolved("Fn::Select of an odd shape".into());
                };
                let index = l
                    .first()
                    .map(|i| self.resolve(i, depth))
                    .and_then(|i| plain(&i))
                    .and_then(|i| i.parse::<usize>().ok());
                match (index, l.get(1).map(|v| self.resolve(v, depth))) {
                    (Some(i), Some(Node::List(items))) => items
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| Node::Unresolved("Fn::Select past the end".into())),
                    _ => Node::Unresolved("Fn::Select of an unknown list".into()),
                }
            }
            "Fn::Split" => {
                let Node::List(l) = arg else {
                    return Node::Unresolved("Fn::Split of an odd shape".into());
                };
                match (
                    l.first().and_then(plain),
                    l.get(1)
                        .map(|v| self.resolve(v, depth))
                        .as_ref()
                        .and_then(plain),
                ) {
                    (Some(d), Some(s)) if !d.is_empty() => Node::List(
                        s.split(d.as_str())
                            .map(|p| Node::Str(p.to_string()))
                            .collect(),
                    ),
                    _ => Node::Unresolved("Fn::Split of an unknown string".into()),
                }
            }
            "Fn::If" => {
                let Node::List(l) = arg else {
                    return Node::Unresolved("Fn::If of an odd shape".into());
                };
                let (Some(c), Some(a), Some(b)) =
                    (l.first().and_then(Node::as_str), l.get(1), l.get(2))
                else {
                    return Node::Unresolved("Fn::If of an odd shape".into());
                };
                match self.condition(c) {
                    Some(true) => self.resolve(a, depth),
                    Some(false) => self.resolve(b, depth),
                    None => {
                        let mut alts = Vec::new();
                        for branch in [self.resolve(a, depth), self.resolve(b, depth)] {
                            match branch {
                                Node::Either(inner) => alts.extend(inner),
                                other => alts.push(other),
                            }
                        }
                        Node::Either(alts)
                    }
                }
            }
            "Fn::FindInMap" => {
                let Node::List(l) = arg else {
                    return Node::Unresolved("Fn::FindInMap of an odd shape".into());
                };
                let keys: Vec<Option<String>> = l
                    .iter()
                    .take(3)
                    .map(|k| plain(&self.resolve(k, depth)))
                    .collect();
                let found = match (self.mappings, keys.as_slice()) {
                    (Some(m), [Some(a), Some(b), Some(c)]) => exact(m, a)
                        .and_then(|t| exact(t, b))
                        .and_then(|t| exact(t, c)),
                    _ => None,
                };
                match (found, l.get(3)) {
                    (Some(v), _) => self.resolve(v, depth),
                    // The language extension's default: `[map, top, second, {DefaultValue: x}]`.
                    (None, Some(d)) => match exact(d, "DefaultValue") {
                        Some(v) => self.resolve(v, depth),
                        None => Node::Unresolved("Fn::FindInMap with no match".into()),
                    },
                    (None, None) => Node::Unresolved("Fn::FindInMap with no match".into()),
                }
            }
            "Fn::Base64" => self.resolve(arg, depth),
            "Fn::Cidr" => {
                let Node::List(l) = arg else {
                    return Node::Unresolved("Fn::Cidr of an odd shape".into());
                };
                let block = l
                    .first()
                    .map(|b| self.resolve(b, depth))
                    .as_ref()
                    .and_then(plain);
                let count = l
                    .get(1)
                    .map(|c| self.resolve(c, depth))
                    .as_ref()
                    .and_then(plain)
                    .and_then(|c| c.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 256);
                match block {
                    // A slice of a private block is private; that is all the scan needs of it.
                    Some(b) if cidr::is_public(&b) == Some(false) => {
                        Node::List(vec![Node::Str(b); count])
                    }
                    _ => Node::Unresolved("Fn::Cidr of a block that is not private".into()),
                }
            }
            "Fn::ToJsonString" => match self.resolve(arg, depth) {
                Node::Str(s) => Node::Str(s),
                other => Node::Str(other.to_json().to_string()),
            },
            // Fn::ImportValue, Fn::GetAZs, Fn::Transform, Fn::Length, and what comes next.
            other => Node::Unresolved(other.to_string()),
        }
    }

    fn reference(&self, name: &str) -> Node {
        match name {
            "AWS::AccountId" => Node::Str(self.ctx.account.clone()),
            "AWS::Region" => Node::Str(self.ctx.region.clone()),
            "AWS::Partition" => Node::Str("aws".into()),
            "AWS::URLSuffix" => Node::Str("amazonaws.com".into()),
            "AWS::NoValue" => Node::Absent,
            _ if name.starts_with("AWS::") => Node::Unresolved(name.to_string()),
            _ => {
                if let Some(v) = self.params.get(name) {
                    v.clone()
                } else if self.resources.and_then(|r| exact(r, name)).is_some() {
                    Node::OwnRef(name.to_string())
                } else {
                    Node::Unresolved(format!(
                        "a Ref to {name}, which the template does not define"
                    ))
                }
            }
        }
    }

    /// `Fn::GetAtt` of a property the same template sets reads that property (a VPC's `CidrBlock`); any
    /// other attribute is the resource's own (its ARN, its id), so in this account.
    fn get_att(&self, resource: &str, attr: &str, depth: usize) -> Node {
        let Some(res) = self.resources.and_then(|r| exact(r, resource)) else {
            return Node::Unresolved(format!(
                "Fn::GetAtt of {resource}, which the template does not define"
            ));
        };
        match exact(res, "Properties").and_then(|p| exact(p, attr)) {
            Some(v) => self.resolve(v, depth),
            None => Node::OwnRef(resource.to_string()),
        }
    }

    fn sub(&self, arg: &Node, depth: usize) -> Node {
        let (text, vars) = match arg {
            Node::Str(s) => (s.as_str(), None),
            Node::List(l) => match (l.first(), l.get(1)) {
                (Some(Node::Str(s)), Some(v @ Node::Map(_))) => (s.as_str(), Some(v)),
                (Some(Node::Str(s)), None) => (s.as_str(), None),
                _ => return Node::Unresolved("Fn::Sub of an odd shape".into()),
            },
            _ => return Node::Unresolved("Fn::Sub of an odd shape".into()),
        };
        let lookup = |name: &str| -> Node {
            if let Some(v) = vars.and_then(|v| exact(v, name)) {
                return self.resolve(v, depth);
            }
            match name.split_once('.') {
                Some((a, b)) if !name.starts_with("AWS::") => self.get_att(a, b, depth),
                _ => self.reference(name),
            }
        };
        // `${Name}` alone is the value itself, so a same-stack ARN stays one.
        if let Some(name) = text.strip_prefix("${").and_then(|t| t.strip_suffix('}')) {
            if !name.starts_with('!') && !name.contains("${") && !name.contains('}') {
                return lookup(name.trim());
            }
        }
        let mut out = String::new();
        let mut rest = text;
        while let Some(i) = rest.find("${") {
            out.push_str(&rest[..i]);
            let after = &rest[i + 2..];
            if let Some(lit) = after.strip_prefix('!') {
                out.push_str("${");
                rest = lit;
                continue;
            }
            let Some(j) = after.find('}') else {
                out.push_str(&rest[i..]);
                rest = "";
                break;
            };
            out.push_str(&text_of(&lookup(after[..j].trim())));
            rest = &after[j + 1..];
        }
        out.push_str(rest);
        Node::Str(out)
    }

    fn condition(&self, name: &str) -> Option<bool> {
        if let Some(v) = self.memo.borrow().get(name) {
            return *v;
        }
        // A condition that reaches itself decides nothing.
        self.memo.borrow_mut().insert(name.to_string(), None);
        let v = self
            .conditions
            .and_then(|c| exact(c, name))
            .and_then(|e| self.cond_expr(e, 0));
        self.memo.borrow_mut().insert(name.to_string(), v);
        v
    }

    fn cond_expr(&self, e: &Node, depth: usize) -> Option<bool> {
        if depth > DEPTH {
            return None;
        }
        let Node::Map(m) = e else { return None };
        let [(k, arg)] = m.as_slice() else {
            return None;
        };
        let args = match arg {
            Node::List(l) => l.as_slice(),
            _ => std::slice::from_ref(arg),
        };
        match k.as_str() {
            "Fn::Equals" => match args {
                [a, b] => {
                    Some(plain(&self.resolve(a, depth + 1))? == plain(&self.resolve(b, depth + 1))?)
                }
                _ => None,
            },
            "Fn::Not" => match args {
                [c] => self.cond_expr(c, depth + 1).map(|b| !b),
                _ => None,
            },
            "Fn::And" => {
                let vs: Vec<Option<bool>> =
                    args.iter().map(|c| self.cond_expr(c, depth + 1)).collect();
                if vs.contains(&Some(false)) {
                    Some(false)
                } else if vs.iter().all(|v| *v == Some(true)) {
                    Some(true)
                } else {
                    None
                }
            }
            "Fn::Or" => {
                let vs: Vec<Option<bool>> =
                    args.iter().map(|c| self.cond_expr(c, depth + 1)).collect();
                if vs.contains(&Some(true)) {
                    Some(true)
                } else if vs.iter().all(|v| *v == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            }
            "Condition" => arg.as_str().and_then(|c| self.condition(c)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        Context {
            account: "111122223333".into(),
            region: "us-west-2".into(),
        }
    }

    #[test]
    fn short_forms_become_long_forms() {
        let t = parse_template(
            r#"
Resources:
  Sg:
    Type: AWS::EC2::SecurityGroup
    Properties:
      VpcId: !Ref Vpc
      GroupDescription: !Sub "${AWS::StackName} web"
      Tags:
        - Key: arn
          Value: !GetAtt Role.Arn
      Flags: [true, 443, "443", ~]
"#,
        )
        .unwrap();
        let props = t
            .get("Resources")
            .unwrap()
            .get("Sg")
            .unwrap()
            .get("Properties")
            .unwrap();
        assert_eq!(
            props.get("VpcId"),
            Some(&Node::Map(vec![("Ref".into(), Node::Str("Vpc".into()))]))
        );
        let att = props.get("Tags").unwrap();
        let Node::List(tags) = att else { panic!() };
        assert_eq!(
            tags[0].get("Value"),
            Some(&Node::Map(vec![(
                "Fn::GetAtt".into(),
                Node::List(vec![Node::Str("Role".into()), Node::Str("Arn".into())])
            )]))
        );
        assert_eq!(
            props.get("Flags"),
            Some(&Node::List(vec![
                Node::Bool(true),
                Node::Num("443".into()),
                Node::Str("443".into()),
                Node::Null
            ]))
        );
    }

    #[test]
    fn the_resolver_reads_parameters_conditions_sub_and_getatt() {
        let t = parse_template(
            r#"
Parameters:
  VpcCidr: { Type: String, Default: 10.42.0.0/16 }
  Nat: { Type: String, Default: "off" }
  Peer: { Type: String }
Conditions:
  HasNat: !Not [!Equals [!Ref Nat, "off"]]
Resources:
  Vpc:
    Type: AWS::EC2::VPC
    Properties: { CidrBlock: !Ref VpcCidr }
  Role:
    Type: AWS::IAM::Role
Outputs: {}
"#,
        )
        .unwrap();
        let given = BTreeMap::new();
        let c = ctx();
        let r = Resolver::new(&t, &c, &given);
        let get = |yaml: &str| r.resolve(&parse_template(yaml).unwrap(), 0);
        assert_eq!(
            get("!GetAtt Vpc.CidrBlock"),
            Node::Str("10.42.0.0/16".into())
        );
        assert_eq!(get("!GetAtt Role.Arn"), Node::OwnRef("Role".into()));
        assert_eq!(
            get("!Sub arn:${AWS::Partition}:iam::${AWS::AccountId}:role/${Role}"),
            Node::Str("arn:aws:iam::111122223333:role/Role".into())
        );
        assert_eq!(get("!Sub ${Role.Arn}"), Node::OwnRef("Role".into()));
        assert_eq!(r.condition("HasNat"), Some(false));
        assert_eq!(get("!If [HasNat, a, b]"), Node::Str("b".into()));
        assert!(matches!(get("!Ref Peer"), Node::Unresolved(_)));
        assert!(matches!(
            get("!ImportValue shared-vpc"),
            Node::Unresolved(_)
        ));
        assert_eq!(get("!Join ['-', [a, !Ref Nat]]"), Node::Str("a-off".into()));
        assert_eq!(
            get("!Select [1, !Split [',', 'x,y,z']]"),
            Node::Str("y".into())
        );
        let mut given = BTreeMap::new();
        given.insert("Nat".to_string(), "lazy".to_string());
        let r = Resolver::new(&t, &c, &given);
        assert_eq!(r.condition("HasNat"), Some(true));
    }
}
