//! The template that applies a candidate: the hands stack's own, with the
//! profile's role holding the hand basics and one new managed policy, the
//! candidate, in place of its wide policies. The patch is text, with its
//! anchors checked (one role block, one `ManagedPolicyArns` list holding the
//! basics), and the result is parsed and read back before it is used; a stack
//! whose template has another shape is refused with the reason, never guessed.
//! The plan and the apply are `live.rs`'s, an owner act.

use serde_json::Value;
use theseus_aws_guard::{parse_template, Node};

/// The stack the hands' roles live in.
pub const STACK: &str = "theseus-hands";
/// A managed policy's size limit, in characters.
pub const POLICY_LIMIT: usize = theseus_aws_guard::MANAGED_POLICY_LIMIT;

/// A hand profile and where its role is in the template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    pub name: &'static str,
    pub logical: &'static str,
    pub role: &'static str,
}

pub const PROFILES: &[Profile] = &[
    Profile {
        name: "owner",
        logical: "HandOwnerRole",
        role: "theseus-hand-owner",
    },
    Profile {
        name: "read",
        logical: "HandReadRole",
        role: "theseus-hand-read",
    },
    Profile {
        name: "basic",
        logical: "HandBasicRole",
        role: "theseus-hand-basic",
    },
];

/// The profile named `s`: `owner`, or a role's name.
pub fn profile(s: &str) -> Option<Profile> {
    PROFILES
        .iter()
        .copied()
        .find(|p| p.name == s || p.role == s)
}

impl Profile {
    /// The new policy's logical id and its name.
    pub fn policy(&self) -> (String, String) {
        (
            format!("{}NarrowPolicy", self.logical.trim_end_matches("Role")),
            format!("{}-narrow", self.role),
        )
    }
}

fn indent(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

/// The template with `p`'s role narrowed to `policy`.
pub fn patch(template: &str, p: Profile, policy: &Value) -> Result<String, String> {
    let json = policy.to_string();
    if json.len() > POLICY_LIMIT {
        return Err(format!(
            "the candidate is {} characters, past a managed policy's {POLICY_LIMIT}",
            json.len()
        ));
    }
    let lines: Vec<&str> = template.lines().collect();
    let head = format!("  {}:", p.logical);
    let starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.trim_end() == head)
        .map(|(i, _)| i)
        .collect();
    let [start] = starts[..] else {
        return Err(format!(
            "the template has {} blocks for {}, not one",
            starts.len(),
            p.logical
        ));
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#') && indent(l) <= 2)
        .map_or(lines.len(), |i| start + 1 + i);
    let list = lines[start..end]
        .iter()
        .position(|l| l.trim() == "ManagedPolicyArns:")
        .map(|i| start + i)
        .ok_or("the role has no ManagedPolicyArns list")?;
    let li = indent(lines[list]);
    let mut items_end = list + 1;
    while items_end < end {
        let l = lines[items_end];
        let in_list = l.trim().is_empty()
            || indent(l) > li
            || (indent(l) == li && l.trim_start().starts_with("- "));
        if !in_list {
            break;
        }
        items_end += 1;
    }
    let items: Vec<&str> = lines[list + 1..items_end]
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let kept: Vec<&&str> = items
        .iter()
        .filter(|l| l.trim_start() == "- !Ref HandBasicsPolicy")
        .collect();
    if kept.len() != 1 {
        return Err("the role's policy list does not hold the hand basics once".into());
    }
    let (id, name) = p.policy();
    let pad = " ".repeat(indent(items[0]));
    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..start].iter().map(|l| l.to_string()));
    out.push(format!("  {id}:"));
    out.push("    Type: AWS::IAM::ManagedPolicy".into());
    out.push("    Properties:".into());
    out.push(format!("      ManagedPolicyName: {name}"));
    out.push(format!(
        "      Description: Narrowed from what CloudTrail showed {} calling; proposed by Theseus, applied on the owner's yes.",
        p.role
    ));
    out.push(format!("      PolicyDocument: {json}"));
    out.push(String::new());
    out.extend(lines[start..=list].iter().map(|l| l.to_string()));
    out.push(format!("{pad}- !Ref HandBasicsPolicy"));
    out.push(format!("{pad}- !Ref {id}"));
    out.extend(lines[items_end..].iter().map(|l| l.to_string()));
    let mut text = out.join("\n");
    if template.ends_with('\n') {
        text.push('\n');
    }
    read_back(&text, p)?;
    Ok(text)
}

/// The patched template parses, holds the new policy with the candidate's
/// own document, and the role lists exactly the basics and it.
fn read_back(text: &str, p: Profile) -> Result<(), String> {
    let t =
        parse_template(text).map_err(|e| format!("the patched template does not parse: {e}"))?;
    let (id, name) = p.policy();
    let res = t.get("Resources").ok_or("no Resources")?;
    let pol = res.get(&id).ok_or("the new policy is missing")?;
    let named = pol
        .get("Properties")
        .and_then(|x| x.get("ManagedPolicyName"))
        .and_then(Node::as_str);
    if named != Some(name.as_str()) {
        return Err("the new policy's name did not read back".into());
    }
    let role = res.get(p.logical).ok_or("the role is missing")?;
    let arns = role
        .get("Properties")
        .and_then(|x| x.get("ManagedPolicyArns"));
    let Some(Node::List(l)) = arns else {
        return Err("the role's policy list did not read back".into());
    };
    if l.len() != 2 {
        return Err(format!("the role lists {} policies, not 2", l.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const HANDS: &str = include_str!("../../../../../infra/aws/theseus-hands.yaml");

    fn pol() -> Value {
        json!({"Version": "2012-10-17", "Statement": [
            {"Effect": "Allow", "Action": ["ec2:DescribeInstances"], "Resource": ["*"]}]})
    }

    #[test]
    fn the_repos_hands_template_patches_each_profile_and_reads_back() {
        for p in PROFILES {
            let t = patch(HANDS, *p, &pol()).unwrap_or_else(|e| panic!("{}: {e}", p.name));
            assert!(t.contains(&format!("ManagedPolicyName: {}-narrow", p.role)));
            // Nothing else moved: every other line is the template's own.
            let before: Vec<&str> = HANDS.lines().collect();
            let after: Vec<&str> = t.lines().collect();
            let removed = before.iter().filter(|l| !after.contains(l)).count();
            assert!(
                removed <= 2,
                "{}: {removed} lines of the template changed",
                p.name
            );
        }
        let owner = patch(HANDS, PROFILES[0], &pol()).unwrap();
        assert!(
            !owner
                .lines()
                .any(|l| l.contains("AllowAllArn")
                    && l.trim_start().starts_with("- Fn::ImportValue")),
            "allow-all leaves the owner role"
        );
        let read = patch(HANDS, PROFILES[1], &pol()).unwrap();
        assert!(!read.contains("- !Sub \"arn:${AWS::Partition}:iam::aws:policy/ReadOnlyAccess\""));
    }

    #[test]
    fn a_template_of_another_shape_is_refused_with_the_reason() {
        let no_role = HANDS.replace("  HandOwnerRole:", "  HandOwnerRoleX:");
        assert!(patch(&no_role, PROFILES[0], &pol())
            .unwrap_err()
            .contains("not one"));
        let no_basics = HANDS.replace("- !Ref HandBasicsPolicy", "- !Ref SomethingElse");
        assert!(patch(&no_basics, PROFILES[0], &pol())
            .unwrap_err()
            .contains("basics"));
        let big = json!({"x": "y".repeat(7000)});
        assert!(patch(HANDS, PROFILES[0], &big)
            .unwrap_err()
            .contains("managed policy"));
    }

    #[test]
    fn the_guards_scan_of_the_patched_template_finds_nothing_new() {
        let g = theseus_aws_guard::embedded();
        let ctx = theseus_aws_guard::Context {
            account: "111122223333".into(),
            region: "us-west-2".into(),
        };
        let params = std::collections::BTreeMap::new();
        let base = g
            .scan(&parse_template(HANDS).unwrap(), &ctx, &params)
            .unwrap()
            .hits
            .len();
        let t = patch(HANDS, PROFILES[0], &pol()).unwrap();
        let now = g
            .scan(&parse_template(&t).unwrap(), &ctx, &params)
            .unwrap()
            .hits
            .len();
        assert_eq!(now, base);
    }
}
