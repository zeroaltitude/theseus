//! A remembered message read by the MCP server's `memory_get` is cut at its
//! byte budget before any value the vault's board knows, so the exit's mask
//! finds each value whole. The value is invented.

use theseus_protocol::memory::MemoryLookupParams;

use crate::provider::Scripted;
use crate::tests_cut_sites::{knowing_core, whole_or_absent};
use crate::tests_places::{session, turn};

const V: &str = "lavo-6602-crem-1149-snub";

#[tokio::test]
async fn a_memory_hit_sent_to_an_mcp_client_holds_a_value_whole_or_not_at_all() {
    let r = knowing_core(V, |_| {}, |_| Scripted::text("Noted."));
    let sid = session(&r.core, None);
    // The page's cut falls at 6,000 bytes, inside V.
    turn(&r.core, &sid, &format!("{}{V} after", "x".repeat(5_990))).await;
    let p = MemoryLookupParams {
        op: "get".into(),
        session: Some(sid),
        ..Default::default()
    };
    let got = super::mcp::get(&r.core, &p).await.unwrap();
    let long = got
        .hits
        .iter()
        .find(|h| h.text.starts_with("xxxx"))
        .expect("the long message is read");
    assert!(long.text.contains(" more characters: "), "{:?}", long.text);
    for h in &got.hits {
        whole_or_absent("a memory hit", &h.text, V);
    }
}
