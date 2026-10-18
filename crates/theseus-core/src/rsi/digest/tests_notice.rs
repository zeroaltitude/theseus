//! The morning notice's text (theseus-pw1q.21.8), from planted ledger rows
//! of a night: a fix, a flow made, a tool retired, a card for the owner. The
//! golden is the notice; the rules are asserted beside it: no "in progress",
//! each line with its numbers and its undo, the totals the ledger's, no
//! sealed id or item text, bench-born labelled, nothing to say, no post.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::notice::{notice_text, Night};
use crate::ledger::LedgerRow;

const DAY0: u64 = 1_792_000_000_000;

fn row(kind: &str, at: u64, data: Value) -> LedgerRow {
    let mut r = LedgerRow::named(kind, None, None, data);
    r.at_unix_ms = at;
    r
}

/// The clock as the notice words it, in this machine's zone.
fn clock(ms: u64) -> String {
    let l = crate::wake::local(ms);
    format!("{:02}:{:02}", l.hour, l.minute)
}

fn night() -> Vec<LedgerRow> {
    let (t1, t2, t3, t4) = (DAY0, DAY0 + 600_000, DAY0 + 1_200_000, DAY0 + 1_800_000);
    vec![
        row("self.backlog.filed", t1, json!({"numbers": {"count": 1}})),
        row("self.backlog.filed", t1, json!({"numbers": {"count": 1}})),
        row("self.backlog.filed", t1, json!({"numbers": {"count": 1}})),
        row("self.backlog.filed", t1, json!({"numbers": {"count": 4}})),
        row("self.backlog.filed", t1, json!({"numbers": {"count": 2}})),
        row(
            "self.review.verdict",
            t1,
            json!({"branch": "self/2026-10-18-lsp-timeout", "head": "h1", "reviewer": "fable (claude-fable-5-1)",
                   "verdict": "accept", "planted_revert": "failed"}),
        ),
        row(
            "self.sealed.checked",
            t1,
            json!({"branch": "self/2026-10-18-lsp-timeout", "head": "h1", "outcome": "passed",
                   "before": 38, "after": 39, "total": 40,
                   "holds": [], "reported": ["opaque-item-ab12"], "broken": []}),
        ),
        row(
            "self.joined",
            t2,
            json!({"item": "tsk_fix", "branch": "self/2026-10-18-lsp-timeout", "head": "h1",
                   "merge": "4a1c9e0b77", "gate": "green",
                   "why": "an LSP call past its 20 s budget now fails fast with a reason",
                   "undo": "theseus self revert 4a1c9e0b77"}),
        ),
        row(
            "self.installed",
            t3,
            json!({"merge": "4a1c9e0b77", "version": "0.99", "branch": "self/2026-10-18-lsp-timeout"}),
        ),
        row(
            "self.mint.deployed",
            t3,
            json!({"name": "repo_land", "kind": "flow", "digest": "sha256:aa", "tool": "flow:repo_land",
                   "bench_born": false,
                   "what": "flow repo_land: test, merge, rerere, retest, push, handing back unresolved conflicts. Live in r:ab12.",
                   "numbers": {"runs": 11, "sessions": 5, "days": 6, "check_same": 4, "check_total": 4,
                               "tokens_pct": -62.0, "time_pct": -41.0, "decisions_removed": 7.0},
                   "undo": "theseus self revert flow:repo_land"}),
        ),
        row(
            "self.mint.retired",
            t4,
            json!({"name": "log_digest", "kind": "mcp", "digest": "sha256:bb", "tool": "ext-log_digest",
                   "bench_born": false, "rule": "unused", "why": "0 calls in 14 days",
                   "undo": "nothing was deleted: its frozen copy is kept at /state/extensions/log_digest/bb"}),
        ),
    ]
}

fn waits() -> Night {
    Night {
        until_ms: DAY0 + 2_000_000,
        spent_usd: 4.12,
        day_usd: 30.0,
        waits: vec![
            "• graduate.ext.net:fetchdocs asks for network (keel): theseus self backlog show 7"
                .into(),
        ],
        mint_items: BTreeSet::new(),
    }
}

/// The notice of a night with one fix, one flow made, one tool retired and
/// one owner card.
#[test]
fn the_notice_of_a_night_golden() {
    let text = notice_text(&night(), &waits()).expect("something landed");
    let l = crate::wake::local(DAY0 + 1_800_000);
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let golden = format!(
        "Self, night of {} {} ({}–{} · $4.12 of $30 · 0 halts)\n\
         \n\
         Fixed\n\
         • self/2026-10-18-lsp-timeout: an LSP call past its 20 s budget now fails fast with a reason.\n  \
         Gate green in L1 · review (fable) ✔, planted revert failed the new tests · sealed 38/40 → 39/40 · joined 4a1c, installed {}\n  \
         undo: theseus self revert 4a1c9e0b77\n\
         \n\
         Made\n\
         • flow repo_land: test, merge, rerere, retest, push, handing back unresolved conflicts. Live in r:ab12.\n  \
         From 11 by-hand runs (5 sessions, 6 days). Check half: 4/4 same result · -62% tokens · -41% time · 7 decisions removed\n  \
         undo: theseus self revert flow:repo_land\n\
         \n\
         Retired\n\
         • ext-log_digest: 0 calls in 14 days.\n  \
         undo: nothing was deleted: its frozen copy is kept at /state/extensions/log_digest/bb\n\
         \n\
         Waits for you\n\
         • graduate.ext.net:fetchdocs asks for network (keel): theseus self backlog show 7\n\
         \n\
         Filed 3 · updated 2 · dropped 0 — \"halt self\" stops everything",
        months[(l.month - 1) as usize],
        l.day,
        clock(DAY0 + 600_000),
        clock(DAY0 + 1_800_000),
        clock(DAY0 + 1_200_000),
    );
    assert_eq!(text, golden);
}

/// The rules, on the same night: nothing in progress, every bullet of the
/// first three sections is followed by an undo line, and the totals are the
/// ledger's (three filed fresh and two seen again, from five rows).
#[test]
fn every_line_carries_its_undo_and_nothing_is_in_progress() {
    let text = notice_text(&night(), &waits()).unwrap();
    assert!(!text.to_lowercase().contains("in progress"), "{text}");
    for section in ["Fixed", "Made", "Retired"] {
        let body = text.split(&format!("\n{section}\n")).nth(1).unwrap();
        let body = body.split("\n\n").next().unwrap();
        let bullets = body.lines().filter(|l| l.starts_with("• ")).count();
        let undos = body
            .lines()
            .filter(|l| l.trim_start().starts_with("undo: "))
            .count();
        assert_eq!(bullets, undos, "{section}: {body}");
        assert!(bullets > 0);
    }
    // A waiting line carries the command that shows it.
    assert!(text.contains("theseus self backlog show 7"));
    // The footer is the ledger's.
    assert!(text.ends_with("Filed 3 · updated 2 · dropped 0 — \"halt self\" stops everything"));
    // Nothing of a sealed set but its score line: no id reaches the text.
    assert!(!text.contains("opaque-item-ab12"), "{text}");
    assert!(text.contains("sealed 38/40 → 39/40"));
}

/// A Rust mint's join is told as made, not fixed; a bench-born tool says so;
/// a night of halts counts them.
#[test]
fn a_rust_mint_is_made_a_bench_born_tool_says_so_and_halts_are_counted() {
    let mut rows = night();
    rows.push(row(
        "self.mint.deployed",
        DAY0 + 1_300_000,
        json!({"name": "cargo_quiet", "kind": "flow", "digest": "sha256:cc", "tool": "flow:cargo_quiet",
               "bench_born": true, "what": "flow cargo_quiet: runs the tests quietly. Live in r:cd34.",
               "numbers": {"runs": 8, "sessions": 6, "days": 3},
               "undo": "theseus self revert flow:cargo_quiet"}),
    ));
    rows.push(row(
        "self.halted",
        DAY0 + 1_400_000,
        json!({"by": "the owner"}),
    ));
    let mut n = waits();
    n.mint_items.insert("tsk_fix".into());
    let text = notice_text(&rows, &n).unwrap();
    assert!(text.contains("• bench-born flow cargo_quiet"), "{text}");
    assert!(text.contains("1 halts"), "{text}");
    let (fixed, made) = (
        text.split("\nFixed\n")
            .nth(1)
            .map(|s| s.contains("lsp-timeout")),
        text.split("\nMade\n")
            .nth(1)
            .map(|s| s.contains("• rust self/2026-10-18-lsp-timeout")),
    );
    assert_eq!(
        fixed, None,
        "no Fixed section: its one join is a Rust mint's: {text}"
    );
    assert_eq!(made, Some(true), "{text}");
}

/// A night that landed nothing and holds nothing for the owner says nothing.
#[test]
fn nothing_to_say_is_no_notice() {
    let quiet = vec![row(
        "self.backlog.filed",
        DAY0,
        json!({"numbers": {"count": 1}}),
    )];
    assert_eq!(notice_text(&quiet, &Night::default()), None);
    assert_eq!(notice_text(&[], &Night::default()), None);
    // A card for the owner alone is something to say.
    let t = notice_text(&[], &waits()).unwrap();
    assert!(
        t.contains("Waits for you") && !t.contains("\nFixed\n"),
        "{t}"
    );
}
