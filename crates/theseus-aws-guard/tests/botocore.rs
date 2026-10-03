//! The list against P1's catalog, the AWS operations Theseus carries (the AWS design's appendix E):
//! - every operation the list names exists;
//! - every path an entry's `when` reads reaches a member of one of its operations' inputs, of the right
//!   type, and every value it compares with is one the member's enum allows: a path that reaches
//!   nothing would never fire, and nothing else would notice;
//! - every pattern that compacts the boundary matches only writes, and what it matches beyond the list
//!   is printed for the record.
//!
//! The catalog is embedded, compiled from the AWS CLI's botocore models (its snapshot names the CLI), so
//! these run wherever the tests do, with no network. A new catalog is checked by the same tests.

use std::collections::HashSet;
use std::sync::Arc;

use theseus_aws_catalog::{Catalog, Kind, OperationRef, Service, ShapeId, ShapeRef};
use theseus_aws_guard::{embedded, glob, When};

/// IAM's service prefixes, by botocore's service ids where the two differ.
const IAM_PREFIX: [(&str, &[&str]); 6] = [
    ("elasticloadbalancing", &["elbv2", "elb"]),
    ("elasticfilesystem", &["efs"]),
    ("states", &["stepfunctions"]),
    ("es", &["opensearch", "es"]),
    ("access-analyzer", &["accessanalyzer"]),
    ("s3", &["s3", "s3control"]),
];

fn catalog() -> &'static Catalog {
    Catalog::embedded().expect("the embedded catalog decodes")
}

/// A service by its botocore name, exactly: the catalog's lookup also answers to aliases (`states`
/// for Step Functions), which the list must not lean on.
fn service(svc: &str) -> Option<Arc<Service>> {
    catalog().service(svc).ok().filter(|s| s.name() == svc)
}

/// An operation by its name, exactly: the catalog's lookup also answers without regard to case.
fn operation<'s>(svc: &'s Service, name: &str) -> Option<OperationRef<'s>> {
    svc.operation(name).filter(|o| o.name() == name)
}

fn split(op: &str) -> (&str, &str) {
    op.split_once(':')
        .expect("the list's own rules hold operations to service:Operation")
}

#[test]
fn every_operation_the_list_names_exists() {
    eprintln!("the catalog: {}", catalog().snapshot());
    let l = embedded();
    let mut named: Vec<(&str, &str)> = Vec::new();
    for g in &l.guardrails {
        named.extend(g.operations.iter().map(|op| (g.name.as_str(), op.as_str())));
    }
    for grp in &l.iac {
        named.extend(
            grp.operations
                .iter()
                .map(|op| (grp.group.as_str(), op.as_str())),
        );
    }
    named.extend(
        l.destructive
            .operations
            .iter()
            .map(|op| ("destructive", op.as_str())),
    );
    named.extend(l.stacks.iter().map(|op| ("stacks", op.as_str())));
    let mut missing = Vec::new();
    for (at, op) in &named {
        let (svc, name) = split(op);
        match service(svc) {
            None => missing.push(format!("{at}: no botocore service {svc} ({op})")),
            Some(s) if operation(&s, name).is_none() => {
                missing.push(format!("{at}: no operation {op}"))
            }
            Some(_) => {}
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
    eprintln!("{} operations named, every one in the catalog", named.len());
}

/// What a test reads at its path.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Want {
    Anything,
    Boolean,
    Text,
}

/// Every test in a `when`, nested ones included: its path, what it reads there, and the values it
/// compares with.
fn tests_of<'w>(w: &'w When, out: &mut Vec<(&'w str, Want, &'w [String])>) {
    for p in w
        .public_cidr
        .iter()
        .chain(&w.foreign_account)
        .chain(&w.own_account)
        .chain(&w.external_principal)
        .chain(w.matches.keys())
    {
        out.push((p.as_str(), Want::Text, &[]));
    }
    for p in w.is_true.iter().chain(&w.is_false) {
        out.push((p.as_str(), Want::Boolean, &[]));
    }
    for (p, values) in &w.equals {
        out.push((p.as_str(), Want::Text, values.values()));
    }
    for p in w.present.iter().chain(&w.absent) {
        out.push((p.as_str(), Want::Anything, &[]));
    }
    for nested in w.any.iter().chain(&w.all).chain(&w.none) {
        tests_of(nested, out);
    }
}

/// The shapes a path reaches from a shape, read as the evaluator reads a call's JSON: names without
/// case, lists walked through, `*` any one member, and `**` any depth.
fn reach<'s>(
    shape: ShapeRef<'s>,
    segs: &[&str],
    seen: &mut HashSet<(ShapeId, usize)>,
    out: &mut Vec<ShapeRef<'s>>,
) {
    if !seen.insert((shape.id(), segs.len())) {
        return;
    }
    if shape.kind() == Kind::List {
        if let Some(member) = shape.list_member() {
            reach(member.shape(), segs, seen, out);
        }
        return;
    }
    let Some((seg, rest)) = segs.split_first() else {
        out.push(shape);
        return;
    };
    // A structure's members by name; a map's values by any key its key shape allows.
    let children: Vec<(Option<&str>, ShapeRef<'s>)> = match shape.kind() {
        Kind::Structure => shape
            .members()
            .map(|m| (Some(m.name()), m.shape()))
            .collect(),
        Kind::Map => shape
            .map_value()
            .map(|v| (None, v.shape()))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    };
    let map_key_allows = |key: &str| {
        shape.map_key().map(|k| k.shape()).is_none_or(|k| {
            k.enum_count() == 0 || k.enum_values().any(|v| v.eq_ignore_ascii_case(key))
        })
    };
    match *seg {
        "**" => {
            reach(shape, rest, seen, out);
            for (_, child) in &children {
                reach(*child, segs, seen, out);
            }
        }
        "*" => {
            for (_, child) in &children {
                reach(*child, rest, seen, out);
            }
        }
        key => {
            for (name, child) in &children {
                let named = match name {
                    Some(n) => n.eq_ignore_ascii_case(key),
                    None => map_key_allows(key),
                };
                if named {
                    reach(*child, rest, seen, out);
                }
            }
        }
    }
}

#[test]
fn every_when_path_reaches_an_input_member_of_its_type() {
    let l = embedded();
    let mut problems = Vec::new();
    let mut checked = 0;
    for g in &l.guardrails {
        let Some(w) = &g.when else { continue };
        let services: Vec<(&str, Arc<Service>)> = g
            .operations
            .iter()
            .filter_map(|op| {
                let (svc, name) = split(op);
                Some((name, service(svc)?))
            })
            .collect();
        let mut tests = Vec::new();
        tests_of(w, &mut tests);
        for (path, want, values) in tests {
            let segs: Vec<&str> = path.split('.').collect();
            let mut reached: Vec<ShapeRef<'_>> = Vec::new();
            for (name, svc) in &services {
                if let Some(input) = operation(svc, name).and_then(|o| o.input()) {
                    reach(input, &segs, &mut HashSet::new(), &mut reached);
                }
            }
            checked += 1;
            if reached.is_empty() {
                problems.push(format!(
                    "{}: {path} reaches no member of {:?}",
                    g.name, g.operations
                ));
                continue;
            }
            match want {
                Want::Boolean if !reached.iter().any(|s| s.kind() == Kind::Boolean) => {
                    problems.push(format!("{}: {path} is never a boolean", g.name));
                }
                Want::Text if !reached.iter().any(|s| s.kind() == Kind::String) => {
                    problems.push(format!("{}: {path} is never a string", g.name));
                }
                _ => {}
            }
            for v in values {
                let allowed = reached.iter().any(|s| {
                    s.kind() == Kind::String
                        && (s.enum_count() == 0
                            || s.enum_values().any(|x| x.eq_ignore_ascii_case(v)))
                });
                if !allowed {
                    problems.push(format!("{}: {path} is never {v:?}", g.name));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    eprintln!("{checked} paths, each reaching its member");
}

/// Operation names that read: a compaction pattern must match none of them.
const READ_VERBS: [&str; 16] = [
    "Describe", "Get", "List", "Head", "Search", "Lookup", "Query", "Scan", "Select", "BatchGet",
    "Check", "Validate", "View", "Preview", "Estimate", "Simulate",
];

#[test]
fn the_boundary_patterns_match_only_writes() {
    let l = embedded();
    let denied: Vec<String> = l
        .policies()
        .iter()
        .filter(|p| p.name.starts_with("theseus-guard-"))
        .flat_map(|p| {
            p.denied_actions()
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .collect();
    let mut problems = Vec::new();
    for pattern in &l.compact {
        let (prefix, _) = split(pattern);
        let services = IAM_PREFIX
            .iter()
            .find(|(p, _)| *p == prefix)
            .map_or_else(|| vec![prefix], |(_, ids)| ids.to_vec());
        let mut also = Vec::new();
        for svc in services {
            let Some(s) = service(svc) else {
                continue;
            };
            for name in s.operations().map(|o| o.name()) {
                let action = format!("{prefix}:{name}");
                if !glob(pattern, &action) {
                    continue;
                }
                if READ_VERBS.iter().any(|v| name.starts_with(v)) {
                    problems.push(format!("{pattern} matches a read, {action}"));
                }
                if !denied.contains(&action.to_lowercase()) {
                    also.push(name.to_owned());
                }
            }
        }
        also.sort();
        also.dedup();
        eprintln!(
            "{pattern}: also {}",
            if also.is_empty() {
                "nothing".to_string()
            } else {
                also.join(" ")
            }
        );
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
