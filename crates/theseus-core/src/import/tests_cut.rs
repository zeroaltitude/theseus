//! An imported session's title, as `import.episodes` writes it on the
//! daemon's path, stops before a known value that runs across its cut. The
//! value is invented.

use theseus_protocol::method;

use super::episode;
use super::tests::{call, fixture, lines};
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::tests_cut_sites::{knowing_core, whole_or_absent, V};

#[tokio::test]
async fn an_imported_sessions_title_stops_before_a_known_value() {
    let r = knowing_core(V, |_| {}, |_| Scripted::text("Noted."));
    let across = format!("{}{V} and after", "x".repeat(50));
    let mut e = fixture().remove(0);
    e["messages"][0]["text"] = across.clone().into();
    if e["summary"].is_object() {
        e["summary"]["text"] = across.into();
    }
    e["hash"] = episode::hash_of(&e).into();
    let params = serde_json::to_value(theseus_protocol::import::ImportEpisodesParams {
        file: "reef.jsonl".into(),
        lines: lines(std::slice::from_ref(&e)),
        ..Default::default()
    })
    .unwrap();
    call(&r.core, method::IMPORT_EPISODES, params)
        .await
        .unwrap();
    let sid = super::session_id_of(e["episode_id"].as_str().unwrap());
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let title = rec.title.unwrap();
    assert_eq!(title, format!("{}…", "x".repeat(50)));
    whole_or_absent("the imported title", &title, V);
}
