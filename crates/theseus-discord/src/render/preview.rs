//! An edit's approval card shows its hunk (theseus-z6au): the question's
//! `preview` as a ```` ```diff ```` block, cut to fit the card's message with a
//! marker where it was cut.

use theseus_protocol::diff::{first_changed, rows};
use theseus_protocol::{ConfirmPreview, ConfirmRequest};

/// The most of a hunk the card carries, in characters.
const HUNK_CHARS: usize = 800;

/// `\n```diff … ```` for a request whose preview is a diff, a plan's
/// numbered steps as a quote for one whose preview is text (theseus-lyxy: a
/// plan is its question, so its card must show it), and the draft a send
/// binds as a quote for a mail's; nothing else.
pub fn hunk(req: &ConfirmRequest) -> String {
    let Some(p) = req.preview.as_ref() else {
        return String::new();
    };
    if p.kind == ConfirmPreview::TEXT {
        return plan(&p.body);
    }
    if p.kind == ConfirmPreview::MAIL {
        return mail(&p.body);
    }
    if p.kind != ConfirmPreview::DIFF {
        return String::new();
    }
    let parsed = rows(&p.body);
    let file = parsed
        .iter()
        .find(|r| r.text.starts_with("+++ "))
        .map(|r| r.text.trim_start_matches("+++ ").trim().to_string());
    let mut out = String::new();
    if let (Some(f), Some(n)) = (file, first_changed(&parsed)) {
        out.push_str(&format!("{f}:{n}\n"));
    }
    let mut cut = false;
    for l in p
        .body
        .lines()
        .filter(|l| !l.starts_with("--- ") && !l.starts_with("+++ "))
    {
        let l = l.replace("```", "'''");
        if out.chars().count() + l.chars().count() + 1 > HUNK_CHARS {
            cut = true;
            break;
        }
        out.push_str(&l);
        out.push('\n');
    }
    if cut {
        out.push_str("… (the rest of the diff is cut)\n");
    }
    format!("\n```diff\n{out}```")
}

/// A plan's steps, one quoted line each, cut to what the card carries.
fn plan(body: &str) -> String {
    let mut out = String::new();
    for l in body.lines() {
        let l = format!("> {}\n", l.replace('\n', " "));
        if out.chars().count() + l.chars().count() > HUNK_CHARS {
            out.push_str("> … (the rest of the plan is cut)\n");
            break;
        }
        out.push_str(&l);
    }
    format!("\n{}", out.trim_end())
}

/// The draft a send binds, as a quote: From, each recipient with a new one
/// marked (` (new)` after one, on a `To:`, `Cc:` or `Bcc:` line alone: a
/// subject or body saying "(new)" stays plain), the subject, the body's first
/// lines and the attachments, cut with a marker; the last line (`Draft <12
/// hex>`, what the approval is bound to) always stays. An `@` is broken so a
/// draft's text can never mention anyone.
fn mail(body: &str) -> String {
    let mut lines: Vec<&str> = body.lines().collect();
    let tail = lines.pop().unwrap_or("");
    let quote = |l: &str| {
        let l = match l
            .strip_prefix("draft ")
            .and_then(|l| l.split_once(" from "))
        {
            Some((_, from)) => format!("From: {from}"),
            None => l.to_string(),
        };
        let flagged = ["To: ", "Cc: ", "Bcc: "].iter().any(|p| l.starts_with(p));
        let l = if flagged {
            l.replace("(new)", "**(new)**")
        } else {
            l
        };
        format!("> {}\n", l.replace('@', "@\u{200b}"))
    };
    let mut out = String::new();
    for l in lines {
        let q = quote(l);
        if out.chars().count() + q.chars().count() + tail.len() + 50 > HUNK_CHARS {
            out.push_str("> … (the rest of the draft is cut)\n");
            break;
        }
        out.push_str(&q);
    }
    format!("\n{out}> -# {tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(body: &str) -> ConfirmRequest {
        ConfirmRequest {
            tool: "fs.edit".into(),
            preview: Some(ConfirmPreview {
                kind: "diff".into(),
                body: body.into(),
            }),
            ..ConfirmRequest::default()
        }
    }

    #[test]
    fn a_cards_hunk_is_a_diff_block_with_its_file_and_line() {
        let h = hunk(&asked(
            "--- tides.py\n+++ tides.py\n@@ -49,1 +49,1 @@\n-    return lowest - highest\n+    return highest - lowest\n",
        ));
        assert_eq!(
            h,
            "\n```diff\ntides.py:49\n@@ -49,1 +49,1 @@\n-    return lowest - highest\n+    return highest - lowest\n```"
        );
        assert_eq!(hunk(&ConfirmRequest::default()), "");
    }

    #[test]
    fn a_plans_card_quotes_its_numbered_steps() {
        let q = ConfirmRequest {
            tool: "plan.present".into(),
            preview: Some(ConfirmPreview {
                kind: ConfirmPreview::TEXT.into(),
                body: "1. add the function\n2. test it".into(),
            }),
            ..ConfirmRequest::default()
        };
        assert_eq!(hunk(&q), "\n> 1. add the function\n> 2. test it");
        let long = ConfirmRequest {
            preview: Some(ConfirmPreview {
                kind: ConfirmPreview::TEXT.into(),
                body: (1..=60)
                    .map(|i| format!("{i}. {}", "step ".repeat(8)))
                    .collect::<Vec<_>>()
                    .join("\n"),
            }),
            ..ConfirmRequest::default()
        };
        let h = hunk(&long);
        assert!(h.chars().count() < 1000 && h.ends_with("is cut)"), "{h}");
    }

    #[test]
    fn a_long_hunk_is_cut_with_a_marker_and_never_breaks_the_fence() {
        let body: String = (0..200).map(|i| format!("+line {i} ```\n")).collect();
        let h = hunk(&asked(&body));
        assert!(h.chars().count() < 1000 && h.contains("is cut)") && h.ends_with("```"));
        assert_eq!(h.matches("```").count(), 2);
    }

    #[test]
    fn the_discord_send_card_shows_the_preview_and_flags_new_recipients() {
        let body = "draft r1 from owner@example.test\nTo: sam@example.test, pat@example.test (new)\n\
                    Subject: Re: Budget\nBody (first 2 of 2 lines):\n  Numbers attached.\n  Thanks.\n\
                    Attachments: ledger.csv\nDraft 0123456789ab";
        let q = ConfirmRequest {
            tool: "gmail.send".into(),
            preview: Some(ConfirmPreview {
                kind: ConfirmPreview::MAIL.into(),
                body: body.into(),
            }),
            ..ConfirmRequest::default()
        };
        assert_eq!(
            hunk(&q),
            "\n> From: owner@\u{200b}example.test\n> To: sam@\u{200b}example.test, pat@\u{200b}example.test **(new)**\n\
             > Subject: Re: Budget\n> Body (first 2 of 2 lines):\n>   Numbers attached.\n>   Thanks.\n\
             > Attachments: ledger.csv\n> -# Draft 0123456789ab"
        );
        // A long body is cut, and the digest line still ends the card.
        let long = format!("{}\nDraft 0123456789ab", "x line\n".repeat(300));
        let h = mail(&long);
        assert!(h.chars().count() < 1000 && h.contains("is cut)"), "{h}");
        assert!(h.ends_with("> -# Draft 0123456789ab"), "{h}");
    }

    #[test]
    fn the_discord_send_card_bolds_only_recipient_flags() {
        let body = "draft r1 from owner@example.test\nTo: sam@example.test (new)\n\
                    Cc: pat@example.test (new)\nBcc: lee@example.test (new)\n\
                    Subject: Plan (new)\nBody (first 1 of 1 lines):\n  see the (new) doc\n\
                    Draft 0123456789ab";
        let q = ConfirmRequest {
            tool: "gmail.send".into(),
            preview: Some(ConfirmPreview {
                kind: ConfirmPreview::MAIL.into(),
                body: body.into(),
            }),
            ..ConfirmRequest::default()
        };
        let h = hunk(&q);
        for to in ["To: sam", "Cc: pat", "Bcc: lee"] {
            let l = h.lines().find(|l| l.contains(to)).unwrap();
            assert!(l.ends_with("**(new)**"), "{l}");
        }
        assert!(h.contains("> Subject: Plan (new)\n"), "{h}");
        assert!(h.contains(">   see the (new) doc\n"), "{h}");
        assert_eq!(h.matches("**(new)**").count(), 3, "{h}");
    }
}
