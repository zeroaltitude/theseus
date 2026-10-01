//! The list against the AWS CLI's botocore models, which stand in for P1's catalog until it lands (the
//! AWS design's appendix E):
//! - every operation the list names exists;
//! - every path an entry's `when` reads reaches a member of one of its operations' inputs, of the right
//!   type, and every value it compares with is one the member's enum allows: a path that reaches
//!   nothing would never fire, and nothing else would notice;
//! - every pattern that compacts the boundary matches only writes, and what it matches beyond the list
//!   is printed for the record.
//!
//! The models are not in the repository. They are read from `THESEUS_BOTOCORE_DATA`, or from the AWS
//! CLI v2's install; where neither exists (CI has no AWS CLI), each test says so and passes. Checked
//! here against the CLI 2.34.15's models.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use serde_json::{Map, Value};
use theseus_aws_guard::{embedded, glob, When};

const CLI_MODELS: &str = "/usr/local/aws-cli/v2/current/dist/awscli/botocore/data";

/// IAM's service prefixes, by botocore's service ids where the two differ.
const IAM_PREFIX: [(&str, &[&str]); 6] = [
    ("elasticloadbalancing", &["elbv2", "elb"]),
    ("elasticfilesystem", &["efs"]),
    ("states", &["stepfunctions"]),
    ("es", &["opensearch", "es"]),
    ("access-analyzer", &["accessanalyzer"]),
    ("s3", &["s3", "s3control"]),
];

/// The services' models, each `service-2.json` of its newest API version.
struct Models {
    dir: PathBuf,
    loaded: BTreeMap<String, Option<Value>>,
}

impl Models {
    fn open() -> Option<Models> {
        let dir = std::env::var_os("THESEUS_BOTOCORE_DATA")
            .map_or_else(|| PathBuf::from(CLI_MODELS), PathBuf::from);
        if dir.is_dir() {
            Some(Models {
                dir,
                loaded: BTreeMap::new(),
            })
        } else {
            eprintln!(
                "skipped: no botocore models at {} (set THESEUS_BOTOCORE_DATA, or install the AWS CLI v2)",
                dir.display()
            );
            None
        }
    }

    fn load(&mut self, svc: &str) {
        if self.loaded.contains_key(svc) {
            return;
        }
        let newest = std::fs::read_dir(self.dir.join(svc))
            .ok()
            .and_then(|versions| {
                versions
                    .filter_map(|v| Some(v.ok()?.path().join("service-2.json")))
                    .filter(|p| p.is_file())
                    .max()
            });
        let model =
            newest.and_then(|p| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok());
        self.loaded.insert(svc.to_string(), model);
    }

    fn service(&self, svc: &str) -> Option<&Value> {
        self.loaded.get(svc).and_then(Option::as_ref)
    }
}

fn split(op: &str) -> (&str, &str) {
    op.split_once(':')
        .expect("the list's own rules hold operations to service:Operation")
}

#[test]
fn every_operation_the_list_names_exists() {
    let Some(mut models) = Models::open() else {
        return;
    };
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
        models.load(svc);
        match models.service(svc) {
            None => missing.push(format!("{at}: no botocore service {svc} ({op})")),
            Some(m) if m["operations"].get(name).is_none() => {
                missing.push(format!("{at}: no operation {op}"))
            }
            Some(_) => {}
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
    eprintln!("{} operations named, every one in the models", named.len());
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
fn reach<'m>(
    shapes: &'m Map<String, Value>,
    shape: &str,
    segs: &[&str],
    seen: &mut HashSet<(String, usize)>,
    out: &mut Vec<&'m Value>,
) {
    let Some(s) = shapes.get(shape) else { return };
    if !seen.insert((shape.to_string(), segs.len())) {
        return;
    }
    if s["type"] == "list" {
        if let Some(member) = s["member"]["shape"].as_str() {
            reach(shapes, member, segs, seen, out);
        }
        return;
    }
    let Some((seg, rest)) = segs.split_first() else {
        out.push(s);
        return;
    };
    // A structure's members by name; a map's values by any key its key shape allows.
    let children: Vec<(Option<&str>, &str)> = match s["type"].as_str() {
        Some("structure") => s["members"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(name, m)| Some((Some(name.as_str()), m["shape"].as_str()?)))
            .collect(),
        Some("map") => s["value"]["shape"]
            .as_str()
            .map(|v| (None, v))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    };
    let map_key_allows = |key: &str| {
        let enumerated = s["key"]["shape"]
            .as_str()
            .and_then(|k| shapes.get(k))
            .and_then(|k| k["enum"].as_array());
        enumerated.is_none_or(|values| {
            values
                .iter()
                .any(|v| v.as_str().is_some_and(|v| v.eq_ignore_ascii_case(key)))
        })
    };
    match *seg {
        "**" => {
            reach(shapes, shape, rest, seen, out);
            for (_, child) in &children {
                reach(shapes, child, segs, seen, out);
            }
        }
        "*" => {
            for (_, child) in &children {
                reach(shapes, child, rest, seen, out);
            }
        }
        key => {
            for (name, child) in &children {
                let named = match name {
                    Some(n) => n.eq_ignore_ascii_case(key),
                    None => map_key_allows(key),
                };
                if named {
                    reach(shapes, child, rest, seen, out);
                }
            }
        }
    }
}

#[test]
fn every_when_path_reaches_an_input_member_of_its_type() {
    let Some(mut models) = Models::open() else {
        return;
    };
    let l = embedded();
    let mut problems = Vec::new();
    let mut checked = 0;
    for g in &l.guardrails {
        let Some(w) = &g.when else { continue };
        for op in &g.operations {
            models.load(split(op).0);
        }
        let mut tests = Vec::new();
        tests_of(w, &mut tests);
        for (path, want, values) in tests {
            let segs: Vec<&str> = path.split('.').collect();
            let mut reached: Vec<&Value> = Vec::new();
            for op in &g.operations {
                let (svc, name) = split(op);
                let Some(m) = models.service(svc) else {
                    continue;
                };
                let (Some(input), Some(shapes)) = (
                    m["operations"][name]["input"]["shape"].as_str(),
                    m["shapes"].as_object(),
                ) else {
                    continue;
                };
                reach(shapes, input, &segs, &mut HashSet::new(), &mut reached);
            }
            checked += 1;
            if reached.is_empty() {
                problems.push(format!(
                    "{}: {path} reaches no member of {:?}",
                    g.name, g.operations
                ));
                continue;
            }
            let ty = |s: &Value, t: &str| s["type"] == t;
            match want {
                Want::Boolean if !reached.iter().any(|s| ty(s, "boolean")) => {
                    problems.push(format!("{}: {path} is never a boolean", g.name));
                }
                Want::Text if !reached.iter().any(|s| ty(s, "string")) => {
                    problems.push(format!("{}: {path} is never a string", g.name));
                }
                _ => {}
            }
            for v in values {
                let allowed = reached.iter().any(|s| {
                    ty(s, "string")
                        && s["enum"].as_array().is_none_or(|e| {
                            e.iter()
                                .any(|x| x.as_str().is_some_and(|x| x.eq_ignore_ascii_case(v)))
                        })
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
    let Some(mut models) = Models::open() else {
        return;
    };
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
            models.load(svc);
            let Some(ops) = models
                .service(svc)
                .and_then(|m| m["operations"].as_object())
            else {
                continue;
            };
            for name in ops.keys() {
                let action = format!("{prefix}:{name}");
                if !glob(pattern, &action) {
                    continue;
                }
                if READ_VERBS.iter().any(|v| name.starts_with(v)) {
                    problems.push(format!("{pattern} matches a read, {action}"));
                }
                if !denied.contains(&action.to_lowercase()) {
                    also.push(name.clone());
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
