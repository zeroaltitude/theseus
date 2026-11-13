//! What a person's own filing is made of (theseus-cux0o): the pure parts of
//! `self.file`, so the verb in [`super::word`] is the judgment, the lock and
//! the frame and nothing else.
//!
//! Until this row the only way onto the self backlog was the nightly miner
//! ([`super::read`]), whose [`super::Work::file`] is private, takes a
//! `Candidate` and a `Night`, and is reachable from nowhere else. So a person
//! who found a defect could not hand it to the loop at all, and the owner's
//! own chosen shape for the self-build bar could not be run.
//!
//! **A person's item is marked for good, in the fields a task already has**
//! (no store format change, and nothing new on the META record):
//!
//! - `origin.self_started = false` and `origin.by_model = false`. Both are
//!   stored, both are already read as "Theseus chose this itself", and a
//!   mined item sets both true, so the pair is the mark.
//! - `origin.because` is `None`. It is documented as "the model's own
//!   one-line reason ... Only a self-started task has one", so leaving it
//!   empty is part of the same mark rather than a gap.
//! - `origin.session` is [`FILED_FROM`], never the miner's `self:backlog`.
//! - an evidence entry whose `by` is the filer, as [`crate::approval::Answerer::who`]
//!   names them. That is where **who** filed it lives, and the existing
//!   machinery already writes `agent:<name>` there for an agent's own
//!   process (`rpc/server.rs`'s `answerer`), so an agent's filing is marked
//!   as the agent's and is never passed off as the owner's.
//!
//! **Its cause key is [`cause_of`], `person:<item>`, unique to the item.** A
//! mined cause key is the thing that recurs, which is what lets a night fold
//! new evidence onto an open item and lets a drop suppress the cause for
//! good. A person's finding is none of those: two people who file the same
//! sentence mean two items, and dropping one must not silence the other. The
//! key also holds no word of the finding, so [`super::keel::class_of`]'s
//! `contains` table cannot read a class out of the filer's prose by accident
//! -- [`keel_class`] asks it of the words deliberately instead.

/// The `origin.session` of a person's item: never the miner's.
pub const FILED_FROM: &str = "self:filed";

/// The `source` of a person's item: where a mined one names a ledger kind,
/// `health` or `learning`.
pub const SOURCE: &str = "person";

/// The prefix every person-filed cause key carries.
pub const PERSON: &str = "person:";

/// A title is cut to this many characters.
pub const TITLE_MAX: usize = 120;

/// A finding longer than this is refused with words, never cut: the backlog
/// is a queue of findings, not a place to put a report.
pub const FINDING_MAX: usize = 4_000;

/// The cause key of the item `item`: unique to it, and holding none of the
/// filer's words. See the module doc for why it is not the finding.
pub fn cause_of(item: &str) -> String {
    format!("{PERSON}{item}")
}

/// Whether a cause key is a person's filing.
pub fn is_person(cause: &str) -> bool {
    cause.starts_with(PERSON)
}

/// The finding, trimmed, or why it cannot be filed.
pub fn finding_of(finding: &str) -> Result<&str, String> {
    let f = finding.trim();
    if f.is_empty() {
        return Err("a finding in your own words: `theseus self file \"<what is wrong>\"`".into());
    }
    if f.chars().count() > FINDING_MAX {
        return Err(format!(
            "that finding is {} characters and the backlog takes {FINDING_MAX}: file the finding \
             and leave the report where the work can read it",
            f.chars().count()
        ));
    }
    Ok(f)
}

/// The line the backlog lists the item by: the filer's `title` when they gave
/// one, else the finding's first sentence. Cut to [`TITLE_MAX`] characters
/// with an ellipsis, so a listing stays one line per item.
pub fn title_of(finding: &str, given: Option<&str>) -> String {
    let given = given.map(str::trim).filter(|t| !t.is_empty());
    let whole = match given {
        Some(t) => t.to_string(),
        None => first_sentence(finding),
    };
    cut(&whole.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The finding's first sentence: up to the first sentence end on the first
/// non-blank line, or the whole line.
///
/// An abbreviation is not a sentence end ([`abbreviation`]): the first draft
/// of this read `.` followed by a space as the end, and its own test caught
/// it cutting `the e.g. case is wrong.` at `the e.g.`, which would have made
/// that the item's title on the backlog for good.
fn first_sentence(finding: &str) -> String {
    let line = finding.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        let ends = matches!(c, '.' | '!' | '?')
            && chars.peek().is_none_or(|n| n.is_whitespace())
            && !abbreviation(&out);
        if ends {
            break;
        }
    }
    out.trim().to_string()
}

/// Whether the text so far ends in an abbreviation rather than a sentence:
/// the last run of non-space characters, with its trailing stops taken off,
/// holds a `.` of its own (`e.g.`, `i.e.`, `U.S.`) or is one character (an
/// initial). Deliberately a rule and not a word list: a list of
/// abbreviations is a thing nobody can finish.
fn abbreviation(so_far: &str) -> bool {
    let token = so_far
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or("")
        .trim_end_matches(['.', '!', '?']);
    token.contains('.') || token.chars().count() == 1
}

/// `s`, cut to [`TITLE_MAX`] characters on a word boundary where there is
/// one, with `…` when anything was left out.
fn cut(s: &str) -> String {
    if s.chars().count() <= TITLE_MAX {
        return s.to_string();
    }
    let head: String = s.chars().take(TITLE_MAX - 1).collect();
    let head = match head.rsplit_once(' ') {
        Some((h, _)) if h.chars().count() >= TITLE_MAX / 2 => h,
        _ => head.trim_end(),
    };
    format!("{head}…")
}

/// The keel class that holds a person's item for the owner, if any, read
/// from the **words** rather than from the key (which holds none of them).
///
/// A mined item whose cause names a keel class is filed `waiting_human` and
/// never starts, so the owner's own yes comes before a fix touches one of
/// those classes. Filing a finding is not that yes -- it says what is wrong,
/// not that a change to the sealed sets or the spend limits may land -- so a
/// person's item is read by the same table and waits the same way. This is
/// the narrower of the two readings; the wider one would let a filing start
/// work the miner's own item could not.
pub fn keel_class(title: &str, finding: &str) -> Option<&'static str> {
    let words = format!("{title} {finding}").to_lowercase();
    super::keel::class_of(&words)
}

/// The objective the item carries: the finding verbatim, who filed it, and
/// the same closing instruction a mined item ends with, so
/// [`super::start`] starts it with no case of its own.
pub fn objective_of(item: &str, finding: &str, filed_by: &str) -> String {
    format!(
        "A person filed this finding; it is not one Theseus mined from its own record. Filed by \
         {filed_by}.\n\n{finding}\n\nFind the root cause, fix it with a test that fails without \
         the fix, and call self.propose_change {{ item: \"{item}\", issue: <the issue>, why: <one \
         sentence> }}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cause_key_is_the_items_own_and_holds_no_words_of_the_finding() {
        let c = cause_of("tsk_x1");
        assert_eq!(c, "person:tsk_x1");
        assert!(is_person(&c));
        assert!(!is_person("turn.failed:timeout"));
        // Two filings of one sentence are two causes, so neither folds into
        // the other and a drop of one leaves the other standing.
        assert_ne!(cause_of("tsk_x1"), cause_of("tsk_x2"));
        // And the key cannot carry a word the keel table reads.
        for word in ["secret", "policy", "sealed", "format", "ceiling"] {
            assert!(
                !cause_of("tsk_x1").contains(word),
                "a person's key must hold no keel word: {word}"
            );
        }
    }

    #[test]
    fn an_empty_or_over_long_finding_is_refused_with_words() {
        assert!(finding_of("   \n ").unwrap_err().contains("your own words"));
        assert_eq!(finding_of("  the gate is slow  ").unwrap(), "the gate is slow");
        let long = "x".repeat(FINDING_MAX + 1);
        let why = finding_of(&long).unwrap_err();
        assert!(why.contains(&(FINDING_MAX + 1).to_string()), "{why}");
        assert!(why.contains("leave the report"), "{why}");
        // The bound itself is not refused, so the message is about over-long
        // findings and not about long ones.
        assert!(finding_of(&"x".repeat(FINDING_MAX)).is_ok());
    }

    #[test]
    fn a_title_is_the_given_one_or_the_findings_first_sentence() {
        assert_eq!(
            title_of("The gate re-runs clippy twice. It costs a minute.", None),
            "The gate re-runs clippy twice."
        );
        assert_eq!(
            title_of("The gate re-runs clippy twice.", Some("  clippy runs twice  ")),
            "clippy runs twice"
        );
        // A blank title is no title.
        assert_eq!(title_of("one sentence", Some("   ")), "one sentence");
        // No sentence end: the whole first line, and later lines are not it.
        assert_eq!(
            title_of("\n  the first line\nthe second line", None),
            "the first line"
        );
        // An abbreviation does not end a sentence, whatever follows it.
        assert_eq!(
            title_of("the e.g. case is wrong. and more", None),
            "the e.g. case is wrong."
        );
        assert_eq!(
            title_of("the U.S. zone is wrong. and more", None),
            "the U.S. zone is wrong."
        );
        // An initial is read as one too, rather than ending the sentence.
        assert_eq!(title_of("ask J. about it. later", None), "ask J. about it.");
        // And a one-word sentence still ends, so the rule is not "never".
        assert_eq!(title_of("Broken. Really.", None), "Broken.");
    }

    #[test]
    fn a_long_title_is_cut_on_a_word_with_an_ellipsis() {
        let words = "alpha ".repeat(60);
        let t = title_of(&words, None);
        assert!(t.chars().count() <= TITLE_MAX, "{} chars", t.chars().count());
        assert!(t.ends_with('…'), "{t}");
        assert!(t.starts_with("alpha alpha"), "{t}");
        // One unbroken word still fits the bound.
        let one = "z".repeat(400);
        let t = title_of(&one, None);
        assert_eq!(t.chars().count(), TITLE_MAX);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn a_findings_words_are_read_by_the_keel_table_as_a_mined_causes_are() {
        // The words name a class: it waits for the owner, as a mined item of
        // that class does.
        assert_eq!(
            keel_class("loosen the sealed sets", "the holdout set is wrong"),
            Some("sealed sets")
        );
        assert_eq!(keel_class("Secrets loader is slow", "it is"), Some("secrets"));
        // Upper case is read too, so a filer's capitals cannot dodge it.
        assert_eq!(keel_class("POLICY is wrong", "x"), Some("security"));
        // And an ordinary finding is unmarked, so the mark discriminates.
        assert_eq!(keel_class("the gate runs clippy twice", "it costs a minute"), None);
    }

    #[test]
    fn an_objective_says_a_person_filed_it_and_ends_with_the_call_to_make() {
        let o = objective_of("tsk_x1", "the gate runs clippy twice", "the owner");
        assert!(o.starts_with("A person filed this finding"), "{o}");
        assert!(o.contains("not one Theseus mined"), "{o}");
        assert!(o.contains("Filed by the owner."), "{o}");
        assert!(o.contains("the gate runs clippy twice"), "{o}");
        assert!(o.contains("self.propose_change { item: \"tsk_x1\""), "{o}");
    }
}
