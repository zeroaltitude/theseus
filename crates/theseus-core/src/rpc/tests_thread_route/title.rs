//! A message that names threads by their titles (theseus-4qdh8): the
//! walkthrough's rehearsal on install #69, with its two young threads.

use super::*;

/// The rehearsal's message: it names both threads by title.
const BOTH: &str = "For the word census of the sample docs and for the harbour log notes: add the census total as a line to the harbour notes, and add the ferry time to the census table, so both stay in step with each other.";

fn young_threads(c: &Core) {
    thread(c, "docs-census", "Docs word census", ws(&[], &[], &[]));
    thread(c, "harbour-notes", "Harbour log notes", ws(&[], &[], &[]));
}

fn rig_with_jev(jev: &FakeJev) -> Rig {
    rig_on(texts(2), Some(jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.threads.route_wait_ms = 3000;
        c.threads.ask_wait_secs = 5;
    })
}

/// Both titles named, neither thread's entities: each contends on its title,
/// Jev's 0.29 pick does not settle it, and the router asks which.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_naming_two_threads_by_title_asks_which() {
    let jev = FakeJev::start().unwrap();
    thread_answer(&jev, "docs-census", 0.29);
    let r = rig_with_jev(&jev);
    let c = &r.core;
    young_threads(c);
    let dm = home(c, None);
    let mut w = Wire::open(c);
    let id = w.send(method::TURN_SUBMIT, params(&dm, BOTH, None)).await;
    let asked = w.notice("thread.asked").await;
    let q = asked["question"].as_str().unwrap();
    assert!(
        q.contains("Docs word census") && q.contains("Harbour log notes"),
        "{q}"
    );
    let mut a = Wire::open(c);
    let aid = a
        .send(
            method::THREAD_ANSWER,
            ThreadAnswerParams {
                ask_id: asked["ask_id"].as_str().unwrap().into(),
                choice: "home".into(),
                author: None,
                discord: None,
            },
        )
        .await;
    a.response(aid).await;
    w.response(id).await;
}

/// The e31an case beside the same threads: a request that shares one word
/// with a title and names neither runs at home, with no question.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plainly_new_request_still_runs_at_home() {
    let jev = FakeJev::start().unwrap();
    thread_answer(&jev, "docs-census", 0.41);
    let r = rig_with_jev(&jev);
    let c = &r.core;
    young_threads(c);
    let dm = home(c, None);
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::TURN_SUBMIT,
            params(
                &dm,
                "start two tasks: a census of the disk, and a slow load",
                None,
            ),
        )
        .await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).unwrap();
    assert_eq!(res.session_id, dm);
    assert_eq!(tag(&res).how, RouteHow::Home);
    assert!(rows(c, "thread.asked").is_empty(), "nothing was asked");
}
