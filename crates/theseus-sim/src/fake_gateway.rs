//! A stand-in for Discord's gateway on 127.0.0.1 (theseus-6g62), for tests
//! and scratch daemons. The binding's `[discord] gateway_proxy` connects here
//! instead of to Discord: enough of the gateway's v10, in JSON without
//! compression, for twilight-gateway 0.17.
//!
//! - It says HELLO on connect, READY to an IDENTIFY, and RESUMED to a RESUME,
//!   and it acknowledges every heartbeat.
//! - `dispatch` sends an event (op 0) to the client that identified last.
//!   `FakeDiscord::say` and `FakeDiscord::press` build the two a person makes,
//!   a message typed (`MESSAGE_CREATE`) and a button pressed
//!   (`INTERACTION_CREATE`), as Discord sends them.
//! - It never keeps or prints what an IDENTIFY or a RESUME carries, since the
//!   bot's token is in both: it counts them.
//!
//! It serves until the process ends, one thread per connection.

use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tungstenite::{Message, WebSocket};

/// What READY carries, built when a client identifies.
pub type Ready = dyn Fn() -> Value + Send + Sync;

/// Discord's heartbeat interval; a test ends long before the first beat.
const HEARTBEAT_MS: u64 = 41_250;

/// How long a connection's read waits before it sends what is queued.
const POLL: Duration = Duration::from_millis(20);

/// What the gateway has done, as `state` reports it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct GatewayState {
    pub connects: u32,
    pub identifies: u32,
    pub resumes: u32,
    pub heartbeats: u32,
    /// The client that identified or resumed last is still connected.
    pub connected: bool,
    /// Each dispatch sent, by its type, in order (READY and RESUMED too).
    pub sent: Vec<String>,
}

#[derive(Default)]
struct Inner {
    st: GatewayState,
    seq: u64,
    /// The connection that identified or resumed last, by number.
    current: Option<u64>,
    next_conn: u64,
    /// Frames waiting for the current connection to send them.
    queue: Vec<String>,
}

pub struct FakeGateway {
    /// `127.0.0.1:<port>`.
    pub addr: String,
    inner: Arc<(Mutex<Inner>, Condvar)>,
    ready: Arc<Ready>,
}

impl FakeGateway {
    /// Listen on `addr` (`127.0.0.1:0` for any port). `ready` builds what
    /// READY carries each time a client identifies.
    pub fn start_on(addr: &str, ready: Arc<Ready>) -> std::io::Result<Arc<Self>> {
        let listener = TcpListener::bind(addr)?;
        let me = Arc::new(Self {
            addr: listener.local_addr()?.to_string(),
            inner: Arc::default(),
            ready,
        });
        let gw = me.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let gw = gw.clone();
                std::thread::spawn(move || gw.serve(stream));
            }
        });
        Ok(me)
    }

    /// What `[discord] gateway_proxy` takes.
    pub fn url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    pub fn state(&self) -> GatewayState {
        self.inner.0.lock().unwrap().st.clone()
    }

    /// Send event `t` with data `d` to the client that identified last.
    /// False when no client is connected that has identified.
    pub fn dispatch(&self, t: &str, d: Value) -> bool {
        let (m, cv) = &*self.inner;
        let mut g = m.lock().unwrap();
        if !g.st.connected {
            return false;
        }
        g.seq += 1;
        let frame = json!({"op": 0, "t": t, "s": g.seq, "d": d}).to_string();
        g.queue.push(frame);
        g.st.sent.push(t.to_string());
        cv.notify_all();
        true
    }

    /// Wait until a client has identified (or resumed) and is connected.
    pub fn wait_connected(&self, within: Duration) -> bool {
        let (m, cv) = &*self.inner;
        let end = Instant::now() + within;
        let mut g = m.lock().unwrap();
        while !g.st.connected {
            let now = Instant::now();
            if now >= end {
                return false;
            }
            g = cv.wait_timeout(g, end - now).unwrap().0;
        }
        true
    }

    fn serve(&self, stream: TcpStream) {
        let conn = {
            let mut g = self.inner.0.lock().unwrap();
            g.st.connects += 1;
            g.next_conn += 1;
            g.next_conn
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        if let Ok(mut ws) = tungstenite::accept(stream) {
            let _ = self.session(&mut ws, conn);
        }
        let (m, cv) = &*self.inner;
        let mut g = m.lock().unwrap();
        if g.current == Some(conn) {
            g.current = None;
            g.st.connected = false;
            g.queue.clear();
        }
        cv.notify_all();
    }

    /// One connection, until it closes: HELLO, then each frame the client
    /// sends answered, and what is queued for it sent.
    fn session(&self, ws: &mut WebSocket<TcpStream>, conn: u64) -> tungstenite::Result<()> {
        ws.get_mut().set_read_timeout(Some(POLL))?;
        send(
            ws,
            json!({"op": 10, "t": null, "s": null, "d": {"heartbeat_interval": HEARTBEAT_MS}}),
        )?;
        loop {
            for frame in self.take(conn) {
                ws.send(Message::text(frame))?;
            }
            match ws.read() {
                Ok(Message::Text(t)) => self.answer(ws, conn, t.as_str())?,
                Ok(Message::Close(_)) => return Ok(()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// The frames queued for `conn`, when it is the current connection.
    fn take(&self, conn: u64) -> Vec<String> {
        let mut g = self.inner.0.lock().unwrap();
        if g.current == Some(conn) {
            std::mem::take(&mut g.queue)
        } else {
            Vec::new()
        }
    }

    /// A client's frame. An IDENTIFY's and a RESUME's data hold the token:
    /// they are counted, and never kept.
    fn answer(
        &self,
        ws: &mut WebSocket<TcpStream>,
        conn: u64,
        text: &str,
    ) -> tungstenite::Result<()> {
        let op = serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|v| v["op"].as_u64());
        match op {
            Some(1) => {
                self.inner.0.lock().unwrap().st.heartbeats += 1;
                send(ws, json!({"op": 11, "t": null, "s": null, "d": null}))
            }
            Some(2) => {
                let d = (self.ready)();
                let s = self.become_current(conn, "READY", |st| st.identifies += 1);
                send(ws, json!({"op": 0, "t": "READY", "s": s, "d": d}))
            }
            Some(6) => {
                let s = self.become_current(conn, "RESUMED", |st| st.resumes += 1);
                send(ws, json!({"op": 0, "t": "RESUMED", "s": s, "d": null}))
            }
            _ => Ok(()),
        }
    }

    /// `conn` takes the dispatches from now on; the sequence number its
    /// first dispatch, `t`, carries.
    fn become_current(&self, conn: u64, t: &str, count: impl FnOnce(&mut GatewayState)) -> u64 {
        let (m, cv) = &*self.inner;
        let mut g = m.lock().unwrap();
        count(&mut g.st);
        g.current = Some(conn);
        g.queue.clear();
        g.st.connected = true;
        g.st.sent.push(t.to_string());
        g.seq += 1;
        cv.notify_all();
        g.seq
    }
}

fn send(ws: &mut WebSocket<TcpStream>, v: Value) -> tungstenite::Result<()> {
    ws.send(Message::text(v.to_string()))
}

/// A user as the gateway's payloads carry one.
pub fn user_json(id: u64, name: &str, bot: bool) -> Value {
    json!({"id": id.to_string(), "username": name, "discriminator": "0", "global_name": null,
        "avatar": null, "bot": bot, "public_flags": 0})
}

/// A guild member's part of a payload: the user is beside it.
fn member_json() -> Value {
    json!({"roles": [], "joined_at": "2026-09-30T00:00:00.000000+00:00", "deaf": false,
        "mute": false, "flags": 0, "communication_disabled_until": null, "nick": null})
}

/// Where a payload happens: a guild channel, or a DM (no guild).
#[derive(Debug, Clone, Copy)]
pub struct Where {
    pub channel: u64,
    pub guild: Option<u64>,
}

/// `MESSAGE_CREATE`: `author` typed `content` in `at`. Every user its
/// content mentions (`<@id>`) is in `mentions`, as Discord fills it.
pub fn message_create(
    id: u64,
    at: Where,
    author: (u64, &str),
    content: &str,
    mentioned: &[(u64, bool)],
) -> Value {
    let mentions: Vec<Value> = mentioned
        .iter()
        .map(|&(u, bot)| user_json(u, &format!("user-{u}"), bot))
        .collect();
    let mut d = json!({"id": id.to_string(), "channel_id": at.channel.to_string(),
        "author": user_json(author.0, author.1, false), "content": content,
        "timestamp": "2026-10-01T22:30:00.000000+00:00", "edited_timestamp": null, "tts": false,
        "mention_everyone": false, "mentions": mentions, "mention_roles": [], "attachments": [],
        "embeds": [], "pinned": false, "type": 0, "flags": 0, "components": []});
    if let Some(g) = at.guild {
        d["guild_id"] = json!(g.to_string());
        d["member"] = member_json();
    }
    d
}

/// One interaction, as the gateway's payload names it.
pub struct Interaction<'a> {
    pub id: u64,
    pub app: u64,
    pub token: &'a str,
    pub at: Where,
    pub user: (u64, &'a str),
}

/// `INTERACTION_CREATE` for a press of the button `custom_id` on `message`
/// (the message as Discord answers it, its components included).
pub fn component_press(i: &Interaction<'_>, message: Value, custom_id: &str) -> Value {
    let mut d = json!({"id": i.id.to_string(), "application_id": i.app.to_string(), "type": 3,
        "token": i.token, "version": 1, "channel_id": i.at.channel.to_string(),
        "message": message,
        "data": {"custom_id": custom_id, "component_type": 2, "values": []},
        "entitlements": [], "locale": "en-US", "app_permissions": "0"});
    match i.at.guild {
        Some(g) => {
            let mut member = member_json();
            member["user"] = user_json(i.user.0, i.user.1, false);
            d["guild_id"] = json!(g.to_string());
            d["guild_locale"] = json!("en-US");
            d["channel"] =
                json!({"id": i.at.channel.to_string(), "type": 0, "guild_id": g.to_string()});
            d["member"] = member;
            d["authorizing_integration_owners"] = json!({"0": g.to_string()});
            d["context"] = json!(0);
        }
        None => {
            d["channel"] = json!({"id": i.at.channel.to_string(), "type": 1});
            d["user"] = user_json(i.user.0, i.user.1, false);
            d["authorizing_integration_owners"] = json!({"1": i.user.0.to_string()});
            d["context"] = json!(1);
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_json(ws: &mut WebSocket<TcpStream>) -> Value {
        loop {
            if let Message::Text(t) = ws.read().unwrap() {
                return serde_json::from_str(t.as_str()).unwrap();
            }
        }
    }

    /// HELLO, READY for an IDENTIFY, an ACK per heartbeat, and a dispatch in
    /// order with its sequence number. What the IDENTIFY carried is kept
    /// nowhere: the state holds only counts.
    #[test]
    fn it_identifies_acknowledges_and_dispatches_and_keeps_no_token() {
        let gw = FakeGateway::start_on(
            "127.0.0.1:0",
            Arc::new(|| json!({"v": 10, "session_id": "s"})),
        )
        .unwrap();
        assert!(
            !gw.dispatch("MESSAGE_CREATE", json!({})),
            "nobody to send to"
        );
        let stream =
            TcpStream::connect_timeout(&gw.addr.parse().unwrap(), Duration::from_secs(5)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (mut ws, _) =
            tungstenite::client::client(format!("{}/?v=10&encoding=json", gw.url()), stream)
                .unwrap_or_else(|e| panic!("handshake: {e}"));
        let hello = read_json(&mut ws);
        assert_eq!(hello["op"], 10);
        assert_eq!(hello["d"]["heartbeat_interval"], HEARTBEAT_MS);
        let token = "not-a-real-token-but-keep-it";
        ws.send(Message::text(
            json!({"op": 2, "d": {"token": token, "intents": 0}}).to_string(),
        ))
        .unwrap();
        let ready = read_json(&mut ws);
        assert_eq!(
            (ready["op"].clone(), ready["t"].clone()),
            (json!(0), json!("READY"))
        );
        assert_eq!(ready["d"]["session_id"], "s");
        assert!(gw.wait_connected(Duration::from_secs(5)));
        ws.send(Message::text(json!({"op": 1, "d": 1}).to_string()))
            .unwrap();
        assert_eq!(read_json(&mut ws)["op"], 11);
        assert!(gw.dispatch("MESSAGE_CREATE", json!({"content": "hi"})));
        let m = read_json(&mut ws);
        assert_eq!(m["t"], "MESSAGE_CREATE");
        assert_eq!(m["d"]["content"], "hi");
        assert!(m["s"].as_u64().unwrap() > ready["s"].as_u64().unwrap());
        let st = gw.state();
        assert_eq!((st.connects, st.identifies, st.heartbeats), (1, 1, 1));
        assert_eq!(st.sent, ["READY", "MESSAGE_CREATE"]);
        let shown = serde_json::to_string(&st).unwrap();
        assert!(!shown.contains(token), "{shown}");
        drop(ws);
    }

    /// A press in a guild carries its member, and in a DM its user; both
    /// name the button and carry the message it is on.
    #[test]
    fn a_press_is_shaped_as_discord_sends_it() {
        let i = Interaction {
            id: 7,
            app: 2,
            token: "t-7",
            at: Where {
                channel: 10,
                guild: Some(1),
            },
            user: (5, "ana"),
        };
        let d = component_press(&i, json!({"id": "99"}), "confirm:approve:act_1");
        assert_eq!(d["data"]["custom_id"], "confirm:approve:act_1");
        assert_eq!(d["member"]["user"]["id"], "5");
        assert_eq!(d["guild_id"], "1");
        assert_eq!(d["message"]["id"], "99");
        let dm = Interaction {
            at: Where {
                channel: 6,
                guild: None,
            },
            ..i
        };
        let d = component_press(&dm, json!({"id": "99"}), "x");
        assert_eq!(d["user"]["id"], "5");
        assert!(d.get("member").is_none() && d.get("guild_id").is_none());
        let m = message_create(
            8,
            Where {
                channel: 10,
                guild: Some(1),
            },
            (5, "ana"),
            "<@3> hi",
            &[(3, true)],
        );
        assert_eq!(m["mentions"][0]["id"], "3");
        assert_eq!(m["mentions"][0]["bot"], true);
        assert_eq!(m["guild_id"], "1");
    }
}
