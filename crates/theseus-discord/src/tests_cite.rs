//! A reply's cited sources on Discord (theseus-ejdk), end to end through the
//! stand-in's gateway: a person's message in a bound channel keeps its id; a
//! reply that cites it posts small-text footnotes and one row of buttons, a
//! link to the original message for a Discord source, and "Show source" for
//! any other, whose press answers the presser alone with an excerpt; and a
//! reply with no sources is the reply it was, handles and all.

use std::sync::Arc;

use serde_json::json;
use theseus_core::node::{Body, Node, RecalledRef};
use theseus_core::provider::{FakeProvider, Scripted};
use theseus_core::session::SessionRecord;
use theseus_core::Core;
use theseus_protocol::cite::{handles_for, header_with, Citation, DiscordSource};
use theseus_protocol::SessionKind;
use theseus_sim::fake_discord::{Guild, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, ANA_DM, LAB};

fn bindings() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

const OAK: &str = "We chose oak for the new pier.";
const LIGHTS: &str = "The pier lights are green.\nThey blink at dusk.";

fn assistant(core: &Core, session: &str, text: &str) -> String {
    let n = Node::assistant(
        session,
        "turn_cite",
        0,
        Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": text})],
            model: "glm-x".into(),
            provider: "zed".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    core.store.append(&[n.record().unwrap()]).unwrap();
    n.id
}

fn reply_body(turn: &str, node: &str, session: &str, cites: Vec<Citation>) -> serde_json::Value {
    let result = json!({"session_id": session, "turn_id": turn, "loops": 1, "output": "",
        "stop_reason": "end_turn", "model": "glm-x", "provider": "zed", "profile": "quick",
        "usage": {"input_tokens": 1, "output_tokens": 1}, "elapsed_ms": 1000, "tool_calls": 0,
        "continuation": false, "cites": cites});
    json!({"kind": "reply", "turn_id": turn, "loops": [[1, node]], "result": result, "reply_to": null})
}

/// Dedicated sources: a Discord message in `#lab` (its id kept by the real
/// path), and a message from the CLI. Returns the DM's session, the two
/// citations as the core would resolve them, and the Discord message's id.
async fn sources(r: &Rig) -> (String, [Citation; 2], String) {
    let mid = r.say((ANA, "ana"), Some(LAB), OAK);
    r.until("the channel's reply", || {
        r.posted(LAB).iter().any(|m| m.content.contains("Noted"))
    })
    .await;
    r.say((ANA, "ana"), None, "hello");
    r.until("the DM's reply", || {
        r.posted(ANA_DM).iter().any(|m| m.content.contains("Hello"))
    })
    .await;
    let outbox = &r.core.outbox;
    let lab = outbox
        .place_session(&format!("channel:{LAB}"))
        .unwrap()
        .unwrap();
    let dm = outbox.place_session(&format!("dm:{ANA}")).unwrap().unwrap();
    let said = r.core.store.session_nodes(&lab).unwrap().remove(0);
    // The id the real path kept beside the node.
    let kept: serde_json::Value = r
        .core
        .store
        .get_meta(&format!("msg.origin.{}", said.1.id))
        .unwrap()
        .expect("the message's origin");
    assert_eq!(kept["message_id"], mid);
    // A message from the CLI, in a session of its own.
    let cli = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&cli.session_id, &cli).unwrap();
    let lights = Node::user(&cli.session_id, None, "test", LIGHTS);
    r.core.store.append(&[lights.record().unwrap()]).unwrap();
    let handles = handles_for(&[&said.1.id, &lights.id]);
    let item = |n: &Node, sid: &str, pos: u64, text: &str, handle: &str| RecalledRef {
        node_id: n.id.clone(),
        session_id: sid.into(),
        position: pos,
        chunk: (0, text.len() as u32),
        header: header_with(
            handle,
            &format!(
                "a message from {} in #x",
                n.author.as_deref().unwrap_or("?")
            ),
        ),
        tokens: 5,
    };
    let lp = r.core.store.session_nodes(&cli.session_id).unwrap()[0].0;
    let items = vec![
        item(&said.1, &lab, said.0, OAK, &handles[0]),
        item(&lights, &cli.session_id, lp, LIGHTS, &handles[1]),
    ];
    let recall = Node::recall(&dm, "turn_cite", "rcl_cite", "baseline", items.clone());
    r.core.store.append(&[recall.record().unwrap()]).unwrap();
    let cites = [(&items[0], 0), (&items[1], 1)].map(|(i, k)| {
        let c = r.core.runner.citation(i, false).expect("a citation");
        assert_eq!(c.handle, handles[k]);
        c
    });
    assert_eq!(
        cites[0].discord,
        Some(DiscordSource {
            target: format!("discord:channel:{LAB}"),
            message_id: mid.clone()
        })
    );
    assert!(cites[1].discord.is_none());
    (dm, cites, mid)
}

fn rig_model() -> impl FnOnce(
    &std::path::Path,
    Arc<theseus_sim::fake_discord::FakeDiscord>,
) -> Arc<dyn theseus_core::provider::Provider> {
    |_, _| {
        Arc::new(FakeProvider::scripted(vec![
            Scripted::text("Noted: oak."),
            Scripted::text("Hello."),
        ]))
    }
}

/// The golden JSON of a reply with a Discord source and a CLI source: its
/// text with numbered marks, a `-#` footnote each above the footer, and one
/// row, the first button a link to the original message, the second "Show
/// source"; and a press of it answers the presser alone with the excerpt.
#[tokio::test]
async fn a_reply_links_a_discord_source_and_offers_the_other_as_an_excerpt() {
    let r = Rig::start_on(
        rig_model(),
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings(),
        &[],
    )
    .await;
    let (dm, cites, mid) = sources(&r).await;
    let (h0, h1) = (cites[0].handle.clone(), cites[1].handle.clone());
    let node = assistant(
        &r.core,
        &dm,
        &format!("Oak [{h0}], lights [{h1}], and [m:ffffff]."),
    );
    r.core
        .outbox
        .post(
            &dm,
            "",
            &format!("discord:dm:{ANA}"),
            reply_body("turn_cite", &node, &dm, cites.to_vec()),
        )
        .unwrap();
    r.until("the cited reply", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("-# [1]"))
    })
    .await;
    let msg = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.contains("-# [1]"))
        .unwrap();
    let lines: Vec<&str> = msg.content.lines().collect();
    assert_eq!(lines[0], "Oak [1], lights [2], and [m:ffffff].");
    assert!(
        lines[1].starts_with("-# [1] a message from discord:ana in #x"),
        "{lines:?}"
    );
    assert!(
        lines[2].starts_with("-# [2] a message from test in #x"),
        "{lines:?}"
    );
    // The row: a link, then "Show source".
    let row = &msg.components_json[0]["components"];
    assert_eq!(msg.components_json[0]["type"], 1);
    assert_eq!(row[0]["label"], "Open [1]");
    assert_eq!(row[0]["style"], 5);
    assert_eq!(
        row[0]["url"],
        format!("https://discord.com/channels/{DEFAULT_GUILD}/{LAB}/{mid}")
    );
    assert_eq!(row[1]["label"], "Show source [2]");
    assert_eq!(row[1]["custom_id"], format!("src:{dm}:{h1}"));
    // The press: ephemeral, the excerpt quoted, the command that opens it.
    r.press(&msg.id, "Show source [2]", (ANA, "ana"));
    r.until("the excerpt", || {
        r.fake.replies().iter().any(|p| {
            p.content
                .as_deref()
                .is_some_and(|c| c.contains("The pier lights"))
        })
    })
    .await;
    let shown = r
        .fake
        .replies()
        .into_iter()
        .find(|p| {
            p.content
                .as_deref()
                .is_some_and(|c| c.contains("The pier lights"))
        })
        .unwrap();
    assert!(shown.ephemeral, "{shown:?}");
    let text = shown.content.unwrap();
    assert!(
        text.starts_with("**Source** · a message from test in #x"),
        "{text}"
    );
    assert!(
        text.contains("> The pier lights are green.\n> They blink at dusk."),
        "{text}"
    );
    assert!(
        text.ends_with(&format!("-# `theseus history {h1}` opens it whole")),
        "{text}"
    );
}

/// A reply whose citations the core left out (a shared place may not show a
/// private source) is its text as written: no marks, no footnotes, no button.
#[tokio::test]
async fn a_reply_with_no_sources_shows_its_handles_as_text_and_no_button() {
    let r = Rig::start_on(
        rig_model(),
        Guild::new(DEFAULT_GUILD, (ANA, "ana")).private_channel(LAB, "lab", &[ANA]),
        &bindings(),
        &[],
    )
    .await;
    let (dm, cites, _) = sources(&r).await;
    let h0 = cites[0].handle.clone();
    let node = assistant(&r.core, &dm, &format!("Oak [{h0}]."));
    r.core
        .outbox
        .post(
            &dm,
            "",
            &format!("discord:dm:{ANA}"),
            reply_body("turn_plain", &node, &dm, vec![]),
        )
        .unwrap();
    let want = format!("Oak [{h0}].");
    r.until("the plain reply", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.starts_with(&want))
    })
    .await;
    let msg = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.starts_with(&want))
        .unwrap();
    assert!(!msg.content.contains("-# ["), "{}", msg.content);
    assert_eq!(msg.components, 0, "{msg:?}");
}
