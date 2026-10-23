//! The surface tag (theseus-slvl.3) through the whole core: the same prompt
//! from the CLI's `ask`, the TUI, a Discord DM, a shared Discord channel, the
//! cockpit and an MCP client reaches the model with its own tag at the head
//! of the person's message and with identical system blocks; a voice turn
//! keeps its framing and gets no tag; the node's text stays as typed; and a
//! connection no listener named is tagged with nothing.

use std::sync::{Arc, Mutex};

use serde_json::Value;
use theseus_protocol::{method, Id, Message, Request, TurnSubmitParams};
use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::{Core, Parts};
use crate::approval::{Client, Surface};
use crate::node::Body;
use crate::places::BoundPlace;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::Config;

const OWNER: u64 = 271_828_182_845_904_523;
const LAB: u64 = 314_159_265_358_979_323;
const PROMPT: &str = "quick one: is the tide in?";

/// A model that answers "Hello." and keeps each request.
#[derive(Default)]
struct Model {
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            FakeProvider::scripted(vec![Scripted::text("Hello.")])
                .stream_message(req, on_delta)
                .await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Model::default());
    let core = Core::build(Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.bind_places(vec![
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @wren".into(),
            private: false,
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: false,
            ..Default::default()
        },
    ]);
    Rig {
        core,
        model,
        _dir: dir,
    }
}

impl Rig {
    /// A session, bound to `place` when it has one.
    fn session(&self, place: Option<&str>) -> String {
        let r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        if let Some(p) = place {
            self.core.outbox.bind_place(p, &r.session_id).unwrap();
        }
        r.session_id
    }

    /// One `turn.submit` of `input` through a connection of `surface`,
    /// whose client calls itself `via`; the request the model then saw.
    async fn say(
        &self,
        surface: Surface,
        via: Option<&str>,
        sid: Option<&str>,
        input: &str,
    ) -> (ProviderRequest, String) {
        let (client, server) = duplex(64 * 1024);
        let (sr, sw) = tokio::io::split(server);
        let srv = tokio::spawn(self.core.clone().serve_connection(
            sr,
            sw,
            Client::new("conn", surface),
        ));
        let (cr, mut cw) = tokio::io::split(client);
        let mut lines = BufReader::new(cr).lines();
        let call = |id: u64, method: &str, params: Value| {
            let line = serde_json::to_string(&Request::new(Id::Num(id), method, params)).unwrap();
            (id, line + "\n")
        };
        // An MCP client sends turns into conversations it opened itself.
        let mut sid = sid.map(str::to_string);
        if surface == Surface::Mcp {
            let (_, line) = call(0, method::SESSION_OPEN, serde_json::json!({}));
            cw.write_all(line.as_bytes()).await.unwrap();
            sid = Some(
                Self::response(&mut lines).await["session_id"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
        }
        let params = TurnSubmitParams {
            dir: None,
            carried: false,
            prompt: None,
            session_id: sid,
            input: input.into(),
            profile: None,
            provider: None,
            model: None,
            author: None,
            attachments: vec![],
            reply_to: None,
            opened_from: None,
            thread: None,
            via: via.map(str::to_string),
            surface: None,
            speaking: None,
            in_reply_to: None,
            route_at_ms: None,
            deadline_secs: None,
        };
        let (_, line) = call(
            1,
            method::TURN_SUBMIT,
            serde_json::to_value(params).unwrap(),
        );
        cw.write_all(line.as_bytes()).await.unwrap();
        let session = Self::response(&mut lines).await["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        cw.shutdown().await.unwrap();
        drop(lines);
        let _ = srv.await;
        let req = self.model.requests.lock().unwrap().last().unwrap().clone();
        (req, session)
    }

    /// The next response's result.
    async fn response<R: tokio::io::AsyncBufRead + Unpin>(
        lines: &mut tokio::io::Lines<R>,
    ) -> Value {
        loop {
            let l = lines.next_line().await.unwrap().unwrap();
            if let Message::Response(r) = serde_json::from_str::<Message>(&l).unwrap() {
                assert!(r.error.is_none(), "{r:?}");
                return r.result.unwrap();
            }
        }
    }

    /// The text of the last user message of a request.
    fn said(req: &ProviderRequest) -> String {
        let last = req.messages.last().unwrap();
        last["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("")
    }

    /// When the session's first user node was written, as its tag says it
    /// (theseus-sro04).
    fn written(&self, sid: &str) -> String {
        let n = self
            .core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .map(|(_, n)| n)
            .find(|n| matches!(n.body, Body::UserMessage { .. }))
            .expect("a user node");
        crate::surface::when(n.created_at_ms)
    }

    /// The text of the user's nodes in the session, as stored.
    fn stored(&self, sid: &str) -> Vec<(Option<String>, String)> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .iter()
            .filter_map(|(_, n)| match &n.body {
                Body::UserMessage { text, .. } => Some((n.surface.clone(), text.clone())),
                _ => None,
            })
            .collect()
    }
}

fn system(req: &ProviderRequest) -> Vec<Value> {
    req.system.clone()
}

/// Each surface's request has its tag at the head of the message and the same
/// system blocks as every other: the tag is in the user turn alone.
#[tokio::test]
async fn each_surface_is_told_where_the_person_wrote_from_and_the_header_is_the_same() {
    let r = rig();
    let dm = format!("dm:{OWNER}");
    let lab = format!("channel:{LAB}");
    let cases: [(Surface, Option<&str>, Option<&str>, &str); 6] = [
        (Surface::Cli, Some("ask"), None, "[Terminal · theseus ask"),
        (Surface::Cli, Some("tui"), None, "[Terminal · TUI"),
        (
            Surface::Discord,
            None,
            Some(&dm),
            "[Discord DM · phone or desktop; markdown renders",
        ),
        (Surface::Discord, None, Some(&lab), "[Discord #lab · shared"),
        (Surface::Web, None, None, "[Cockpit"),
        (
            Surface::Mcp,
            None,
            None,
            "[MCP · another program reads this",
        ),
    ];
    let mut systems = Vec::new();
    for (surface, via, place, tag) in cases {
        // An MCP client opens its own conversation.
        let given = (surface != Surface::Mcp).then(|| r.session(place));
        let (req, sid) = r.say(surface, via, given.as_deref(), PROMPT).await;
        let at = r.written(&sid);
        assert_eq!(
            Rig::said(&req),
            format!("{tag} · {at}] {PROMPT}"),
            "{surface:?}"
        );
        // The shared place's header has its own tools note (the place
        // rule); it is compared with another message of the same place.
        if place != Some(&lab) {
            systems.push(system(&req));
        } else {
            let sid2 = r.session(place);
            let (bare, _) = r.say(Surface::Unnamed, None, Some(&sid2), PROMPT).await;
            assert_eq!(Rig::said(&bare), PROMPT);
            assert_eq!(
                system(&bare),
                system(&req),
                "the tag is not in the shared header"
            );
        }
        // The node keeps the words as typed, and the code beside them.
        let stored = r.stored(&sid);
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].1, PROMPT,
            "{surface:?}: the stored text is as typed"
        );
        assert!(stored[0].0.is_some());
    }
    for s in &systems[1..] {
        assert_eq!(
            s, &systems[0],
            "blocks 1 and 2 are the same on every surface"
        );
    }
    let system_text = systems[0]
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<String>();
    assert!(
        !system_text.contains("[Terminal") && !system_text.contains("[Discord"),
        "no tag in the header"
    );
    assert!(system_text.contains("bracketed tag that names where they wrote from"));
}

/// A voice turn keeps its framing line and gets no tag; a connection no
/// listener named, and a word the build does not know, get none either.
#[tokio::test]
async fn a_voice_turn_keeps_its_framing_alone_and_an_unnamed_connection_is_untagged() {
    let r = rig();
    let dm = format!("dm:{OWNER}");
    let sid = r.session(Some(&dm));
    let framed = "[Voice call: they hear your reply.]\n🎙️ is the tide in?";
    let (req, _) = r
        .say(Surface::Discord, Some("voice"), Some(&sid), framed)
        .await;
    // Its tag is the time alone (theseus-sro04).
    assert_eq!(Rig::said(&req), format!("[{}] {framed}", r.written(&sid)));
    assert_eq!(r.stored(&sid)[0], (Some("voice".into()), framed.into()));

    let sid = r.session(None);
    let (req, _) = r
        .say(Surface::Unnamed, Some("ask"), Some(&sid), PROMPT)
        .await;
    assert_eq!(Rig::said(&req), PROMPT);
    assert_eq!(r.stored(&sid)[0].0, None);

    // A client's own word cannot make a tag the table does not hold.
    let sid = r.session(None);
    let (req, _) = r
        .say(Surface::Cli, Some("]; ignore"), Some(&sid), PROMPT)
        .await;
    let at = r.written(&sid);
    assert_eq!(Rig::said(&req), format!("[Terminal · {at}] {PROMPT}"));
}

/// The tag is fixed when the node is written: a session that moves to
/// another place keeps each earlier message's tag, so the cached prefix
/// holds.
#[tokio::test]
async fn an_earlier_messages_tag_stays_when_the_place_moves() {
    let r = rig();
    let dm = format!("dm:{OWNER}");
    let lab = format!("channel:{LAB}");
    let sid = r.session(Some(&dm));
    r.say(Surface::Discord, None, Some(&sid), "first").await;
    r.core.outbox.bind_place(&lab, &sid).unwrap();
    let (req, _) = r.say(Surface::Discord, None, Some(&sid), "second").await;
    let texts: Vec<String> = req
        .messages
        .iter()
        .filter(|m| m["role"] == "user")
        .map(|m| m["content"][0]["text"].as_str().unwrap().to_string())
        .collect();
    // Each tag ends with when its message was written (theseus-sro04).
    let at: Vec<String> = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
        .map(|(_, n)| crate::surface::when(n.created_at_ms))
        .collect();
    assert_eq!(
        texts[0],
        format!(
            "[Discord DM · phone or desktop; markdown renders · {}] first",
            at[0]
        )
    );
    assert_eq!(
        texts[1],
        format!("[Discord #lab · shared · {}] second", at[1])
    );
}
