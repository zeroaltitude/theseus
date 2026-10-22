//! `sheets.read`, `sheets.write` and `slides.write` through the core, against
//! the loopback fake's Sheets and Slides: the quota's backoff, what a write
//! sends, a read-back of what landed, deferral, and a partial grant.

use std::time::Duration;

use serde_json::{json, Value};
use theseus_google::fake::Reply;
use theseus_google::{scopes, Request, Service};
use theseus_tools::{ToolCtx, ToolFailure, ToolOutput};

use super::tests::*;
use super::Meta;

async fn run(r: &Rig, name: &str, input: Value) -> Result<ToolOutput, ToolFailure> {
    let tool = r
        .core
        .tools
        .registry
        .get(name)
        .expect("the tool is registered")
        .clone();
    let ctx = ToolCtx::for_tests(r.dir.path());
    tool.run_async(&input, &ctx).await.map(|(out, _)| out)
}

async fn made(r: &Rig, name: &str, input: Value) -> ToolOutput {
    run(r, name, input)
        .await
        .unwrap_or_else(|f| panic!("{name} failed: {}", f.message))
}

/// A new spreadsheet holding `values` from A1; its id.
async fn sheet_with(r: &Rig, values: Value) -> String {
    let out = made(
        r,
        "sheets.write",
        json!({"title": "Tide tables", "values": values}),
    )
    .await;
    out.meta["id"].as_str().unwrap().to_string()
}

async fn read(r: &Rig, id: &str, range: &str) -> String {
    made(r, "sheets.read", json!({"id": id, "ranges": [range]}))
        .await
        .text
}

fn puts(r: &Rig) -> Vec<theseus_google::fake::Logged> {
    r.fake
        .log()
        .into_iter()
        .filter(|l| l.verb == "PUT")
        .collect()
}

fn query(l: &theseus_google::fake::Logged, key: &str) -> Option<String> {
    l.query
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

#[tokio::test]
async fn sheets_429_backs_off_and_succeeds() {
    let r = rig(vec![]).await;
    r.g.check().await;
    let id = sheet_with(&r, json!([["tide", "height"]])).await;
    tokio::time::pause();
    r.fake.fail_next(2, &Reply::throttled(Some(1)));
    let began = tokio::time::Instant::now();
    let out = made(
        &r,
        "sheets.write",
        json!({"id": id, "range": "Sheet1!A2", "values": [["low", 0.4]]}),
    )
    .await;
    // Two refusals, each waited out at least the second Google asked for, then one write.
    assert!(
        began.elapsed() >= Duration::from_secs(2),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(out.meta["google_called"]["attempts"], 3, "{out:?}");
    // The fake saw the create's values, then the write three times; it landed once.
    assert_eq!(puts(&r).len(), 1 + 3);
    tokio::time::resume();
    assert_eq!(
        read(&r, &id, "Sheet1!A1:B2")
            .await
            .matches("low\t0.4")
            .count(),
        1
    );
}

#[tokio::test]
async fn user_entered_formulas_evaluate() {
    let r = rig(vec![]).await;
    let id = sheet_with(&r, json!([[1], [2]])).await;
    made(
        &r,
        "sheets.write",
        json!({"id": id, "range": "A3", "values": [["=SUM(A1:A2)"]], "input": "user"}),
    )
    .await;
    made(
        &r,
        "sheets.write",
        json!({"id": id, "range": "B1", "values": [["=1+1"]]}),
    )
    .await;
    let sent = puts(&r);
    assert_eq!(
        query(&sent[1], "valueInputOption").as_deref(),
        Some("USER_ENTERED")
    );
    assert_eq!(
        query(&sent[2], "valueInputOption").as_deref(),
        Some("RAW"),
        "raw is the default"
    );
}

#[tokio::test]
async fn append_inserts_rows() {
    let r = rig(vec![]).await;
    let id = sheet_with(&r, json!([["a", 1], ["b", 2]])).await;
    let out = made(
        &r,
        "sheets.write",
        json!({"id": id, "range": "Sheet1", "mode": "append", "values": [["c", 3]]}),
    )
    .await;
    assert!(out.text.contains("Sheet1!A3:B3"), "{}", out.text);
    let post = r
        .fake
        .log()
        .into_iter()
        .find(|l| l.path.ends_with(":append"))
        .expect("an append");
    assert_eq!(
        query(&post, "insertDataOption").as_deref(),
        Some("INSERT_ROWS")
    );
    assert_eq!(
        read(&r, &id, "Sheet1!A1:B3")
            .await
            .lines()
            .filter(|l| l.contains('\t'))
            .count(),
        3
    );
    // Clear empties the range.
    made(
        &r,
        "sheets.write",
        json!({"id": id, "range": "Sheet1!A1:B3", "mode": "clear"}),
    )
    .await;
    assert!(!read(&r, &id, "Sheet1!A1:B3").await.contains('\t'));
}

#[tokio::test]
async fn a_write_that_cannot_run_is_refused_before_a_call_is_made() {
    let r = rig(vec![]).await;
    for input in [
        json!({"mode": "append", "range": "A1", "values": [[1]]}),
        json!({"id": "s", "values": [[1]]}),
        json!({"id": "s", "range": "A1", "values": [[[1]]]}),
        json!({"id": "s", "range": "A1"}),
    ] {
        assert!(
            run(&r, "sheets.write", input.clone()).await.is_err(),
            "{input}"
        );
    }
    let bad = json!({"slides": [{"layout": "no such layout"}]});
    assert!(run(&r, "slides.write", bad)
        .await
        .unwrap_err()
        .message
        .contains("unknown layout"));
    assert!(r.fake.log().is_empty(), "{:?}", r.fake.log());
}

/// The fake's deck as Slides' `get` answers it.
async fn deck(r: &Rig, id: &str) -> Value {
    let req = Request::get(Service::Slides, format!("/v1/presentations/{id}"));
    let meta = Meta {
        tool: "slides.write",
        method: "test".into(),
        resource: None,
        class: "read",
        recipients: None,
    };
    match r.g.call(meta, req).await {
        Ok(done) => done.response.json(),
        Err(f) => panic!("{}", f.message),
    }
}

/// The grant changes: the next call asks Google again what it holds.
async fn regrant(r: &Rig, granted: &[&str]) {
    r.fake.set_granted(granted);
    r.g.tokens.forget();
    r.g.check().await;
}

#[tokio::test]
async fn a_deck_is_created_with_two_slides() {
    let r = rig(vec![]).await;
    let out = made(
        &r,
        "slides.write",
        json!({"title": "Harbour review", "slides": [
            {"layout": "TITLE", "title": "Harbour review", "body": "2026"},
            {"title": "Tides", "body": "Spring tides run high", "notes": "Mention the dredging"}]}),
    )
    .await;
    let id = out.meta["id"].as_str().unwrap().to_string();
    let deck = deck(&r, &id).await;
    assert_eq!(
        deck["slides"].as_array().unwrap().len(),
        2,
        "the deck's own empty slide went: {deck}"
    );
    // The second slide's notes went to its notes shape, in a second batch.
    let second = deck["slides"][1]["objectId"].as_str().unwrap();
    let batches: Vec<_> = r
        .fake
        .log()
        .into_iter()
        .filter(|l| l.path.ends_with(":batchUpdate"))
        .collect();
    assert_eq!(batches.len(), 2, "{batches:?}");
    assert_eq!(
        batches[1].body["requests"][0]["insertText"],
        json!({"objectId": format!("{second}_notes"), "text": "Mention the dredging"})
    );
    let first_batch = batches[0].body["requests"].as_array().unwrap();
    assert_eq!(
        first_batch
            .iter()
            .filter(|q| q.get("createSlide").is_some())
            .count(),
        2
    );
    assert_eq!(
        first_batch
            .iter()
            .filter(|q| q.get("deleteObject").is_some())
            .count(),
        1
    );
    // Both calls are rows, and the bearer token is in no result.
    let all = format!("{out:?}");
    assert!(!all.contains("ya29."), "{all}");
    assert!(
        out.meta["google_called_also"]
            .as_array()
            .is_some_and(|a| a.len() == 3),
        "{:?}",
        out.meta
    );
}

#[tokio::test]
async fn raw_requests_pass_through() {
    let r = rig(vec![]).await;
    let id = made(&r, "slides.write", json!({"title": "Raw"})).await.meta["id"]
        .as_str()
        .unwrap()
        .to_string();
    let raw = json!([
        {"createSlide": {"objectId": "raw_slide_one"}},
        {"updatePageProperties": {"objectId": "raw_slide_one", "fields": "pageBackgroundFill"}}
    ]);
    made(
        &r,
        "slides.write",
        json!({"id": id, "requests": raw, "replace_text": {"{{date}}": "21 October"}}),
    )
    .await;
    let batch = r
        .fake
        .log()
        .into_iter()
        .rfind(|l| l.path.ends_with(":batchUpdate"))
        .unwrap();
    let sent = batch.body["requests"].as_array().unwrap();
    assert_eq!(
        &sent[sent.len() - 2..],
        raw.as_array().unwrap().as_slice(),
        "raw requests go last, untouched"
    );
    assert_eq!(
        sent[0]["replaceAllText"]["containsText"]["text"],
        "{{date}}"
    );
}

#[tokio::test]
async fn sheets_read_works_on_a_drive_only_grant() {
    let r = rig(vec![]).await;
    let id = sheet_with(&r, json!([["port", "depth"], ["Arden", 7]])).await;
    // The consent was given for Drive alone: Sheets' tools run on it, Slides' too.
    regrant(&r, &[scopes::OPENID, scopes::EMAIL, Service::Drive.scope()]).await;
    let text = read(&r, &id, "Sheet1!A1:B2").await;
    assert!(text.contains("Arden\t7"), "{text}");
    assert!(text.contains("Tide tables"), "{text}");
    // A grant of Slides alone turns Sheets off, in words.
    regrant(
        &r,
        &[scopes::OPENID, scopes::EMAIL, Service::Slides.scope()],
    )
    .await;
    let off = run(&r, "sheets.read", json!({"id": id}))
        .await
        .unwrap_err()
        .message;
    assert!(off.contains("is off"), "{off}");
}

#[tokio::test]
async fn every_sheets_and_slides_tool_is_deferred_and_small() {
    let dir = tempfile::tempdir().unwrap();
    let fake = theseus_google::fake::Fake::start().await;
    let cfg = super::tests::config(dir.path(), &fake, "file:/tmp/unused-google-refresh-token");
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![]));
    let mut parts = crate::rpc::Parts {
        secrets: board(Some(theseus_google::fake::REFRESH_TOKEN)),
        ..crate::rpc::Parts::for_tests(cfg, model, store)
    };
    parts.cfg.tools.defer.enabled = true;
    let core = crate::Core::build(parts).unwrap();
    let (census, request) = core
        .context_census(&theseus_protocol::census::ContextCensusParams {
            session_id: None,
            place: Some("cli".into()),
            exact: false,
        })
        .map(|(c, r, _)| (c, r))
        .unwrap();
    for name in ["sheets_read", "sheets_write", "slides_write"] {
        let line = census
            .tools
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is not in the census"));
        assert!(
            line.deferred,
            "{name} must be deferred: {:?}",
            census
                .tools
                .iter()
                .map(|t| (&t.name, t.deferred))
                .collect::<Vec<_>>()
        );
        assert!(
            line.description_tokens + line.schema_tokens <= 220,
            "{name}: {line:?}"
        );
        let def = request.tools.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(def["defer_loading"], true, "{name}");
    }
}
