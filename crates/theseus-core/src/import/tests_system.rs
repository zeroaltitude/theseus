//! The import's rows for the work board (theseus-jn17m): the numbers a
//! client sends ride in the `import.batch` row of the frame the batch
//! already writes, so an import writes the frames it wrote before.

use serde_json::json;
use theseus_protocol::import::{ImportEpisodesParams, ImportEpisodesResult};
use theseus_protocol::method;

use super::tests::{call, fixture, lines, live};

/// The `import.batch` rows of the fixture's tag, oldest first.
fn batch_rows(core: &crate::Core) -> Vec<crate::ledger::LedgerRow> {
    core.store
        .scope_after(&crate::fact::import::scope(super::tests::TAG), 0)
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == theseus_store::kinds::LEDGER)
        .filter_map(|r| r.decode::<crate::ledger::LedgerRow>().ok())
        .filter(|r| r.kind == "import.batch")
        .collect()
}

async fn send(core: &std::sync::Arc<crate::Core>, p: ImportEpisodesParams) -> ImportEpisodesResult {
    serde_json::from_value(
        call(
            core,
            method::IMPORT_EPISODES,
            serde_json::to_value(p).unwrap(),
        )
        .await
        .unwrap(),
    )
    .unwrap()
}

/// The fixture in four batches of three episodes, numbered or not: the
/// same frames, the numbers in the rows, and the source of the tag.
#[tokio::test]
async fn numbering_a_batch_writes_no_frame_and_the_row_carries_the_numbers() {
    let all = lines(&fixture());
    let run = |numbered: bool| {
        let all = all.clone();
        async move {
            let r = live();
            let c = r.core.clone();
            let before = c.store.stats().unwrap().frames_appended;
            let mut frames = 0;
            for (i, chunk) in all.chunks(3).enumerate() {
                let n = i as u32 + 1;
                let got = send(
                    &c,
                    ImportEpisodesParams {
                        file: "reef.jsonl".into(),
                        lines: chunk.to_vec(),
                        batch: numbered.then_some(n),
                        batches: numbered.then_some(4),
                    },
                )
                .await;
                assert_eq!(got.imported, 3, "{got:?}");
                frames += got.frames;
            }
            let wal = c.store.stats().unwrap().frames_appended - before;
            (frames, wal, batch_rows(&c))
        }
    };
    let (frames_plain, wal_plain, rows_plain) = run(false).await;
    let (frames_numbered, wal_numbered, rows) = run(true).await;
    assert_eq!((frames_plain, wal_plain), (4, 4), "one frame a batch");
    assert_eq!(
        (frames_numbered, wal_numbered),
        (frames_plain, wal_plain),
        "numbering adds no frame"
    );
    assert_eq!(rows.len(), 4);
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(r.data["batch"], json!(i + 1));
        assert_eq!(r.data["batches"], json!(4));
        assert!(r.data["source"].is_string(), "{:?}", r.data);
    }
    // A client that sends no numbers leaves the row as it was.
    for r in &rows_plain {
        assert!(r.data.get("batch").is_none() && r.data.get("batches").is_none());
    }
}
