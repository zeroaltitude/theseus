//! The daemon survives its connections (theseus-7vtp): the soft open-files
//! limit raised at start, a ceiling under it past which a connection is told
//! why and closed, and an accept error that is logged and waited out, never
//! the end of the serving loop (1,017 connections once ended the daemon with
//! "Too many open files").

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::time::Duration;

use common::Served;
use serde_json::{json, Value};

/// A connection held open, with its reader.
struct Conn(BufReader<UnixStream>);

impl Conn {
    fn open(served: &Served) -> Self {
        let s = UnixStream::connect(served.path("sock")).expect("connecting");
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        Self(BufReader::new(s))
    }

    /// One line the daemon sent, parsed, or `None` at its close.
    fn line(&mut self) -> Option<Value> {
        let mut l = String::new();
        match self.0.read_line(&mut l) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_str(&l).expect("a JSON line")),
        }
    }

    /// `health`'s result, asked on this connection.
    fn health(&mut self) -> Value {
        let req = json!({"jsonrpc": "2.0", "id": 7, "method": "health", "params": null});
        self.0
            .get_mut()
            .write_all(format!("{req}\n").as_bytes())
            .unwrap();
        loop {
            let v = self.line().expect("health's answer");
            if v["id"] == 7 {
                return v["result"].clone();
            }
        }
    }
}

/// The soft and hard open-files limits a daemon is born with.
fn born_with(soft: u64, hard: u64) -> impl FnOnce(&mut std::process::Command) {
    move |cmd| {
        // SAFETY: `setrlimit` alone, between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                let r = libc::rlimit {
                    rlim_cur: soft as libc::rlim_t,
                    rlim_max: hard as libc::rlim_t,
                };
                if libc::setrlimit(libc::RLIMIT_NOFILE, &r) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
}

fn set_ceiling(n: u64) -> impl FnOnce(&mut toml::Table) {
    move |t| {
        let server = t
            .entry("server")
            .or_insert_with(|| toml::Value::Table(Default::default()));
        server
            .as_table_mut()
            .unwrap()
            .insert("max_connections".into(), toml::Value::Integer(n as i64));
    }
}

fn refusal_of(v: &Value) -> bool {
    v["error"]["code"] == -32007
        && v["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("its ceiling; close one and retry"))
}

#[test]
fn past_a_ceiling_a_connection_is_told_why_and_closed_and_the_daemon_serves_on() {
    let s = Served::start(|_| {}, set_ceiling(5));
    // The start's own health polls may still be giving their places back.
    let mut held: Vec<Conn> = Vec::new();
    while held.len() < 5 {
        let mut c = Conn::open(&s);
        match c.health_or_refusal() {
            Some(h) => {
                assert_eq!(h["result"]["push"]["connections"]["ceiling"], 5);
                held.push(c);
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    // Two past it: each gets the one frame, then the close.
    for _ in 0..2 {
        let mut extra = Conn::open(&s);
        let frame = extra.line().expect("the refusal frame");
        assert!(refusal_of(&frame), "{frame}");
        assert_eq!(frame["error"]["data"]["ceiling"], 5);
        assert!(extra.line().is_none(), "closed after the frame");
    }
    let c = held[0].health()["push"]["connections"].clone();
    assert_eq!(
        (c["held"].as_u64(), c["refused"].as_u64()),
        (Some(5), Some(2))
    );
    // A place given back is taken again.
    drop(held.pop());
    let mut again = None;
    for _ in 0..200 {
        let mut c = Conn::open(&s);
        let h = c.health_or_refusal();
        if h.is_some() {
            again = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(again.is_some(), "a closed connection's place is free again");
    drop((held, again));
    let mut alive = false;
    for _ in 0..200 {
        alive = s.daemon_alive();
        if alive {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(alive, "{}", s.log());
}

impl Conn {
    /// `health`, unless the daemon answers with the refusal first.
    fn health_or_refusal(&mut self) -> Option<Value> {
        let req = json!({"jsonrpc": "2.0", "id": 7, "method": "health", "params": null});
        if self
            .0
            .get_mut()
            .write_all(format!("{req}\n").as_bytes())
            .is_err()
        {
            return None;
        }
        let v = self.line()?;
        (!refusal_of(&v)).then_some(v)
    }
}

impl Served {
    fn daemon_alive(&self) -> bool {
        self.call("health", Value::Null).is_ok()
    }
}

#[test]
fn the_soft_limit_is_raised_to_the_hard_one_and_the_ceiling_follows_it() {
    // Born with 200 soft and 4,096 hard: raised, the ceiling is the hard
    // limit less the 256 reserve, and 300 connections all fit.
    let s = Served::start_with(|_| {}, |_| {}, born_with(200, 4096));
    let mut held: Vec<Conn> = (0..300).map(|_| Conn::open(&s)).collect();
    let c = held[0].health()["push"]["connections"].clone();
    assert_eq!(c["fd_soft"], 4096, "{c}");
    assert_eq!(c["ceiling"], 4096 - 256, "{c}");
    for conn in held.iter_mut().step_by(37) {
        assert_eq!(conn.health()["push"]["connections"]["refused"], 0);
    }
    assert!(s.log().contains("open files limit"), "{}", s.log());
}

#[test]
fn a_daemon_that_cannot_raise_its_limit_refuses_under_it_and_lives() {
    // 200 soft and hard: no raise, so the ceiling is 200 less its reserve
    // (half, under 512): 100. The daemon's own descriptors, the store's among
    // them, stay clear of it.
    let s = Served::start_with(|_| {}, |_| {}, born_with(200, 200));
    let mut served = Vec::new();
    let mut refused = 0;
    for _ in 0..130 {
        let mut c = Conn::open(&s);
        match c.health_or_refusal() {
            Some(_) => served.push(c),
            None => refused += 1,
        }
    }
    assert_eq!((served.len(), refused), (100, 30), "{}", s.log());
    let c = served[0].health()["push"]["connections"].clone();
    assert_eq!(
        (c["ceiling"].as_u64(), c["refused"].as_u64()),
        (Some(100), Some(30))
    );
    assert!(served[1].health()["name"].is_string(), "{}", s.log());
}

#[test]
fn two_thousand_connections_leave_a_daemon_that_serves_health() {
    // The test process's own limit first: it holds the clients' end.
    let mut r = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: plain getrlimit/setrlimit on this process.
    unsafe {
        assert_eq!(libc::getrlimit(libc::RLIMIT_NOFILE, &mut r), 0);
        r.rlim_cur = r.rlim_max.min(8192);
        assert_eq!(libc::setrlimit(libc::RLIMIT_NOFILE, &r), 0);
    }
    assert!(
        r.rlim_cur >= 2300,
        "the hard limit is too low for this test"
    );
    // Born with the usual 1,024: the raise is what lets 2,000 in.
    let s = Served::start_with(|_| {}, |_| {}, born_with(1024, r.rlim_max));
    let mut held: Vec<Conn> = (0..2000).map(|_| Conn::open(&s)).collect();
    let c = held[1999].health()["push"]["connections"].clone();
    assert_eq!(c["refused"], 0, "{c}");
    assert!(c["held"].as_u64().unwrap() >= 2000, "{c}");
    drop(held);
    assert!(s.daemon_alive(), "{}", s.log());
}

#[test]
fn an_accept_error_is_logged_once_and_the_daemon_serves_on() {
    // A debug build's plant: the first 4 accepts fail with EMFILE.
    let s = Served::start_with(
        |_| {},
        |_| {},
        |cmd| {
            cmd.env("THESEUS_TEST_ACCEPT_ERRORS", "4");
        },
    );
    assert!(s.daemon_alive(), "{}", s.log());
    let log = s.log();
    let said = log.lines().filter(|l| l.contains("accept failed")).count();
    assert_eq!(said, 1, "one line for the burst, not one each:\n{log}");
    assert!(log.contains("socket"), "{log}");
}
