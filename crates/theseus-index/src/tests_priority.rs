//! A query at the daemon's priority, the tender's own work lowered
//! (theseus-y9a3). The tender lowered its whole process to nice 10 in the
//! idle I/O class, its socket's threads with it: beside a busy machine a
//! recall's 3 ms word search waited ~250 ms for a core, and ~450 ms cold, past
//! the 250 ms deadline, so the recall had nothing. `open_served` starts the
//! socket's thread before it lowers the caller, and `index.status` says both.

use std::time::Duration;

use crate::client::Client;
use crate::priority::{self, Priority};
use crate::proto::{method, IndexStatus, QueryParams, QueryResult};
use crate::tests::{settle, user, Rig};

/// A query is answered at the priority the tender started at, while its own
/// work (the thread that opened it, and so every thread that one starts) runs
/// at the nice it was given, in the idle I/O class.
#[test]
fn a_query_keeps_the_tenders_starting_priority_while_its_work_is_lowered() {
    // On a thread of its own: a priority is a thread's, and the threads it
    // starts inherit it; the test harness's other threads keep theirs.
    std::thread::spawn(|| {
        let rig = Rig::new();
        rig.put(&[user("ses_1", "the ocelot sleeps by the river")]);
        let start = priority::current();
        let nice = (start.nice + 5).min(19);
        if nice == start.nice {
            eprintln!("already at nice 19: nothing lower to tell apart");
            return;
        }
        let mut t = crate::open_served(rig.cfg(&rig.index), nice).unwrap();
        settle(&mut t);
        assert_eq!(
            priority::current(),
            Priority {
                nice,
                io: "idle".into()
            },
            "the opening thread is the work's"
        );
        let mut c = Client::connect(&t.paths().socket(), Duration::from_secs(10)).unwrap();
        let s: IndexStatus = c.call(method::STATUS, ()).unwrap();
        assert_eq!(
            s.queries_at,
            Some(start),
            "a query is answered at the priority the tender started at"
        );
        assert_eq!(
            s.work_at,
            Some(Priority {
                nice,
                io: "idle".into()
            })
        );
        let r: QueryResult = c.call(method::QUERY, QueryParams::new("ocelot")).unwrap();
        assert_eq!(r.hits.len(), 1, "and it answers");
    })
    .join()
    .unwrap();
}

/// A tender that cannot take its index (another holds it) never binds the
/// socket: the other's stays answering.
#[test]
fn a_tender_refused_its_index_leaves_the_others_socket() {
    std::thread::spawn(|| {
        let rig = Rig::new();
        rig.put(&[user("ses_1", "the ocelot sleeps by the river")]);
        let mut first = crate::open_served(rig.cfg(&rig.index), 19).unwrap();
        settle(&mut first);
        assert!(matches!(
            crate::open_served(rig.cfg(&rig.index), 19),
            Err(crate::OpenError::Held(_))
        ));
        // Time for a socket's thread that would bind anyway to have done it
        // (and, its tender gone, to have dropped the listener).
        std::thread::sleep(Duration::from_millis(300));
        let mut c = Client::connect(&first.paths().socket(), Duration::from_secs(10)).unwrap();
        let r: QueryResult = c.call(method::QUERY, QueryParams::new("ocelot")).unwrap();
        assert_eq!(r.hits.len(), 1, "the first tender still answers");
    })
    .join()
    .unwrap();
}
