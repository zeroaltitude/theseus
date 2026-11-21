//! A message a thread takes keeps the place it arrived in (issue 44): its
//! mark and its exchange name the DM it came from and the DM's own session,
//! though it ran in the thread's.

use super::*;
use crate::exchange::{exchange, mark, newest, Root};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_a_thread_takes_records_the_place_it_arrived_in() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let res = submit(c, &dm, "what is failing in PR 412?").await;
    let ran = session_of(c, "pr-round");
    assert_eq!(res.session_id, ran, "the thread took it");
    let node = c
        .store
        .session_nodes(&ran)
        .unwrap()
        .into_iter()
        .find(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }))
        .map(|(_, n)| n.id)
        .unwrap();
    let place = format!("discord:dm:{OWNER}");
    let m = mark(&c.store, &node).unwrap().expect("its mark");
    assert_eq!(
        (m.place.as_str(), m.arrived_session.as_str()),
        (place.as_str(), dm.as_str())
    );
    assert_eq!(m.ran_session, ran);
    let ex = exchange(&c.store, &res.turn_id)
        .unwrap()
        .expect("its exchange");
    assert_eq!(
        (ex.place.as_str(), ex.session_id.as_str()),
        (place.as_str(), ran.as_str())
    );
    assert_eq!(newest(&c.store, Root::Place(&place), 4).unwrap().len(), 1);
}
