//! A narrowing proposal as stored and shown: the owner's card reads its
//! words, a tool reads its JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::policy::Doc;

/// Where a proposal stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Open,
    Applied,
    Declined,
    Reverted,
}

/// A narrower policy for a hand role, with its evidence and its checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    /// The role: `theseus-hand-owner`.
    pub role: String,
    /// The profile: `owner`.
    pub profile: String,
    pub at_ms: u64,
    pub days: u32,
    /// Events of this role read in the window, and pages read.
    pub events: u64,
    pub pages: u32,
    /// About how many actions the replaced policies reach, and the candidate.
    pub reach_before: u64,
    pub reach_after: u64,
    /// The replaced policies' names and their allows as one document.
    pub wide_names: Vec<String>,
    pub current: Value,
    pub candidate: Value,
    pub not_visible: Vec<String>,
    pub kept: Vec<String>,
    pub elsewhere: Vec<String>,
    pub unexplained: Vec<String>,
    /// Access Analyzer's suggestions and warnings, in words.
    pub analyzer: Vec<String>,
    /// Observed calls the simulator allowed.
    pub simulated: usize,
    pub observed: Vec<String>,
    pub state: State,
    #[serde(default)]
    pub applied_ms: Option<u64>,
    #[serde(default)]
    pub change_set: Option<String>,
    /// Denied actions the owner was told of in the watch.
    #[serde(default)]
    pub red: Vec<String>,
}

/// An id from a role and its candidate: stable for the same proposal.
pub fn id_of(role: &str, candidate: &Value) -> String {
    super::super::session::digest(&format!("{role}|{candidate}"))
}

fn by_service(actions: &[String]) -> String {
    let mut m: BTreeMap<&str, usize> = BTreeMap::new();
    for a in actions {
        *m.entry(a.split(':').next().unwrap_or(a)).or_default() += 1;
    }
    m.iter()
        .map(|(s, n)| format!("{s} ({n})"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn short(items: &[String], n: usize) -> String {
    if items.len() <= n {
        items.join(", ")
    } else {
        format!("{}, and {} more", items[..n].join(", "), items.len() - n)
    }
}

fn allows_words(d: &Doc) -> Vec<String> {
    d.allows()
        .map(|s| {
            format!(
                "allow {} on {}",
                short(&s.actions, 4),
                short(&s.resources, 2)
            )
        })
        .collect()
}

impl Proposal {
    pub fn current_doc(&self) -> Doc {
        Doc::from_value(&self.current).unwrap_or_default()
    }

    pub fn candidate_doc(&self) -> Doc {
        Doc::from_value(&self.candidate).unwrap_or_default()
    }

    /// The change in words: what goes, what stays, what the checks said.
    pub fn words(&self) -> String {
        let mut w = format!(
            "A narrower policy for {} (proposal {}).\n\nEvidence: {} days of CloudTrail, {} events of \
             this role over {} pages, calling {} distinct actions.\n\n",
            self.role,
            self.id,
            self.days,
            self.events,
            self.pages,
            self.observed.len(),
        );
        let drop = (self.reach_before.saturating_sub(self.reach_after) * 100)
            .checked_div(self.reach_before)
            .unwrap_or(0);
        w.push_str(&format!(
            "Size: about {} actions allowed today, about {} after ({drop}% fewer).\n",
            self.reach_before, self.reach_after
        ));
        w.push_str(&format!(
            "Replaced: {} ({}).\n",
            self.wide_names.join(", "),
            allows_words(&self.current_doc()).join("; ")
        ));
        let cand = self.candidate_doc();
        let seen: Vec<String> = self.observed.clone();
        if !seen.is_empty() {
            w.push_str(&format!("Allowed on evidence: {}.\n", by_service(&seen)));
        }
        if !self.kept.is_empty() {
            w.push_str(&format!(
                "Kept from the code (the hand's own): {}.\n",
                short(&self.kept, 8)
            ));
        }
        if !self.not_visible.is_empty() {
            w.push_str(&format!(
                "Kept as they are because CloudTrail's history cannot show them (data events), not \
                 because they are unused: {}.\n",
                short(&self.not_visible, 8)
            ));
        }
        if !self.elsewhere.is_empty() {
            w.push_str(&format!(
                "Called, and allowed by a policy that stays attached: {}.\n",
                short(&self.elsewhere, 6)
            ));
        }
        if !self.unexplained.is_empty() {
            w.push_str(&format!(
                "Called with no policy of the role allowing it (look at these): {}.\n",
                short(&self.unexplained, 6)
            ));
        }
        w.push_str(&format!(
            "The new document has {} allow statements and {} deny statements, none wider than \
             today's (checked statement by statement).\n",
            cand.allows().count(),
            cand.denies().count()
        ));
        w.push_str(&format!(
            "Checks: Access Analyzer found no errors or security warnings{}; the simulator allowed \
             all {} observed calls against the new policy; the guards' scan of the template that \
             applies it found nothing.\n",
            if self.analyzer.is_empty() {
                String::new()
            } else {
                format!(" ({} to read: {})", self.analyzer.len(), short(&self.analyzer, 3))
            },
            self.simulated
        ));
        w.push_str(&format!(
            "\nApply: `theseus self narrow apply {id}` plans a change set of the hands stack, checks \
             that it touches only {role} and its new policy, and runs it. Not now: `theseus self \
             narrow decline {id}`. After an apply, a week's watch tells you of any AccessDenied from \
             the role, with a one-step revert.\n",
            id = self.id,
            role = self.role
        ));
        w
    }

    /// The owner's card (theseus-x8b9k): `words` under its head, its first
    /// line folded into the head.
    pub fn card(&self) -> String {
        let w = self.words();
        let body = w.split_once("\n\n").map_or(w.as_str(), |(_, b)| b);
        crate::owner_note::note(
            crate::owner_note::Kind::Decision,
            &format!("Apply or decline a narrower AWS policy for {}", self.role),
            &format!(
                "the weekly read of its CloudTrail found it allows more than it calls (proposal {})",
                self.id
            ),
            body,
        )
    }

    /// The change as JSON: the proposal's numbers, and the two documents.
    pub fn diff_json(&self) -> Value {
        json!({
            "proposal": self.id, "role": self.role, "evidence_days": self.days,
            "actions_before": self.reach_before, "actions_after": self.reach_after,
            "before": self.current, "after": self.candidate,
            "not_visible": self.not_visible, "kept": self.kept,
            "analyzer_findings": self.analyzer,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> Proposal {
        let cur = json!({"Statement": [{"Effect": "Allow", "Action": "*", "Resource": "*"}]});
        let cand = json!({"Statement": [
            {"Effect": "Allow", "Action": ["ec2:DescribeInstances", "ec2:DescribeVpcs"], "Resource": "*"},
            {"Effect": "Allow", "Action": ["logs:PutLogEvents"], "Resource": "arn:aws:logs:*:*:log-group:hands:*"}]});
        Proposal {
            id: id_of("theseus-hand-owner", &cand),
            role: "theseus-hand-owner".into(),
            profile: "owner".into(),
            at_ms: 1,
            days: 14,
            events: 412,
            pages: 9,
            reach_before: 930,
            reach_after: 118,
            wide_names: vec!["theseus-allow-all".into()],
            current: cur,
            candidate: cand,
            not_visible: vec!["s3:GetObject".into(), "s3:PutObject".into()],
            kept: vec!["logs:PutLogEvents".into()],
            elsewhere: vec![],
            unexplained: vec![],
            analyzer: vec!["SUGGESTION EMPTY_ARRAY_CONDITION: x".into()],
            simulated: 2,
            observed: vec!["ec2:DescribeInstances".into(), "ec2:DescribeVpcs".into()],
            state: State::Open,
            applied_ms: None,
            change_set: None,
            red: vec![],
        }
    }

    #[test]
    fn the_words_say_what_goes_what_stays_and_how_to_answer() {
        let p = sample();
        let w = p.words();
        for must in [
            "theseus-hand-owner",
            "about 930 actions allowed today, about 118 after (87% fewer)",
            "allow * on *",
            "ec2 (2)",
            "Kept from the code",
            "cannot show them (data events), not because they are unused: s3:GetObject, s3:PutObject",
            "theseus self narrow apply",
            "theseus self narrow decline",
            "412 events",
        ] {
            assert!(w.contains(must), "missing {must:?} in\n{w}");
        }
        let j = p.diff_json();
        assert_eq!(j["actions_before"], 930);
        assert_eq!(j["after"], p.candidate);
    }

    #[test]
    fn the_id_is_stable_for_one_candidate_and_differs_for_another() {
        let p = sample();
        assert_eq!(p.id, id_of(&p.role, &p.candidate));
        assert_ne!(p.id, id_of(&p.role, &json!({"Statement": []})));
    }
}
