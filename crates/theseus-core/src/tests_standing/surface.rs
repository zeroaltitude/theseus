//! A record scoped to a surface (theseus-slvl.4), through the whole core: it
//! rides the user turn's tail on turns whose input came through that surface
//! and nowhere else, so the system and the tools are byte-identical across
//! surfaces; a shared place gets the same privacy rule there; a request that
//! drops it is refused; and the `context.compiled` row names what the tail
//! carried.

use super::*;

const CODE_FIRST: &str = "code first, no pleasantries";

/// A turn whose input came through `surface`.
async fn turn_on(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
    surface: Option<&'static str>,
) -> anyhow::Result<()> {
    let rec = core.store.get_session::<SessionRecord>(sid)?.unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None)?;
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "the CLI".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .map(|_| ())
}

/// Every text in the request's last message: where the tail's blocks are.
fn tail_text(req: &ProviderRequest) -> String {
    let last = req.messages.last().unwrap();
    match &last["content"] {
        Value::String(s) => s.clone(),
        c => c
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn prefix_bytes(req: &ProviderRequest) -> String {
    serde_json::to_string(&(&req.system, &req.tools)).unwrap()
}

fn terminal(r: &Rig, kind: StandingKind, text: &str) -> String {
    r.add(kind, StandingScope::Surface("terminal".into()), text)
}

/// The scope names a surface the daemon knows, and a list for a session asks
/// as a surface or sees none of them; a list for no session shows them all.
#[test]
fn a_surface_scope_is_added_for_a_known_surface_and_listed_by_it() {
    let r = rig();
    let id = terminal(&r, StandingKind::Preference, CODE_FIRST);
    let e = r
        .core
        .standing_add(
            &StandingAddParams {
                text: "short".into(),
                scope: StandingScope::Surface("phone".into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap_err();
    assert!(
        e.message.contains("no surface") && e.message.contains("terminal"),
        "{e:?}"
    );

    let sid = session(&r.core, None);
    let ids = |surface: Option<&str>, session: Option<&str>| -> Vec<String> {
        r.core
            .standing_list(
                &StandingListParams {
                    session_id: session.map(Into::into),
                    surface: surface.map(Into::into),
                    ..Default::default()
                },
                "the CLI",
            )
            .unwrap()
            .records
            .into_iter()
            .map(|i| i.record.id)
            .collect()
    };
    assert_eq!(
        ids(None, None),
        std::slice::from_ref(&id),
        "no session: every record"
    );
    assert_eq!(ids(Some("terminal"), Some(&sid)), std::slice::from_ref(&id));
    assert!(ids(Some("discord"), Some(&sid)).is_empty());
    assert!(
        ids(None, Some(&sid)).is_empty(),
        "a session's scope with no surface has none"
    );
    assert_eq!(
        StandingScope::Surface("terminal".into()).words(),
        "surface:terminal"
    );
}

/// The record is in the tail of a terminal turn, after the person's words,
/// and in no system block; a discord turn of the same session carries none
/// of it, and the system and the tools are byte-identical across the two,
/// with no recompile between them.
#[tokio::test]
async fn a_surface_record_rides_the_tail_of_its_surface_and_the_prefix_stays_identical() {
    let r = rig();
    let sid = session(&r.core, None);
    let id = terminal(&r, StandingKind::Preference, CODE_FIRST);
    let order = r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);

    turn_on(&r.core, &sid, "one", Some("terminal"))
        .await
        .unwrap();
    let on_terminal = r.last_request();
    let tail = tail_text(&on_terminal);
    assert!(
        tail.contains(&format!(
            "Standing here:\nPreferences:\n- {id}: {CODE_FIRST}"
        )),
        "{tail}"
    );
    assert!(
        tail.find("one").unwrap() < tail.find("Standing here:").unwrap(),
        "{tail}"
    );
    let system = system_text(&on_terminal);
    assert!(
        !system.contains(&id) && !system.contains(CODE_FIRST),
        "{system}"
    );
    assert!(
        system.contains(&format!("- {order}: {NO_PUSH}")),
        "{system}"
    );
    // The tail's block is the last block, past the breakpoint.
    let last = on_terminal.messages.last().unwrap()["content"]
        .as_array()
        .unwrap()
        .clone();
    assert!(last.last().unwrap()["cache_control"].is_null(), "{last:?}");

    turn_on(&r.core, &sid, "two", Some("discord"))
        .await
        .unwrap();
    let on_discord = r.last_request();
    assert!(
        !tail_text(&on_discord).contains("Standing here:"),
        "{}",
        tail_text(&on_discord)
    );
    assert!(!serde_json::to_string(&on_discord)
        .unwrap()
        .contains(CODE_FIRST));
    assert_eq!(prefix_bytes(&on_terminal), prefix_bytes(&on_discord));
    assert_eq!(
        r.triggers(&sid),
        ["new_session"],
        "no recompile for a surface"
    );

    // A turn that names no surface (a continuation's, a caller's that says
    // none) carries none either.
    turn_on(&r.core, &sid, "three", None).await.unwrap();
    assert!(!serde_json::to_string(&r.last_request())
        .unwrap()
        .contains(CODE_FIRST));

    // Back on the terminal it is there again, from the same compilation.
    turn_on(&r.core, &sid, "four", Some("terminal"))
        .await
        .unwrap();
    assert!(tail_text(&r.last_request()).contains(CODE_FIRST));
    assert_eq!(prefix_bytes(&on_terminal), prefix_bytes(&r.last_request()));
    assert_eq!(r.triggers(&sid), ["new_session"]);

    // Revoked, it leaves the tail at the next turn, still with no recompile.
    r.revoke(&id);
    turn_on(&r.core, &sid, "five", Some("terminal"))
        .await
        .unwrap();
    assert!(!tail_text(&r.last_request()).contains("Standing here:"));
    assert_eq!(r.triggers(&sid), ["new_session"]);
}

/// Two sessions on two surfaces, one record live: the system block and the
/// tools of a fresh session are the same bytes on every surface.
#[tokio::test]
async fn a_fresh_session_has_the_same_system_on_every_surface() {
    let r = rig();
    terminal(&r, StandingKind::Preference, CODE_FIRST);
    r.add(StandingKind::Order, StandingScope::Everywhere, NO_PUSH);
    let mut prefixes = Vec::new();
    for surface in ["terminal", "discord", "cockpit", "mcp"] {
        let sid = session(&r.core, None);
        turn_on(&r.core, &sid, "hello", Some(surface))
            .await
            .unwrap();
        let req = r.last_request();
        assert_eq!(
            tail_text(&req).contains(CODE_FIRST),
            surface == "terminal",
            "{surface}"
        );
        prefixes.push(prefix_bytes(&req));
    }
    assert!(prefixes.windows(2).all(|w| w[0] == w[1]));
}

/// In a shared place the tail shows a surface's orders and, for the rest, the
/// withheld line: never their text.
#[tokio::test]
async fn a_surface_record_in_a_shared_place_is_withheld_by_the_same_rule() {
    let r = rig();
    let order = r.add(
        StandingKind::Order,
        StandingScope::Surface("discord".into()),
        "no links",
    );
    let pref = r.add(
        StandingKind::Preference,
        StandingScope::Surface("discord".into()),
        CODE_FIRST,
    );
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    turn_on(&r.core, &shared, "hi", Some("discord"))
        .await
        .unwrap();
    let req = r.last_request();
    let tail = tail_text(&req);
    assert!(tail.contains(&format!("- {order}: no links")), "{tail}");
    assert!(tail.contains(crate::standing::WITHHELD), "{tail}");
    let all = serde_json::to_string(&req).unwrap();
    assert!(
        !all.contains(CODE_FIRST) && !all.contains(&pref),
        "leaked: {all}"
    );

    // The owner's DM, on the same surface, carries it.
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn_on(&r.core, &dm, "hi", Some("discord")).await.unwrap();
    let tail = tail_text(&r.last_request());
    assert!(
        tail.contains(&format!("- {pref}: {CODE_FIRST}"))
            && !tail.contains(crate::standing::WITHHELD),
        "{tail}"
    );
}

/// A request that dropped the tail block is refused before anything is sent,
/// naming the record, and the `context.compiled` row of one that carried it
/// names it.
#[tokio::test]
async fn a_request_that_drops_a_surface_record_is_refused_and_one_that_carries_it_says_so() {
    let r = rig();
    let sid = session(&r.core, None);
    let id = terminal(&r, StandingKind::Order, "tests before commits");
    turn_on(&r.core, &sid, "fine", Some("terminal"))
        .await
        .unwrap();
    let rows = rows_in(&r.core, "context.compiled", Some(&sid));
    assert_eq!(
        rows.last().unwrap()["standing_here"],
        serde_json::json!([id])
    );
    turn_on(&r.core, &sid, "elsewhere", Some("discord"))
        .await
        .unwrap();
    assert!(rows_in(&r.core, "context.compiled", Some(&sid))
        .last()
        .unwrap()
        .get("standing_here")
        .is_none());
    let sent = r.model.requests.lock().unwrap().len();

    crate::standing::DROP_THE_TAIL.store(true, std::sync::atomic::Ordering::SeqCst);
    let e = turn_on(&r.core, &sid, "dropped", Some("terminal")).await;
    crate::standing::DROP_THE_TAIL.store(false, std::sync::atomic::Ordering::SeqCst);
    let e = format!("{:?}", e.unwrap_err());
    assert!(e.contains(&id) && e.contains("context_unadmitted"), "{e}");
    assert_eq!(
        r.model.requests.lock().unwrap().len(),
        sent,
        "nothing was sent"
    );
    let rows = rows_of(&r.core, "context.unadmitted");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["why"], "standing_missing");
    assert_eq!(rows[0]["piece"], format!("standing {id} missing"));
}

/// The surface a connection speaks through, as a record's scope names it.
#[test]
fn a_connections_surface_names_the_scope_it_holds() {
    use crate::approval::Surface as S;
    let n = crate::standing::surface_name;
    assert_eq!(n(S::Cli, false), Some("terminal"));
    assert_eq!(n(S::Web, false), Some("cockpit"));
    assert_eq!(n(S::Discord, false), Some("discord"));
    assert_eq!(n(S::Discord, true), Some("voice"));
    assert_eq!(n(S::Mcp, false), Some("mcp"));
    assert_eq!(n(S::Unnamed, false), None);
    assert_eq!(
        n(S::Cli, true),
        Some("terminal"),
        "only Discord speaks as voice"
    );
}

/// "Said it again" is counted from rows, not from the record: a repeat of a
/// live record is one `standing.repeated` row of ids, scoped to the session,
/// and a record that is not live is refused.
#[tokio::test]
async fn a_repeat_is_a_row_of_ids_for_a_live_record_alone() {
    let r = rig();
    let sid = session(&r.core, None);
    let id = terminal(&r, StandingKind::Preference, CODE_FIRST);
    turn_on(&r.core, &sid, "short please", Some("terminal"))
        .await
        .unwrap();
    let node = r.core.store.transcript(&sid).unwrap()[0].1.id.clone();
    r.core.standing_repeated(&sid, &id, &node).unwrap();
    r.core.standing_repeated(&sid, &id, &node).unwrap();
    // The rows ride the next frame, which a turn writes.
    turn_on(&r.core, &sid, "again", Some("terminal"))
        .await
        .unwrap();
    let rows = rows_in(&r.core, "standing.repeated", Some(&sid));
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0], serde_json::json!({"record": id, "node": node}));
    assert!(
        !serde_json::to_string(&rows).unwrap().contains(CODE_FIRST),
        "ids only"
    );

    r.revoke(&id);
    let e = r.core.standing_repeated(&sid, &id, &node).unwrap_err();
    assert!(e.to_string().contains("no live standing record"), "{e}");
    assert!(r
        .core
        .standing_repeated(&sid, "std_nothing", &node)
        .is_err());
}

/// A `turn.submit` takes its surface from its connection: the CLI's is the
/// terminal, the web UI's the cockpit, Discord's its own, and Discord alone
/// may say a turn is spoken (`surface: "voice"`); no other connection can
/// claim it.
#[tokio::test]
async fn a_turn_submit_takes_its_surface_from_its_connection() {
    use crate::approval::Surface as S;
    let r = rig();
    for s in ["terminal", "cockpit", "discord", "voice"] {
        r.add(
            StandingKind::Order,
            StandingScope::Surface(s.into()),
            &format!("held on {s}"),
        );
    }
    let on = |surface: S, says: Option<&str>| {
        let r = &r;
        let sid = session(&r.core, None);
        let mut p = serde_json::json!({"session_id": sid, "input": "hi"});
        if let Some(s) = says {
            p["surface"] = s.into();
        }
        async move {
            crate::tests_m3::rpc_as(
                &r.core,
                crate::tests_m3::surface("sock#1", surface),
                "turn.submit",
                p,
            )
            .await
            .unwrap();
            let tail = tail_text(&r.last_request());
            ["terminal", "cockpit", "discord", "voice"]
                .into_iter()
                .filter(|s| tail.contains(&format!("held on {s}")))
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(on(S::Cli, None).await, ["terminal"]);
    assert_eq!(on(S::Web, None).await, ["cockpit"]);
    assert_eq!(on(S::Discord, None).await, ["discord"]);
    assert_eq!(on(S::Discord, Some("voice")).await, ["voice"]);
    assert_eq!(
        on(S::Cli, Some("voice")).await,
        ["terminal"],
        "a CLI cannot be heard"
    );
    assert_eq!(on(S::Discord, Some("phone")).await, ["discord"]);
}
