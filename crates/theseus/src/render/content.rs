//! A session's content labels in the CLI's lines (theseus-9ymz0): the column
//! `theseus sessions` shows, `theseus content`'s list, and what a removal
//! prints. The words of what each flag does are the core's
//! (`content::Flag::means`); the CLI carries only the short form, since it
//! links the protocol and nothing else.

use theseus_protocol::{LabelView, SessionInfo, UnlabelResult};

use super::fmt_date;

/// What a flag does to a session, in a few words.
pub fn effect(flag: &str) -> &'static str {
    match flag {
        "injection" => "writes, programs and sends are refused",
        "secret" | "confidential" | "personal" => "China-origin models are off",
        _ => "",
    }
}

/// `theseus sessions`' flag column: `\t⚑ injection,secret`, or nothing for a
/// session with no label (and from a daemon before them), so a row without
/// one is the row it always was.
pub(super) fn flag_column(s: &SessionInfo) -> String {
    if s.labels.is_empty() {
        return String::new();
    }
    let flags: Vec<&str> = s.labels.iter().map(|l| l.flag.as_str()).collect();
    format!("\t⚑ {}", flags.join(","))
}

/// One label, as `theseus content` lists it: the flag, when it was raised,
/// what raised it (the item's kind and its node), and from where.
pub fn label_line(l: &LabelView) -> String {
    let node = match &l.node {
        Some(n) => format!(" (node {n})"),
        None => String::new(),
    };
    format!(
        "{}\t{}\t{}{}\tfrom {}\t{} {:.2}",
        l.flag,
        fmt_date(l.at_ms),
        l.kind,
        node,
        l.source,
        l.band,
        l.confidence
    )
}

/// What `theseus content SESSION` prints: a line for each label, each with
/// what it does, or that there is none.
pub fn labels_lines(session: &str, labels: &[LabelView]) -> Vec<String> {
    if labels.is_empty() {
        return vec![format!("{session}: no content labels")];
    }
    let mut out = vec![format!(
        "{session}: {} content label{}",
        labels.len(),
        if labels.len() == 1 { "" } else { "s" }
    )];
    for l in labels {
        out.push(format!("  {}", label_line(l)));
        out.push(format!(
            "    {}; clear it with `theseus content remove {session} {}`",
            effect(&l.flag),
            l.flag
        ));
        if let Some(j) = &l.judgment {
            out.push(format!(
                "    raised by {j}; if it should not have been, `theseus judge label {j} no --question {}` teaches the classifier and leaves the label",
                l.flag
            ));
        }
    }
    out
}

/// What `theseus content remove` prints for one removal.
pub fn unlabelled_line(r: &UnlabelResult) -> String {
    format!(
        "removed {} from session {} (by {}) · it was raised by {} from {} · {}",
        r.flag,
        r.session_id,
        r.by,
        r.label.kind,
        r.label.source,
        match r.flag.as_str() {
            "injection" => "its calls that act run at their postures again",
            _ => "China-origin models are allowed again, until something else disqualifies it",
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(flag: &str) -> LabelView {
        LabelView {
            flag: flag.into(),
            node: Some("msg_7".into()),
            kind: "tool_result".into(),
            source: "http.fetch example.invalid/x".into(),
            confidence: 0.93,
            band: "act".into(),
            at_ms: 1_760_000_000_000,
            judgment: None,
        }
    }

    fn info(labels: Vec<LabelView>) -> SessionInfo {
        let mut s: SessionInfo = serde_json::from_value(serde_json::json!({
            "session_id": "ses_a1b2", "kind": "conversation", "label": null,
            "created_at_unix_ms": 1, "turns": 0}))
        .unwrap();
        s.labels = labels;
        s
    }

    /// A session with labels shows its flags after its other columns, and one
    /// without shows no column at all, so a daemon before the flags prints the
    /// rows it always did.
    #[test]
    fn the_flag_column_is_there_only_for_a_labelled_session() {
        assert_eq!(flag_column(&info(vec![])), "");
        assert_eq!(
            flag_column(&info(vec![label("injection"), label("secret")])),
            "\t⚑ injection,secret"
        );
    }

    /// A label's line says the flag, what raised it and from where, with the
    /// node it came from and how sure it was.
    #[test]
    fn a_label_line_says_what_raised_it_and_from_where() {
        let l = label_line(&label("injection"));
        assert!(l.starts_with("injection\t"), "{l}");
        assert!(l.contains("tool_result (node msg_7)"), "{l}");
        assert!(l.contains("from http.fetch example.invalid/x"), "{l}");
        assert!(l.ends_with("act 0.93"), "{l}");
        let none = LabelView {
            node: None,
            ..label("secret")
        };
        assert!(!label_line(&none).contains("node"));
    }

    /// A label that names its judgment says how to call it wrong; one raised
    /// before store format 53 names none.
    #[test]
    fn the_list_names_the_judgment_a_wrong_label_is_taught_through() {
        let with = LabelView {
            judgment: Some("jdg_x1".into()),
            ..label("secret")
        };
        let lines = labels_lines("ses_a1b2", &[with, label("personal")]);
        let said: Vec<&String> = lines.iter().filter(|l| l.contains("judge label")).collect();
        assert_eq!(said.len(), 1, "{lines:#?}");
        assert!(said[0].contains("theseus judge label jdg_x1 no --question secret"));
    }

    /// The list names the remedy for each label, and says so when there is none.
    #[test]
    fn the_list_names_the_command_that_clears_each_label() {
        let lines = labels_lines("ses_a1b2", &[label("injection"), label("personal")]);
        assert_eq!(lines[0], "ses_a1b2: 2 content labels");
        assert!(lines
            .iter()
            .any(|l| l.contains("writes, programs and sends are refused")));
        assert!(lines
            .iter()
            .any(|l| l.contains("theseus content remove ses_a1b2 personal")));
        assert_eq!(
            labels_lines("ses_a1b2", &[]),
            vec!["ses_a1b2: no content labels".to_string()]
        );
    }
}
