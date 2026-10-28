//! What a needs-you card says (theseus-suvpe): three short lines from the
//! request's structured fields, never its `reason` text. **What** it wants
//! (the tool in words and its target), **why** it asks (who, and the rule
//! that held it, from `floor`, `budget`, `change` and the
//! tool), and **what yes does**. The Details press shows the full request.

use serde_json::{json, Value};
use theseus_kernel::usd_to_micros;
use theseus_protocol::term::words::summarize;
use theseus_protocol::work::QuestionKind;
use theseus_protocol::ConfirmRequest;

use crate::narrative::dollars;

/// The three lines.
pub struct Words {
    pub what: String,
    pub why: String,
    pub yes: String,
    /// An approval's plain words (theseus-cudar), empty for the other kinds:
    /// who asks, what No does, the risk badge, the rule that holds it, and
    /// the one line a settled card repeats.
    pub who: String,
    pub no: String,
    pub risk: String,
    pub held: String,
    pub line: String,
}

impl Words {
    fn of(what: String, why: String, yes: String) -> Self {
        Self {
            what,
            why,
            yes,
            who: String::new(),
            no: String::new(),
            risk: String::new(),
            held: String::new(),
            line: String::new(),
        }
    }
}

/// `s` in at most `max` characters: cut at a word, or for one long token
/// (a path) at its last `/` with the head left off, so no name is cut.
pub fn short(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    if let Some(at) = head.rfind(char::is_whitespace) {
        return format!("{}…", head[..at].trim_end());
    }
    let tail: String = s.chars().skip(s.chars().count() - (max - 1)).collect();
    match tail.find('/') {
        Some(at) => format!("…{}", &tail[at..]),
        None => format!("…{tail}"),
    }
}

fn text<'a>(input: &'a Value, k: &str) -> Option<&'a str> {
    input.get(k).and_then(Value::as_str)
}

/// The call as a person says it, with its target in code.
fn what_of(tool: &str, input: &Value) -> String {
    let path = text(input, "path").map(|p| short(p, 90));
    let argv = input.get("argv").and_then(Value::as_array).map(|a| {
        let words: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
        short(&words.join(" "), 110)
    });
    match (tool, path, argv) {
        ("fs.edit", Some(p), _) => format!("edit `{p}`"),
        ("fs.write", Some(p), _) => format!("write `{p}`"),
        ("fs.patch", Some(p), _) => format!("patch `{p}`"),
        ("fs.read", Some(p), _) => format!("read `{p}`"),
        ("proc.run", _, Some(a)) => format!("run `{a}`"),
        ("http.fetch", ..) => match text(input, "url") {
            Some(u) => format!("fetch `{}`", short(u, 100)),
            None => "fetch a page".into(),
        },
        _ => short(&summarize(tool, input).replace('`', "'"), 120),
    }
}

/// The card's words for a question, and the kind it is. `asked` is an
/// `owner_ask`'s own question.
pub fn words(asked: Option<String>, req: &ConfirmRequest, who: &str) -> (QuestionKind, Words) {
    let kind = crate::rpc::answer::kind_of(req);
    let w = match kind {
        QuestionKind::Ask => Words::of(
            asked.unwrap_or_else(|| "a question".into()),
            format!("{who} asks you to choose."),
            "Your pick is what it goes on with.".into(),
        ),
        QuestionKind::Budget => {
            let b = req.budget.clone().unwrap_or_default();
            let d = |x: f64| dollars(usd_to_micros(x));
            Words::of(
                format!("spend {} more on {who}", d(b.needed_usd)),
                format!(
                    "it has used {} of its {} limit",
                    d(b.spent_usd),
                    d(b.limit_usd)
                ),
                "Yes resets its spend to $0 and the call runs.".into(),
            )
        }
        QuestionKind::Change => Words::of(
            req.change.as_ref().map_or_else(
                || short(&summarize(&req.tool, &req.input), 120),
                |c| short(&c.question(), 160),
            ),
            format!("{who} wants to change a task, and a task's layer 1 is yours."),
            "Yes makes the change; no leaves the task as it is.".into(),
        ),
        QuestionKind::Approval if req.plain.is_some() => {
            let p = req.plain.clone().unwrap_or_default();
            Words {
                what: p.what.clone(),
                why: p.why.clone(),
                yes: format!("Yes {}", p.yes),
                no: format!("No {}", p.no),
                who: p.who.clone(),
                risk: p.risk.clone(),
                held: p.held.clone(),
                line: p.line(),
            }
        }
        QuestionKind::Extension | QuestionKind::Approval => {
            let held = super::plain::held(req);
            Words::of(
                what_of(&req.tool, &req.input),
                format!("{who} asks: {}.", held.strip_suffix('.').unwrap_or(&held)),
                match kind {
                    QuestionKind::Extension => "Yes loads it.".into(),
                    _ => "Yes runs this one call.".into(),
                },
            )
        }
    };
    (kind, w)
}

/// The `card` object of a post's body.
pub fn card_of(kind: QuestionKind, w: &Words, nth: Option<(usize, usize)>) -> Value {
    json!({
        "kind": format!("{kind:?}").to_lowercase(),
        "what": w.what,
        "why": w.why,
        "yes": w.yes,
        "who": w.who,
        "no": w.no,
        "risk": w.risk,
        "held": w.held,
        "line": w.line,
        "n": nth.map(|(n, _)| n),
        "of": nth.map(|(_, of)| of),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::BudgetAsk;

    fn req(tool: &str, input: Value, reason: &str) -> ConfirmRequest {
        ConfirmRequest {
            correlation_id: "act_0000feed".into(),
            tool: tool.into(),
            input,
            reason: reason.into(),
            ..Default::default()
        }
    }

    /// An edit, a run and a spend each say what, why and what yes does, from
    /// the request's fields; none repeats the reason's text or ends in a
    /// word cut short.
    #[test]
    fn the_card_speaks_plain_words() {
        let raw = "fs.edit — approve (enforcement = approve; its file is under /var/lib/theseus/state, fa";
        let edit = req(
            "fs.edit",
            json!({"path": "crates/theseus-core/src/compiler/effort.rs"}),
            raw,
        );
        let (_, w) = words(None, &edit, "Calls from harness fail");
        assert_eq!(w.what, "edit `crates/theseus-core/src/compiler/effort.rs`");
        assert_eq!(
            w.why,
            "Calls from harness fail asks: `fs.edit` is set to ask before it runs."
        );
        assert_eq!(w.yes, "Yes runs this one call.");
        let run = req(
            "proc.run",
            json!({"argv": ["cargo", "test", "-p", "theseus-core"]}),
            raw,
        );
        let (_, w) = words(None, &run, "task 7f3a2c");
        assert_eq!(w.what, "run `cargo test -p theseus-core`");
        let mut spend = req("budget.reset", json!({}), raw);
        spend.budget = Some(BudgetAsk {
            spent_usd: 10.0,
            limit_usd: 10.0,
            needed_usd: 4.0,
            lifetime_usd: 12.0,
        });
        spend.tool = theseus_kernel::BUDGET_TOOL.into();
        let (kind, w) = words(None, &spend, "task 7f3a2c");
        assert_eq!(kind, QuestionKind::Budget);
        assert_eq!(w.what, "spend $4 more on task 7f3a2c");
        assert_eq!(w.why, "it has used $10 of its $10 limit");
        assert!(w.yes.starts_with("Yes resets"));
        for line in [&edit, &run, &spend].map(|r| {
            let (_, w) = words(None, r, "task 7f3a2c");
            format!("{}\n{}\n{}", w.what, w.why, w.yes)
        }) {
            assert!(!line.contains("enforcement"), "{line}");
            assert!(!line.contains("fa\n") && !line.ends_with("fa"), "{line}");
        }
        // The floor is the rule when it is why.
        let mut floor = edit;
        floor.floor = true;
        assert!(words(None, &floor, "x").1.why.contains("always asks"));
    }

    /// A request with plain words speaks them (theseus-cudar): who wants to
    /// do what, the risk and the reason, and what Yes and No do; the line a
    /// settled card keeps is the whole question again.
    #[test]
    fn an_approval_with_plain_words_speaks_them() {
        let mut r = req("fs.read", json!({"path": "x"}), "raw");
        r.plain = Some(theseus_protocol::PlainWords {
            who: "Theseus's self-repair: fix the tides".into(),
            what: "read its own report from the 05:12 run".into(),
            why: "to see why the last run stopped".into(),
            held: "`fs.read` is set to ask before it runs".into(),
            risk: "read-only".into(),
            yes: "runs this one call.".into(),
            no: "refuses it; the session is told and carries on without it.".into(),
        });
        let (_, w) = words(None, &r, "ignored");
        assert_eq!(w.what, "read its own report from the 05:12 run");
        assert_eq!(w.yes, "Yes runs this one call.");
        assert!(w.no.starts_with("No refuses it"), "{}", w.no);
        for must in [
            "self-repair: fix the tides",
            "05:12",
            "read-only",
            "to see why",
        ] {
            assert!(w.line.contains(must), "{must} in {}", w.line);
        }
        let card = card_of(QuestionKind::Approval, &w, None);
        assert_eq!(card["risk"], "read-only");
        assert_eq!(card["who"], "Theseus's self-repair: fix the tides");
        assert_eq!(card["line"], json!(w.line));
    }

    /// A held clause that ends in a period gets no second one; one without
    /// gets exactly one; `?` and `!` are left alone.
    #[test]
    fn an_asks_sentence_ends_in_one_period() {
        for tool in ["fs.write", crate::extend::ACK] {
            for (clause, said) in [
                ("the folder is protected.", "the folder is protected."),
                ("the folder is protected", "the folder is protected."),
                ("is the folder protected?", "is the folder protected?."),
            ] {
                let r = req(
                    tool,
                    json!({"path": "x"}),
                    &format!("{tool} — approve ({clause})"),
                );
                let (_, w) = words(None, &r, "ignored");
                assert!(w.why.ends_with(&format!("asks: {said}")), "{}", w.why);
                assert!(!w.why.ends_with(".."), "{}", w.why);
            }
        }
    }

    #[test]
    fn a_long_target_keeps_its_end_and_a_long_sentence_ends_at_a_word() {
        let path = format!("/home/x/{}/effort.rs", "deep/".repeat(40));
        let cut = short(&path, 60);
        assert!(cut.starts_with('…') && cut.ends_with("/effort.rs"), "{cut}");
        assert!(cut.chars().count() <= 60);
        let said = short(
            "run the whole of the long suite once more before the release",
            30,
        );
        assert_eq!(said, "run the whole of the long…");
        assert_eq!(short("short", 30), "short");
    }
}
