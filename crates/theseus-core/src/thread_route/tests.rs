use super::*;
use theseus_protocol::threads::ThreadState;

fn th(slug: &str, title: &str, ws: WorkingSet) -> ThreadInfo {
    ThreadInfo {
        slug: slug.into(),
        title: title.into(),
        goal: String::new(),
        topics: vec![],
        working_set: ws,
        card: vec![],
        persona: None,
        state: ThreadState::Active,
        sealed: None,
        seal_pending: None,
        surfaces: vec![],
        parent: None,
        merged_into: None,
        split_from: None,
        links: vec![],
        session_id: None,
        last_active_ms: 1,
        created_at_ms: 1,
    }
}

fn ws(
    refs: &[&str],
    repos: &[&str],
    branches: &[&str],
    files: &[&str],
    hosts: &[&str],
    roots: &[&str],
) -> WorkingSet {
    let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect();
    WorkingSet {
        roots: v(roots),
        repos: v(repos),
        branches: v(branches),
        refs: v(refs),
        files: v(files),
        hosts: v(hosts),
    }
}

fn pr_round() -> ThreadInfo {
    th(
        "pr-round",
        "pr round",
        ws(
            &["#412"],
            &["lanterns/tidewatch"],
            &["lane/pr-round"],
            &["src/lighthouse.rs"],
            &[],
            &["/work/tidewatch"],
        ),
    )
}

fn security_fix() -> ThreadInfo {
    th(
        "security-fix",
        "security fix",
        ws(
            &["#977", "tidewatch-9x2k"],
            &["lanterns/tidewatch"],
            &["fix/token-leak"],
            &["src/vault.rs"],
            &["login.example.net"],
            &["/work/vault"],
        ),
    )
}

fn refs(v: &[ThreadInfo]) -> Vec<&ThreadInfo> {
    v.iter().collect()
}

// ---- addressing

/// A leading `#slug`, `/t slug`, and "in the <title> thread:" name a live
/// thread and are taken off; an unknown name is no address.
#[test]
fn explicit_words_name_a_live_thread_and_come_off_the_message() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let ex = |s: &str| explicit(s, &t);
    assert_eq!(
        ex("#pr-round check the lint run"),
        Some(Address::Explicit {
            slug: "pr-round".into(),
            text: "check the lint run".into()
        })
    );
    assert_eq!(
        ex("/t security-fix: rotate the key"),
        Some(Address::Explicit {
            slug: "security-fix".into(),
            text: "rotate the key".into()
        })
    );
    assert_eq!(
        ex("In the security fix thread: rotate the key"),
        Some(Address::Explicit {
            slug: "security-fix".into(),
            text: "rotate the key".into()
        })
    );
    assert_eq!(
        ex("in the PR round thread, rerun it"),
        Some(Address::Explicit {
            slug: "pr-round".into(),
            text: "rerun it".into()
        })
    );
    assert_eq!(ex("#412 is failing"), None, "a number is no slug");
    assert_eq!(ex("#nothing-here do it"), None);
    assert_eq!(ex("in the garden thread: water it"), None);
    assert_eq!(
        ex("please /t pr-round later"),
        None,
        "only a leading word addresses"
    );
}

/// The place addresses only when one thread alone lists it; the first
/// match wins: place, then explicit, then reply.
#[test]
fn addressing_goes_place_then_explicit_then_reply() {
    let mut a = pr_round();
    a.surfaces = vec!["discord:channel:7".into()];
    let mut b = security_fix();
    b.surfaces = vec!["discord:channel:8".into(), "cli".into()];
    let all = [a, b];
    let t = refs(&all);
    let at = |text: &'static str, place: Option<&'static str>, reply: Option<&'static str>| {
        address(
            &Addressing {
                text,
                place,
                reply_thread: reply,
            },
            &t,
        )
    };
    assert_eq!(
        at("hello", Some("discord:channel:7"), None),
        Some(Address::Place("pr-round".into()))
    );
    assert_eq!(
        at("hello", None, None),
        Some(Address::Place("security-fix".into())),
        "cli is a place"
    );
    assert_eq!(
        at(
            "#security-fix hello",
            Some("discord:channel:7"),
            Some("security-fix")
        ),
        Some(Address::Place("pr-round".into())),
        "the place comes first"
    );
    assert_eq!(
        at(
            "#security-fix hello",
            Some("discord:dm:1"),
            Some("pr-round")
        ),
        Some(Address::Explicit {
            slug: "security-fix".into(),
            text: "hello".into()
        }),
        "explicit before reply"
    );
    assert_eq!(
        at("hello", Some("discord:dm:1"), Some("pr-round")),
        Some(Address::Reply("pr-round".into()))
    );
    assert_eq!(
        at("hello", Some("discord:dm:1"), Some("gone")),
        None,
        "a reply to a thread no longer live"
    );
    // Two threads listing a place is a pair of candidates.
    let mut c = security_fix();
    c.surfaces = vec!["discord:channel:7".into()];
    let both = [
        {
            let mut p = pr_round();
            p.surfaces = vec!["discord:channel:7".into()];
            p
        },
        c,
    ];
    assert_eq!(by_place(Some("discord:channel:7"), &refs(&both)), None);
}

/// The owner's "that was for ..." names a live thread or home, and only
/// that: any other message is a message.
#[test]
fn the_owners_words_that_a_message_was_placed_wrong_are_read_narrowly() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    for (said, want) in [
        ("that was for the security fix", Some("security-fix")),
        (
            "No, that was for the security fix thread.",
            Some("security-fix"),
        ),
        ("Oops, that was for pr-round!", Some("pr-round")),
        ("that was about the PR round", Some("pr-round")),
        ("that should have gone to home", Some("home")),
        ("that was for the garden", None),
        ("that was for the security fix, and rotate the key", None),
        ("was that for the security fix?", None),
        ("rotate the key", None),
    ] {
        assert_eq!(moved_words(said, &t).as_deref(), want, "{said:?}");
    }
}

// ---- entities

#[test]
fn a_message_names_a_threads_entities_as_whole_tokens() {
    let w = pr_round().working_set;
    let hit = |s: &str| {
        entity_hits(s, &w)
            .into_iter()
            .map(|(_, e)| e)
            .collect::<Vec<_>>()
    };
    assert_eq!(hit("what is failing in PR 412?"), ["#412"]);
    assert_eq!(hit("look at pull/412 again"), ["#412"]);
    assert_eq!(hit("issue #412 again"), ["#412"]);
    assert!(
        hit("we have 412 users").is_empty(),
        "a bare number is no PR"
    );
    assert!(hit("see #4120").is_empty());
    assert_eq!(
        hit("open src/lighthouse.rs and read it"),
        ["src/lighthouse.rs"]
    );
    assert_eq!(
        hit("the lighthouse.rs file"),
        ["src/lighthouse.rs"],
        "a basename counts"
    );
    assert_eq!(hit("push lane/pr-round"), ["lane/pr-round"]);
    assert_eq!(
        hit("tidewatch builds slowly"),
        ["lanterns/tidewatch", "/work/tidewatch"],
        "a repo's own name, and a root's last part"
    );
    assert!(
        hit("the main branch builds").is_empty(),
        "generic words name nothing"
    );
    let s = security_fix().working_set;
    assert_eq!(
        entity_hits("is login.example.net down", &s)[0].1,
        "login.example.net"
    );
    assert_eq!(
        entity_hits("what about tidewatch-9x2k", &s)[0].1,
        "tidewatch-9x2k"
    );
}

#[test]
fn a_unique_hit_is_strong_and_a_shared_one_is_not() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let p = pre_route("what is failing in PR 412?", &t, None, 1_000, 600_000);
    assert!(p[0].score >= ACT, "{:?}", p[0]);
    assert_eq!(p[1].score, 0.0);
    assert!(decisive(&p));
    // The shared repo name hits both: neither is sure.
    let p = pre_route("tidewatch builds slowly", &t, None, 1_000, 600_000);
    assert!(
        p[0].score < CONFIRM + 0.1 && p[1].score < CONFIRM + 0.1,
        "{p:?}"
    );
    assert!(!decisive(&p));
    // No entity, no history: nothing.
    let p = pre_route("what time is it", &t, None, 1_000, 600_000);
    assert!(p.iter().all(|x| x.score == 0.0));
}

// ---- stickiness and the chain

#[test]
fn stickiness_is_strong_for_a_minute_and_weak_after_an_hour() {
    let hl = 600_000;
    assert!(sticky(0, hl) > 0.59);
    assert!(sticky(60_000, hl) > 0.55, "a minute");
    assert!(
        (sticky(600_000, hl) - 0.3).abs() < 1e-9,
        "one half-life halves it"
    );
    assert!(sticky(3_600_000, hl) < 0.01, "an hour");
    assert!(chain(0, hl) > 0.69 && chain(150_000, hl) < 0.36 && chain(3_600_000, hl) < 0.001);
    // Together they never reach the act bar: a prior is not a decision.
    assert!(noisy_or(&[sticky(0, hl), chain(0, hl)]) < ACT);
}

#[test]
fn right_after_a_threads_reply_its_follow_up_stays_and_a_new_entity_breaks_it() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let last = Last {
        slug: Some("security-fix".into()),
        at_ms: 100_000,
        replied_ms: Some(110_000),
    };
    let now = 130_000;
    let p = pre_route("and the tests too", &t, Some(&last), now, 600_000);
    assert!(p[1].score >= CONFIRM && p[1].score < ACT, "{:?}", p[1]);
    assert_eq!(p[0].score, 0.0);
    assert!(
        !decisive(&p),
        "a follow-up is a prior, Jev tells a fragment"
    );
    // A message naming the other thread's PR goes there, and the last
    // thread's stickiness and chain fall away.
    let p = pre_route("and PR 412 too", &t, Some(&last), now, 600_000);
    assert!(p[0].score >= ACT && p[1].score < 0.35, "{p:?}");
    assert_eq!(p[1].chain, 0.0);
    // An hour later the thread is not the place's any more.
    let late = Last {
        slug: Some("security-fix".into()),
        at_ms: 100_000,
        replied_ms: Some(110_000),
    };
    let p = pre_route("and the tests too", &t, Some(&late), 3_800_000, 600_000);
    assert!(p.iter().all(|x| x.score < GUESS), "{p:?}");
}

/// A title the message names takes the replier's chain away as an entity
/// does (theseus-7nx13, tail4-route F4): right after thread A's reply, a
/// message naming thread B by its title no longer scores A's chain 0.7
/// against B's 0.35. A title naming the replier itself leaves its chain.
#[test]
fn a_title_named_right_after_a_reply_takes_the_repliers_chain() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let last = Last {
        slug: Some("security-fix".into()),
        at_ms: 100_000,
        replied_ms: Some(110_000),
    };
    let now = 130_000;
    let p = pre_route("and the pr round too", &t, Some(&last), now, 600_000);
    assert_eq!(p[0].hits, ["pr round"], "{p:?}");
    assert_eq!(p[1].chain, 0.0, "{p:?}");
    assert!(p[0].score > p[1].score, "{p:?}");
    // Half its stickiness goes too, as when another thread's entity is named.
    let plain = pre_route("and the tests too", &t, Some(&last), now, 600_000);
    assert_eq!(p[1].sticky, plain[1].sticky * 0.5, "{p:?}");
    // The replier named by its own title keeps its chain and stickiness.
    let own = pre_route("and the security fix too", &t, Some(&last), now, 600_000);
    assert_eq!(own[1].hits, ["security fix"], "{own:?}");
    assert_eq!(own[1].chain, plain[1].chain, "{own:?}");
    assert_eq!(own[1].sticky, plain[1].sticky, "{own:?}");
}

/// The directory rule leaves a thread its title (theseus-7nx13): a title
/// said whole names the thread wherever the turn runs, so an apart thread
/// the message names keeps that evidence and takes the others' chain.
#[test]
fn an_apart_thread_keeps_its_named_title() {
    let home = Path::new("/h");
    let dev = th(
        "dev",
        "lighthouse dev",
        ws(&["#412"], &[], &[], &[], &[], &["~/projects/lighthouse"]),
    );
    let ops = th("ops", "ops rota", ws(&["#977"], &[], &[], &[], &[], &[]));
    let t = [&dev, &ops];
    let last = Last {
        slug: Some("ops".into()),
        at_ms: 1_000,
        replied_ms: Some(1_000),
    };
    let text = "what is left in the lighthouse dev thread";
    let mut pre = pre_route(text, &t, Some(&last), 1_000, 600_000);
    let base = |t: &ThreadInfo| (t.slug == "dev").then_some("/h/projects/lighthouse");
    apart_by_dir(
        &mut pre,
        &t,
        text,
        Some("~/projects/walkthrough"),
        base,
        Some(home),
    );
    assert_eq!(pre[0].hits, ["lighthouse dev"], "{pre:?}");
    assert!(pre[0].score > 0.0, "{pre:?}");
    assert_eq!(pre[1].chain, 0.0, "{pre:?}");
    assert!(pre[0].score > pre[1].score, "{pre:?}");
}

/// The debug trace names a thread whose evidence the rule dropped: a thread
/// named only by its title lost nothing; one whose file hit went did.
#[test]
fn a_title_only_apart_thread_is_not_dropped() {
    let home = Path::new("/h");
    let dev = th(
        "dev",
        "lighthouse dev",
        ws(
            &["#412"],
            &[],
            &[],
            &["slow.sh"],
            &[],
            &["~/projects/harbor"],
        ),
    );
    let ops = th("ops", "ops rota", ws(&["#977"], &[], &[], &[], &[], &[]));
    let t = [&dev, &ops];
    let base = |t: &ThreadInfo| (t.slug == "dev").then_some("/h/projects/harbor");
    let run = |text: &str| {
        let mut pre = pre_route(text, &t, None, 1_000, 600_000);
        apart_by_dir(
            &mut pre,
            &t,
            text,
            Some("~/projects/walkthrough"),
            base,
            Some(home),
        )
    };
    let titled = run("what is left in the lighthouse dev thread");
    assert!(titled.is_empty(), "{titled:?}");
    let filed = run("why is slow.sh failing");
    assert_eq!(filed, ["dev"], "{filed:?}");
}

// ---- fusion and the decision table

fn tags(d: &Decision) -> (RouteHow, Option<&str>, bool) {
    match d {
        Decision::Route(t) => (t.how, t.slug.as_deref(), t.movable),
        other => panic!("not a route: {other:?}"),
    }
}

fn pre_with(scores: &[(&str, f64)]) -> Vec<Pre> {
    scores
        .iter()
        .map(|(s, p)| Pre {
            slug: (*s).into(),
            title: s.replace('-', " "),
            entity: *p,
            sticky: 0.0,
            chain: 0.0,
            score: *p,
            hits: vec![],
            learned: false,
            standing: (0.0, 0.0),
        })
        .collect()
}

/// The rules as they were before theseus-eqp1v, and as they ship.
const OFF: Rules = Rules {
    sticky_alone_asks: false,
    new_entities: 0,
    ask_every_thread: false,
    guess_unsure: false,
};
const ON: Rules = Rules {
    sticky_alone_asks: true,
    new_entities: 0,
    ask_every_thread: true,
    guess_unsure: false,
};

/// `decide` under today's rules, before theseus-eqp1v.
fn today(
    t: &[&ThreadInfo],
    pre: &[Pre],
    jev: Option<&Jev>,
    voice: Voice,
    new_title: Option<&str>,
) -> Decision {
    decide(t, pre, jev, voice, new_title, OFF)
}

/// Every band of the table, with Jev answering.
#[test]
fn the_decision_table_by_band() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let jev = |s: &str, p: f64| Jev {
        pick: Pick::Thread(s.into()),
        p,
    };
    // Jev sure, the router silent: quiet.
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.0), ("security-fix", 0.0)]),
        Some(&jev("pr-round", 0.95)),
        Voice::Jev,
        None,
    );
    assert_eq!(tags(&d), (RouteHow::Quiet, Some("pr-round"), false));
    // Jev likely: a visible tag and a move handle.
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.0), ("security-fix", 0.0)]),
        Some(&jev("security-fix", 0.75)),
        Voice::Jev,
        None,
    );
    assert_eq!(tags(&d), (RouteHow::Visible, Some("security-fix"), true));
    // Jev unsure of one thread, no other in contention: home, movable,
    // never a question (theseus-e31an).
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.0), ("security-fix", 0.0)]),
        Some(&jev("security-fix", 0.45)),
        Voice::Jev,
        None,
    );
    assert_eq!(tags(&d), (RouteHow::Home, None, true));
    // Two threads within 0.15 of each other: ask, whatever the top.
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.80), ("security-fix", 0.0)]),
        Some(&jev("security-fix", 0.70)),
        Voice::Jev,
        None,
    );
    assert!(matches!(d, Decision::Ask { .. }), "{d:?}");
    // Jev and the router agree: the evidence adds (Jev's 0.85 alone is
    // only likely).
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.0), ("security-fix", 0.0)]),
        Some(&jev("pr-round", 0.85)),
        Voice::Jev,
        None,
    );
    assert_eq!(tags(&d), (RouteHow::Visible, Some("pr-round"), true));
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.50), ("security-fix", 0.0)]),
        Some(&jev("pr-round", 0.85)),
        Voice::Jev,
        None,
    );
    assert_eq!(tags(&d), (RouteHow::Quiet, Some("pr-round"), false));
    // Jev overrules a sticky router when it is sure.
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.80), ("security-fix", 0.0)]),
        Some(&jev("security-fix", 0.96)),
        Voice::Jev,
        None,
    );
    assert_eq!(tags(&d), (RouteHow::Quiet, Some("security-fix"), false));
    // Home, sure: no thread, no tag. Home, likely: asks nothing but offers a move.
    let home = |p: f64| Jev {
        pick: Pick::Home,
        p,
    };
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.2), ("security-fix", 0.0)]),
        Some(&home(0.95)),
        Voice::Jev,
        None,
    );
    assert_eq!(tags(&d), (RouteHow::Home, None, false));
    // A new thread at the confirm bar is offered; below it, asked.
    let new = |p: f64| Jev { pick: Pick::New, p };
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.1), ("security-fix", 0.0)]),
        Some(&new(0.7)),
        Voice::Jev,
        Some("tide gauge"),
    );
    assert!(matches!(d, Decision::OfferNew { .. }), "{d:?}");
    let d = today(
        &t,
        &pre_with(&[("pr-round", 0.1), ("security-fix", 0.0)]),
        Some(&new(0.5)),
        Voice::Jev,
        Some("tide gauge"),
    );
    let Decision::Ask { options } = &d else {
        panic!("{d:?}")
    };
    assert!(options
        .iter()
        .any(|o| o.option.key == "new" && o.option.label.contains("tide gauge")));
}

/// The rule changes no band of the table (theseus-eqp1v): with
/// `sticky_alone_asks` on, each of its rows decides as it does off, since
/// none of them stands on stickiness alone.
#[test]
fn the_decision_table_is_the_same_with_the_sticky_rule_on() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let j = |pick: Pick, p: f64| Jev { pick, p };
    let pr = |s: &str| Pick::Thread(s.into());
    let rows = [
        (0.0, j(pr("pr-round"), 0.95)),
        (0.0, j(pr("security-fix"), 0.75)),
        (0.0, j(pr("security-fix"), 0.45)),
        (0.80, j(pr("security-fix"), 0.70)),
        (0.0, j(pr("pr-round"), 0.85)),
        (0.50, j(pr("pr-round"), 0.85)),
        (0.80, j(pr("security-fix"), 0.96)),
        (0.2, j(Pick::Home, 0.95)),
        (0.1, j(Pick::New, 0.7)),
        (0.1, j(Pick::New, 0.5)),
    ];
    for (score, jev) in rows {
        let pre = pre_with(&[("pr-round", score), ("security-fix", 0.0)]);
        let at = |r| decide(&t, &pre, Some(&jev), Voice::Jev, Some("tide gauge"), r);
        assert_eq!(at(OFF), at(ON), "{score} {jev:?}");
    }
}

/// A lone thread that leads home only closely keeps its lead
/// (theseus-m7fwf): theseus-e31an rightly stopped the ask when no two
/// threads contend, but then handed the turn to home unconditionally, so
/// a thread at the confirm bar lost to the choice it had beaten. Under
/// the bar nothing changes: home, unasked.
#[test]
fn a_lone_thread_that_leads_home_closely_keeps_its_lead() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    // Fused 0.94 x 0.85 = 0.799 against home's 0.70: a 0.099 gap, inside
    // CLOSE, so the table falls to the ask arm -- one thread contending.
    let at = |thread: f64, home: f64| {
        let jev = Jev {
            pick: Pick::Home,
            p: home,
        };
        let pre = pre_with(&[("pr-round", thread), ("security-fix", 0.0)]);
        let d = today(&t, &pre, Some(&jev), Voice::Jev, None);
        let (how, slug, movable) = tags(&d);
        (how, slug.map(str::to_string), movable)
    };
    let pr = || Some("pr-round".to_string());
    assert_eq!(at(0.94, 0.70), (RouteHow::Visible, pr(), true));
    // The plant: under the confirm bar the lead is not one, and the turn
    // still goes home without a question (theseus-e31an).
    assert_eq!(at(0.60, 0.45), (RouteHow::Home, None, true));
}

/// Without Jev an ask offers the new thread the message's own entities
/// raise (theseus-m7fwf). No Jev means no `New` in the ranking, so the
/// entity bar -- two concrete entities no live thread holds, the bar
/// [`lone_offer`] uses -- is the only evidence of a new stream there is.
/// Two threads must still contend: it adds an option to an ask that
/// already happens, it never makes one.
#[test]
fn an_ask_without_jev_offers_the_new_thread_the_entities_raise() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let close = pre_with(&[("pr-round", 0.50), ("security-fix", 0.45)]);
    let at = |pre: &[Pre], entities: usize| {
        let rules = Rules {
            new_entities: entities,
            ..OFF
        };
        decide(&t, pre, None, Voice::Alone, Some("tide gauge"), rules)
    };
    let keys = |d: &Decision| match d {
        Decision::Ask { options } => options
            .iter()
            .map(|o| o.option.key.clone())
            .collect::<Vec<_>>(),
        other => panic!("not an ask: {other:?}"),
    };
    let d = at(&close, 2);
    assert_eq!(keys(&d), ["1", "2", "new", "home"], "{d:?}");
    assert!(
        format!("{d:?}").contains("New thread \u{ab}tide gauge\u{bb}"),
        "{d:?}"
    );
    // The plants. One entity is no new stream: the same ask, without it.
    assert_eq!(keys(&at(&close, 1)), ["1", "2", "home"]);
    // And the entities never turn a clear guess into a question.
    let lone = pre_with(&[("pr-round", 0.50), ("security-fix", 0.0)]);
    let d = at(&lone, 2);
    assert_eq!(tags(&d), (RouteHow::Guess, Some("pr-round"), true), "{d:?}");
}

/// The threadbench's msg-5 shape (theseus-eqp1v): a new stream's first
/// message scores the place's last thread 0.88 from stickiness and the
/// reply chain alone. Jev picks another thread, or a new one: the sticky
/// thread no longer routes `visible` (0.88 x 0.85 = 0.748), the message
/// is asked about, and two entities no thread holds offer a new thread.
#[test]
fn stickiness_alone_does_not_overrule_jev() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let text = "homesvc maintenance: branch chore/bump-1-8, ticket OPS-4214, bump the base image";
    let last = Last {
        slug: Some("pr-round".into()),
        at_ms: 1_000,
        replied_ms: Some(1_000),
    };
    let pre = pre_route(text, &t, Some(&last), 1_000, 600_000);
    assert!(
        (pre[0].score - 0.88).abs() < 1e-9 && pre[0].entity == 0.0,
        "{pre:?}"
    );
    let rules = Rules {
        new_entities: new_entities(text, &t),
        ..ON
    };
    assert_eq!(rules.new_entities, 2, "a branch and a ticket");
    let keys = |d: &Decision| match d {
        Decision::Ask { options } => options
            .iter()
            .map(|o| o.option.key.clone())
            .collect::<Vec<_>>(),
        other => panic!("not an ask: {other:?}"),
    };
    let other = Jev {
        pick: Pick::Thread("security-fix".into()),
        p: 0.3,
    };
    let d = decide(&t, &pre, Some(&other), Voice::Jev, Some("homesvc"), rules);
    assert_eq!(keys(&d), ["1", "2", "new", "home"]);
    let quiet = Rules {
        new_entities: 0,
        ..rules
    };
    let d = decide(&t, &pre, Some(&other), Voice::Jev, Some("homesvc"), quiet);
    assert_eq!(keys(&d), ["1", "2", "home"], "no new entity, no new thread");
    let new = Jev {
        pick: Pick::New,
        p: 0.52,
    };
    let d = decide(&t, &pre, Some(&new), Voice::Jev, Some("homesvc"), quiet);
    assert!(keys(&d).contains(&"new".to_string()), "{d:?}");
    // Off: today's table routes it to the sticky thread with no question.
    let d = decide(&t, &pre, Some(&other), Voice::Jev, Some("homesvc"), OFF);
    assert_eq!(tags(&d), (RouteHow::Visible, Some("pr-round"), true));
    // Jev new at the confirm bar is still the offer, on or off.
    let sure = Jev {
        pick: Pick::New,
        p: 0.65,
    };
    let d = decide(&t, &pre, Some(&sure), Voice::Jev, Some("homesvc"), rules);
    assert!(matches!(d, Decision::OfferNew { .. }), "{d:?}");
}

/// The rule holds back stickiness alone, nothing else: a sticky thread
/// the message names an entity of, one a learned correction lifted, one
/// Jev agrees with, and a router alone all decide as before.
#[test]
fn evidence_beyond_stickiness_still_routes() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let last = Last {
        slug: Some("pr-round".into()),
        at_ms: 1_000,
        replied_ms: None,
    };
    let pre = pre_route(
        "and the formatter on lighthouse.rs",
        &t,
        Some(&last),
        1_000,
        600_000,
    );
    assert!(pre[0].entity > 0.0, "{pre:?}");
    let other = Jev {
        pick: Pick::Thread("security-fix".into()),
        p: 0.3,
    };
    let d = decide(&t, &pre, Some(&other), Voice::Jev, None, ON);
    assert_eq!(tags(&d), (RouteHow::Visible, Some("pr-round"), true));
    let mut learned = pre_route("and the formatter too", &t, Some(&last), 1_000, 600_000);
    learned[0].score = LEARNED;
    learned[0].learned = true;
    let d = decide(&t, &learned, Some(&other), Voice::Jev, None, ON);
    assert_eq!(tags(&d), (RouteHow::Visible, Some("pr-round"), true));
    let sticky = pre_route("and the formatter too", &t, Some(&last), 1_000, 600_000);
    let agree = Jev {
        pick: Pick::Thread("pr-round".into()),
        p: 0.3,
    };
    let d = decide(&t, &sticky, Some(&agree), Voice::Jev, None, ON);
    assert_eq!(tags(&d).1, Some("pr-round"));
    let d = decide(&t, &sticky, None, Voice::Alone, None, ON);
    assert_eq!(tags(&d), (RouteHow::Guess, Some("pr-round"), true));
}

/// Without Jev (a timeout or none), the router's pick stands as a guess,
/// and two close threads are asked.
#[test]
fn without_jev_the_routers_pick_stands_as_a_guess() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let d = decide(
        &t,
        &pre_with(&[("pr-round", 0.0), ("security-fix", 0.70)]),
        None,
        Voice::Alone,
        None,
        OFF,
    );
    assert_eq!(tags(&d), (RouteHow::Guess, Some("security-fix"), true));
    let d = decide(
        &t,
        &pre_with(&[("pr-round", 0.5), ("security-fix", 0.55)]),
        None,
        Voice::Alone,
        None,
        OFF,
    );
    assert!(matches!(d, Decision::Ask { .. }), "{d:?}");
    let d = decide(
        &t,
        &pre_with(&[("pr-round", 0.05), ("security-fix", 0.0)]),
        None,
        Voice::Alone,
        None,
        OFF,
    );
    assert_eq!(
        tags(&d),
        (RouteHow::Guess, None, true),
        "nothing to go on: home, said as a guess"
    );
}

#[test]
fn a_single_threads_place_waits_for_nothing() {
    let all = [pr_round()];
    let t = refs(&all);
    assert_eq!(
        decide_single(&t, &pre_with(&[("pr-round", 0.95)])).how,
        RouteHow::Quiet
    );
    let v = decide_single(&t, &pre_with(&[("pr-round", 0.7)]));
    assert_eq!((v.how, v.movable), (RouteHow::Visible, true));
    let h = decide_single(&t, &pre_with(&[("pr-round", 0.2)]));
    assert_eq!((h.how, h.slug), (RouteHow::Home, None));
}

/// A lone thread offers a new one (theseus-98bz.7): below the confirm bar,
/// with two entities no working set holds, the ask lists the thread, a new
/// one and home; a message the thread holds, or one naming fewer new
/// entities, is left to `decide_single` as before.
#[test]
fn a_lone_thread_offers_a_new_one_beside_it() {
    let tarn = ws(&[], &[], &[], &[], &[], &["tarnlib"]);
    let all = [th("tarnlib-work", "tarnlib work", tarn)];
    let t = refs(&all);
    let msg = "homesvc maintenance: branch chore/bump-1-8, ticket OPS-4214";
    let pre = pre_route(msg, &t, None, 0, 600_000);
    assert!(pre[0].score < CONFIRM);
    let n = new_entities(msg, &t);
    assert_eq!(n, 2);
    let Some(Decision::Ask { options }) = lone_offer(&t, &pre, n, "homesvc maintenance", true)
    else {
        panic!("an ask")
    };
    let keys: Vec<&str> = options.iter().map(|o| o.option.key.as_str()).collect();
    assert_eq!(keys, ["1", "new", "home"]);
    assert_eq!(options[0].choice, Choice::Thread("tarnlib-work".into()));
    assert_eq!(
        options[1].option.label,
        "New thread \u{ab}homesvc maintenance\u{bb}"
    );
    let held = "retry the flaky test in tarnlib";
    let pre = pre_route(held, &t, None, 0, 600_000);
    assert_eq!(
        lone_offer(&t, &pre, new_entities(held, &t), "retry", true),
        None
    );
    let v = decide_single(&t, &pre);
    assert_eq!(
        (v.slug.as_deref(), v.how),
        (Some("tarnlib-work"), RouteHow::Visible)
    );
    let low = pre_with(&[("tarnlib-work", 0.2)]);
    assert_eq!(lone_offer(&t, &low, 1, "x", true), None);
}

#[test]
fn the_question_is_one_line_and_the_options_are_keyed() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let ranked = vec![
        (Choice::Thread("pr-round".into()), 0.5),
        (Choice::Thread("security-fix".into()), 0.45),
        (Choice::Home, 0.0),
    ];
    let opts = ask_options(&t, &ranked, Some("tide gauge"), false);
    assert_eq!(
        question(&opts),
        "Is this about the pr round or the security fix?"
    );
    let keys: Vec<&str> = opts.iter().map(|o| o.option.key.as_str()).collect();
    assert_eq!(keys, ["1", "2", "new", "home"]);
    assert_eq!(opts[2].option.label, "New thread \u{ab}tide gauge\u{bb}");
    assert_eq!(question(&opts[..1]), "Is this about the pr round thread?");
}

/// An ask names every live, unsealed thread (theseus-98bz.4): all four of
/// four, ranked by fused score; of five, the best three and `other`; never a
/// sealed one. Off, the best two as before.
#[test]
fn an_ask_lists_every_thread_up_to_four_then_three_and_other() {
    let th_n = |n: usize| th(&format!("t-{n}"), &format!("t {n}"), WorkingSet::default());
    let ranked = |n: usize| -> Vec<(Choice, f64)> {
        (1..=n)
            .rev()
            .map(|i| (Choice::Thread(format!("t-{i}")), i as f64 / 10.0))
            .chain([(Choice::Home, 0.0)])
            .collect()
    };
    let keys = |o: &[AskChoice]| o.iter().map(|o| o.option.key.clone()).collect::<Vec<_>>();
    let four: Vec<ThreadInfo> = (1..=4).map(th_n).collect();
    let o = ask_options(&refs(&four), &ranked(4), None, true);
    assert_eq!(keys(&o), ["1", "2", "3", "4", "home"]);
    assert_eq!(o[0].option.label, "t 4", "the best first");
    assert_eq!(
        question(&o),
        "Is this about the t 4, the t 3, the t 2 or the t 1?"
    );
    let five: Vec<ThreadInfo> = (1..=5).map(th_n).collect();
    let o = ask_options(&refs(&five), &ranked(5), Some("x"), true);
    assert_eq!(keys(&o), ["1", "2", "3", "other", "new", "home"]);
    assert_eq!(
        question(&o),
        "Is this about the t 5, the t 4, the t 3 or another thread?"
    );
    let o = ask_options(&refs(&five), &ranked(5), None, false);
    assert_eq!(keys(&o), ["1", "2", "home"], "off: the best two");
    assert_eq!(question(&o), "Is this about the t 5 or the t 4?");
    // A sealed thread is never named; four askable ones are all listed.
    let mut sealed = five;
    sealed[4].sealed = Some(theseus_protocol::threads::Seal {
        reason: "embargo".into(),
        at_ms: 1,
        by: "the owner".into(),
    });
    let o = ask_options(&refs(&sealed), &ranked(5), None, true);
    assert_eq!(keys(&o), ["1", "2", "3", "4", "home"]);
    assert!(o.iter().all(|c| c.choice != Choice::Thread("t-5".into())));
    assert_eq!(
        askable(&refs(&sealed)).len(),
        4,
        "a sealed thread answers no ask"
    );
}

#[test]
fn a_new_threads_title_and_slug_come_from_the_message() {
    assert_eq!(
        propose("Let's look at the tide gauge calibration", &[]),
        ("look tide gauge".to_string(), "look-tide-gauge".to_string())
    );
    let (_, slug) = propose("tide gauge calibration notes", &["tide-gauge-calibration"]);
    assert_eq!(slug, "tide-gauge-calibration-2");
    assert_eq!(propose("ok", &[]).1, "thread");
}

/// A plainly new request no thread claims runs at home without a
/// question (theseus-e31an; the walkthrough's step 1 was asked about at
/// 0.38): Jev's weak lean to one thread, or to home, is no contest. Two
/// threads with evidence each still ask.
#[test]
fn a_request_no_thread_claims_runs_at_home_without_asking() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let quiet = pre_with(&[("pr-round", 0.05), ("security-fix", 0.0)]);
    let lean = Jev {
        pick: Pick::Thread("security-fix".into()),
        p: 0.38,
    };
    let d = decide(&t, &quiet, Some(&lean), Voice::Jev, Some("haiku"), ON);
    assert_eq!(tags(&d), (RouteHow::Home, None, true));
    let home = Jev {
        pick: Pick::Home,
        p: 0.38,
    };
    let d = decide(&t, &quiet, Some(&home), Voice::Jev, Some("haiku"), ON);
    assert_eq!(tags(&d), (RouteHow::Home, None, true));
    // Two threads, each with evidence, close: still asked.
    let both = pre_with(&[("pr-round", 0.80), ("security-fix", 0.0)]);
    let rival = Jev {
        pick: Pick::Thread("security-fix".into()),
        p: 0.70,
    };
    let d = decide(&t, &both, Some(&rival), Voice::Jev, None, ON);
    assert!(matches!(d, Decision::Ask { .. }), "{d:?}");
}

/// The directory rule (theseus-vw8k7), pure: a thread whose roots lie
/// apart from the turn's directory keeps its entity score and nothing
/// else; under a root, above one, with no roots or no directory, nothing
/// changes.
#[test]
fn a_thread_apart_from_the_turns_directory_keeps_only_its_entities() {
    let home = Path::new("/h");
    let dev = th(
        "dev",
        "lighthouse dev",
        ws(
            &["#412"],
            &[],
            &[],
            &[],
            &[],
            &["~/projects/lighthouse", "crates"],
        ),
    );
    let ops = th("ops", "ops rota", ws(&["#977"], &[], &[], &[], &[], &[]));
    let rel = th(
        "rel",
        "relative",
        ws(&["#3"], &[], &[], &[], &[], &["docs"]),
    );
    let t = [&dev, &ops, &rel];
    let last = Last {
        slug: Some("dev".into()),
        at_ms: 1_000,
        replied_ms: Some(1_000),
    };
    let scored = |text: &str| {
        let mut pre = pre_route(text, &t, Some(&last), 1_000, 600_000);
        pre[0].score = pre[0].score.max(LEARNED);
        pre[0].learned = true;
        pre
    };
    let fresh = scored("[dry-run] Start 2 tasks: bash slow.sh census 2");
    let base = |t: &ThreadInfo| (t.slug == "dev").then_some("/h/projects/lighthouse");
    let apart = |text: &str, dir: Option<&str>| {
        let mut pre = scored(text);
        let dropped = apart_by_dir(&mut pre, &t, text, dir, base, Some(home));
        (pre, dropped)
    };
    // Apart (`~` read as home): the prior and the correction are gone,
    // the other threads untouched.
    let (pre, dropped) = apart(
        "[dry-run] Start 2 tasks: bash slow.sh census 2",
        Some("~/projects/walkthrough"),
    );
    assert_eq!(dropped, ["dev"]);
    assert_eq!(
        (pre[0].score, pre[0].sticky, pre[0].chain, pre[0].learned),
        (0.0, 0.0, 0.0, false),
        "{pre:?}"
    );
    assert_eq!(
        pre[1..],
        fresh[1..],
        "a thread with no roots, or roots unread, as before"
    );
    // An entity the message names still counts.
    let (pre, _) = apart("what about #412", Some("/h/projects/walkthrough"));
    assert!(
        pre[0].score > 0.0 && pre[0].score == pre[0].entity,
        "{pre:?}"
    );
    assert!(!pre[0].learned);
    // Under a root (absolute, or a relative one read under its base),
    // above one, or no directory: as before.
    for dir in [
        Some("/h/projects/lighthouse/src"),
        Some("/h/projects/lighthouse/crates/core"),
        Some("/h/projects"),
        None,
        Some("relative/dir"),
    ] {
        let (pre, dropped) = apart("[dry-run] Start 2 tasks: bash slow.sh census 2", dir);
        assert!(dropped.is_empty(), "{dir:?}");
        assert_eq!(pre, fresh, "{dir:?}");
    }
    // A thread whose only roots are relative, with no base, is unread.
    let mut pre = pre_route("census", &[&rel], None, 1_000, 600_000);
    pre[0].score = LEARNED;
    pre[0].learned = true;
    let before = pre.clone();
    apart_by_dir(
        &mut pre,
        &[&rel],
        "census",
        Some("/h/elsewhere"),
        |_| None,
        Some(home),
    );
    assert_eq!(pre, before);
}

/// The directory rule's silence, in words (theseus-2gtyi): from a
/// directory above every root the rule drops nothing, and says so once
/// in the row; from under a root, or apart from all of them, it adds
/// nothing.
#[test]
fn a_directory_above_every_root_is_said_in_words() {
    let home = Path::new("/h");
    let dev = th(
        "dev",
        "lighthouse dev",
        ws(&[], &[], &[], &[], &[], &["~/projects/lighthouse"]),
    );
    let ops = th("ops", "ops rota", ws(&["#977"], &[], &[], &[], &[], &[]));
    let t = [&dev, &ops];
    let said = |dir: Option<&str>| above_every_root(&t, dir, |_| None, Some(home));
    let words = said(Some("~")).expect("above the roots");
    assert!(words.contains("/h,"), "{words}");
    assert!(words.contains("dev"), "{words}");
    assert!(!words.contains("ops"), "{words}");
    for w in ["cd into a root", "absolute path", "/move"] {
        assert!(words.contains(w), "{w}: {words}");
    }
    assert_eq!(said(Some("/h")), said(Some("~")));
    for dir in [
        Some("/h/projects/lighthouse"),
        Some("/h/projects/lighthouse/src"),
        Some("/h/elsewhere"),
        Some("relative"),
        None,
    ] {
        assert_eq!(said(dir), None, "{dir:?}");
    }
    assert_eq!(
        above_every_root(&[&ops], Some("/h"), |_| None, Some(home)),
        None
    );
}

/// The directory rule's file hits (theseus-vw8k7): a file named bare, as
/// a working set grows it from a message, is another directory's; named
/// by its absolute path, it counts.
#[test]
fn a_bare_file_name_from_another_directory_is_no_hit() {
    let home = Path::new("/h");
    let tool = th(
        "tool",
        "census tool",
        ws(
            &[],
            &[],
            &[],
            &["slow.sh", "/h/projects/lighthouse/bin/fast.sh"],
            &[],
            &["~/projects/lighthouse"],
        ),
    );
    let one = [&tool];
    for (text, hit) in [
        ("bash slow.sh census 2", false),
        ("bash fast.sh census 2", false),
        ("bash /h/projects/lighthouse/bin/fast.sh census 2", true),
    ] {
        let mut pre = pre_route(text, &one, None, 1_000, 600_000);
        assert!(pre[0].entity > 0.0, "{text}: {pre:?}");
        apart_by_dir(
            &mut pre,
            &one,
            text,
            Some("/h/projects/walkthrough"),
            |_| None,
            Some(home),
        );
        assert_eq!(pre[0].entity > 0.0, hit, "{text}: {pre:?}");
        assert_eq!(pre[0].score, pre[0].entity);
    }
}

/// The directory rule scores what is left as `pre_route` would have, had
/// the dropped hits never been there (theseus-ax4dp): another thread's
/// entity no longer counts less for a hit that was dropped, and a thread
/// is no longer treated as beside another's named entity when the only
/// one named was a dropped file.
#[test]
fn the_directory_rule_recounts_what_it_dropped() {
    let home = Path::new("/h");
    let dir = Some("/h/projects/walkthrough");
    let base = |_: &ThreadInfo| None;
    let apart = th(
        "apart",
        "lighthouse dev",
        WorkingSet {
            files: vec!["slow.sh".into()],
            branches: vec!["feat-x".into()],
            ..ws(&[], &[], &[], &[], &[], &["/h/projects/lighthouse"])
        },
    );
    let near = th(
        "near",
        "walkthrough",
        ws(
            &[],
            &[],
            &["feat-y"],
            &[],
            &[],
            &["/h/projects/walkthrough"],
        ),
    );
    let last = Last {
        slug: Some("near".into()),
        at_ms: 1_000,
        replied_ms: Some(1_000),
    };
    let t = [&apart, &near];
    let run = |text: &str| {
        let mut pre = pre_route(text, &t, Some(&last), 1_000, 600_000);
        apart_by_dir(&mut pre, &t, text, dir, base, Some(home));
        pre
    };
    // The apart thread's hit is its bare file, dropped: the near
    // thread's branch hit no longer shares the message.
    let pre = run("bash slow.sh on feat-y");
    assert_eq!(pre[0].entity, 0.0, "{pre:?}");
    assert_eq!(
        pre[1].entity,
        entity_of(&[(Kind::Branch, "feat-y".into())], 1)
    );
    // The apart thread keeps its branch hit, scored alone; a branch of
    // its own is still another thread's entity named, as before.
    let pre = run("bash slow.sh on feat-x");
    assert_eq!(
        pre[0].entity,
        entity_of(&[(Kind::Branch, "feat-x".into())], 1)
    );
    assert_eq!(pre[0].hits, ["feat-x"]);
    assert_eq!(pre[1].sticky, sticky(0, 600_000) * 0.5, "{pre:?}");
    // Only the dropped file is named: nothing names a thread, so the
    // near thread keeps its whole stickiness and chain.
    let pre = run("bash slow.sh census");
    let plain = pre_route("census", &[&near], Some(&last), 1_000, 600_000);
    assert_eq!(
        (pre[1].sticky, pre[1].chain, pre[1].score),
        (plain[0].sticky, plain[0].chain, plain[0].score),
        "{pre:?}"
    );
}

/// The `new` option rides only the ask the stickiness cap made
/// (theseus-q3ggn): two new entities beside a capped sticky thread offer
/// it; the same entities in an ask of two close threads do not, unless
/// Jev raised it.
#[test]
fn the_new_offer_rides_only_the_ask_the_cap_made() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let keys = |d: &Decision| match d {
        Decision::Ask { options } => options
            .iter()
            .map(|o| o.option.key.clone())
            .collect::<Vec<_>>(),
        other => panic!("not an ask: {other:?}"),
    };
    let rules = Rules {
        new_entities: 2,
        ..ON
    };
    // Capped: the place's sticky thread, Jev elsewhere.
    let last = Last {
        slug: Some("pr-round".into()),
        at_ms: 1_000,
        replied_ms: Some(1_000),
    };
    let text = "homesvc maintenance: branch chore/bump-1-8, ticket OPS-4214";
    let sticky = pre_route(text, &t, Some(&last), 1_000, 600_000);
    let other = Jev {
        pick: Pick::Thread("security-fix".into()),
        p: 0.3,
    };
    let d = decide(
        &t,
        &sticky,
        Some(&other),
        Voice::Jev,
        Some("homesvc"),
        rules,
    );
    assert!(keys(&d).contains(&"new".to_string()), "{d:?}");
    // No cap: two threads within CLOSE on entities.
    let close = pre_with(&[("pr-round", 0.70), ("security-fix", 0.65)]);
    let low = Jev {
        pick: Pick::Thread("security-fix".into()),
        p: 0.2,
    };
    let d = decide(&t, &close, Some(&low), Voice::Jev, Some("homesvc"), rules);
    assert_eq!(keys(&d), ["1", "2", "home"], "no cap, no new: {d:?}");
    // Jev raising it still offers it.
    let new = Jev {
        pick: Pick::New,
        p: 0.5,
    };
    let d = decide(&t, &close, Some(&new), Voice::Jev, Some("homesvc"), rules);
    assert!(keys(&d).contains(&"new".to_string()), "{d:?}");
}

// ---- `[threads] unsure = "guess"` (theseus-ttlze)

const GUESSING: Rules = Rules {
    guess_unsure: true,
    ..ON
};

fn guess_of(d: &Decision) -> &ThreadTag {
    match d {
        Decision::Route(tag) => {
            assert_eq!(tag.how, RouteHow::Guess, "{d:?}");
            assert!(tag.movable, "a guess can be moved: {d:?}");
            tag
        }
        other => panic!("not a guess: {other:?}"),
    }
}

/// Every table path that asks under `ask` places a guess under `guess`, and a
/// path that already routes is untouched.
#[test]
fn under_guess_every_ask_and_offer_is_a_guess_and_ask_is_unchanged() {
    let all = [pr_round(), security_fix()];
    let t = refs(&all);
    let close = pre_with(&[("pr-round", 0.70), ("security-fix", 0.65)]);
    let low = Jev {
        pick: Pick::Thread("security-fix".into()),
        p: 0.2,
    };
    let asked = decide(&t, &close, Some(&low), Voice::Jev, Some("x"), ON);
    assert!(
        matches!(asked, Decision::Ask { .. }),
        "the premise: {asked:?}"
    );
    let g = decide(&t, &close, Some(&low), Voice::Jev, Some("x"), GUESSING);
    let tag = guess_of(&g);
    assert!(tag.slug.is_some(), "Jev's thread pick is the guess: {g:?}");
    // A new-thread offer is a guess of home, never a thread made.
    let new = Jev {
        pick: Pick::New,
        p: 0.95,
    };
    let offer = decide(&t, &close, Some(&new), Voice::Jev, Some("x"), ON);
    assert!(matches!(offer, Decision::OfferNew { .. }), "{offer:?}");
    let g = decide(&t, &close, Some(&new), Voice::Jev, Some("x"), GUESSING);
    let _ = guess_of(&g); // a Route cannot make a thread
                          // A route that was not an ask is not touched.
    let sure = pre_with(&[("pr-round", 0.95), ("security-fix", 0.05)]);
    let same = decide(&t, &sure, None, Voice::Alone, None, ON);
    assert_eq!(decide(&t, &sure, None, Voice::Alone, None, GUESSING), same);
}

/// A guess never lands in a sealed or seal-pending thread: it goes home.
#[test]
fn a_guess_never_lands_in_a_sealed_or_seal_pending_thread() {
    let mut sealed = pr_round();
    sealed.sealed = Some(theseus_protocol::threads::Seal {
        by: "owner".into(),
        at_ms: 1,
        reason: "embargo".into(),
    });
    let mut pending = pr_round();
    pending.seal_pending = Some("looks embargoed".into());
    for top in [sealed, pending] {
        let all = [top, security_fix()];
        let t = refs(&all);
        let close = pre_with(&[("pr-round", 0.70), ("security-fix", 0.65)]);
        let d = guessed(Decision::Ask { options: vec![] }, &t, &close, None);
        assert_eq!(
            guess_of(&d).slug,
            None,
            "home, not the sealed thread: {d:?}"
        );
    }
}
