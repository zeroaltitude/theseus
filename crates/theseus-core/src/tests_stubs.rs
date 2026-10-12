//! A big old attachment stubbed at a cold rewrite, through the core
//! (theseus-ezeg): the request sends its line, `context.compiled` names it,
//! the stored message keeps it whole, the warm window after it renders the
//! same bytes, a file that grows old inside a warm window stays whole, and a
//! keep-warm read renders the stubs as the next turn does.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::{Attachment, SessionKind};

use crate::bus::EventSink;
use crate::keep_warm::MAX_TOKENS;
use crate::node::{AttachmentContent, Body};
use crate::provider::{FakeProvider, ProviderRequest};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const STUB: &str = "left out of this request";

fn one_hour() -> Config {
    let text =
        Config::EXAMPLE_TOML.replace("live = \"sonnet\"", "live = \"sonnet\"\ncache_ttl = \"1h\"");
    let mut c = Config::parse(&text).unwrap().0;
    c.cache.keep_warm_min_tokens = 0;
    c
}

fn build() -> (tempfile::TempDir, Arc<Core>, Arc<FakeProvider>) {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour();
    c.server.state_dir = dir.path().to_string_lossy().into_owned();
    c.tools.roots = vec![];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::default());
    let core = Core::build(crate::rpc::Parts::for_tests(c, fake.clone(), store)).unwrap();
    (dir, core, fake)
}

/// A text file of about `tokens` tokens of its own words.
fn file(name: &str, word: &str, tokens: usize) -> Attachment {
    let text = format!("{word} ").repeat(tokens * 33 / 10 / (word.len() + 1));
    Attachment {
        name: name.into(),
        media_type: "text/plain".into(),
        size: text.len() as u64,
        text: Some(text),
        data: None,
        not_read: None,
    }
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str, files: Vec<Attachment>) {
    core.runner.keep_warm.message(sid, true);
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            recompile: None,
            attachments: files,
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap();
}

/// Two hours pass with no call: the next request writes the cache whole.
fn go_cold(core: &Core, sid: &str) {
    let mut rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    rec.last_active_ms -= 2 * 3_600_000;
    core.store.put_session(sid, &rec).unwrap();
}

fn last(fake: &FakeProvider) -> ProviderRequest {
    fake.requests()
        .into_iter()
        .rfind(|r| r.max_tokens != MAX_TOKENS)
        .unwrap()
}

fn text(r: &ProviderRequest) -> String {
    serde_json::to_string(&r.messages).unwrap()
}

fn stubbed_rows(core: &Core) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(5000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == "context.compiled")
        .map(|(_, r)| r.data["cache"]["stubbed"].clone())
        .collect()
}

/// At a cold rewrite the old big file is its line, `context.compiled` names
/// it, and its node keeps it whole; the warm turns after render the same
/// bytes; a big file that grows old inside the warm window stays whole.
#[tokio::test]
async fn an_attachment_is_stubbed_at_a_cold_rewrite_and_not_inside_a_warm_window() {
    let (_dir, core, fake) = build();
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    let sid = r.session_id.clone();
    turn(
        &core,
        &sid,
        "read this",
        vec![file("contract.txt", "clause", 30_000)],
    )
    .await;
    for i in 2..=4 {
        turn(&core, &sid, &format!("question {i}"), vec![]).await;
    }
    // Warm: four turns on, the file is old, and still whole.
    assert!(text(&last(&fake)).contains("clause clause"));
    assert!(!text(&last(&fake)).contains(STUB));
    // Cold: the rewrite sends it as its line.
    go_cold(&core, &sid);
    turn(
        &core,
        &sid,
        "after lunch",
        vec![file("appendix.txt", "annex", 30_000)],
    )
    .await;
    let cold = last(&fake);
    assert!(
        !text(&cold).contains("clause clause"),
        "the old file is a stub"
    );
    assert!(text(&cold).contains(STUB) && text(&cold).contains("contract.txt"));
    assert!(text(&cold).contains("annex annex"), "the new file is whole");
    let rows = stubbed_rows(&core);
    let named = rows.last().unwrap();
    assert_eq!(named[0]["name"], "contract.txt");
    assert!(named[0]["tokens"].as_u64().unwrap() > 20_000);
    // The stored message keeps the file whole.
    let nodes = core.store.transcript(&sid).unwrap();
    let kept = nodes.iter().find_map(|(_, n)| match &n.body {
        Body::UserMessage { attachments, .. } if !attachments.is_empty() => {
            Some(attachments[0].clone())
        }
        _ => None,
    });
    assert!(matches!(
        kept.unwrap().content,
        AttachmentContent::Text { ref text, .. } if text.starts_with("clause")
    ));
    // Warm again: the same stub, and the appendix, old by the third turn
    // on, stays whole: the warm window changes no byte the cache holds.
    let n = cold.messages.len();
    for i in 6..=9 {
        turn(&core, &sid, &format!("warm question {i}"), vec![]).await;
    }
    let warm = last(&fake);
    let bytes = |m: &[Value]| serde_json::to_string(m).unwrap();
    assert_eq!(
        bytes(&warm.messages[..n - 1]),
        bytes(&cold.messages[..n - 1])
    );
    assert!(
        text(&warm).contains("annex annex"),
        "not stubbed inside the window"
    );
    assert_eq!(
        stubbed_rows(&core).last().unwrap()[0]["name"],
        "contract.txt"
    );
}

/// A keep-warm read renders the stubs in force, so its prefix is the next
/// turn's, stub and all.
#[tokio::test]
async fn a_keep_warm_read_renders_the_stubs_in_force() {
    let (_dir, core, fake) = build();
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    let sid = r.session_id.clone();
    turn(
        &core,
        &sid,
        "read this",
        vec![file("contract.txt", "clause", 30_000)],
    )
    .await;
    for i in 2..=4 {
        turn(&core, &sid, &format!("question {i}"), vec![]).await;
    }
    go_cold(&core, &sid);
    turn(&core, &sid, "after lunch", vec![]).await;
    let live = core.live_profile().0;
    let got = core.runner.keep_warm_once(&sid, &live).await;
    assert!(
        matches!(got, crate::keep_warm::Outcome::Read { .. }),
        "{got:?}"
    );
    turn(&core, &sid, "the next message", vec![]).await;
    let read = fake
        .requests()
        .into_iter()
        .rfind(|r| r.max_tokens == MAX_TOKENS)
        .unwrap();
    let next = last(&fake);
    let n = read.messages.len() - 1;
    assert!(text(&read).contains(STUB));
    assert_eq!(
        serde_json::to_string(&read.messages[..n]).unwrap(),
        serde_json::to_string(&next.messages[..n]).unwrap()
    );
}
