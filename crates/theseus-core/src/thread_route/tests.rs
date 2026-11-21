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
