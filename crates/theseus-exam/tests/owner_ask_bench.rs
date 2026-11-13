//! The `owner.ask` bench against the memory exam (card `theseus-mqret`, board
//! line 14). A MEASUREMENT, not a gate: it prints a table and asserts only
//! that the harness itself ran over the items it claims to have measured.
//!
//! **What is being measured, and why it needs a promotion.**
//! `theseus_core::owner_ask::answer` is a pure retrieval with citation over
//! two substrates and no others: the owner's live standing records of kind
//! `fact` and `preference`, and the card observer's open observations. The
//! memory exam (exam-v2) holds every item's past as SESSION NODES, and
//! exam-v2 declares no standing records and no observations at all. So
//! `owner_ask` can read nothing of the exam as it ships: its score on the
//! unmodified exam is zero for want of a substrate, not for want of recall,
//! and a bench that reported that zero would be measuring the fixture's
//! shape rather than the feature.
//!
//! To get a number that means something, this harness PROMOTES each item's
//! gold node -- the owner's own words, the very line a capture pipeline would
//! keep -- to a live standing record, and then asks the item's own task. That
//! is a CEILING: the right line is present, by construction, with no decay, no
//! miscapture and no competition from the store's other 1,550 nodes. A real
//! `owner.ask` cannot beat this number; it can only fall short of it.
//!
//! Two conditions:
//! - `isolated`: only the item's own gold line is held. A pure cover test of
//!   `MIN_COVER` against the task's wording.
//! - `pooled`: every measured item's gold line is held at once, so the right
//!   line must also outrank its siblings.
//!
//! Two metrics per cell:
//! - `hit`: the item's own gold line came back at all, and whether at rank 1.
//! - `check`: the exam's OWN check passes with the cited lines as the reply,
//!   which is the only metric comparable to the frozen per-family rates in
//!   `docs/benchmarks/2026-10-12-memory-exam-arms.md`.

use theseus_core::owner_ask::{self, Held, MAX_ITEMS, MIN_COVER};
use theseus_exam::check::Answer as Reply;
use theseus_exam::item::{Exam, Family, Item};
use theseus_protocol::standing::{StandingKind, StandingRecord, StandingScope};

/// The families this bench measures: `preference` is the proxy for the class
/// `owner.ask` would serve (the owner's own standing preferences), and `fact`
/// rides beside it as the contrast -- exam-v2's `fact` items are facts about
/// the SYSTEM (a port, a channel id), not about him.
const MEASURED: [Family; 2] = [Family::Preference, Family::Fact];

/// A synthetic base for `since_ms`. `owner_ask` reads a record's time only to
/// break a tie in the ranking, so the exact instant is immaterial; what
/// matters is that the order is total and stable.
const BASE_MS: u64 = 1_750_000_000_000;

/// The owner's own gold lines of an item, promoted to live standing records.
/// Only the operator's nodes are promoted: the agent's replies are not the
/// owner's words and a capture pipeline would not keep them as his facts.
fn promoted(item: &Item, kind: StandingKind, from: u64) -> Vec<StandingRecord> {
    let mut out = Vec::new();
    for (i, key) in item.gold.iter().enumerate() {
        let Some((_session, node)) = item.node(key) else {
            continue;
        };
        if !node.is_operator() {
            continue;
        }
        out.push(StandingRecord {
            id: format!("std_{}_{}", item.id.replace('-', "_"), i + 1),
            kind,
            text: node.text.clone(),
            scope: StandingScope::Everywhere,
            since_ms: from + (i as u64) * 1_000,
            until_ms: None,
            source: "capture:exam".into(),
            ends: None,
        });
    }
    out
}

/// A retrieval-with-citation surface's reply: the lines Theseus holds, each
/// named by the record to look up. Nothing here writes a sentence of its own,
/// which is the decision that governs this card.
fn rendered(a: &owner_ask::Answer) -> String {
    if a.items.is_empty() {
        return format!(
            "Theseus holds nothing about that (searched {} line(s)).",
            a.searched
        );
    }
    a.items
        .iter()
        .map(|c| {
            format!(
                "- {} [{}, {}{}]",
                c.text,
                c.citation,
                c.held_as,
                if c.unconfirmed { ", unconfirmed" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn kind_of(family: Family) -> StandingKind {
    match family {
        Family::Preference => StandingKind::Preference,
        _ => StandingKind::Fact,
    }
}

#[test]
fn the_owner_ask_bench() {
    let exam = Exam::load(None).expect("the built-in exam loads");
    println!("\n================ owner.ask bench (theseus-mqret) ================");
    println!("exam      : {} ({})", exam.file.version, exam.digest);
    println!("owner_ask : MIN_COVER = {MIN_COVER}, MAX_ITEMS = {MAX_ITEMS}");

    // The denominators, for every family, so the proxy's size is visible.
    println!("\n---- exam-v2 items by family (held in / held out) ----");
    for f in Family::ALL {
        let items: Vec<&Item> = exam
            .file
            .items
            .iter()
            .filter(|i| i.family == f)
            .collect();
        if items.is_empty() {
            continue;
        }
        let out = items.iter().filter(|i| i.held_out).count();
        println!(
            "{:<12} {:>2} items  ({} in / {} out)",
            f.as_str(),
            items.len(),
            items.len() - out,
            out
        );
    }

    // The substrate finding, asserted rather than asserted-about: exam-v2
    // declares no standing records and no observations anywhere.
    let declared_standing: usize = exam.file.items.iter().map(|i| i.standing.len()).sum();
    println!("\n---- substrate ----");
    println!(
        "standing records declared anywhere in {}: {}",
        exam.file.version, declared_standing
    );
    println!(
        "=> on the exam AS IT SHIPS, owner.ask reads {declared_standing} line(s): \
         every cell is a no-answer, for want of a substrate."
    );

    for family in MEASURED {
        let items: Vec<&Item> = exam
            .file
            .items
            .iter()
            .filter(|i| i.family == family)
            .collect();
        let kind = kind_of(family);

        // The pooled condition's whole set, built once.
        let mut pool: Vec<StandingRecord> = Vec::new();
        for (n, item) in items.iter().enumerate() {
            pool.extend(promoted(item, kind, BASE_MS + (n as u64) * 100_000));
        }

        for (label, pooled) in [("isolated", false), ("pooled", true)] {
            let mut hits = 0usize;
            let mut rank1 = 0usize;
            let mut checks = 0usize;
            let mut cells = 0usize;
            println!(
                "\n======== family {} / condition {} ({} records in play) ========",
                family.as_str(),
                label,
                if pooled { pool.len() } else { 1 }
            );
            for (n, item) in items.iter().enumerate() {
                let own = promoted(item, kind, BASE_MS + (n as u64) * 100_000);
                let standing: Vec<StandingRecord> = if pooled { pool.clone() } else { own.clone() };
                let ans = owner_ask::answer(&item.task, Held {
                    standing: &standing,
                    observed: &[],
                });
                let reply = rendered(&ans);
                let scored = Reply {
                    reply: reply.clone(),
                    calls: Vec::new(),
                    root: None,
                };
                let pass = exam
                    .checks
                    .get(&item.id)
                    .map(|c| c.passes(&scored))
                    .unwrap_or(false);
                let mine: Vec<&String> = own.iter().map(|r| &r.id).collect();
                let got_mine = ans
                    .items
                    .iter()
                    .any(|c| mine.iter().any(|id| c.citation == format!("standing:{id}")));
                let top_mine = ans
                    .items
                    .first()
                    .map(|c| mine.iter().any(|id| c.citation == format!("standing:{id}")))
                    .unwrap_or(false);

                cells += 1;
                hits += usize::from(got_mine);
                rank1 += usize::from(top_mine);
                checks += usize::from(pass);

                println!("\n-- {} ({}) {}", item.id, item.family.as_str(), if item.held_out { "[held out]" } else { "[held in]" });
                println!("   task       : {}", item.task);
                println!("   asked words: {:?}", ans.words);
                println!("   searched   : {}", ans.searched);
                for (r, c) in ans.items.iter().enumerate() {
                    println!(
                        "   item #{}    : covers {:.2}  {}  {:?}",
                        r + 1,
                        c.covers,
                        c.citation,
                        c.text
                    );
                }
                if ans.items.is_empty() {
                    println!("   item #1    : (none: no line reached MIN_COVER)");
                }
                println!("   gold back  : {got_mine} (at rank 1: {top_mine})");
                println!("   exam check : {}", if pass { "PASS" } else { "FAIL" });
                if let Some(c) = exam.checks.get(&item.id) {
                    for lr in c.run(&scored) {
                        println!("     [{}] {}", if lr.pass { "ok" } else { "no" }, lr.line);
                    }
                }
            }
            println!(
                "\n**** {} / {}: gold returned {}/{}, at rank 1 {}/{}, EXAM CHECK PASSED {}/{} ****",
                family.as_str(),
                label,
                hits,
                cells,
                rank1,
                cells,
                checks,
                cells
            );
            assert_eq!(cells, items.len(), "every item of the family was measured");
        }
    }
    println!("\n================ end of bench ================\n");
}
