//! What a label says of an answer (design §2.9; the forms are
//! `theseus_protocol::learning`'s), which one counts, and the operator's
//! label, checked against its judgment's pack before it is written.

use serde_json::{json, Value};
use theseus_judge::band::Top;
use theseus_judge::client::Kind;
use theseus_judge::judge::AnswerRecord;
use theseus_judge::pack::QuestionDef;
use theseus_judge::Pack;

use super::LabelRow;

/// What a label settles of one answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Truth {
    /// A Noul's truth.
    Bool(bool),
    /// A Choice's right class, or a class known to be wrong.
    Class(String),
    NotClass(String),
    /// A Score's right level, or a level known to be wrong.
    Level(usize),
    NotLevel(usize),
}

/// Whole-judgment words: `right` and `wrong` grade every answer's lean;
/// `wrong role` is role.v1's "wrong role" press, a `wrong`; `noise` and
/// `useful` are kept for the ladder's rules and grade nothing.
pub const WHOLE: [&str; 5] = ["right", "wrong", "wrong role", "noise", "useful"];

/// What `label` says of `answer`, if anything.
pub fn truth(label: &Value, answer: &AnswerRecord) -> Option<Truth> {
    let top = &answer.band.top;
    if let Some(s) = label.as_str() {
        match s {
            "right" => return Some(same(top)),
            "wrong" | "wrong role" => return Some(opposite(top)),
            // The ladder's (26a's `labels_per_day`): they grade nothing.
            "noise" | "useful" => return None,
            _ => {}
        }
    }
    let not = || label.get("not");
    match top {
        Top::Noul(_) => label.as_bool().map(Truth::Bool),
        Top::Choice(_) => match label {
            Value::String(c) => Some(Truth::Class(c.clone())),
            _ => not()
                .and_then(Value::as_str)
                .map(|c| Truth::NotClass(c.into())),
        },
        Top::Level(_) => match label.as_u64() {
            Some(n) => Some(Truth::Level(n as usize)),
            None => not()
                .and_then(Value::as_u64)
                .map(|n| Truth::NotLevel(n as usize)),
        },
    }
}

fn same(top: &Top) -> Truth {
    match top {
        Top::Noul(b) => Truth::Bool(*b),
        Top::Choice(c) => Truth::Class(c.clone()),
        Top::Level(n) => Truth::Level(*n),
    }
}

fn opposite(top: &Top) -> Truth {
    match top {
        Top::Noul(b) => Truth::Bool(!b),
        Top::Choice(c) => Truth::NotClass(c.clone()),
        Top::Level(n) => Truth::NotLevel(*n),
    }
}

/// The label that counts for a judgment's answer to `question`, and what it
/// says: of the labels on that question or on the whole judgment that say
/// something of it, the heaviest, and the newest of equal weight.
pub fn resolve<'a>(
    labels: &'a [LabelRow],
    question: &str,
    answer: &AnswerRecord,
) -> Option<(&'a LabelRow, Truth)> {
    labels
        .iter()
        .filter(|l| l.question.as_deref().is_none_or(|q| q == question))
        .filter_map(|l| truth(&l.label, answer).map(|t| (l, t)))
        .max_by(|(a, _), (b, _)| {
            a.weight
                .total_cmp(&b.weight)
                .then(a.position.cmp(&b.position))
        })
}

/// An operator's label, as written: the question checked against the pack,
/// and the label in its stored form (`"true"` from a CLI is `true`, `"not:x"`
/// is `{"not": "x"}`, `"2"` for a Score is `2`).
pub fn check(pack: &Pack, question: Option<&str>, label: &Value) -> Result<Value, String> {
    let words = |v: &Value| v.as_str().map(|s| s.trim().to_lowercase());
    let Some(q) = question else {
        return match words(label) {
            Some(w) if WHOLE.contains(&w.as_str()) => Ok(json!(w)),
            _ => Err(format!(
                "a label on the whole judgment is one of {}; name a question (`--question`) for \
                 anything else",
                WHOLE.join(", ")
            )),
        };
    };
    let def = pack.question(q).ok_or_else(|| {
        let ids: Vec<&str> = pack.questions.iter().map(|q| q.id.as_str()).collect();
        format!(
            "{} asks no question {q:?}: it asks {}",
            pack.name(),
            ids.join(", ")
        )
    })?;
    if let Some(w) = words(label).filter(|w| w == "right" || w == "wrong") {
        return Ok(json!(w));
    }
    match def.kind {
        Kind::Noul => noul(label).ok_or_else(|| {
            format!("{q} is a yes-or-no question: its label is true, false, right, or wrong")
        }),
        Kind::Choice => choice(def, label),
        Kind::Score => score(def, label),
    }
}

/// An operator's label on a per-item answer (`helps.3`, 32d), which is a
/// Noul's: true, false, right, or wrong.
pub fn check_item(question: &str, label: &Value) -> Result<Value, String> {
    let words = label.as_str().map(|s| s.trim().to_lowercase());
    if let Some(w) = words.filter(|w| w == "right" || w == "wrong") {
        return Ok(json!(w));
    }
    noul(label).ok_or_else(|| {
        format!("{question} is a yes-or-no question about one item: its label is true, false, right, or wrong")
    })
}

fn noul(label: &Value) -> Option<Value> {
    match label {
        Value::Bool(b) => Some(json!(b)),
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "true" | "yes" => Some(json!(true)),
            "false" | "no" => Some(json!(false)),
            _ => None,
        },
        _ => None,
    }
}

fn choice(def: &QuestionDef, label: &Value) -> Result<Value, String> {
    let (class, not) = match label {
        Value::String(s) => match s.trim().strip_prefix("not:") {
            Some(c) => (c.trim().to_string(), true),
            None => (s.trim().to_string(), false),
        },
        Value::Object(o) => match o.get("not").and_then(Value::as_str) {
            Some(c) => (c.trim().to_string(), true),
            None => (String::new(), false),
        },
        _ => (String::new(), false),
    };
    let ids: Vec<&str> = def.options.iter().map(|o| o.id.as_str()).collect();
    // A question whose options come from the state (role.v1's roles, a
    // task's ids) takes any non-empty id.
    let known = def.options_from.is_some() || ids.contains(&class.as_str());
    if class.is_empty() || !known {
        return Err(format!(
            "{} is a choice: its label is one of {}, not:<one of them>, right, or wrong",
            def.id,
            ids.join(", ")
        ));
    }
    Ok(if not {
        json!({"not": class})
    } else {
        json!(class)
    })
}

fn score(def: &QuestionDef, label: &Value) -> Result<Value, String> {
    let n = match label {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    };
    match n {
        Some(n) if (n as usize) < def.levels.len().max(1) => Ok(json!(n)),
        _ => Err(format!(
            "{} is a score: its label is a level from 0 to {}, right, or wrong",
            def.id,
            def.levels.len().saturating_sub(1)
        )),
    }
}

/// The key of a system label: its judgment, question, and rule, hashed, so a
/// second run finds it written and writes nothing.
pub fn system_key(judgment: &str, question: &str, rule: &str) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for part in [judgment, question, rule] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    let d = h.finalize();
    let hex: String = d[..12].iter().map(|b| format!("{b:02x}")).collect();
    format!("lbl_{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_judge::band::band;
    use theseus_judge::client::Answer;

    fn noul_answer(p: f64) -> AnswerRecord {
        let answer = Answer::Noul { noul: p };
        AnswerRecord {
            question: "risky".into(),
            def: "risky".into(),
            about: None,
            band: band(
                &answer,
                theseus_judge::Thresholds {
                    act: 0.9,
                    confirm: 0.6,
                },
            ),
            answer,
        }
    }

    fn row(position: u64, question: Option<&str>, label: Value, weight: f64) -> LabelRow {
        LabelRow {
            position,
            at_ms: position,
            id: format!("lbl_{position}"),
            judgment: "jdg_a".into(),
            pack: "security.v1".into(),
            question: question.map(str::to_string),
            about: None,
            label,
            source: if weight < 1.0 { "system" } else { "operator" }.into(),
            weight,
        }
    }

    /// The heaviest label counts, the newest of equal weight, and a label
    /// that says nothing of the question is passed over.
    #[test]
    fn the_heaviest_label_counts_then_the_newest() {
        let a = noul_answer(0.8);
        let labels = vec![
            row(1, Some("risky"), json!(false), 0.5),
            row(2, None, json!("noise"), 1.0),
            row(3, Some("risky"), json!(true), 1.0),
            row(4, Some("other"), json!(false), 1.0),
        ];
        let (l, t) = resolve(&labels, "risky", &a).unwrap();
        assert_eq!((l.id.as_str(), t), ("lbl_3", Truth::Bool(true)));
        let labels = vec![
            row(1, Some("risky"), json!(true), 1.0),
            row(2, None, json!("wrong"), 1.0),
        ];
        assert_eq!(
            resolve(&labels, "risky", &a).unwrap().1,
            Truth::Bool(false),
            "the newest of equal weight: `wrong` of a lean to true"
        );
        assert!(resolve(&[], "risky", &a).is_none());
    }

    /// `noise` and `useful` on the whole judgment grade no answer, a
    /// Choice's among them, and a lighter label on the question still
    /// counts.
    #[test]
    fn noise_and_useful_grade_nothing() {
        let answer = Answer::Choice {
            choice: "other".into(),
            probabilities: vec![("other".into(), 0.7), ("coder".into(), 0.3)],
            confidence: 0.7,
        };
        let a = AnswerRecord {
            question: "role".into(),
            def: "role".into(),
            about: None,
            band: band(
                &answer,
                theseus_judge::Thresholds {
                    act: 0.9,
                    confirm: 0.6,
                },
            ),
            answer,
        };
        for word in ["noise", "useful"] {
            assert_eq!(truth(&json!(word), &a), None, "{word}");
            let labels = vec![row(1, None, json!(word), 1.0)];
            assert!(resolve(&labels, "role", &a).is_none(), "{word}");
        }
        let labels = vec![
            row(1, Some("role"), json!("coder"), 0.5),
            row(2, None, json!("noise"), 1.0),
        ];
        assert_eq!(
            resolve(&labels, "role", &a).unwrap().1,
            Truth::Class("coder".into())
        );
    }

    /// The operator's forms, checked against the pack.
    #[test]
    fn an_operators_label_is_checked_against_its_pack() {
        let lp = theseus_judge::pack::by_name("loop.v1").unwrap();
        let ok = |q: Option<&str>, l: Value| check(&lp, q, &l);
        assert_eq!(
            ok(Some("work_state"), json!("progressing")),
            Ok(json!("progressing"))
        );
        assert_eq!(
            ok(Some("work_state"), json!("not:complete")),
            Ok(json!({"not": "complete"}))
        );
        assert!(ok(Some("work_state"), json!("sideways")).is_err());
        assert_eq!(
            ok(Some("announced_unfinished"), json!("yes")),
            Ok(json!(true))
        );
        assert_eq!(
            ok(Some("announced_unfinished"), json!(false)),
            Ok(json!(false))
        );
        assert!(ok(Some("announced_unfinished"), json!("maybe")).is_err());
        assert_eq!(ok(Some("work_state"), json!("Wrong")), Ok(json!("wrong")));
        assert!(ok(Some("nope"), json!(true)).is_err());
        assert_eq!(ok(None, json!("noise")), Ok(json!("noise")));
        assert!(ok(None, json!("progressing")).is_err());
        // A role's options come from the state: any id goes.
        let role = theseus_judge::pack::by_name("role.v1").unwrap();
        assert_eq!(
            check(&role, Some("role"), &json!("reviewer")),
            Ok(json!("reviewer"))
        );
    }

    #[test]
    fn a_system_labels_key_is_its_judgment_question_and_rule() {
        let k = system_key("jdg_a", "work_state", "continuation");
        assert_eq!(k, system_key("jdg_a", "work_state", "continuation"));
        assert_ne!(k, system_key("jdg_a", "work_state", "false_completion"));
        assert_ne!(k, system_key("jdg_b", "work_state", "continuation"));
        assert!(k.starts_with("lbl_") && k.len() == 28);
    }
}
