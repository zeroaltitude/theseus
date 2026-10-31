//! Persona engagement through the whole core (theseus-c1uz, T10a): an
//! explicit address wins at once, Jev's `persona` answer engages by its
//! bands, the engaged persona is one line in the request's tail (so a switch
//! changes no byte of the cached prefix), it rides the one inbound request,
//! and the owner's correction labels the persona question and steers the
//! next message.

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::config::{PersonaConfig, PersonaEngage};
use crate::tests_route::{mode, rig, until_late, Rig};

/// A person's message in a DM, where a message can engage another persona (theseus-nsp9l.1: a cli session's
/// persona is fixed). With no session it opens one in the owner's DM.
async fn turn(
    core: &std::sync::Arc<crate::Core>,
    session: Option<&str>,
    input: &str,
    chosen: Option<&str>,
) -> theseus_protocol::TurnSubmitResult {
    let id = match session {
        Some(id) => id.to_string(),
        None => {
            let rec = crate::session::SessionRecord::new(
                theseus_protocol::SessionKind::Conversation,
                None,
            );
            core.store.put_session(&rec.session_id, &rec).unwrap();
            core.bind_places(vec![crate::places::BoundPlace {
                target: "discord:dm:42".into(),
                name: "DM".into(),
                ..Default::default()
            }]);
            core.outbox.bind_place("dm:42", &rec.session_id).unwrap();
            rec.session_id
        }
    };
    crate::tests_route::turn(core, Some(&id), input, chosen).await
}

fn personas(c: &mut crate::Config, act: bool) {
    for (key, card) in [
        (
            "birch",
            "Security engineer: CVEs, sandbox escapes, threat models and disclosure.",
        ),
        (
            "alder",
            "Release manager: changelogs, version bumps, packaging and CI.",
        ),
    ] {
        c.personas.insert(
            key.into(),
            PersonaConfig {
                card: Some(card.into()),
                files: vec![crate::personas::test_soul(key)],
                ..Default::default()
            },
        );
    }
    c.context.default_persona = Some("alder".into());
    if act {
        c.threads.persona_engage = PersonaEngage::Act;
    }
}

fn persona(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "persona",
        Jev::Choice {
            option: option.into(),
            confidence,
        },
    );
}

fn engaged(r: &Rig) -> Vec<Value> {
    crate::tests_judge::kinds(&r.core.store, "persona.engaged")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// The last message's blocks of a request, as texts.
fn tail(req: &crate::provider::ProviderRequest) -> Vec<String> {
    let last = req.messages.last().unwrap();
    match &last["content"] {
        Value::String(s) => vec![s.clone()],
        Value::Array(b) => b
            .iter()
            .filter_map(|b| b["text"].as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    }
}

/// An address names the persona at once, at confidence 1, whatever Jev
/// says and with Jev not even waited on for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explicit_address_engages_at_one_and_the_tail_says_so() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    persona(&jev, "alder", 0.99);
    let r = rig(Some(&jev), 1, |c| personas(c, true));
    let res = turn(&r.core, None, "Birch, is this a CVE?", None).await;
    let rows = engaged(&r);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["persona"], "birch");
    assert_eq!(rows[0]["how"], "explicit");
    assert_eq!(rows[0]["p"], 1.0);
    let req = &r.claude.requests()[0];
    let t = tail(req);
    assert_eq!(
        t.last().map(String::as_str),
        Some("Birch answers this turn"),
        "{t:?}"
    );
    assert!(t[0].contains("Birch, is this a CVE?"));
    let p = res.route.unwrap().persona.unwrap();
    assert_eq!((p.name.as_str(), p.how.as_str()), ("Birch", "explicit"));
}

/// Jev's pick at the act bar engages silently; between confirm and act with a
/// visible why; below confirm, or in shadow, the prior stands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jevs_pick_engages_by_its_bands() {
    for (p, act, want_how, want_name, shown) in [
        (0.95, true, "jev", "Birch", false),
        (0.75, true, "jev_shown", "Birch", true),
        (0.40, true, "prior", "Alder", false),
        (0.95, false, "prior", "Alder", false),
    ] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "chat", 0.95);
        persona(&jev, "birch", p);
        let r = rig(Some(&jev), 1, |c| personas(c, act));
        let res = turn(&r.core, None, "Is this CVE a sandbox escape?", None).await;
        let rows = engaged(&r);
        assert_eq!(rows[0]["how"], want_how, "{p} {act}: {rows:?}");
        let t = tail(&r.claude.requests()[0]);
        assert_eq!(
            t.last().unwrap(),
            &format!("{want_name} answers this turn"),
            "{p} {act}"
        );
        let shown_p = res.route.unwrap().persona.unwrap();
        assert_eq!(!shown_p.why.is_empty(), shown, "{p} {act}: {shown_p:?}");
        if shown {
            assert!(
                shown_p.why.contains("\"CVEs\"") && shown_p.why.contains("next "),
                "{}",
                shown_p.why
            );
        }
        if want_how == "prior" && p > 0.9 {
            assert_eq!(rows[0]["jev"], "birch", "Jev's pick is kept for the label");
        }
    }
}

/// Jev's `none` engages the default persona: after birch was addressed, a
/// message for nobody in particular goes back to alder, which the question
/// offers as `none` and not under its own key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jevs_none_engages_the_default_persona() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    persona(&jev, "none", 0.95);
    let r = rig(Some(&jev), 1, |c| personas(c, true));
    let first = turn(&r.core, None, "Birch, is this a CVE?", None).await;
    let second = turn(
        &r.core,
        Some(&first.session_id),
        "Thanks. What's for lunch?",
        None,
    )
    .await;
    assert_eq!(
        second.route.unwrap().persona.unwrap().key.as_deref(),
        Some("alder")
    );
    let rows = engaged(&r);
    assert_eq!(
        (rows[0]["persona"].as_str(), rows[0]["how"].as_str()),
        (Some("birch"), Some("explicit"))
    );
    assert_eq!(
        (
            rows[1]["persona"].as_str(),
            rows[1]["how"].as_str(),
            rows[1]["jev"].as_str()
        ),
        (Some("alder"), Some("jev"), Some("none")),
        "{rows:?}"
    );
}

/// The persona question rides the inbound request that was already going out:
/// the same two calls with personas as without, and the route request asks
/// it beside the mode and the effort.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_persona_question_adds_no_call() {
    let with = FakeJev::start().unwrap();
    mode(&with, "chat", 0.95);
    let r = rig(Some(&with), 1, |c| personas(c, true));
    turn(&r.core, None, "Is this CVE a sandbox escape?", None).await;
    crate::tests_route::until_calls(&with, 2).await;
    let without = FakeJev::start().unwrap();
    mode(&without, "chat", 0.95);
    let r2 = rig(Some(&without), 1, |_| {});
    turn(&r2.core, None, "Is this CVE a sandbox escape?", None).await;
    crate::tests_route::until_calls(&without, 2).await;
    assert_eq!(with.connections(), without.connections(), "no call added");
    assert_eq!(with.connections(), 2);
    let asked = |j: &FakeJev| -> Vec<String> {
        j.seen()
            .iter()
            .flat_map(|s| {
                s.body["questions"]
                    .as_object()
                    .into_iter()
                    .flat_map(|q| q.keys().cloned())
            })
            .collect()
    };
    let (a, b) = (asked(&with), asked(&without));
    assert!(a.contains(&"route.v10/persona".to_string()), "{a:?}");
    assert!(a.contains(&"route.v10/model".to_string()));
    assert!(
        !b.iter().any(|q| q.ends_with("/persona")),
        "no personas, no question: {b:?}"
    );
}

/// A switch of persona changes no byte before the new message: the system
/// blocks and every earlier message of two turns' requests are the same, and
/// the line sits after them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_switch_changes_no_prefix_byte() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 2, |c| personas(c, true));
    let first = turn(&r.core, None, "Birch, is this a CVE?", None).await;
    let sid = first.session_id.clone();
    turn(
        &r.core,
        Some(&sid),
        "Alder, what changed in the changelog?",
        None,
    )
    .await;
    let reqs = r.claude.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(
        reqs[0].system, reqs[1].system,
        "the system blocks are the same"
    );
    assert_eq!(reqs[0].tools, reqs[1].tools);
    // The first request's one message is the first message of the second.
    let strip = |m: &Value| -> String {
        match &m["content"] {
            Value::String(s) => s.clone(),
            Value::Array(b) => b
                .iter()
                .filter_map(|b| b["text"].as_str())
                .filter(|t| !t.ends_with("answers this turn"))
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        }
    };
    assert_eq!(strip(&reqs[0].messages[0]), strip(&reqs[1].messages[0]));
    let second = &reqs[1].messages[0]["content"];
    let text = serde_json::to_string(second).unwrap();
    assert!(
        !text.contains("answers this turn"),
        "the earlier line is not repeated: {text}"
    );
    assert_eq!(tail(&reqs[1]).last().unwrap(), "Alder answers this turn");
    assert_eq!(tail(&reqs[0]).last().unwrap(), "Birch answers this turn");
    // Every persona's files and the built-in default are in the prefix whoever answers.
    let sys = |q: &crate::provider::ProviderRequest| serde_json::to_string(&q.system).unwrap();
    assert!(
        !sys(&reqs[0]).contains("answers this turn"),
        "no persona in the header"
    );
}

/// The owner's correction labels the persona question, answers the next
/// message as the persona named, and a close message after it learns.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_correction_labels_steers_and_teaches() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    persona(&jev, "alder", 0.95);
    let r = rig(Some(&jev), 3, |c| personas(c, true));
    let first = turn(
        &r.core,
        None,
        "Tag the release and bump the packaging",
        None,
    )
    .await;
    let sid = first.session_id.clone();
    let p = theseus_protocol::route::RouteCorrectParams {
        session_id: sid.clone(),
        turn_id: Some(first.turn_id.clone()),
        to: "persona:birch".into(),
        via: Some("cli".into()),
        ..Default::default()
    };
    let res = r.core.route_correct(&p, "cli").unwrap();
    assert!(res.label.is_some(), "{res:?}");
    let labels = crate::tests_judge::kinds(&r.core.store, "judge.label");
    let l = labels.last().expect("a label").data.clone();
    assert_eq!(l["question"], "persona");
    assert_eq!(l["label"], "birch");
    // The very next message is answered by the corrected persona.
    turn(
        &r.core,
        Some(&sid),
        "Anything else about the release notes",
        None,
    )
    .await;
    let rows = engaged(&r);
    assert_eq!(
        (rows[1]["persona"].as_str(), rows[1]["how"].as_str()),
        (Some("birch"), Some("corrected"))
    );
    // A message close to the corrected one is learned.
    turn(
        &r.core,
        Some(&sid),
        "Tag the release and bump the packaging again",
        None,
    )
    .await;
    let rows = engaged(&r);
    assert_eq!(
        (rows[2]["persona"].as_str(), rows[2]["how"].as_str()),
        (Some("birch"), Some("learned"))
    );
}

/// Every persona's files are in the request's persona layer whoever answers
/// (the prefix never changes at a switch), and none of their text is ever in
/// what Jev is sent: it reads the cards.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_personas_files_are_in_the_prefix_and_never_in_jevs_state() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    persona(&jev, "birch", 0.95);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("birch.md");
    std::fs::write(
        &file,
        "BIRCH-FILE-MARKER: I speak of exploits and embargoes.",
    )
    .unwrap();
    let path = file.to_string_lossy().into_owned();
    let r = rig(Some(&jev), 1, |c| {
        personas(c, true);
        c.personas.get_mut("birch").unwrap().files = vec![path.clone().into()];
    });
    turn(&r.core, None, "Is this CVE a sandbox escape?", None).await;
    crate::tests_route::until_calls(&jev, 2).await;
    let req = &r.claude.requests()[0];
    let sys = serde_json::to_string(&req.system).unwrap();
    assert!(
        sys.contains("BIRCH-FILE-MARKER"),
        "the file is in the persona layer"
    );
    assert!(
        sys.contains("Candour first"),
        "the header keeps the built-in default"
    );
    for s in jev.seen() {
        let body = serde_json::to_string(&s.body).unwrap();
        assert!(
            !body.contains("BIRCH-FILE-MARKER"),
            "a persona file reached Jev"
        );
    }
    let asked = serde_json::to_string(
        &jev.seen()
            .iter()
            .map(|s| s.body.clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(asked.contains("sandbox escapes"), "Jev reads the card");
}

/// A seal verdict at the confirm bar or above, in a thread, proposes sealing
/// it when `[threads] seal_recommend` is on, and never seals: the thread is
/// pending, and the owner decides. Off, or in no thread, nothing is proposed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seal_yes_in_a_thread_proposes_and_never_seals() {
    for (on, slug_in_thread, want) in [
        (true, true, true),
        (false, true, false),
        (true, false, false),
    ] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "chat", 0.95);
        jev.script(
            "seal",
            Jev::Choice {
                option: "yes".into(),
                confidence: 0.93,
            },
        );
        let r = rig(Some(&jev), 1, |c| {
            personas(c, false);
            c.threads.seal_recommend = on;
        });
        r.core
            .thread_new(
                &theseus_protocol::threads::ThreadNewParams {
                    slug: "vault-fix".into(),
                    title: Some("vault fix".into()),
                    ..Default::default()
                },
                "cli",
            )
            .unwrap();
        let sid = match slug_in_thread {
            true => {
                r.core
                    .thread_session("vault-fix", "cli")
                    .unwrap()
                    .session_id
            }
            false => {
                let rec = crate::session::SessionRecord::new(
                    theseus_protocol::SessionKind::Conversation,
                    None,
                );
                r.core.store.put_session(&rec.session_id, &rec).unwrap();
                rec.session_id
            }
        };
        turn(
            &r.core,
            Some(&sid),
            "The embargoed CVE fix is in the vault branch",
            None,
        )
        .await;
        let t0 = std::time::Instant::now();
        let proposed = loop {
            let rows = crate::tests_judge::kinds(&r.core.store, "thread.seal_proposed");
            if !rows.is_empty() || t0.elapsed() > std::time::Duration::from_millis(1500) {
                break !rows.is_empty();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        assert_eq!(proposed, want, "on {on}, in a thread {slug_in_thread}");
        let t = r.core.threads().get("vault-fix").cloned().unwrap();
        assert!(t.sealed.is_none(), "Jev never seals");
        assert_eq!(t.seal_pending.is_some(), want);
    }
}

/// A persona verdict that missed the wait changes nothing: the prior stands
/// for that turn, and the late verdict, carried to the next message for the
/// mode, is not a persona's for it either.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_persona_verdict_leaves_the_prior() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    persona(&jev, "birch", 0.97);
    jev.set_mode(theseus_judge::fake::FakeMode::Held);
    let r = rig(Some(&jev), 2, |c| {
        personas(c, true);
        c.routing.max_wait_ms = 200;
        c.judge.total_secs = 30;
    });
    let one = turn(&r.core, None, "Is this CVE a sandbox escape?", None).await;
    assert_eq!(
        engaged(&r)[0]["how"],
        "prior",
        "the verdict missed the wait"
    );
    jev.release();
    until_late(&r.core, &one.session_id).await;
    turn(&r.core, Some(&one.session_id), "And the other CVE?", None).await;
    jev.release();
    let rows = engaged(&r);
    assert_eq!(
        (rows[1]["persona"].as_str(), rows[1]["how"].as_str()),
        (Some("alder"), Some("prior")),
        "a carried verdict is the last message's"
    );
    assert_eq!(
        tail(&r.claude.requests()[1]).last().unwrap(),
        "Alder answers this turn"
    );
}

/// A routed move keeps the persona layer (theseus-nsp9l.1): a DM turn Jev sends to another profile is a
/// `speaking_as` of the turn's target, so it still engages the persona the message calls for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dm_turn_routed_to_another_profile_still_engages_a_persona() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |c| personas(c, true));
    turn(
        &r.core,
        None,
        "Birch, weigh two designs for a crash-safe write-ahead log.",
        None,
    )
    .await;
    let reqs = r.claude.requests();
    let last = reqs.last().unwrap();
    assert_eq!(last.model, "claude-opus-5-5", "the turn moved");
    assert_eq!(tail(last).last().unwrap(), "Birch answers this turn");
}
