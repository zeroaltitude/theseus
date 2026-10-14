//! An edit's approval card shows its hunk (theseus-z6au): the question's
//! `preview` as a ```` ```diff ```` block, cut to fit the card's message with a
//! marker where it was cut.

use theseus_protocol::diff::{first_changed, rows};
use theseus_protocol::{ConfirmPreview, ConfirmRequest};

/// The most of a hunk the card carries, in characters.
const HUNK_CHARS: usize = 800;

/// `\n```diff … ```` for a request whose preview is a diff; nothing else.
pub fn hunk(req: &ConfirmRequest) -> String {
    let Some(p) = req
        .preview
        .as_ref()
        .filter(|p| p.kind == ConfirmPreview::DIFF)
    else {
        return String::new();
    };
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
    fn a_long_hunk_is_cut_with_a_marker_and_never_breaks_the_fence() {
        let body: String = (0..200).map(|i| format!("+line {i} ```\n")).collect();
        let h = hunk(&asked(&body));
        assert!(h.chars().count() < 1000 && h.contains("is cut)") && h.ends_with("```"));
        assert_eq!(h.matches("```").count(), 2);
    }
}
