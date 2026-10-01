//! Every guardrail has a hit and a near miss: for calls, and for templates and change sets when the
//! entry reads them. The brief's pair is among them: `10.0.0.0/8` is not public and `0.0.0.0/0` is; a
//! trust policy naming this account is fine, and one naming another account is a hit.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use theseus_aws_guard::{embedded, parse_template, ChangeAction, Context, Node, ResourceChange};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cases {
    call: Vec<Call>,
    template: Vec<Template>,
    change: Vec<Change>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    guardrail: String,
    op: String,
    hit: bool,
    input: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    guardrail: String,
    #[serde(rename = "type")]
    ty: String,
    hit: bool,
    properties: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    guardrail: String,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    physical: Option<String>,
    action: String,
    hit: bool,
}

fn ctx() -> Context {
    Context {
        account: "111122223333".into(),
        region: "us-west-2".into(),
    }
}

/// One resource, `R`, in an otherwise empty template.
fn one_resource(ty: &str, properties: Node) -> Node {
    Node::Map(vec![(
        "Resources".into(),
        Node::Map(vec![(
            "R".into(),
            Node::Map(vec![
                ("Type".into(), Node::Str(ty.into())),
                ("Properties".into(), properties),
            ]),
        )]),
    )])
}

#[test]
fn every_guardrail_has_a_hit_and_a_near_miss() {
    let cases: Cases = toml::from_str(include_str!("cases.toml")).expect("cases.toml parses");
    let list = embedded();
    let mut failures = Vec::new();
    // (guardrail, kind, hit?) seen.
    let mut seen: BTreeSet<(String, &str, bool)> = BTreeSet::new();
    let known = |name: &str| {
        list.guardrail(name)
            .unwrap_or_else(|| panic!("no guardrail {name}"))
    };

    for c in &cases.call {
        let g = known(&c.guardrail);
        let input: serde_json::Value = serde_json::from_str(&c.input)
            .unwrap_or_else(|e| panic!("{} {}: input is not JSON: {e}", c.guardrail, c.op));
        let got = g.operations.contains(&c.op) && g.hit(&Node::from_json(&input), &ctx()).is_some();
        if got != c.hit {
            failures.push(format!(
                "call {} {}: expected hit={}, got {got}",
                c.guardrail, c.op, c.hit
            ));
        }
        seen.insert((c.guardrail.clone(), "call", c.hit));
    }

    for t in &cases.template {
        known(&t.guardrail);
        let props = parse_template(&t.properties)
            .unwrap_or_else(|e| panic!("{} {}: properties: {e}", t.guardrail, t.ty));
        let scan = list
            .scan(&one_resource(&t.ty, props), &ctx(), &BTreeMap::new())
            .expect("one resource scans");
        let got = scan.hits.iter().any(|h| h.guardrail.name == t.guardrail);
        if got != t.hit {
            failures.push(format!(
                "template {} {}: expected hit={}, got {got}",
                t.guardrail, t.ty, t.hit
            ));
        }
        seen.insert((t.guardrail.clone(), "template", t.hit));
    }

    for c in &cases.change {
        known(&c.guardrail);
        let action = match c.action.as_str() {
            "add" => ChangeAction::Add,
            "modify" => ChangeAction::Modify,
            "remove" => ChangeAction::Remove,
            other => panic!("{}: no action {other}", c.guardrail),
        };
        let v = list.check_change_set(&[ResourceChange {
            logical_id: "R".into(),
            physical_id: c.physical.clone(),
            resource_type: c.ty.clone(),
            action,
            replacement: false,
        }]);
        let got = v
            .floor
            .iter()
            .any(|h| h.guardrail.is_some_and(|g| g.name == c.guardrail));
        if got != c.hit {
            failures.push(format!(
                "change {} {} {}: expected hit={}, got {got}",
                c.guardrail, c.ty, c.action, c.hit
            ));
        }
        seen.insert((c.guardrail.clone(), "change", c.hit));
    }

    for g in &list.guardrails {
        let mut kinds = vec!["call"];
        if !g.template.is_empty() {
            kinds.push("template");
        }
        if !g.change.is_empty() {
            kinds.push("change");
        }
        for kind in kinds {
            for hit in [true, false] {
                if !seen.contains(&(g.name.clone(), kind, hit)) {
                    failures.push(format!(
                        "{}: no {kind} case for a {}",
                        g.name,
                        if hit { "hit" } else { "near miss" }
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
