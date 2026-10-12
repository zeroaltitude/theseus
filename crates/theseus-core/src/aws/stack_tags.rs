//! Stable stack tags, and the tag-only change set (theseus-mpk2s).
//!
//! CloudFormation hands a stack's tags to every taggable resource in it, so a
//! stack tag whose value is the applying execution's id made every plan from a
//! later session a change of every resource's `Tags`, with its knock-on
//! evaluations and the floor's guard and budget lines. A stack's tags are
//! now the ones that do not move: the stack's own, carried from what it holds
//! (a new stack gets `theseus:owner`, `theseus:stack`, `theseus:deployment`).
//! The execution that applied a change set is in its description and in the
//! ledger's rows, never in a tag.
//!
//! The stacks made before carry `theseus:execution`, and the first change set
//! that drops it touches every resource's tags. [`migration`] says whether a
//! change set's tags differ from the stack's by that one removal, and
//! [`tag_only_gaps`] whether every one of its changes is nothing but that:
//! both hold, and the change set changes no permission and no amount, so the
//! floor's guard and budget lines for it are not asked.

use std::collections::HashSet;

use serde_json::Value;

/// The one tag that moved with the execution, and is dropped from the stacks
/// that carry it.
pub const VOLATILE: &str = "theseus:execution";

/// A stack's tags as `DescribeStacks` gives them.
pub fn tags_of(stack: Option<&Value>) -> Vec<(String, String)> {
    stack
        .and_then(|s| s["Tags"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|t| {
            Some((
                t["Key"].as_str()?.to_string(),
                t["Value"].as_str()?.to_string(),
            ))
        })
        .collect()
}

/// The tags a change set sets on a stack: a new stack's own three, and an
/// existing stack's tags as they are, less the volatile one. A second session
/// plans exactly the tags the first one set.
pub fn stable_tags(
    existing: Option<&Value>,
    stack: &str,
    deployment: &str,
) -> Vec<(String, String)> {
    match existing {
        None => vec![
            ("theseus:owner".into(), "theseus".into()),
            ("theseus:stack".into(), stack.into()),
            ("theseus:deployment".into(), deployment.into()),
        ],
        Some(_) => tags_of(existing)
            .into_iter()
            .filter(|(k, _)| k != VOLATILE)
            .collect(),
    }
}

/// Whether `sent` differs from the stack's tags by removing the volatile tag
/// and nothing else: the stack holds it, and every other tag stays as it is.
pub fn migration(existing: Option<&Value>, sent: &[(String, String)]) -> bool {
    let held = tags_of(existing);
    let mut rest: Vec<&(String, String)> = held.iter().filter(|(k, _)| k != VOLATILE).collect();
    let mut want: Vec<&(String, String)> = sent.iter().collect();
    rest.sort();
    want.sort();
    held.iter().any(|(k, _)| k == VOLATILE) && rest == want
}

/// Each change that is not a tag change, as the diff names it (`Modify
/// Bucket`); none when the change set is nothing but tags. A change is one of
/// two things:
///
/// - **a tag change**: a modification whose scope is `Tags` alone, every
///   detail targeting `Tags` and requiring no recreation, replacing nothing;
/// - **its knock-on**: a modification whose scope is `Properties` alone, every
///   detail an evaluation CloudFormation can only make at execution
///   (`Dynamic`, from a resource attribute) of a resource that is itself a tag
///   change in this set, replacing nothing for certain. A reference to an
///   attribute of a resource whose tags changed is listed as a possible change
///   of the referrer's property, and a tag cannot change an attribute.
///
/// Any change a direct edit of a property makes (`Static`, `DirectModification`),
/// any add, remove, import or replacement, and a reference to a custom
/// resource's attribute, is a gap. An empty change set has none, and is no
/// migration: see [`tag_only`].
pub fn tag_only_gaps(changes: &[Value]) -> Vec<String> {
    let direct: HashSet<&str> = changes
        .iter()
        .filter_map(|c| c.get("ResourceChange"))
        .filter(|r| is_tag_change(r) && !is_custom(r))
        .filter_map(|r| r["LogicalResourceId"].as_str())
        .collect();
    changes
        .iter()
        .filter(|c| match c.get("ResourceChange") {
            Some(r) => !(is_tag_change(r) || is_knock_on(r, &direct)),
            None => true,
        })
        .map(named)
        .collect()
}

/// A change as the diff names it: `Modify Bucket`.
pub fn named(c: &Value) -> String {
    let r = &c["ResourceChange"];
    format!(
        "{} {}",
        r["Action"].as_str().unwrap_or("?"),
        r["LogicalResourceId"].as_str().unwrap_or("?")
    )
}

/// A change set that is nothing but tags (and their knock-on): not empty, and
/// no gap.
pub fn tag_only(changes: &[Value]) -> bool {
    !changes.is_empty() && tag_only_gaps(changes).is_empty()
}

fn strings(v: &Value) -> Vec<&str> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn details(r: &Value) -> &[Value] {
    r["Details"].as_array().map_or(&[], Vec::as_slice)
}

fn modifies_in_place(r: &Value) -> bool {
    r["Action"].as_str() == Some("Modify")
}

/// A custom resource's attributes are whatever its function returns.
fn is_custom(r: &Value) -> bool {
    let ty = r["ResourceType"].as_str().unwrap_or_default();
    ty.starts_with("Custom::") || ty == "AWS::CloudFormation::CustomResource"
}

fn is_tag_change(r: &Value) -> bool {
    modifies_in_place(r)
        && matches!(r["Replacement"].as_str(), None | Some("False"))
        && strings(&r["Scope"]) == ["Tags"]
        && details(r).iter().all(|d| {
            d["Target"]["Attribute"].as_str() == Some("Tags")
                && matches!(
                    d["Target"]["RequiresRecreation"].as_str(),
                    None | Some("Never")
                )
        })
}

fn is_knock_on(r: &Value, direct: &HashSet<&str>) -> bool {
    modifies_in_place(r)
        && matches!(
            r["Replacement"].as_str(),
            None | Some("False" | "Conditional")
        )
        && strings(&r["Scope"]) == ["Properties"]
        && !details(r).is_empty()
        && details(r).iter().all(|d| {
            d["Evaluation"].as_str() == Some("Dynamic")
                && d["ChangeSource"].as_str() == Some("ResourceAttribute")
                && d["CausingEntity"]
                    .as_str()
                    .and_then(|e| e.split('.').next())
                    .is_some_and(|id| direct.contains(id))
        })
}
