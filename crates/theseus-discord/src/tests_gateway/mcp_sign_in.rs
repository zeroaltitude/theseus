//! Signing in to an MCP server from Discord (issue 59), through the fake gateway: the offer
//! card reaches the owner's DM with Sign in and Sign out; a form's address goes to the daemon's
//! finish and is in no exit (a reply, a post, a ledger row, the outbox, the log at trace level);
//! a form from a shared channel is refused in fixed words, and one from someone else signs nothing
//! in; Sign out answers
//! in fixed words. The successful sign-in itself is the core's test (`mcp/tests/discord_login.rs`):
//! a daemon outside its own tests reaches only a public `https` issuer, so it cannot be run here
//! without the network.

use std::sync::{Arc, Mutex};

use serde_json::json;
use theseus_core::config::mcp::McpOauth;
use theseus_core::config::McpServerConfig;
use theseus_core::provider::{FakeProvider, Provider};
use theseus_sim::fake_discord::commands::Sender;
use theseus_sim::fake_discord::Modal;

use super::{bindings, guild, Rig, ANA, ANA_DM, BEN, LAB};
use crate::runtime::mcp_sign_in::{EXPIRED, NOTHING_KEPT, NOT_HERE};

/// Invented: the address a browser ended on, with a code in it. Never in any output.
const PASTED: &str = "https://mcp.example.invalid/callback?code=INVENTED-CODE-5521&state=abc";
const CODE: &str = "INVENTED-CODE-5521";

#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        self.clone()
    }
}

async fn rig() -> Rig {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    Rig::start_tweaked(model, guild(true), &bindings(false), &[], |cfg| {
        // Never reached: nothing here connects.
        cfg.mcp.servers.insert(
            "docs".into(),
            McpServerConfig {
                command: vec![],
                env: Default::default(),
                url: Some("https://mcp.example.invalid/mcp".into()),
                auth_secret: None,
                oauth: Some(McpOauth::default()),
                read: vec![],
                sandbox: Default::default(),
                egress: vec![],
                frozen: None,
                external: true,
                enabled: true,
                start_timeout_secs: 10,
                call_timeout_secs: 30,
            },
        );
    })
    .await
}

fn told(r: &Rig, interaction: &str, words: &str) -> bool {
    r.fake.replies().iter().any(|x| {
        x.interaction.as_deref() == Some(interaction)
            && x.content.as_deref().is_some_and(|c| c.starts_with(words))
    })
}

#[tokio::test]
async fn the_offer_the_form_and_sign_out_say_fixed_words_and_the_address_is_in_no_exit() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sub = tracing_subscriber::fmt()
        .with_writer(Sink(buf.clone()))
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);
    let r = rig().await;
    // The card a turn offers, as the offer tool stages it.
    let body = json!({"kind": "mcp_sign_in", "server": "docs",
                      "text": theseus_core::mcp::offer::card_text("docs")});
    let (post, frame) = r.core.outbox.stage_to_operator(None, body).unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&post);
    let card_of = |m: &theseus_sim::fake_discord::Msg| {
        m.versions[0].contains("Sign in to the MCP server `docs`")
    };
    r.until("the card in ana's DM", || {
        r.posted(ANA_DM).iter().any(card_of)
    })
    .await;
    let card = r.posted(ANA_DM).into_iter().find(card_of).unwrap();
    let labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Sign in", "Sign out"]);
    assert!(r.posted(LAB).iter().all(|m| !card_of(m)), "never in #lab");

    // Sign out: nothing was kept, in fixed words.
    let out = r.press(&card.id, "Sign out", (ANA, "ana"));
    r.until("sign out's words", || told(&r, &out, NOTHING_KEPT))
        .await;

    // The form, from her DM, for a sign-in that is not open: fixed words, and the address goes nowhere.
    let fields = [("address", PASTED)];
    let from_dm = r
        .fake
        .submit_modal(
            &Sender {
                user: ANA,
                name: "ana",
                channel: None,
            },
            "mcpin:m:0123abcd:0",
            &fields,
            &[],
            None,
        )
        .unwrap();
    r.until("ana is told it is not open", || told(&r, &from_dm, EXPIRED))
        .await;
    // From someone else, and from a shared channel: refused, and no address.
    let from_ben = r
        .fake
        .submit_modal(
            &Sender {
                user: BEN,
                name: "ben",
                channel: None,
            },
            "mcpin:m:0123abcd:0",
            &fields,
            &[],
            None,
        )
        .unwrap();
    let from_lab = r
        .fake
        .modal(&Modal {
            user: ANA,
            name: "ana",
            channel: LAB,
            custom_id: "mcpin:m:0123abcd:0",
            fields: &fields,
        })
        .unwrap();
    r.until("the shared channel is refused", || {
        told(&r, &from_lab, NOT_HERE)
    })
    .await;
    // Ben's direct messages are no place of this binding: his form signs nothing in, and the
    // daemon judges any that reaches it (`mcp/tests/discord_login.rs`).
    assert!(
        r.fake
            .replies()
            .iter()
            .all(|x| x.interaction.as_deref() != Some(from_ben.as_str())
                || x.content.as_deref().is_none_or(|c| !c.starts_with("✅"))),
        "ben signed in"
    );

    in_no_exit(&r, &buf);
}

/// Neither the code nor the pasted address is in any exit: the replies, every post, every ledger
/// row, the outbox, or the log at trace level (`buf`).
fn in_no_exit(r: &Rig, buf: &Arc<Mutex<Vec<u8>>>) {
    let replies = serde_json::to_string(&r.fake.replies()).unwrap();
    let posts: String = [ANA_DM, LAB]
        .iter()
        .flat_map(|c| r.posted(*c))
        .map(|m| format!("{m:?}"))
        .collect();
    r.core.store.flush_waiting();
    let rows: Vec<(u64, theseus_core::ledger::LedgerRow)> =
        r.core.store.ledger_tail(100_000).unwrap();
    let rows = format!("{rows:?}");
    let outbox = format!(
        "{:?}",
        r.core
            .outbox
            .open_for(theseus_core::outbox::OPERATOR_TARGET)
    );
    let log = String::from_utf8_lossy(&buf.lock().unwrap()).into_owned();
    for (what, text) in [
        ("a reply", &replies),
        ("a post", &posts),
        ("a ledger row", &rows),
        ("the outbox", &outbox),
        ("the log", &log),
    ] {
        assert!(!text.contains(CODE), "the code is in {what}");
        assert!(!text.contains("callback?code"), "the address is in {what}");
    }
}
