use super::*;
use serde_json::json;

pub(super) fn upserts(ops: &[Op]) -> Vec<(String, String)> {
    ops.iter()
        .filter_map(|o| match o {
            Op::Upsert { key, content, .. } => Some((key.clone(), content.clone())),
            Op::Typing | Op::Notice { .. } => None,
        })
        .collect()
}

fn result_of(v: Value) -> TurnSubmitResult {
    serde_json::from_value(v).unwrap()
}

fn request(v: Value) -> ConfirmRequest {
    serde_json::from_value(v).unwrap()
}

pub(super) fn ended(turn: &str, awaiting: Option<&str>) -> Value {
    json!({"session_id": "s", "turn_id": turn, "loops": 2, "output": "", "stop_reason": "no_tool_calls",
           "provider_stop_reason": null, "model": "claude-sonnet-5", "provider": "anthropic", "profile": "sonnet",
           "usage": {"input_tokens": 1, "output_tokens": 2, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
           "elapsed_ms": 4200, "tool_calls": 1, "cost_usd": 0.0123, "session_cost_usd": 47.52, "awaiting_confirm": awaiting, "continuation": false})
}

/// A held turn's footer says so instead of naming a model (theseus-2rvwv).
#[test]
fn a_held_turns_footer_names_no_model() {
    let mut v = ended("t1", None);
    v["stop_reason"] = json!("held");
    let held = footer(&result_of(v));
    assert!(
        held.starts_with("-# sonnet · held: no model ran · 2 loops"),
        "{held}"
    );
    assert!(
        !held.contains("claude-sonnet-5") && !held.contains("stopped"),
        "{held}"
    );
    let done = footer(&result_of(ended("t2", None)));
    assert!(
        done.starts_with("-# sonnet · claude-sonnet-5 · 2 loops"),
        "{done}"
    );
}

#[test]
fn a_turn_streams_as_edits_and_ends_with_a_footer() {
    let mut r = Renderer::default();
    assert_eq!(
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"})),
        vec![Op::Typing]
    );
    assert!(r.busy());
    r.on_notification(
        "model.delta",
        &json!({"turn_id": "t1", "loop_index": 0, "text": "Hel"}),
    );
    r.on_notification(
        "model.delta",
        &json!({"turn_id": "t1", "loop_index": 0, "text": "lo"}),
    );
    let first = upserts(&r.tick());
    assert_eq!(first, vec![("t1:L0:p0".into(), "Hello".into())]);
    assert!(r.tick().is_empty(), "nothing changed, nothing sent");
    r.on_notification(
        "tool.proposed",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "fs.read",
        "input": {"path": "src/lib.rs"}, "gate": {"result": {"gate": "allow"}}}),
    );
    r.on_notification(
        "tool.ended",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "ok", "duration_ms": 12}),
    );
    let ops = upserts(&r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
    assert_eq!(
        ops,
        vec![(
            "t1:L0:tools".into(),
            "✅ `fs.read` src/lib.rs · 12 ms".into()
        )]
    );
    r.on_notification(
        "model.delta",
        &json!({"turn_id": "t1", "loop_index": 1, "text": "Done."}),
    );
    // The turn's end is its reply's post (theseus-q4v): the stream stops,
    // and the post's parts edit the streamed messages under their keys.
    assert!(upserts(&r.on_notification("turn.ended", &ended("t1", None))).is_empty());
    assert!(!r.busy());
    let parts = reply_parts(
        "t1",
        &[(0, "Hello".into()), (1, "Done.".into())],
        Some(&result_of(ended("t1", None))),
    );
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0], ("t1:L0:p0".into(), "Hello".into()));
    assert_eq!(parts[1].0, "t1:L1:p0");
    assert!(
        parts[1].1.starts_with(
            "Done.\n-# sonnet · claude-sonnet-5 · 2 loops · 1 tool call · $47.52 total ($0.0123 this reply) · 4.2 s"
        ),
        "{}",
        parts[1].1
    );
    // With no text, the footer stands alone; an older daemon's gives no total.
    let mut old = ended("t2", None);
    old.as_object_mut().unwrap().remove("session_cost_usd");
    let alone = reply_parts("t2", &[], Some(&result_of(old)));
    assert_eq!(alone.len(), 1);
    assert_eq!(alone[0].0, "t2:footer");
    assert!(alone[0].1.ends_with("· $0.0123 · 4.2 s"), "{alone:?}");
    // A reply recall fed says how many notes (M6 30b); shadow says none.
    let mut fed = ended("t3", None);
    fed["recalled"] = json!(2);
    let fed = reply_parts("t3", &[], Some(&result_of(fed)));
    assert!(
        fed[0].1.ends_with(" · 4.2 s · 🧠 2 recalled"),
        "{}",
        fed[0].1
    );
    assert!(!alone[0].1.contains("recalled"), "{}", alone[0].1);
}

/// The footer lists the labels the session carries now (theseus-emqbm),
/// each with a link that removes that one from that session, so the
/// operator can take one off at any time and not only from the message
/// that raised it. The links follow `[inspect]`'s house form: masked,
/// in angle brackets so Discord draws no embed, on the footer's one line.
///
/// **The empty case is the assertion this row turns on**: a session with
/// no labels gets the footer it always had, byte for byte, which is what
/// keeps the locked DM flow's golden where it is.
#[test]
fn the_footer_lists_the_sessions_labels_each_a_link_that_removes_it() {
    let link = |flag: &str| format!("http://localhost:17433/session/s/unlabel/{flag}");
    let mut two = ended("t1", None);
    two["labels"] = json!([
        {"flag": "injection", "remove_url": link("injection")},
        {"flag": "personal", "remove_url": link("personal")},
    ]);
    let with = footer(&result_of(two));
    // Both labels are named, each with its own link, and each link
    // carries this session's id and that flag and no other.
    assert_eq!(with.lines().count(), 1, "the footer stays one line: {with}");
    assert!(
        with.contains(&format!(
            "labels: injection ([remove](<{}>)), personal ([remove](<{}>))",
            link("injection"),
            link("personal")
        )),
        "{with}"
    );
    assert_eq!(with.matches("[remove](<").count(), 2, "{with}");
    // A label with no address (no web UI, no `public_url`) is still said.
    let mut bare = ended("t2", None);
    bare["labels"] = json!([{"flag": "secret"}]);
    let bare = footer(&result_of(bare));
    assert!(bare.ends_with(" · 4.2 s · labels: secret"), "{bare}");
    assert!(!bare.contains("remove"), "{bare}");
}

/// A session carrying nothing produces the footer it always produced: no
/// separator, no count, no empty bracket (theseus-emqbm). Asserted beside
/// the positive count above, so an empty label list cannot pass both.
#[test]
fn a_session_with_no_labels_has_the_footer_it_always_had() {
    let mut with = ended("t1", None);
    with["inspect_url"] = json!("http://localhost:17433/session/s/turn/t1");
    let before = "-# sonnet · claude-sonnet-5 · 2 loops · 1 tool call · $47.52 total \
($0.0123 this reply) · 4.2 s · [inspect](<http://localhost:17433/session/s/turn/t1>)";
    assert_eq!(footer(&result_of(with)), before);
    let plain = footer(&result_of(ended("t2", None)));
    assert!(!plain.contains("labels"), "{plain}");
    assert!(!plain.contains("remove"), "{plain}");
}

/// The footer links to its turn in the cockpit (theseus-x9swu): a masked
/// link, angle brackets and all so Discord draws no embed, on the footer's
/// one line, from the result's `inspect_url`; none without one.
#[test]
fn the_footer_links_to_its_turn_on_its_own_line() {
    let url = "http://localhost:17433/session/s/turn/t1";
    let mut with = ended("t1", None);
    with["inspect_url"] = json!(url);
    let parts = reply_parts("t1", &[(0, "Done.".into())], Some(&result_of(with.clone())));
    let footer = parts[0].1.lines().last().unwrap();
    assert!(footer.starts_with("-# "), "{footer}");
    assert!(
        footer.ends_with(&format!(" · [inspect](<{url}>)")),
        "{footer}"
    );
    assert_eq!(parts.len(), 1, "no new message");
    let alone = reply_parts("t1", &[], Some(&result_of(with)));
    assert_eq!(alone.len(), 1);
    assert_eq!(
        alone[0].1.lines().count(),
        1,
        "the footer stays one line: {:?}",
        alone[0].1
    );
    assert!(!alone[0].1.contains("@"), "no mention");
    let none = reply_parts("t2", &[], Some(&result_of(ended("t2", None))));
    assert!(!none[0].1.contains("inspect"), "{}", none[0].1);
}

/// The stream shows without waiting (theseus-ck0n): a loop's first text
/// goes out at once, the rest of its stream at the tick, and its whole
/// answer (`model.answered`, sent before the settle's frame) at once, a
/// dropped delta mended. Once the turn ended, its post has the text, and
/// a late answer sends nothing.
#[test]
fn a_loops_first_text_and_its_whole_answer_go_out_at_once() {
    let mut r = Renderer::default();
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    let delta = |r: &mut Renderer, text: &str| {
        upserts(&r.on_notification(
            "model.delta",
            &json!({"turn_id": "t1", "loop_index": 0, "text": text}),
        ))
    };
    assert!(delta(&mut r, "").is_empty(), "no text yet");
    assert_eq!(
        delta(&mut r, "Good"),
        vec![("t1:L0:p0".into(), "Good".into())]
    );
    assert!(
        delta(&mut r, " eve").is_empty(),
        "the rest waits for the tick"
    );
    // A delta the place missed ("ning!") is in the whole answer.
    let whole = upserts(&r.on_notification(
        "model.answered",
        &json!({"turn_id": "t1", "loop_index": 0, "text": "Good evening! How are you?"}),
    ));
    assert_eq!(
        whole,
        vec![("t1:L0:p0".into(), "Good evening! How are you?".into())]
    );
    assert!(r.tick().is_empty(), "nothing more to send");
    assert!(upserts(&r.on_notification("turn.ended", &ended("t1", None))).is_empty());
    assert!(r
        .on_notification(
            "model.answered",
            &json!({"turn_id": "t1", "loop_index": 0, "text": "late"}),
        )
        .is_empty());
}

/// A session at its spend limit asks with its own message and buttons
/// (theseus-0sg); approve says the spend was reset, decline says it keeps
/// waiting, and a new message replaces the question.
#[test]
fn a_budget_question_asks_with_buttons_and_says_what_each_answer_does() {
    let ask = |corr: &str| {
        json!({"correlation_id": corr, "session_id": "s", "execution_id": "e", "tool": "budget.reset",
            "input": {"spent_usd": 99.48, "limit_usd": 100.0}, "by": "operator", "requested_at_ms": 1, "expires_at_ms": 0,
            "reason": "This session has spent $99.48 of its $100 limit. Reset its spend to $0 and continue?",
            "budget": {"spent_usd": 99.48, "limit_usd": 100.0, "needed_usd": 1.368, "lifetime_usd": 212.4}})
    };
    let card = card(&request(ask("act_b")), &Route::Here, ELSEWHERE);
    assert!(card.budget);
    assert!(card.content.starts_with(&format!("{BUDGET_ASK}This session has spent $99.48 of its $100 limit. Reset its spend to $0 and continue?\n")), "{}", card.content);
    assert!(
        card.content
            .contains("lifetime cost ($212.40) keeps counting"),
        "{}",
        card.content
    );
    assert!(
        !card.content.contains("expires"),
        "a budget question does not expire: {}",
        card.content
    );
    assert_eq!(card.line, "spend reset ($99.48 of the $100 limit)");
    for (closed, want) in [
        (Closed::new("approved", Some("discord:zeroaltitude")), "✅ **Approved** by discord:zeroaltitude · spend reset ($99.48 of the $100 limit)"),
        (Closed::new("declined", Some("discord:zeroaltitude")), "❎ **Declined** by discord:zeroaltitude · spend reset ($99.48 of the $100 limit); the session keeps waiting, and a new message asks again"),
        (Closed::new("superseded", None), "⏭️ **Replaced**: a new message came first, and its call asks again if it still does not fit. spend reset ($99.48 of the $100 limit)"),
        // A raise withdraws it (theseus-3pj, the stale card of S1).
        (Closed { how: "withdrawn".into(), by: None, note: Some("the spend limit was raised from $100 to $200".into()) },
         "↩️ **Withdrawn**: the spend limit was raised from $100 to $200 · spend reset ($99.48 of the $100 limit)"),
    ] {
        assert_eq!(settled(&closed, &card.line, true), want);
    }
}

#[test]
fn a_confirm_gets_buttons_and_loses_them_when_answered() {
    let mut r = Renderer::default();
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
        "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
    let req = json!({"correlation_id": "act_1", "session_id": "s",
        "execution_id": "e", "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "reason": "run cargo test",
        "by": "operator", "requested_at_ms": 1, "expires_at_ms": 1790000000000u64});
    // The card is a post; the stream marks the tool line, and shows it at
    // once, so that it goes before the card (theseus-50p).
    let tools = upserts(&r.on_notification("confirm.requested", &req));
    assert!(
        tools
            .iter()
            .any(|(k, c)| k == "t1:L0:tools" && c.contains("waiting for approval")),
        "{tools:?}"
    );
    let card = card(&request(req), &Route::Here, ELSEWHERE);
    assert!(card
        .content
        .starts_with("**Approve?** `proc.run` cargo test\nrun cargo test"));
    assert_eq!(card.line, "`proc.run` cargo test");
    // The turn's end has nothing new for it.
    let tools = upserts(&r.on_notification("turn.ended", &ended("t1", Some("act_1"))));
    assert!(!tools.iter().any(|(k, _)| k == "t1:L0:tools"), "{tools:?}");
    assert!(r
        .on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_1", "approved": true, "by": "discord:zeroaltitude"}),
        )
        .is_empty());
    assert_eq!(
        settled(
            &Closed::new("approved", Some("discord:zeroaltitude")),
            &card.line,
            false
        ),
        "✅ **Approved** by discord:zeroaltitude · `proc.run` cargo test"
    );
    let tools = upserts(&r.tick());
    assert!(
        tools[0].1.contains("approved by discord:zeroaltitude"),
        "{tools:?}"
    );
}

/// `tool.proposed` and `policy.notified` for a call that runs under
/// `enforcement = notify`, shaped as the core sends them.
fn notified(use_id: &str, cmd: &str) -> (Value, Value) {
    let argv: Vec<&str> = cmd.split(' ').collect();
    let rule = "proc.run — notify (enforcement = notify)";
    let proposed = json!({"turn_id": "t1", "tool_use_id": use_id, "tool": "proc.run",
        "input": {"argv": argv}, "gate": {"result": {"gate": "allow"}, "decision": {"posture": "notify",
        "reason": rule, "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": rule}}}});
    let notice = json!({"session_id": "s", "turn_id": "t1", "tool_use_id": use_id, "tool": "proc.run",
        "input": {"argv": argv}, "summary": format!("run `{cmd}` in /w"), "kind": "notify",
        "setting": "enforcement = notify", "rule": rule});
    (proposed, notice)
}

/// A notified call's score (M5 step 24) lands after its notice: its tool
/// line gains `risk N% (shadow)`, and its card, with embeds on, a field.
#[test]
fn a_notified_calls_score_rides_on_its_tool_line_and_card() {
    let scored = json!({"session_id": "s", "turn_id": "t1", "tool_use_id": "u1",
        "tool": "proc.run", "pack": "security.v1", "judgment": "jdg_1", "mode": "shadow",
        "risky": 0.12, "percent": 12});
    for embeds in [false, true] {
        let mut r = Renderer::new(embeds);
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        let (proposed, notice) = notified("u1", "cargo test");
        r.on_notification("tool.proposed", &proposed);
        r.on_notification("policy.notified", &notice);
        let ops = r.on_notification("judge.scored", &scored);
        if embeds {
            let Op::Notice { card, .. } = &ops[0] else {
                panic!("{ops:?}")
            };
            assert!(card
                .fields
                .contains(&("Risk".into(), "risk 12% (shadow)".into())));
        } else {
            assert_eq!(ops, vec![]);
        }
        let lines = upserts(&r.tick());
        assert!(
            lines[0]
                .1
                .contains("🔔 notified (enforcement = notify) · risk 12% (shadow)"),
            "{lines:?}"
        );
    }
}

/// Jev's live notice (step 24's notices) goes to the owner's DM as its
/// own post; its `judge.noticed` draws nothing in the place.
#[test]
fn jevs_notice_draws_nothing_in_the_place() {
    for embeds in [false, true] {
        let mut r = Renderer::new(embeds);
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        let ops = r.on_notification(
            "judge.noticed",
            &json!({"session_id": "s", "turn_id": "t1",
            "tool_use_id": "u1", "tool": "proc.run", "judgment": "jdg_1", "percent": 95}),
        );
        assert!(ops.is_empty(), "{ops:?}");
    }
}

#[test]
fn with_notice_embeds_a_notified_call_posts_a_card_and_the_card_gets_its_outcome() {
    let mut r = Renderer::new(true);
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
        "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "allow"},
        "decision": {"mode": "allow", "posture": "notify", "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}}}}));
    let ops = r.on_notification("policy.notified", &json!({"session_id": "s", "turn_id": "t1", "tool_use_id": "u1",
        "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "summary": "run `cargo test` in /w",
        "kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}));
    let Op::Notice { key, card } = &ops[0] else {
        panic!("{ops:?}")
    };
    assert_eq!(key, "t1:notice:u1");
    assert_eq!(card.title, "🔔 Ran with a notice");
    assert_eq!(card.color, AMBER);
    assert_eq!(card.description, "`proc.run` cargo test");
    assert_eq!(
        card.fields,
        vec![
            ("What".into(), "run `cargo test` in /w".into()),
            ("Posture".into(), "`enforcement = notify`".into()),
            ("Outcome".into(), "⏳ running".into()),
        ]
    );
    let ops = r.on_notification(
        "tool.ended",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "ok", "duration_ms": 900}),
    );
    let Op::Notice { card, .. } = &ops[0] else {
        panic!("{ops:?}")
    };
    assert_eq!(card.fields[2].1, "✅ ok · 900 ms");
    let lines =
        upserts(&r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
    assert!(
        lines[0].1.contains("🔔 notified (enforcement = notify)"),
        "{lines:?}"
    );
}

/// Without `notice_embeds` (the default, theseus-w4f) a notified call posts
/// nothing of its own: its tool line says it was notified, under which
/// setting, and then how it ended.
#[test]
fn without_notice_embeds_a_notified_call_rides_on_its_tool_line() {
    let mut r = Renderer::default();
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    let (proposed, notice) = notified("u1", "cargo test");
    r.on_notification("tool.proposed", &proposed);
    assert_eq!(r.on_notification("policy.notified", &notice), vec![]);
    r.on_notification(
        "tool.started",
        &json!({"turn_id": "t1", "tool_use_id": "u1"}),
    );
    assert_eq!(
        r.tick(),
        vec![Op::Upsert {
            key: "t1:L0:tools".into(),
            content: "⏳ `proc.run` cargo test · 🔔 notified (enforcement = notify)".into(),
            buttons: Buttons::ShouldHaveAsked(vec![asked("proc.run", None)])
        }]
    );
    let ops = r.on_notification(
        "tool.ended",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "error", "duration_ms": 900}),
    );
    assert_eq!(ops, vec![], "no card to edit");
    assert_eq!(
        r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})),
        vec![Op::Upsert {
            key: "t1:L0:tools".into(),
            content:
                "❌ `proc.run` cargo test · 🔔 notified (enforcement = notify) · error · 900 ms"
                    .into(),
            buttons: Buttons::Keep
        }]
    );
}

/// A call the secret broker gives a secret says so on its tool line
/// (theseus-dcy): the gate's word first, then what the job's start says it
/// actually got. A notice card gets a Secrets field.
#[test]
fn a_call_given_a_secret_says_so_on_its_tool_line() {
    let mut r = Renderer::new(true);
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    r.on_notification(
        "tool.proposed",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
        "input": {"argv": ["gh", "api", "user"]}, "gate": {"result": {"gate": "allow"},
        "decision": {"posture": "notify", "granted": "gh got GH_TOKEN", "notify": {"kind": "notify",
        "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}}}}),
    );
    let ops = r.on_notification(
        "policy.notified",
        &json!({"session_id": "s", "turn_id": "t1",
        "tool_use_id": "u1", "tool": "proc.run", "input": {"argv": ["gh", "api", "user"]},
        "summary": "run `gh api user` in /w", "kind": "notify", "setting": "enforcement = notify",
        "rule": "proc.run — notify (enforcement = notify)", "granted": "gh got GH_TOKEN"}),
    );
    let Op::Notice { card, .. } = &ops[0] else {
        panic!("{ops:?}")
    };
    assert_eq!(
        card.fields[3],
        ("Secrets".into(), "🔑 gh got GH_TOKEN".into())
    );
    r.on_notification(
        "tool.started",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "granted": "gh got GH_TOKEN", "withheld": []}),
    );
    let line = |r: &mut Renderer| match r.tick().first() {
        Some(Op::Upsert { content, .. }) => content.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        line(&mut r),
        "⏳ `proc.run` gh api user · 🔑 gh got GH_TOKEN · 🔔 notified (enforcement = notify)"
    );
    // A job that got nothing it was granted says so instead.
    r.on_notification(
        "tool.proposed",
        &json!({"turn_id": "t1", "tool_use_id": "u2", "tool": "proc.run",
        "input": {"argv": ["gh", "pr", "list"]}, "gate": {"decision": {"posture": "open",
        "granted": "gh got GH_TOKEN"}}}),
    );
    r.on_notification(
        "tool.started",
        &json!({"turn_id": "t1", "tool_use_id": "u2", "withheld": ["gh got no GH_TOKEN"]}),
    );
    assert!(line(&mut r).contains("⏳ `proc.run` gh pr list · 🔑 gh got no GH_TOKEN"));
}

fn asked(tool: &str, corr: Option<&str>) -> Asked {
    Asked {
        tool: tool.into(),
        correlation_id: corr.map(str::to_string),
    }
}

/// A notified call of any tool proposed and started, as the core sends it.
fn ran_notified(r: &mut Renderer, use_id: &str, tool: &str, corr: &str) {
    let rule = format!("{tool} — notify (enforcement = notify)");
    r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": use_id, "tool": tool,
        "input": {"path": "a"}, "gate": {"result": {"gate": "allow"}, "decision": {"posture": "notify",
        "reason": rule, "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": rule}}}}));
    r.on_notification(
        "tool.started",
        &json!({"turn_id": "t1", "tool_use_id": use_id, "correlation_id": corr}),
    );
}

/// The loop's tool message among `ops`: its text and its components.
fn tool_message(ops: &[Op]) -> (String, Buttons) {
    ops.iter()
        .find_map(|o| match o {
            Op::Upsert {
                key,
                content,
                buttons,
            } if key == "t1:L0:tools" => Some((content.clone(), buttons.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no tool message in {ops:?}"))
}

/// Without notice embeds (theseus-sgh), a loop's tool message carries one
/// "Should have asked…" menu: an option per distinct notified tool, each
/// naming its newest notified call there. A tightening puts who tightened
/// the tool on the message and takes its option away, and the last one
/// takes the menu; an undo offers the tool again. At most 25 options.
#[test]
fn the_tool_message_offers_one_menu_of_its_distinct_notified_tools() {
    let mut r = Renderer::default();
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    ran_notified(&mut r, "u1", "proc.run", "act_1");
    ran_notified(&mut r, "u2", "fs.write", "act_2");
    ran_notified(&mut r, "u3", "proc.run", "act_3");
    r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u4", "tool": "fs.read",
        "input": {"path": "b"}, "gate": {"result": {"gate": "allow"}, "decision": {"posture": "open"}}}));
    let (_, buttons) = tool_message(&r.tick());
    assert_eq!(
        buttons,
        Buttons::ShouldHaveAsked(vec![
            asked("proc.run", Some("act_3")),
            asked("fs.write", Some("act_2"))
        ]),
        "the open read is not offered"
    );
    assert!(r.tick().is_empty(), "nothing changed, nothing sent");

    let ops = r.on_notification(
        "policy.tightened",
        &json!({"tool": "proc.run", "by": "discord:zeroaltitude"}),
    );
    let (content, buttons) = tool_message(&ops);
    assert!(
        content.ends_with(
            "\n-# 🔒 `proc.run` asks first from now on: tightened by discord:zeroaltitude"
        ),
        "{content}"
    );
    assert_eq!(
        buttons,
        Buttons::ShouldHaveAsked(vec![asked("fs.write", Some("act_2"))])
    );
    let (content, buttons) = tool_message(&r.on_notification(
        "policy.tightened",
        &json!({"tool": "fs.write", "by": "web#1"}),
    ));
    assert!(
        content.contains("-# 🔒 `fs.write` asks first from now on: tightened by web#1"),
        "{content}"
    );
    assert_eq!(buttons, Buttons::Clear, "nothing left to offer");
    let (content, buttons) = tool_message(&r.on_notification(
        "policy.untightened",
        &json!({"tool": "proc.run", "by": "sock#1"}),
    ));
    assert!(!content.contains("`proc.run` asks first"), "{content}");
    assert_eq!(
        buttons,
        Buttons::ShouldHaveAsked(vec![asked("proc.run", Some("act_3"))])
    );
    assert!(
        r.on_notification("policy.tightened", &json!({"tool": "git.log", "by": "x"}))
            .is_empty(),
        "another tool's news leaves the message alone"
    );

    let mut r = Renderer::default();
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    for i in 0..30 {
        ran_notified(
            &mut r,
            &format!("u{i}"),
            &format!("tool.n{i}"),
            &format!("act_{i}"),
        );
    }
    let (content, buttons) = tool_message(&r.tick());
    assert!(content.len() <= PART_LIMIT);
    let Buttons::ShouldHaveAsked(options) = buttons else {
        panic!("{buttons:?}")
    };
    assert_eq!(options.len(), MAX_ASKED);
    assert_eq!(options[0], asked("tool.n0", Some("act_0")));
}

/// With notice embeds, the tool message has no menu. Each notice card has
/// a "Should have asked" button for its call instead, and after the press
/// the card says who tightened the tool and loses the button; an undo
/// brings the button back.
#[test]
fn with_notice_embeds_each_card_gets_a_button_instead() {
    let mut r = Renderer::new(true);
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    let (proposed, mut notice) = notified("u1", "cargo test");
    notice["correlation_id"] = json!("act_1");
    r.on_notification("tool.proposed", &proposed);
    let ops = r.on_notification("policy.notified", &notice);
    let Op::Notice { card, .. } = &ops[0] else {
        panic!("{ops:?}")
    };
    assert_eq!(card.ask, Some(asked("proc.run", Some("act_1"))));
    r.on_notification(
        "tool.started",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "correlation_id": "act_1"}),
    );
    assert_eq!(
        tool_message(&r.tick()).1,
        Buttons::Keep,
        "no menu with embeds"
    );
    let ops = r.on_notification(
        "policy.tightened",
        &json!({"tool": "proc.run", "by": "discord:zeroaltitude"}),
    );
    let Some(Op::Notice { key, card }) = ops.first() else {
        panic!("{ops:?}")
    };
    assert_eq!((key.as_str(), card.ask.as_ref()), ("t1:notice:u1", None));
    assert_eq!(
        card.fields.last(),
        Some(&(
            "Asks first now".to_string(),
            "🔒 `proc.run` asks first from now on: tightened by discord:zeroaltitude".to_string()
        ))
    );
    let (content, buttons) = tool_message(&ops);
    assert!(
        content.contains("tightened by discord:zeroaltitude"),
        "{content}"
    );
    assert_eq!(buttons, Buttons::Keep);
    let ops = r.on_notification("policy.untightened", &json!({"tool": "proc.run"}));
    let Some(Op::Notice { card, .. }) = ops.first() else {
        panic!("{ops:?}")
    };
    assert_eq!(card.ask, Some(asked("proc.run", Some("act_1"))));
    assert!(card.fields.iter().all(|(n, _)| n != "Asks first now"));
}

/// The Daily Driver's proof (theseus-w4f): thirty notified calls in one
/// loop post one tool message, edited in place as they run, and nothing
/// else; the turn's reply is its only other message. The message keeps
/// the newest lines, each with its notice and outcome, and counts the
/// notices of the lines it folds.
#[test]
fn thirty_notified_calls_post_one_tool_message_edited_in_place() {
    let mut r = Renderer::default();
    assert_eq!(
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"})),
        vec![Op::Typing]
    );
    let calls: Vec<(Value, Value)> = (0..30)
        .map(|i| notified(&format!("u{i}"), &format!("echo {i}")))
        .collect();
    // One model response proposes all thirty; they then run in order.
    let mut ops = vec![];
    for (proposed, _) in &calls {
        ops.extend(r.on_notification("tool.proposed", proposed));
    }
    ops.extend(r.tick());
    for (i, (_, notice)) in calls.iter().enumerate() {
        let id = json!(format!("u{i}"));
        ops.extend(r.on_notification("policy.notified", notice));
        ops.extend(r.on_notification("tool.started", &json!({"turn_id": "t1", "tool_use_id": id})));
        ops.extend(r.tick());
        ops.extend(r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": id, "status": "ok", "duration_ms": 3}),
        ));
        ops.extend(r.tick());
    }
    ops.extend(r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
    assert!(
        ops.iter()
            .all(|o| matches!(o, Op::Upsert { key, .. } if key == "t1:L0:tools")),
        "{ops:?}"
    );
    assert!(
        ops.len() > 30,
        "created once, then edited as the calls run: {ops:?}"
    );
    let Some(Op::Upsert { content, .. }) = ops.last() else {
        unreachable!()
    };
    assert!(content.len() <= PART_LIMIT, "{}", content.len());
    let shown: Vec<&str> = content.lines().skip(1).collect();
    let folded = 30 - shown.len();
    assert!(
        content.starts_with(&format!(
            "-# … {folded} earlier call(s) · 🔔 {folded} notified\n"
        )),
        "{content}"
    );
    for (line, i) in shown.iter().zip(folded..) {
        assert_eq!(
            *line,
            format!("✅ `proc.run` echo {i} · 🔔 notified (enforcement = notify) · 3 ms")
        );
    }
    r.on_notification(
        "model.delta",
        &json!({"turn_id": "t1", "loop_index": 1, "text": "All thirty ran."}),
    );
    let mut end = ended("t1", None);
    end["tool_calls"] = json!(30);
    // The reply is a post (theseus-q4v): the stream sends nothing more,
    // and the post is one message.
    assert!(upserts(&r.on_notification("turn.ended", &end)).is_empty());
    let result: TurnSubmitResult = serde_json::from_value(end).unwrap();
    let keys: Vec<String> = reply_parts("t1", &[(1, "All thirty ran.".into())], Some(&result))
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(keys, ["t1:L1:p0"]);
}

fn waiting_write() -> ConfirmRequest {
    request(json!({"correlation_id": "act_1", "session_id": "s",
        "execution_id": "e", "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "reason": "run cargo test",
        "by": "operator", "requested_at_ms": 1, "expires_at_ms": 1790000000000u64}))
}

fn to_dm(why: &str) -> Route {
    Route::Dm {
        user: 271828182845904523,
        dm: "DM @zeroaltitude".into(),
        place: "#general".into(),
        why: why.into(),
    }
}

/// A shared place (the place rule, theseus-zmgb) sends its card to
/// the trusted user's DM and says so in one line here; the answer settles
/// the card in the DM and the note here.
#[test]
fn a_card_for_a_shared_place_goes_to_the_dm_with_a_note_here() {
    let route = to_dm("this channel is shared, and an answer counts only from a private place");
    let c = card(&waiting_write(), &route, "with `theseus answer`");
    assert!(
        c.content
            .starts_with("**Approve?** `proc.run` cargo test\nrun cargo test\n"),
        "{}",
        c.content
    );
    assert!(
        c.content.ends_with(
            "-# for #general · expires <t:1790000000:R> · you can also answer with `theseus answer`"
        ),
        "{}",
        c.content
    );
    assert_eq!(
        card_note(&route, &c.line, "with `theseus answer`").unwrap(),
        "🔐 Approval for `proc.run` cargo test was asked in DM @zeroaltitude: this channel is shared, \
         and an answer counts only from a private place."
    );
    let done = settled(
        &Closed::new("approved", Some("discord:zeroaltitude")),
        &c.line,
        false,
    );
    assert_eq!(
        done,
        "✅ **Approved** by discord:zeroaltitude · `proc.run` cargo test"
    );
    assert_eq!(
        settled_note(&done, Some("DM @zeroaltitude")),
        "🔐 ✅ **Approved** by discord:zeroaltitude · `proc.run` cargo test (in DM @zeroaltitude)"
    );
    // The tool line here says who approved it, as it does for a card here.
    let mut r = Renderer::default();
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
        "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
    r.on_notification(
        "confirm.requested",
        &serde_json::to_value(waiting_write()).unwrap(),
    );
    r.on_notification(
        "confirm.resolved",
        &json!({"correlation_id": "act_1", "approved": true, "by": "discord:zeroaltitude"}),
    );
    let tools = upserts(&r.tick());
    assert!(
        tools[0].1.contains("approved by discord:zeroaltitude"),
        "{tools:?}"
    );
}

/// A call whose execution was cancelled before an answer reads as not
/// run, never as declined: no one declined it (theseus-w98).
#[test]
fn a_waiting_call_its_cancel_closed_reads_as_not_run() {
    let mut r = Renderer::default();
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
        "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
    r.on_notification(
        "confirm.requested",
        &serde_json::to_value(waiting_write()).unwrap(),
    );
    r.on_notification(
        "confirm.resolved",
        &json!({"correlation_id": "act_1", "approved": false, "cancelled": true, "by": "cli"}),
    );
    let tools = upserts(&r.tick());
    assert!(tools[0].1.contains("· not run"), "{tools:?}");
    assert!(!tools[0].1.contains("declined"), "{tools:?}");
}

/// A call a `/stop` ended reads `⏹️ … stopped by <who>`, as the stop's own
/// card says, never `❌ … cancelled`, and apart from a cancel's `🚫 … not
/// run` (theseus-4uw): one that ran (its result says who stopped it),
/// and one that waited for approval (the stop's `confirm.resolved`), whose
/// not-run result keeps the line. A notice card says the same.
#[test]
fn a_call_a_stop_ended_reads_as_stopped_not_failed() {
    let mut r = Renderer::new(true);
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
        "input": {"argv": ["sleep", "30"]}, "gate": {"result": {"gate": "allow"},
        "decision": {"mode": "allow", "posture": "notify", "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}}}}));
    r.on_notification("policy.notified", &json!({"session_id": "s", "turn_id": "t1", "tool_use_id": "u1",
        "tool": "proc.run", "input": {"argv": ["sleep", "30"]}, "summary": "run `sleep 30` in /w",
        "kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}));
    r.on_notification(
        "tool.started",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "correlation_id": "act_9"}),
    );
    let ops = r.on_notification(
        "tool.ended",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "cancelled", "duration_ms": 2100,
                "stopped_by": "discord:zeroaltitude", "verified": "verified: process tree, 2 processes"}),
    );
    let Op::Notice { card, .. } = &ops[0] else {
        panic!("{ops:?}")
    };
    assert_eq!(
        card.fields[2].1,
        "⏹️ stopped by discord:zeroaltitude (verified) · 2100 ms"
    );
    // A waiting call the stop declined.
    r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u2", "tool": "fs.write",
        "input": {"path": "a.txt", "content": "x"}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
    let write = request(json!({"correlation_id": "act_2", "session_id": "s",
        "execution_id": "e", "tool": "fs.write", "input": {"path": "a.txt", "content": "x"},
        "reason": "write a.txt", "by": "operator", "requested_at_ms": 1,
        "expires_at_ms": 1790000000000u64}));
    r.on_notification("confirm.requested", &serde_json::to_value(write).unwrap());
    r.on_notification(
        "confirm.resolved",
        &json!({"correlation_id": "act_2", "approved": false, "stopped": true, "by": "discord:zeroaltitude"}),
    );
    r.on_notification(
        "tool.ended",
        &json!({"turn_id": "t1", "tool_use_id": "u2", "status": "cancelled", "duration_ms": 0}),
    );
    let lines =
        upserts(&r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
    let text = &lines[0].1;
    assert!(
        text.contains("⏹️ `proc.run` sleep 30 · 🔔 notified (enforcement = notify) · stopped by discord:zeroaltitude (verified)"),
        "{text}"
    );
    assert!(
        text.contains("⏹️ `fs.write` a.txt · stopped by discord:zeroaltitude"),
        "{text}"
    );
    assert!(
        !text.contains("❌") && !text.contains("cancelled") && !text.contains("declined"),
        "{text}"
    );
}

/// A call approved after its turn parked runs in the next turn, whose
/// events name that turn: its line, in the turn that proposed it, still
/// says how it ran and ended (here a stop), not `👍 approved` forever
/// (theseus-4uw's live check found it so).
#[test]
fn an_approved_call_that_runs_in_a_later_turn_updates_its_own_line() {
    let mut r = Renderer::default();
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
        "input": {"argv": ["sleep", "30"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
    let sleep = request(json!({"correlation_id": "act_3", "session_id": "s",
        "execution_id": "e", "tool": "proc.run", "input": {"argv": ["sleep", "30"]},
        "reason": "run sleep 30", "by": "operator", "requested_at_ms": 1,
        "expires_at_ms": 1790000000000u64}));
    r.on_notification("confirm.requested", &serde_json::to_value(sleep).unwrap());
    r.on_notification("turn.ended", &json!({"session_id": "s", "turn_id": "t1"}));
    r.on_notification(
        "confirm.resolved",
        &json!({"correlation_id": "act_3", "approved": true, "by": "the CLI"}),
    );
    let line = |r: &mut Renderer| upserts(&r.tick())[0].1.clone();
    assert!(
        line(&mut r).contains("👍 `proc.run` sleep 30 · approved by the CLI"),
        "the answer shows first"
    );
    // The continuation runs it.
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t2"}));
    r.on_notification(
        "tool.started",
        &json!({"turn_id": "t2", "tool_use_id": "u1", "correlation_id": "act_3"}),
    );
    assert!(
        line(&mut r).contains("⏳ `proc.run` sleep 30"),
        "it runs, on its own line"
    );
    r.on_notification(
        "tool.ended",
        &json!({"turn_id": "t2", "tool_use_id": "u1", "status": "cancelled",
                "duration_ms": 7853, "stopped_by": "the CLI"}),
    );
    let ended = line(&mut r);
    assert!(
        ended.contains("⏹️ `proc.run` sleep 30 · stopped by the CLI"),
        "{ended}"
    );
}

/// A budget question takes the same route as a tool call.
#[test]
fn a_budget_question_for_a_shared_place_goes_to_the_dm_too() {
    let route = Route::Dm {
        user: 7,
        dm: "DM @zeroaltitude".into(),
        place: "#general".into(),
        why: "this channel is shared, and an answer counts only from a private place".into(),
    };
    let req = request(json!({"correlation_id": "act_b", "session_id": "s",
        "execution_id": "e", "tool": "budget.reset", "input": {}, "by": "operator", "requested_at_ms": 1,
        "expires_at_ms": 0, "reason": "This session has spent $99.48 of its $100 limit. Reset its spend to $0 and continue?",
        "budget": {"spent_usd": 99.48, "limit_usd": 100.0, "needed_usd": 1.0, "lifetime_usd": 212.4}}));
    let c = card(&req, &route, ELSEWHERE);
    assert!(
        c.content
            .contains("\n-# For #general. Approve resets its spend"),
        "{}",
        c.content
    );
    assert!(
        c.content
            .ends_with("You can also answer in the web UI or with `theseus answer`."),
        "{}",
        c.content
    );
    let said = card_note(&route, &c.line, ELSEWHERE).unwrap();
    assert!(
        said.starts_with(
            "🔐 Approval for spend reset ($99.48 of the $100 limit) was asked in DM @zeroaltitude"
        ),
        "{said}"
    );
}

/// Shared, and no DM with an owner: no card on Discord, only a note that
/// says where to answer (or that nowhere here can).
#[test]
fn with_no_owners_dm_the_place_gets_only_a_note() {
    for (elsewhere, tail) in [
        (
            "in the web UI or with `theseus answer`",
            ": answer in the web UI or with `theseus answer`.",
        ),
        ("", ", and no DM with an owner is bound here to answer it."),
    ] {
        let route = Route::Elsewhere {
            why: "this channel is shared, and an answer counts only from a private place".into(),
        };
        let c = card(&waiting_write(), &route, elsewhere);
        assert_eq!(
            card_note(&route, &c.line, elsewhere).unwrap(),
            format!(
                "🔐 `proc.run` cargo test waits for approval: this channel is shared, and an \
                 answer counts only from a private place{tail}"
            )
        );
        let done = settled(&Closed::new("declined", Some("sock#3")), &c.line, false);
        assert_eq!(
            settled_note(&done, None),
            "🔐 ❎ **Declined** by sock#3 · `proc.run` cargo test"
        );
    }
    assert!(card_note(&Route::Here, "x", ELSEWHERE).is_none());
}

/// A card names where else it can be answered: the owner's own surfaces,
/// the CLI always and the web UI when it is on; with none named, none.
#[test]
fn a_card_names_where_else_it_can_be_answered() {
    let text = |elsewhere: &str| {
        card(&waiting_write(), &Route::Here, elsewhere)
            .content
            .lines()
            .last()
            .unwrap()
            .to_string()
    };
    assert_eq!(
        text(ELSEWHERE),
        "-# expires <t:1790000000:R> · you can also answer in the web UI or with `theseus answer`"
    );
    assert_eq!(
        text("in the web UI"),
        "-# expires <t:1790000000:R> · you can also answer in the web UI"
    );
    assert_eq!(text(""), "-# expires <t:1790000000:R>");
}

#[test]
fn a_floor_confirm_says_so_and_names_the_reason() {
    let reason = "read /home/x/.openclaw-1password-service-token: fs.read — approve (floor: /home/x/.openclaw-1password-service-token is Theseus's own state or the 1Password token)";
    let c = card(
        &request(json!({"correlation_id": "act_9",
        "session_id": "s", "execution_id": "e", "tool": "fs.read",
        "input": {"path": "~/.openclaw-1password-service-token"}, "reason": reason, "by": "operator",
        "requested_at_ms": 0, "expires_at_ms": 60_000, "floor": true})),
        &Route::Here,
        ELSEWHERE,
    );
    assert!(c.content.starts_with(FLOOR_ASK), "{}", c.content);
    assert!(
        c.content.contains("fs.read — approve (floor: "),
        "{}",
        c.content
    );
    assert!(c.line.ends_with("(floor)"), "{}", c.line);
}

/// A turn the stream never saw (a continuation that ended before the
/// binding was back) is its reply's post alone: its text and footer.
#[test]
fn a_turn_seen_only_at_its_end_is_its_reply() {
    let mut r = Renderer::default();
    let mut end = ended("t9", None);
    end["continuation"] = json!(true);
    assert!(r.on_notification("turn.ended", &end).is_empty());
    assert!(!r.busy());
    let parts = reply_parts(
        "t9",
        &[(0, "Done. The command printed `done`.".into())],
        Some(&result_of(end)),
    );
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].0, "t9:L0:p0");
    assert!(
        parts[0]
            .1
            .starts_with("Done. The command printed `done`.\n-# sonnet"),
        "{}",
        parts[0].1
    );
    assert!(parts[0].1.contains("continued"));
}

/// How a card reads when its question closed without an answer.
#[test]
fn a_card_closed_without_an_answer_says_what_closed_it() {
    assert_eq!(
        settled(&Closed::new("superseded", None), "`fs.write` a", false),
        "⏭️ **Not run**: a new message replaced this request. `fs.write` a"
    );
    assert_eq!(
        settled(&Closed::new("ended", None), "`fs.write` a", false),
        "⏹️ **Closed**: the session's work ended · `fs.write` a"
    );
    // `/stop` (W1): the question closed, and the conversation goes on.
    assert_eq!(
        settled(
            &Closed::new("stopped", Some("discord:zeroaltitude")),
            "`fs.write` a",
            false
        ),
        "⏹️ **Not run**: discord:zeroaltitude stopped this session's work · `fs.write` a"
    );
    let other = Closed {
        how: "closed".into(),
        by: None,
        note: Some("its question is no longer in the store".into()),
    };
    assert_eq!(
        settled(&other, "`fs.write` a", false),
        "⏹️ **Closed**: its question is no longer in the store · `fs.write` a"
    );
}

#[test]
fn a_declined_call_keeps_saying_who_declined_it() {
    // `denied` is the status name before theseus-8az.
    for status in ["declined", "denied"] {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "fs.read",
            "input": {"path": "/etc/hosts"}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        r.on_notification("confirm.requested", &json!({"correlation_id": "act_9",
            "session_id": "s", "execution_id": "e", "tool": "fs.read", "input": {"path": "/etc/hosts"},
            "reason": "read /etc/hosts: fs.read — approve (/etc/hosts is protected: /etc is on the approve list)",
            "by": "operator", "requested_at_ms": 0, "expires_at_ms": 60_000}));
        r.on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_9", "approved": false, "by": "zeroaltitude"}),
        );
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": status, "duration_ms": 0}),
        );
        let ops = upserts(&r.on_notification("turn.ended", &ended("t1", None)));
        assert!(
            ops.contains(&(
                "t1:L0:tools".into(),
                "👎 `fs.read` /etc/hosts · declined by zeroaltitude".into()
            )),
            "{status}: {ops:?}"
        );
    }
}

#[test]
fn a_call_that_never_ran_and_a_failed_turn_say_so() {
    // `denied` is the status name before theseus-8az.
    for status in ["declined", "denied"] {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "fs.read",
            "input": {"path": "~/.ssh/config"}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": status, "duration_ms": 0}),
        );
        let ops = upserts(&r.on_notification(
            "turn.failed",
            &json!({"session_id": "s", "turn_id": "t1", "class": "overloaded", "error": "try later"}),
        ));
        assert!(
            ops.contains(&(
                "t1:L0:tools".into(),
                "🚫 `fs.read` ~/.ssh/config · not run".into()
            )),
            "{status}: {ops:?}"
        );
        // The failure itself is a post (theseus-q4v).
        assert_eq!(ops.len(), 1, "{ops:?}");
        assert!(!r.busy());
    }
    assert_eq!(
        failed("overloaded", "try later", None, 1),
        "⚠️ **Turn failed** (overloaded): try later"
    );
    // A run of failures says what follows (theseus-ljr).
    assert!(failed("overloaded", "try later", Some("backoff"), 1)
        .ends_with("Retrying with backoff while it lasts. No more notices for it: the reply comes when the provider answers."));
    assert!(failed("invalid_request", "bad", Some("park"), 2)
        .ends_with("Failed 2 times in a row, so nothing retries it now: your next message does."));
    assert!(failed("over_limit", "too big", Some("park"), 1)
        .ends_with("\n-# Nothing retries it now: your next message does."));
    assert!(failed("stopping", "not sent", Some("backoff"), 1)
        .ends_with("\n-# Not sent as the daemon stopped: its next start retries it."));
}

#[test]
fn long_text_splits_under_the_limit_and_repairs_fences() {
    let code: String = (0..300).map(|i| format!("let x{i} = {i};\n")).collect();
    let text = format!("Here:\n```rust\n{code}```\nThat is all.");
    let parts = split_text(&text, PART_LIMIT);
    assert!(parts.len() > 1);
    for p in &parts {
        assert!(p.len() <= DISCORD_LIMIT, "{}", p.len());
        assert_eq!(
            p.matches("```").count() % 2,
            0,
            "each part's fences balance: {p}"
        );
    }
    assert!(parts[1].starts_with("```rust\n"));
    // Appending never moves an earlier cut.
    let longer = split_text(&format!("{text} More."), PART_LIMIT);
    assert_eq!(parts[..parts.len() - 1], longer[..parts.len() - 1]);
}

#[test]
fn summaries_pick_the_argument_a_person_looks_for() {
    assert_eq!(
        summarize("proc.run", &json!({"argv": ["cargo", "test", "-q"]})),
        "cargo test -q"
    );
    assert_eq!(
        summarize("fs.grep", &json!({"pattern": "fn main", "path": "src"})),
        "fn main in src"
    );
    assert_eq!(summarize("fs.read", &json!({"path": "a`b"})), "a'b");
    assert!(summarize("fs.write", &json!({"path": "x", "content": "secret body"})) == "x");
    // The web tools (DD5): the URL, or the quoted query.
    assert_eq!(
        summarize(
            "http.fetch",
            &json!({"url": "https://doc.rust-lang.org/std/", "max_bytes": 4096})
        ),
        "https://doc.rust-lang.org/std/"
    );
    assert_eq!(
        summarize(
            "web.search",
            &json!({"query": "ignore WalkParallel", "count": 3})
        ),
        "\"ignore WalkParallel\""
    );
    // `aws.describe` (row 29, C1): the service, and its operation.
    assert_eq!(
        summarize(
            "aws.describe",
            &json!({"service": "s3", "operation": "ListObjectsV2"})
        ),
        "s3:ListObjectsV2"
    );
    assert_eq!(summarize("aws.describe", &json!({"service": "ec2"})), "ec2");
}

/// An AWS call's tool line (row 29, C1) names its operation, region, and
/// account, and the resources its input gives, from its plan, whatever
/// its input says.
#[test]
fn an_aws_calls_line_names_its_operation_region_and_account() {
    let mut r = Renderer::default();
    r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
    r.on_notification(
        "tool.proposed",
        &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "aws.s3.list",
            "input": {"path": "s3://example-bucket/logs/"},
            "gate": {"result": {"gate": "allow"}, "decision": {"posture": "open"},
                "plan": {"resources": [], "summary": "list s3://example-bucket/logs/ in us-west-2",
                    "class": "read",
                    "aws": {"account": "111122223333", "region": "us-west-2", "service": "s3",
                        "operation": "ListObjectsV2", "resources": ["example-bucket", "logs/"]}}}}),
    );
    let (content, _) = tool_message(&r.tick());
    assert!(
        content.contains(
            "`aws.s3.list` s3:ListObjectsV2 · us-west-2 · account 111122223333 · example-bucket, logs/"
        ),
        "{content}"
    );
}

/// A glide's post (38b) is its text with the line naming its session; a
/// long one is parts under the limit, the line on the last.
#[test]
fn a_glides_post_names_its_session_on_its_last_part() {
    let post = |text: &str| glide(&json!({"kind": "glide", "text": text, "session": "a1b2c3"}));
    assert_eq!(
        post("Deployed v2.\n"),
        ["Deployed v2.\n-# ↪ posted from Theseus session `a1b2c3`"]
    );
    let parts = post(&"word ".repeat(700));
    assert_eq!(parts.len(), 2, "{parts:?}");
    assert!(parts.iter().all(|p| p.chars().count() <= DISCORD_LIMIT));
    assert!(parts[1].ends_with("-# ↪ posted from Theseus session `a1b2c3`"));
}

/// A task's report (DD7) is one message that names the task: a long last
/// message is clipped to fit, and a cancel or a failure says so.
#[test]
fn a_tasks_report_is_one_message_that_names_it() {
    let body = |outcome: &str, reason: Option<&str>| {
        json!({"kind": "report", "task": "ses_x", "short": "a1b2c3", "title": "Run the gate",
               "outcome": outcome, "reason": reason, "spent_usd": 0.0123, "limit_usd": 2.5,
               "turns": 3, "elapsed_ms": 64_000})
    };
    let done = report(&body("complete", None), Some("The gate passed."), 0, None);
    assert_eq!(
        done,
        "📋 **Task `a1b2c3` finished** · Run the gate\nThe gate passed.\n\
         -# 3 turns · $0.0123 of $2.50 · 1 min 4 s"
    );
    let long = report(
        &body("complete", None),
        Some(&"word ".repeat(2_000)),
        0,
        None,
    );
    assert!(long.chars().count() <= DISCORD_LIMIT, "{}", long.len());
    assert!(long.ends_with("-# 3 turns · $0.0123 of $2.50 · 1 min 4 s"));
    let stopped = report(
        &body("cancelled", Some("cancelled by discord:zeroaltitude")),
        None,
        0,
        None,
    );
    assert!(
        stopped.starts_with(
            "⏹️ **Task `a1b2c3` stopped** · Run the gate: cancelled by discord:zeroaltitude\n-# 3 turns"
        ),
        "{stopped}"
    );
    let failed = report(
        &body("failed", Some("provider_overloaded (529)")),
        None,
        0,
        None,
    );
    assert!(
        failed.starts_with("⚠️ **Task `a1b2c3` failed** · Run the gate: provider_overloaded"),
        "{failed}"
    );
    // A check's report carries its basis (M5 28a).
    let mut checked = body("complete", None);
    checked["check"] =
        json!("🔍 check of task d4e5f6 · independent (excluded ses_…d4e5f6, glm-4.6)");
    assert_eq!(
        report(&checked, Some("It holds."), 0, None),
        "📋 **Task `a1b2c3` finished** · Run the gate\n\
         -# 🔍 check of task d4e5f6 · independent (excluded ses_…d4e5f6, glm-4.6)\nIt holds.\n\
         -# 3 turns · $0.0123 of $2.50 · 1 min 4 s"
    );
}

/// A task that works in a forum post or a channel of its own has its
/// mention on the report's first line, so a tap opens it; one with
/// neither reports as before (theseus-ypnxt.5).
#[test]
fn a_report_links_the_tasks_own_post_or_channel() {
    let body = json!({"kind": "report", "task": "ses_x", "short": "a1b2c3",
        "title": "Run the gate", "outcome": "complete", "spent_usd": 0.0, "limit_usd": 2.0,
        "turns": 1, "elapsed_ms": 1_000});
    let plain = report(&body, None, 0, None);
    let linked = report(&body, None, 0, Some(900_000_000_000_500_007));
    assert!(
        linked
            .starts_with("📋 **Task `a1b2c3` finished** · Run the gate · <#900000000000500007>\n"),
        "{linked}"
    );
    assert_eq!(linked.replace(" · <#900000000000500007>", ""), plain);
    assert!(!plain.contains("<#"));
}

/// A card's reason past 300 characters loses its middle, not its end, which
/// says why: a fetch of a private address from a shared place keeps its
/// clause behind a long URL (theseus-94a6). A short reason is whole.
#[test]
fn a_long_reason_keeps_its_end_on_the_card() {
    let url = format!("http://10.0.0.5:8080/reports?{}", "tide=high&".repeat(20));
    let why = "http.fetch — approve (10.0.0.5 is a private address, and a private address \
               waits for approval; this is a shared place, so the page joins a conversation \
               others can read)";
    let req = |reason: &str| {
        request(json!({
            "correlation_id": "act_1", "session_id": "ses_x", "execution_id": "exe_x",
            "tool": "http.fetch", "input": {"url": url}, "reason": reason,
            "by": "operator", "requested_at_ms": 1, "expires_at_ms": 60_000
        }))
    };
    let long = format!("fetch {url}: {why}");
    assert!(long.chars().count() > 300);
    let shown = card(&req(&long), &Route::Here, "").content;
    let reason = shown.lines().nth(1).unwrap();
    assert_eq!(reason.chars().count(), 300, "{reason}");
    assert!(
        reason.starts_with("fetch http://10.0.0.5:8080/reports?"),
        "{reason}"
    );
    assert!(reason.contains('…'), "{reason}");
    assert!(reason.ends_with(why), "the end, which says why: {reason}");
    let short = "fetch http://10.0.0.5/: http.fetch — approve (10.0.0.5 is a private address)";
    let whole = card(&req(short), &Route::Here, "").content;
    assert_eq!(whole.lines().nth(1), Some(short));
}

/// A task's question names the task on its card, and so on its settle.
#[test]
fn a_tasks_card_names_the_task() {
    let req = request(json!({
        "correlation_id": "act_1", "session_id": "ses_x", "execution_id": "exe_x",
        "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "reason": "run cargo test",
        "by": "operator", "requested_at_ms": 1, "expires_at_ms": 60_000,
        "task": {"task_id": "ses_xa1b2c3", "short": "a1b2c3", "title": "Run the gate"}
    }));
    let card = card(&req, &Route::Here, "");
    assert!(
        card.content
            .contains("task `a1b2c3`: `proc.run` cargo test"),
        "{}",
        card.content
    );
    assert_eq!(card.line, "task `a1b2c3`: `proc.run` cargo test");
}

/// `/tasks` (DD7): each task by its short id, with its state, what it
/// waits on, its spend of its limit, its age, and its title.
#[test]
fn slash_tasks_lists_each_with_state_and_spend() {
    let t =
        |short: &str, state: &str, waiting: Option<&str>, asks: u32| theseus_protocol::TaskInfo {
            task_id: format!("ses_{short}"),
            short: short.into(),
            title: Some("Run the gate".into()),
            state: state.into(),
            waiting_on: waiting.map(str::to_string),
            spent_usd: 0.0123,
            limit_usd: 2.5,
            pending_confirms: asks,
            created_at_ms: 1_000,
            ..Default::default()
        };
    let text = tasks(
        &[
            t("a1b2c3", "waiting", Some("an approval"), 1),
            t("d4e5f6", "complete", None, 0),
        ],
        181_000,
    );
    assert_eq!(
        text,
        "**Tasks here** (2)\n\
         • `a1b2c3` waiting on an approval · **needs you** · $0.0123 of $2.50 · 3 min · Run the gate\n\
         • `d4e5f6` complete · $0.0123 of $2.50 · 3 min · Run the gate\n\
         -# `/cancel <task>` stops one."
    );
    assert!(tasks(&[], 0).starts_with("No tasks here yet"));
}

/// A wake's turn's reply opens with the wake's line, on its first part;
/// a part with no room left keeps its text.
#[test]
fn a_wakes_reply_opens_with_the_wakes_line() {
    let mut parts = vec![
        ("t1:L0:p0".to_string(), "The build is green.".to_string()),
        ("t1:footer".to_string(), "-# glm".to_string()),
    ];
    wake_header(
        &mut parts,
        &["⏰ wake (set 13:05): check the build".to_string()],
    );
    assert_eq!(
        parts[0].1,
        "-# ⏰ wake (set 13:05): check the build\nThe build is green."
    );
    assert_eq!(parts[1].1, "-# glm");
    let full = "x".repeat(1_990);
    let mut parts = vec![("t2:L0:p0".to_string(), full.clone())];
    wake_header(&mut parts, &["⏰ wake (set 13:05): n".to_string()]);
    assert_eq!(parts[0].1, full, "no room: the text as it was");
    let mut none = vec![("t3:L0:p0".to_string(), "hi".to_string())];
    wake_header(&mut none, &[]);
    assert_eq!(none[0].1, "hi");
}

/// `f`'s value, or `None` if it has not returned within `secs`: a split
/// that stops moving through its text never returns, and a test of it
/// must fail, not hang.
fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()
}

/// A budget smaller than the next character, or a carried fence line as
/// long as the limit, left no room to cut: the cut fell at 0, and the
/// split pushed empty parts forever without moving (theseus-s68, found by
/// the property test below). Each part now moves the text on, and a
/// reopened fence is at most `MAX_FENCE` bytes.
#[test]
fn a_split_always_moves_through_multibyte_text_and_long_fences() {
    let parts = within(2, || split_text("中", 4)).expect("split_text(\"中\", 4) returned");
    assert_eq!(parts.concat(), "中");
    let fence = format!("```{}", "x".repeat(PART_LIMIT));
    let text = format!("{fence}\n{}", "中".repeat(1000));
    let parts =
        within(2, move || split_text(&text, PART_LIMIT)).expect("the long fence's split returned");
    for p in &parts {
        assert!(p.len() <= PART_LIMIT, "{}", p.len());
    }
    let kept: usize = parts.iter().map(|p| p.matches('中').count()).sum();
    assert_eq!(kept, 1000, "every character is in some part, once");
}

fn fenced() -> impl proptest::strategy::Strategy<Value = String> {
    use proptest::prelude::*;
    let piece = prop_oneof![
        3 => proptest::sample::select(vec![
            "```", "```rust", "\n", " ", "中", "😀", "é", "a", "\n```\n", "  ```py\n",
        ])
        .prop_map(str::to_string),
        1 => any::<String>(),
        1 => "x{0,300}",
    ];
    proptest::collection::vec(piece, 0..40).prop_map(|v| v.concat())
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        cases: 1500,
        failure_persistence: None,
        ..proptest::test_runner::Config::default()
    })]

    /// Any text splits, at any limit: every part moves the text on, a
    /// part is never empty, it fits a real limit, and appending text
    /// never moves an earlier cut.
    #[test]
    fn any_text_splits_and_keeps_its_earlier_cuts(
        text in fenced(),
        more in fenced(),
        limit in proptest::prop_oneof![1usize..64, 64usize..600, proptest::strategy::Just(PART_LIMIT)],
    ) {
        let split = within(5, move || {
            (split_text(&text, limit), split_text(&format!("{text}{more}"), limit))
        });
        proptest::prop_assert!(split.is_some(), "split_text did not return (limit {})", limit);
        let (parts, longer) = split.unwrap();
        proptest::prop_assert!(parts.iter().all(|p| !p.is_empty()));
        if limit >= 64 {
            for p in &parts {
                proptest::prop_assert!(p.len() <= limit, "a part of {} bytes, over {}", p.len(), limit);
            }
        }
        if parts.len() > 1 {
            proptest::prop_assert_eq!(&parts[..parts.len() - 1], &longer[..parts.len() - 1]);
        }
    }
}

/// A fence line of any length, up to three times the limit, with no space
/// or newline in it (a fence whose language is minified JSON or base64),
/// then a body with no newline, of ASCII and multibyte characters: the
/// cases Review 2's R5 named. A fence's word is `j`s and a body never
/// holds one, so each body character can be counted across the parts.
fn adversarial() -> impl proptest::strategy::Strategy<Value = (String, String, usize)> {
    use proptest::prelude::*;
    let limit = prop_oneof![1usize..64, 64usize..600, Just(PART_LIMIT)];
    limit.prop_flat_map(|limit| {
        let fence = (0..=3 * limit).prop_map(|n| format!("```{}", "j".repeat(n)));
        // No backquote: a body line that began a fence would be carried
        // too, and its characters counted twice.
        let body = proptest::collection::vec(
            proptest::sample::select(vec!["x", "中", "é", "😀"]),
            0..2 * limit.min(800),
        )
        .prop_map(|v| v.concat());
        (fence, body, Just(limit))
    })
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        cases: 400,
        failure_persistence: None,
        ..proptest::test_runner::Config::default()
    })]

    /// Long fences, a fence longer than the limit, and no newlines: the
    /// split returns, every part is whole and fits a real limit, and every
    /// body character is in some part, once (R5, theseus-xonq).
    #[test]
    fn a_long_fence_with_no_newline_still_splits_and_keeps_every_character(
        (fence, body, limit) in adversarial(),
    ) {
        let text = format!("{fence}\n{body}");
        let split = within(5, move || split_text(&text, limit));
        proptest::prop_assert!(split.is_some(), "split_text did not return (limit {})", limit);
        let parts = split.unwrap();
        proptest::prop_assert!(parts.iter().all(|p| !p.is_empty()));
        if limit >= 64 {
            for p in &parts {
                proptest::prop_assert!(p.len() <= limit, "a part of {} bytes, over {}", p.len(), limit);
            }
        }
        for c in ['x', '中', 'é', '😀'] {
            let want = body.matches(c).count();
            let got: usize = parts.iter().map(|p| p.matches(c).count()).sum();
            proptest::prop_assert_eq!(got, want, "{:?} kept once each", c);
        }
    }
}
