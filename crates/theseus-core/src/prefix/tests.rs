//! The prefix library's pure parts (theseus-rr15): the fit, the reuse,
//! coarser and mint decision on planted fits and hit logs, the place classes,
//! a source's change, the cap, the plan's render, and the false-fit report.
//! The whole-core tests are `tests_prefix.rs`.

use std::sync::{Arc, Mutex};

use serde_json::json;

use super::fit::{self, Bands, Best, Choice, Odds, Prices};
use super::*;
use crate::ledger::LedgerRow;

const BANDS: Bands = Bands {
    act: 0.90,
    confirm: 0.60,
};
/// Sonnet 5.5's 5-minute write and its read, per million tokens.
const PRICES: Prices = Prices {
    write: 3.75,
    read: 0.30,
};

fn files(n: usize) -> String {
    format!("## Context file: harbour.md\n{}", "wave ".repeat(n))
}

fn guidance(name: &str, body: &str) -> String {
    format!("## Guidance: {name}\n{body}")
}

fn library() -> Library {
    Library(Mutex::new(Some(State::default())))
}

struct Ask {
    files: String,
    guidance: Vec<String>,
    class: &'static str,
    model: &'static str,
    max_bytes: u64,
    now_ms: u64,
}

impl Ask {
    fn new(files: String, guidance: Vec<String>) -> Ask {
        Ask {
            files,
            guidance,
            class: "private",
            model: "claude-sonnet-5-5",
            max_bytes: 10 * 1024 * 1024,
            now_ms: 1_790_000_000_000,
        }
    }

    fn pick(&self, lib: &Library) -> Picked {
        let sections = fit::sections_of(&self.files, &self.guidance);
        lib.pick(&Query {
            sections: &sections,
            lower: "tools+header+persona".into(),
            class: self.class,
            model: self.model,
            sketch: sketch::of(sections.iter().map(|(_, t)| *t)),
            prices: PRICES,
            bands: BANDS,
            candidates: 16,
            max_bytes: self.max_bytes,
            now_ms: self.now_ms,
        })
        .unwrap()
    }

    fn fresh_text(&self) -> String {
        fit::join(
            [self.files.as_str()]
                .into_iter()
                .chain(self.guidance.iter().map(String::as_str))
                .filter(|t| !t.is_empty()),
        )
    }
}

fn keys(p: &Picked) -> Vec<&str> {
    p.records.iter().map(|(k, _)| k.as_str()).collect()
}

/// A header fits when every section it holds is the fresh layer's, byte for
/// byte; its fit is the share of the fresh bytes it covers. A section changed
/// since, or one the fresh layer lacks (another project's), never fits.
#[test]
fn a_fit_covers_the_fresh_sections_and_a_changed_or_foreign_one_never_fits() {
    let f = files(300);
    let tides = guidance("tides", "Quote every level in metres.");
    let gauges = guidance("gauges", "Run the tests with: cargo test -p gauges");
    let secs = |g: &[String]| -> Vec<fit::Section> {
        fit::sections_of(&f, g)
            .into_iter()
            .map(|(s, _)| s)
            .collect()
    };
    let fresh = secs(std::slice::from_ref(&tides));
    let project = secs(&[]);
    let whole = fit::fit(&fresh, &fresh).unwrap();
    assert_eq!(whole, 1.0);
    let part = fit::fit(&fresh, &project).unwrap();
    let expect = f.len() as f64 / (f.len() + tides.len()) as f64;
    assert!((part - expect).abs() < 1e-9, "{part} vs {expect}");
    assert_eq!(
        fit::fit(&fresh, &secs(std::slice::from_ref(&gauges))),
        None,
        "surplus"
    );
    let edited: Vec<fit::Section> = fit::sections_of(&files(301), &[])
        .into_iter()
        .map(|(s, _)| s)
        .collect();
    assert_eq!(fit::fit(&fresh, &edited), None, "a changed source");
    let missing = fit::missing(&fresh, &project);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].name, "## Guidance: tides");
}

/// The decision on planted fits and hit logs: reuse at or above `confirm`;
/// below it, mint when the expected reads pay for rewriting the warm
/// coarser header, else the coarser; mint with nothing to reuse, or when the
/// coarser header is cold.
#[test]
fn reuse_at_confirm_and_below_it_mint_only_when_minting_pays() {
    let even = Odds::of(0, 0, 0);
    assert_eq!((even.p_reuse, even.reads), (0.5, 1.0));
    let warm = |fit: f64, tokens: u64| Best {
        fit,
        warm: true,
        tokens,
    };
    let d = |best: Option<Best>, fresh: u64, odds: Odds| {
        fit::decide(best, fresh, BANDS, odds, PRICES).0
    };
    assert_eq!(d(Some(warm(0.95, 20_000)), 21_000, even), Choice::Reuse);
    assert_eq!(d(Some(warm(0.60, 20_000)), 33_000, even), Choice::Reuse);
    // Below confirm, a 20,000-token warm project header and a 15,000-token
    // topic: 0.5 x 1 x 15,000 < 20,000, so the coarser one with the delta.
    assert_eq!(d(Some(warm(0.57, 20_000)), 35_000, even), Choice::Coarser);
    // A history of headers like it read again often: 0.8 x 3.5 x 15,000 > 20,000.
    let hot = Odds::of(8, 7, 27);
    assert!(hot.p_reuse > 0.75 && hot.reads > 3.0);
    assert_eq!(d(Some(warm(0.57, 20_000)), 35_000, hot), Choice::Mint);
    // The coarser one cold: nothing is lost by passing it by.
    let cold = Best {
        warm: false,
        ..warm(0.57, 20_000)
    };
    assert_eq!(d(Some(cold), 35_000, even), Choice::Mint);
    assert_eq!(d(None, 35_000, even), Choice::Mint);
    // The sums say why, in dollars.
    let (_, s) = fit::decide(Some(warm(0.57, 20_000)), 35_000, BANDS, even, PRICES);
    assert!((s.gain_usd - 0.5 * 15_000.0 * 3.45 / 1e6).abs() < 1e-9);
    assert!((s.premium_usd - 20_000.0 * 3.45 / 1e6).abs() < 1e-9);
}

/// A mint is refused when reuse doesn't pay, through the library: a warm
/// project header, and a topic too big for the fit, too small to mint for.
#[test]
fn a_mint_is_refused_when_reusing_the_coarser_header_pays() {
    let lib = library();
    let project = Ask::new(files(300), vec![]);
    let first = project.pick(&lib);
    assert_eq!(first.choice, Choice::Mint);
    lib.touch(
        &first.plan.used.header,
        project.model,
        300_000,
        project.now_ms,
    );
    // A topic of about 0.4 of the layer: below confirm.
    let topic = guidance("tides", &"metres ".repeat(150));
    let mut next = Ask::new(files(300), vec![topic.clone()]);
    next.now_ms += 60_000;
    let p = next.pick(&lib);
    assert_eq!(p.choice, Choice::Coarser, "{:?}", p.sums);
    assert!(p.fit.unwrap() < 0.60);
    assert_eq!(p.plan.used.header, first.plan.used.header);
    assert_eq!(&*p.plan.delta, topic.as_str());
    assert_eq!(keys(&p), [format!("{HITS}{}", first.plan.used.header)]);
    // The same, with the project header cold: the mint is free, so it mints.
    let cold = library();
    let _ = project.pick(&cold);
    let m = next.pick(&cold);
    assert_eq!(m.choice, Choice::Mint);
    assert_eq!(&*m.plan.header, next.fresh_text().as_str());
    assert!(m.plan.delta.is_empty());
}

/// theseus-i4y7l: a pick whose compilation the call does not use (routing moved the turn) is
/// given back, so the compile the call uses mints the header, with its record, instead of
/// reusing one that was never stored; a discarded reuse gives its hit back.
#[test]
fn a_discarded_pick_leaves_the_library_as_it_was() {
    let lib = library();
    let project = Ask::new(files(300), vec![]);
    let first = project.pick(&lib);
    assert_eq!(first.choice, Choice::Mint);
    lib.discard(&first);
    let again = project.pick(&lib);
    assert_eq!(
        again.choice,
        Choice::Mint,
        "no header of the dropped compile to reuse"
    );
    assert_eq!(
        keys(&again),
        [format!("{HEADER}{}", again.plan.used.header)]
    );
    // A reuse that is dropped gives its hit back; a kept one counts.
    let reads = |lib: &Library, digest: &str| {
        lib.0.lock().unwrap().as_ref().unwrap().entries[digest]
            .hits
            .reuses
    };
    let digest = &again.plan.used.header;
    let reuse = project.pick(&lib);
    assert_eq!(reuse.choice, Choice::Reuse);
    assert_eq!(reads(&lib, digest), 1);
    lib.discard(&reuse);
    assert_eq!(reads(&lib, digest), 0);
}

/// A second thread on the project reuses the project's header verbatim; its
/// topic goes after it; the same layer again reuses its own header whole.
#[test]
fn a_topic_reuses_the_project_header_and_the_same_layer_reuses_itself() {
    let lib = library();
    let project = Ask::new(files(300), vec![]);
    let minted = project.pick(&lib);
    assert_eq!(minted.choice, Choice::Mint);
    assert_eq!(
        keys(&minted),
        [format!("{HEADER}{}", minted.plan.used.header)]
    );
    let tides = guidance("tides", "Quote every level in metres.");
    let topic = Ask::new(files(300), vec![tides.clone()]);
    let p = topic.pick(&lib);
    assert_eq!(p.choice, Choice::Reuse);
    assert!(p.fit.unwrap() >= 0.90 && !p.below_act);
    assert_eq!(&*p.plan.header, files(300).as_str());
    assert_eq!(&*p.plan.delta, tides.as_str());
    assert_ne!(p.plan.used.header, p.plan.used.fresh);
    // The replay of that use is the same plan, from memory.
    assert_eq!(lib.replay(&p.plan.used).unwrap(), p.plan);
    let again = project.pick(&lib);
    assert_eq!(again.choice, Choice::Reuse);
    assert_eq!(again.plan.used.header, again.plan.used.fresh);
    assert_eq!(again.fit, Some(1.0));
}

/// A near-miss project (the same layout, another test command) is never
/// reused: its guidance would sit in the cached prefix beside this one's.
#[test]
fn a_near_miss_project_is_not_reused() {
    let lib = library();
    let gauges = Ask::new(
        files(300),
        vec![guidance(
            "project",
            "Run the tests with: cargo test -p gauges",
        )],
    );
    let a = gauges.pick(&lib);
    let tides = Ask::new(
        files(300),
        vec![guidance(
            "project",
            "Run the tests with: cargo test -p tides",
        )],
    );
    let b = tides.pick(&lib);
    assert_eq!((a.choice, b.choice), (Choice::Mint, Choice::Mint));
    assert_ne!(a.plan.header, b.plan.header);
    assert!(b.plan.header.contains("-p tides") && !b.plan.header.contains("-p gauges"));
}

/// What a prefix carries into a thread's request (theseus-wncdv): the layer
/// is the thread's own sections and nothing else. Each stream has guidance
/// that holds its own colliding facts (a port, a test command); across 90 picks
/// in a seeded order, whatever the library holds, the header and the delta
/// together hold the asking stream's facts and none of another's. A header holding
/// a section the fresh layer lacks (another stream's topic) is never the pick,
/// so the library may be warm for one stream and cold for the next with
/// no crossing. (The threadbench's A1 vs A1n difference was routing, not this:
/// no header, reused or minted, can hold a foreign section.)
#[test]
fn a_prefix_carries_only_the_asking_streams_own_sections() {
    let streams = [
        (
            "sec-fix",
            "ledgerweb staging port 8443, tests: make check-sec",
        ),
        (
            "pipeline",
            "datatidy preview port 8444, tests: make test-data",
        ),
        ("local-ops", "homesvc staging port 8453, tests: ./verify.sh"),
        (
            "pr-round",
            "tarnlib docs preview port 9443, tests: make test-pr",
        ),
    ];
    let lib = library();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for i in 0..90 {
        let me = (next() % 4) as usize;
        let mut sections = vec![guidance("project", "Work in the owner's workspace.")];
        // Each stream's own guidance, and now and then a sibling's topic of its own.
        sections.push(guidance(streams[me].0, streams[me].1));
        if next() % 3 == 0 {
            sections.push(guidance(
                &format!("{}-topic", streams[me].0),
                "Keep it short.",
            ));
        }
        let ask = Ask::new(files(300), sections);
        let p = ask.pick(&lib);
        let carried = fit::join(
            [&*p.plan.header, &*p.plan.delta]
                .into_iter()
                .filter(|t| !t.is_empty()),
        );
        assert!(
            carried.contains(streams[me].1),
            "pick {i} ({}) lost its own section",
            streams[me].0
        );
        for (j, (name, fact)) in streams.iter().enumerate() {
            assert!(
                j == me || !carried.contains(fact),
                "pick {i} for {} carries {name}'s section ({:?})",
                streams[me].0,
                p.choice
            );
        }
        // The two halves are exactly the fresh layer's sections.
        let mut have: Vec<&str> = carried.split("\n\n").collect();
        let fresh = ask.fresh_text();
        let mut want: Vec<&str> = fresh.split("\n\n").collect();
        have.sort_unstable();
        want.sort_unstable();
        assert_eq!(have, want, "pick {i}");
    }
}

/// A shared place reuses only a shared place's header, and a private place
/// only a private one's, the same bytes notwithstanding.
#[test]
fn shared_and_private_headers_never_cross() {
    let lib = library();
    let private = Ask::new(files(300), vec![]);
    let p = private.pick(&lib);
    let mut shared = Ask::new(files(300), vec![]);
    shared.class = "shared";
    let s = shared.pick(&lib);
    assert_eq!(
        s.choice,
        Choice::Mint,
        "a private header is no shared candidate"
    );
    assert_eq!(s.candidates, 0);
    let again = shared.pick(&lib);
    assert_eq!(again.choice, Choice::Reuse);
    assert_eq!(again.class, "shared");
    let g = lib.0.lock().unwrap();
    let classes: Vec<&str> = g
        .as_ref()
        .unwrap()
        .entries
        .values()
        .map(|e| e.header.class.as_str())
        .collect();
    assert_eq!(classes.len(), 1, "one record a digest: {classes:?}");
    drop(g);
    assert_eq!(p.plan.used.header, s.plan.used.header);
}

/// A source that changed is a new header under its own digest; the stale one
/// retires once it is cold, never while a model still reads it.
#[test]
fn a_changed_source_mints_a_new_header_and_the_stale_one_retires_once_cold() {
    let lib = library();
    let v1 = Ask::new(files(300), vec![]);
    let old = v1.pick(&lib);
    lib.touch(&old.plan.used.header, v1.model, 300_000, v1.now_ms);
    let mut v2 = Ask::new(files(301), vec![]);
    v2.now_ms += 60_000;
    let warm = v2.pick(&lib);
    assert_eq!(warm.choice, Choice::Mint);
    assert_eq!(
        keys(&warm),
        [format!("{HEADER}{}", warm.plan.used.header)],
        "warm: kept"
    );
    // Later, both cold: the next mint of the project retires the stale one.
    let lib2 = library();
    let _ = v1.pick(&lib2);
    let p = v2.pick(&lib2);
    assert_eq!(
        keys(&p),
        [
            format!("{HEADER}{}", p.plan.used.header),
            format!("{RETIRED}{}", old.plan.used.header)
        ]
    );
    assert_eq!(p.records[1].1["why"], "source_changed");
    assert_eq!(lib2.stats(v2.now_ms).headers, 1);
}

/// Past `max_bytes` the header read least recently retires, and never the
/// one just minted.
#[test]
fn past_the_cap_the_header_read_least_recently_retires() {
    let lib = library();
    let mut asks: Vec<Ask> = (0..3)
        .map(|i| {
            let mut a = Ask::new(files(300), vec![guidance(&format!("t{i}"), "x")]);
            a.max_bytes = 4_200;
            a.now_ms += i * 1_000;
            a
        })
        .collect();
    let first = asks[0].pick(&lib);
    let second = asks[1].pick(&lib);
    // The first is read again, so the second is the least recently read.
    asks[0].now_ms = asks[1].now_ms + 500;
    let read = asks[0].pick(&lib);
    assert_eq!(read.choice, Choice::Reuse);
    let third = asks[2].pick(&lib);
    assert!(lib.stats(0).bytes <= 4_200, "{}", lib.stats(0).bytes);
    let retired: Vec<&str> = keys(&third)
        .into_iter()
        .filter(|k| k.starts_with(RETIRED))
        .collect();
    assert_eq!(retired, [format!("{RETIRED}{}", second.plan.used.header)]);
    assert_eq!(third.records.last().unwrap().1["why"], "evicted");
    assert!(lib.replay(&first.plan.used).is_some());
    assert!(lib.replay(&second.plan.used).is_none());
}

/// The plan renders the header in layer 3 and the delta after the cached
/// prefix; a spec whose third layer is not the fresh one the plan stands
/// for is left as it is (its source changed: the compile recompiles).
#[test]
fn the_plan_puts_the_header_in_layer_three_and_the_delta_after_it() {
    let tides = guidance("tides", "Quote every level in metres.");
    let fresh = fit::join([files(300).as_str(), tides.as_str()]);
    let used = PrefixUse {
        header: sha(&files(300)),
        fresh: sha(&fresh),
    };
    let mut spec = crate::compiler::tests_ring::spec();
    spec.context_text = fresh;
    spec.session_text = "Directory: /work/tide-gauges".into();
    spec.prefix = Some(Arc::new(Plan {
        used: used.clone(),
        header: Arc::from(files(300).as_str()),
        delta: Arc::from(tides.as_str()),
    }));
    let s = applied(&spec);
    assert_eq!(s.context_text, files(300));
    assert_eq!(
        s.session_text,
        format!("{tides}\n\nDirectory: /work/tide-gauges")
    );
    assert_eq!(s.prefix_used.as_ref(), Some(&used));
    spec.context_text.push_str(" edited");
    assert!(matches!(applied(&spec), std::borrow::Cow::Borrowed(_)));
}

fn row(kind: &str, sid: &str, turn: &str, at: u64, data: serde_json::Value) -> LedgerRow {
    LedgerRow {
        at_unix_ms: at,
        kind: kind.into(),
        session_id: Some(sid.into()),
        turn_id: Some(turn.into()),
        data,
    }
}

/// The false-fit log: a reuse below `act` that the owner corrected, or asked
/// again after, is a labelled false fit with its score; one above `act`, or
/// one that went well, is not; a reuse whose first call read the cache saved
/// its header's write.
#[test]
fn a_reuse_below_act_that_went_wrong_is_a_labelled_false_fit() {
    let pick = |sid: &str, turn: &str, at: u64, choice: &str, fit: f64, below: bool| {
        row(
            "prefix.picked",
            sid,
            turn,
            at,
            json!({"choice": choice, "fit": fit, "below_act": below, "header": "h1",
                   "header_bytes": 40_000, "write_per_mtok": 3.75, "read_per_mtok": 0.30}),
        )
    };
    let picks = [
        pick("s1", "t1", 10, "mint", 0.0, false),
        pick("s2", "t2", 20, "reuse", 0.95, false),
        pick("s3", "t3", 30, "reuse", 0.70, true),
        pick("s4", "t4", 40, "reuse", 0.65, true),
        pick("s5", "t5", 50, "coarser", 0.50, true),
    ];
    let calls = [row(
        "provider.call",
        "s2",
        "t2",
        21,
        json!({"usage": {"cache_read_input_tokens": 12_000}}),
    )];
    let wrongs = [row("route.corrected", "s3", "t3b", 31, json!({}))];
    let nexts = [
        report::Next {
            session_id: "s3".into(),
            turn_id: "t3b".into(),
            re_ask: false,
        },
        report::Next {
            session_id: "s5".into(),
            turn_id: "t5b".into(),
            re_ask: true,
        },
    ];
    let r = report::build(&picks, &calls, &wrongs, &nexts);
    assert_eq!((r.picks, r.reuses, r.coarser, r.mints), (5, 3, 1, 1));
    assert_eq!(r.below_act, 3);
    assert_eq!(r.cache_hits, 1);
    assert_eq!(r.writes_saved_tokens, 10_000, "the header, under the read");
    assert!((r.writes_saved_usd - 10_000.0 * 3.45 / 1e6).abs() < 1e-9);
    let ff: Vec<(&str, &str, f64)> = r
        .false_fits
        .iter()
        .map(|f| (f.session_id.as_str(), f.why.as_str(), f.fit))
        .collect();
    assert_eq!(ff, [("s3", "correction", 0.70), ("s5", "re_ask", 0.50)]);
    assert!((r.reuse_rate - 0.8).abs() < 1e-9 && (r.mint_rate - 0.2).abs() < 1e-9);
    assert!(report::re_ask(
        "how do the tide gauges drift each week",
        "how do the tide gauges drift each week, again?"
    ));
    assert!(!report::re_ask(
        "how do the tide gauges drift",
        "thanks, all clear now"
    ));
}
