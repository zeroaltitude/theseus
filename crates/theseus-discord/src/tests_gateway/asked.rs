//! "Should have asked" through the stand-in's gateway: a tool message that ran
//! notified calls carries a small grey button for each tool (one to five of
//! them), a press tightens that tool as the presser and takes its button, the
//! last takes the row, a guest's press of a copy in a shared place is refused, and a pull-down an
//! older build posted still answers a pick.

use std::sync::Arc;

use serde_json::json;
use theseus_core::policy::Posture;
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_sim::fake_discord::Msg;

use super::{bindings, guild, Rig, ANA, ANA_DM, CY, LAB};

/// A message of the bot's in `channel` with these components, as an older build's, or a forgery, would be.
async fn forged(r: &Rig, channel: u64, components: serde_json::Value) -> String {
    let url = format!("http://{}/api/v10/channels/{channel}/messages", r.fake.addr);
    let posted: serde_json::Value = reqwest::Client::new()
        .post(url)
        .json(&json!({"content": "an earlier tool message", "components": components}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    posted["id"].as_str().unwrap().to_string()
}

/// What the presser alone was told, once it starts with `start`.
fn told(r: &Rig, pressed: &str, start: &str) -> impl Fn() -> bool {
    let fake = r.fake.clone();
    let (pressed, start) = (pressed.to_string(), start.to_string());
    move || {
        fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content.as_deref().is_some_and(|c| c.starts_with(&start))
        })
    }
}

fn tightened(r: &Rig) -> Vec<String> {
    let mut t: Vec<String> = r
        .core
        .health()
        .tightenings
        .into_iter()
        .map(|t| t.tool)
        .collect();
    t.sort();
    t
}

/// The tool message with `tighten` controls the bot posted in ana's DM.
fn tool_message(r: &Rig) -> Option<Msg> {
    r.posted(ANA_DM).into_iter().find(|m| {
        m.versions[0].contains("🔔 notified") && m.components_json.to_string().contains("tighten")
    })
}

#[tokio::test]
async fn a_tool_message_offers_small_buttons_and_a_press_tightens_that_tool_as_the_presser() {
    let model = |_: &std::path::Path, _| -> Arc<dyn Provider> {
        Arc::new(FakeProvider::scripted(vec![
            Scripted::tools(
                "",
                &[
                    ("r1", "proc_run", json!({"argv": ["echo", "hi"]})),
                    ("r2", "fs_read", json!({"path": "notes.txt"})),
                ],
            ),
            Scripted::text("Done."),
        ]))
    };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |c| {
        c.policy.tools.insert("proc.run".into(), Posture::Notify);
        c.policy.tools.insert("fs.read".into(), Posture::Notify);
    })
    .await;
    r.say((ANA, "ana"), None, "Run echo hi and read my notes.");
    r.until("the tool message with its two buttons", || {
        tool_message(&r).is_some_and(|m| m.buttons.len() == 2)
    })
    .await;
    let m = tool_message(&r).unwrap();
    let got: Vec<(&str, bool)> = m
        .buttons
        .iter()
        .map(|b| (b.label.as_str(), b.custom_id.starts_with("tighten:")))
        .collect();
    assert_eq!(got, [("🔔 proc.run", true), ("🔔 fs.read", true)]);
    let id_of = |tool: &str| {
        m.buttons
            .iter()
            .find(|b| b.label.ends_with(tool))
            .unwrap()
            .custom_id
            .clone()
    };
    let (run_id, read_id) = (id_of("proc.run"), id_of("fs.read"));

    // A copy of a button in a shared place, pressed by a guest the place is not
    // bound to, is refused as it always was, and nothing is tightened.
    let copy = forged(
        &r,
        LAB,
        json!([{"type": 1, "components": [{"type": 2, "style": 2, "label": "🔔 proc.run",
                                           "custom_id": run_id}]}]),
    )
    .await;
    let refused = r.press(&copy, &run_id, (CY, "cy"));
    r.until(
        "the guest is told no",
        told(
            &r,
            &refused,
            "Only the people this place is bound to can do that.",
        ),
    )
    .await;
    assert!(tightened(&r).is_empty(), "nothing was tightened");

    // Ana's press in her DM tightens proc.run as hers, and its button goes.
    let pressed = r.press(&m.id, &run_id, (ANA, "ana"));
    r.until(
        "the tightening told to ana",
        told(&r, &pressed, "🔒 `proc.run` asks first from now on"),
    )
    .await;
    assert_eq!(tightened(&r), ["proc.run"]);
    let by = &r.ledger("discord.tighten")[0];
    assert_eq!(
        (by["tool"].as_str(), by["ok"].as_bool()),
        (Some("proc.run"), Some(true))
    );
    assert!(by["by"].as_str().unwrap().contains("ana"), "{by}");
    r.until("only fs.read's button is left", || {
        tool_message(&r).is_some_and(|m| m.buttons.len() == 1 && m.buttons[0].label == "🔔 fs.read")
    })
    .await;

    // The second tool's button still works, and the last one takes the row.
    let pressed = r.press(&m.id, &read_id, (ANA, "ana"));
    r.until(
        "the second tightening",
        told(&r, &pressed, "🔒 `fs.read` asks first from now on"),
    )
    .await;
    assert_eq!(tightened(&r), ["fs.read", "proc.run"]);
    r.until("the row is gone", || {
        r.posted(ANA_DM)
            .iter()
            .any(|x| x.id == m.id && x.buttons.is_empty() && x.content.contains("tightened by"))
    })
    .await;

    // A pull-down an older build posted: its pick still tightens.
    let old = forged(
        &r,
        ANA_DM,
        json!([{"type": 1, "components": [{"type": 3, "custom_id": "tighten",
            "placeholder": "Should I have asked?", "min_values": 1, "max_values": 1,
            "options": [{"label": "Make actions like this ask in the future",
                         "description": "Every git.log call asks you first", "value": "git.log"}]}]}]),
    )
    .await;
    let pick = r
        .fake
        .press_values(
            &theseus_sim::fake_discord::Pressed {
                message: &old,
                button: "tighten",
                user: ANA,
                name: "ana",
            },
            &["git.log"],
        )
        .unwrap();
    r.until(
        "the pick told to ana",
        told(&r, &pick, "🔒 `git.log` asks first from now on"),
    )
    .await;
    assert_eq!(tightened(&r), ["fs.read", "git.log", "proc.run"]);
}
