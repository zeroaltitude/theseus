//! The judgment surfaces (M5 23b; design §3, "23b"), against the fake Jev:
//! a shadow dispatch's mark in the turn's trace, naming the id its row
//! carries; a judgment's sentence when it lands; `judge.list`'s filters;
//! `judge.get`'s state, equal to its blob; and a judged turn's frames and
//! request bytes, as with the judge off.

use std::sync::Arc;

use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::judge::{JudgeGetParams, JudgeListParams};
use theseus_protocol::{error_code, NarrativeLine};

use crate::judge::mark;
use crate::rpc::Core;
use crate::tests_judge::{kinds, rig_with, texts, turn, until_judged};

fn jev() -> FakeJev {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "work_state",
        Jev::Choice {
            option: "progressing".into(),
            confidence: 0.95,
        },
    );
    jev
}

fn list(core: &Arc<Core>, p: JudgeListParams) -> Vec<String> {
    core.judge_list(p)
        .unwrap()
        .judgments
        .into_iter()
        .map(|e| e.data["id"].as_str().unwrap().to_string())
        .collect()
}

/// Wait (on the runtime's timer) for a narrative line that `ok` takes.
async fn until_said(core: &Arc<Core>, ok: impl Fn(&NarrativeLine) -> bool) -> NarrativeLine {
    let t0 = std::time::Instant::now();
    loop {
        if let Some(l) = core.narrator.tail().into_iter().find(|l| ok(l)) {
            return l;
        }
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(20),
            "no such line: {:#?}",
            core.narrator.tail()
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// A shadow dispatch marks the turn's trace, in the turn's last frame (its
/// `turn.trace` row), with a zero-length `judge` span that names the pack,
/// the point, the mode, and the id the judgment's row then carries. With the
/// judge off, no mark.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shadow_dispatch_marks_the_trace_with_the_id_its_row_carries() {
    let jev = jev();
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let res = turn(&r.core, None, "Say done.").await;
    let marks = mark::marks(res.trace.as_ref().unwrap());
    assert_eq!(marks.len(), 1, "one mark: {marks:?}");
    let m = &marks[0];
    assert_eq!(
        (m.pack.as_str(), m.point.as_str(), m.mode),
        ("loop.v1", "loop_end", theseus_judge::Mode::Shadow)
    );
    // The turn's last frame carries the same trace, mark and all.
    let traced = kinds(&r.core.store, "turn.trace");
    let row_trace: theseus_protocol::Span =
        serde_json::from_value(traced.last().unwrap().data.clone()).unwrap();
    assert_eq!(mark::marks(&row_trace), marks, "the turn.trace row's");
    let span = row_trace
        .children
        .iter()
        .find(|s| s.name == mark::SPAN)
        .unwrap();
    assert_eq!(
        (span.kind.as_str(), span.end_us),
        ("mark", Some(span.start_us))
    );
    assert_eq!(span.attrs["loop"], 0);
    assert_eq!(span.attrs["class"], "reply");
    let rows = until_judged(&r.core.store, 1).await;
    let (rec, row) = &rows[0];
    assert_eq!(
        row.data["id"].as_str(),
        Some(m.id.as_str()),
        "the row is the mark's"
    );
    assert_eq!(rec.key.as_deref(), Some(m.id.as_str()));

    let off = rig_with(texts(1), None, |_| {});
    let res = turn(&off.core, None, "Say done.").await;
    assert!(mark::marks(res.trace.as_ref().unwrap()).is_empty());
}

/// With the judge on, a judged turn writes the frames, and sends the
/// request bytes, it does with the judge off: the mark rides in the last
/// frame's trace, and no request carries it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judged_turn_keeps_its_frames_and_its_request_bytes() {
    let jev = jev();
    let on = rig_with(texts(2), Some(&jev), |_| {});
    let off = rig_with(texts(2), None, |_| {});
    let mut frames = Vec::new();
    for r in [&on, &off] {
        let first = turn(&r.core, None, "warm up").await;
        // The first turn's judgment lands in a frame of its own: let it, first.
        if std::ptr::eq(r, &on) {
            until_judged(&r.core.store, 1).await;
        }
        let before = r.core.store.stats().unwrap().frames_appended;
        turn(&r.core, Some(&first.session_id), "hi").await;
        frames.push(r.core.store.stats().unwrap().frames_appended - before);
    }
    assert_eq!(frames[0], frames[1], "the same frames, judge or no judge");
    assert!(frames[0] <= 5, "{frames:?}");
    until_judged(&on.core.store, 2).await;
    let bytes = |r: &crate::tests_judge::Rig| {
        r.fake
            .requests()
            .iter()
            .map(|q| serde_json::to_string(q).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        bytes(&on),
        bytes(&off),
        "the same requests, judge or no judge"
    );
}

/// When a judgment lands, the narrative says it once its frame is written,
/// as the turn's: what Jev answered first, its band, and what the baseline
/// did.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgment_landing_says_its_sentence() {
    let jev = jev();
    jev.script("announced_unfinished", Jev::Noul(0.97));
    let r = rig_with(texts(1), Some(&jev), |c| c.narrative = true);
    let res = turn(&r.core, None, "Say done.").await;
    let line = until_said(&r.core, |l| l.text.starts_with("Jev,")).await;
    assert_eq!(
        line.text,
        "Jev, in shadow, judged the stop: progressing (0.95, act band). \
         The baseline ended the turn; recorded, not acted on. Jev disagrees."
    );
    assert_eq!(line.session_id.as_deref(), Some(res.session_id.as_str()));
    assert_eq!(line.turn_id.as_deref(), Some(res.turn_id.as_str()));
    let rows = until_judged(&r.core.store, 1).await;
    assert_eq!(rows[0].1.data["disagrees"], true, "the row says it too");
}

/// `judge.list` filters by pack (a name with its version, or an id), by
/// session, and by time, and keeps the newest `limit`, saying how many
/// matched and which scopes it read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn judge_list_filters_by_pack_session_and_time() {
    let jev = jev();
    let r = rig_with(texts(3), Some(&jev), |_| {});
    let a = turn(&r.core, None, "one").await;
    turn(&r.core, Some(&a.session_id), "two").await;
    let b = turn(&r.core, None, "three").await;
    let rows = until_judged(&r.core.store, 3).await;
    let ids: Vec<String> = rows
        .iter()
        .map(|(_, row)| row.data["id"].as_str().unwrap().to_string())
        .collect();
    let all = JudgeListParams::default();
    assert_eq!(
        list(&r.core, all.clone()),
        ids,
        "every pack's, oldest first"
    );
    let scopes = r.core.judge_list(all.clone()).unwrap().scopes;
    assert!(scopes.contains(&"judge:loop".to_string()), "{scopes:?}");
    let by = |p: JudgeListParams| list(&r.core, p);
    for pack in ["loop.v1", "loop"] {
        let p = JudgeListParams {
            pack: Some(pack.into()),
            ..all.clone()
        };
        assert_eq!(by(p), ids, "{pack}");
    }
    for pack in ["loop.v2", "security.v1"] {
        let p = JudgeListParams {
            pack: Some(pack.into()),
            ..all.clone()
        };
        assert!(by(p).is_empty(), "{pack}");
    }
    let one = r
        .core
        .judge_list(JudgeListParams {
            pack: Some("security.v1".into()),
            ..all.clone()
        })
        .unwrap();
    assert_eq!(one.scopes, ["judge:security"], "the scope it read");
    let mine = JudgeListParams {
        session_id: Some(a.session_id.clone()),
        ..all.clone()
    };
    assert_eq!(by(mine), ids[..2]);
    let theirs = JudgeListParams {
        session_id: Some(b.session_id.clone()),
        ..all.clone()
    };
    assert_eq!(by(theirs), ids[2..]);
    let later = JudgeListParams {
        since: Some(rows[2].1.at_unix_ms + 60_000),
        ..all.clone()
    };
    assert!(by(later).is_empty());
    let since = JudgeListParams {
        since: Some(rows[0].1.at_unix_ms),
        ..all.clone()
    };
    assert_eq!(by(since), ids);
    let newest = r
        .core
        .judge_list(JudgeListParams {
            limit: Some(1),
            ..all
        })
        .unwrap();
    // Read from the newest back, one match past the limit: more matched
    // than counted, so `matched` is the floor it proved (theseus-wse2).
    assert_eq!((newest.matched, newest.more), (2, true));
    assert_eq!(newest.judgments.len(), 1);
    assert_eq!(
        newest.judgments[0].data["id"].as_str(),
        Some(ids[2].as_str())
    );
    assert!(
        newest.judgments[0]
            .data
            .get("state")
            .is_some_and(|s| s.get("sha256").is_some()),
        "the state's record, not its body"
    );
}

/// `judge.get` gives the row and the state Jev was sent: the blob's JSON,
/// whose digest the row names. An unknown id is not found.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn judge_get_gives_the_row_and_its_state_from_the_blob() {
    let jev = jev();
    let r = rig_with(texts(1), Some(&jev), |_| {});
    turn(&r.core, None, "Say done.").await;
    let rows = until_judged(&r.core.store, 1).await;
    let id = rows[0].1.data["id"].as_str().unwrap().to_string();
    let got = r.core.judge_get(JudgeGetParams { id }).unwrap();
    assert_eq!(got.judgment.data, rows[0].1.data);
    assert_eq!(got.state_missing, None);
    let digest = rows[0].1.data["state"]["sha256"].as_str().unwrap();
    let bytes = std::fs::read(r.core.store.blobs().path(digest)).unwrap();
    let blob: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(got.state, Some(blob));
    // The state Jev saw is the blob's: the fake's request carries it whole.
    let sent = jev.seen().last().unwrap().body["state"].clone();
    assert_eq!(Some(&sent), got.state.as_ref());
    let e = r
        .core
        .judge_get(JudgeGetParams {
            id: "jdg_nothing".into(),
        })
        .unwrap_err();
    assert_eq!(e.code, error_code::NOT_FOUND);
}
