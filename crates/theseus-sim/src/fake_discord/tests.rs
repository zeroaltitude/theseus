use super::*;

mod reread;

/// Discord notifies a mentioned user only when `allowed_mentions` allows
/// it, and every mentioned user when a create sends none (theseus-9j9).
#[test]
fn a_create_notifies_only_the_mentions_it_allows() {
    let text = "<@101> <@!202> look, and <@303 and <@101> again";
    assert_eq!(notified(text, &Value::Null), ["101", "202"]);
    assert!(notified(text, &json!({"parse": [], "replied_user": false})).is_empty());
    assert_eq!(
        notified(text, &json!({"parse": [], "users": ["202", "404"]})),
        ["202"]
    );
    assert_eq!(notified(text, &json!({"parse": ["users"]})), ["101", "202"]);
}

/// One request to the fake, as a client sends it: the status and the
/// body.
fn http(addr: &str, method: &str, path: &str, body: &Value) -> (u16, Value) {
    let addr = addr.parse().unwrap();
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let b = if body.is_null() {
        String::new()
    } else {
        body.to_string()
    };
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nhost: fake\r\ncontent-length: {}\r\n\r\n{b}",
        b.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

const ANA: u64 = 900_000_000_000_000_101;
const CY: u64 = 900_000_000_000_000_303;
const LAB: u64 = 900_000_000_000_000_010;
const COMMONS: u64 = 900_000_000_000_000_020;

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(CY, "cy")
        .private_channel(LAB, "lab", &[ANA])
        .channel(COMMONS, "commons")
}

/// theseus-ck0k: with a guild set, the reads the binding's viewer check
/// makes answer as Discord does, in the shapes its model reads: the
/// application's flags with the members intent, a channel and its
/// overwrites, the guild with its owner and roles, and its members, a
/// page at a time. Without one, the application has no such intent.
#[test]
fn the_viewer_checks_reads_answer_from_the_guild() {
    use twilight_model::oauth::{Application, ApplicationFlags};
    let fake = FakeDiscord::start();
    let get = |p: &str| http(&fake.addr, "GET", &format!("/api/v10{p}"), &Value::Null);
    let app: Application = serde_json::from_value(get("/applications/@me").1).unwrap();
    assert_eq!(app.flags, Some(ApplicationFlags::empty()));
    assert_eq!(get("/guilds/{DEFAULT_GUILD}").0, 404);
    assert_eq!(get("/users/@me/guilds").1, json!([]));
    fake.set_guild(guild());
    let app: Application = serde_json::from_value(get("/applications/@me").1).unwrap();
    assert!(app
        .flags
        .unwrap()
        .contains(ApplicationFlags::GATEWAY_GUILD_MEMBERS_LIMITED));
    let ch: twilight_model::channel::Channel =
        serde_json::from_value(get(&format!("/channels/{LAB}")).1).unwrap();
    assert_eq!(ch.guild_id.map(|g| g.get()), Some(DEFAULT_GUILD));
    let ow = ch.permission_overwrites.unwrap();
    assert_eq!(
        ow.len(),
        3,
        "@everyone denied, ana and the bot allowed: {ow:?}"
    );
    let g: twilight_model::guild::Guild =
        serde_json::from_value(get(&format!("/guilds/{DEFAULT_GUILD}")).1).unwrap();
    assert_eq!(g.owner_id.get(), ANA);
    assert_eq!(g.roles[0].id.get(), DEFAULT_GUILD, "@everyone first");
    let page = |q: &str| -> Vec<u64> {
        let v = get(&format!("/guilds/{DEFAULT_GUILD}/members?{q}")).1;
        let ms: Vec<twilight_model::guild::Member> = serde_json::from_value(v).unwrap();
        ms.iter().map(|m| m.user.id.get()).collect()
    };
    assert_eq!(page("limit=1000"), [ANA, CY, BOT_ID]);
    assert_eq!(page(&format!("limit=1&after={ANA}")), [CY]);
    assert_eq!(
        get(&format!("/guilds/{DEFAULT_GUILD}/members/{BOT_ID}")).0,
        200
    );
    assert_eq!(get("/channels/12345").0, 404);
    assert_eq!(
        get("/users/@me/guilds").1[0]["id"],
        DEFAULT_GUILD.to_string()
    );
}

/// theseus-6g62: an interaction's callback is answered 204, and its
/// original response's edit and its follow-up a message; every answer
/// is kept, and no token reaches the record.
#[test]
fn interaction_answers_are_kept_and_their_tokens_are_not() {
    let fake = FakeDiscord::start();
    let tok = format!("{TOKEN_PREFIX}77");
    let (s, _) = http(
        &fake.addr,
        "POST",
        &format!("/api/v10/interactions/77/{tok}/callback"),
        &json!({"type": 6}),
    );
    assert_eq!(s, 204);
    let (s, m) = http(
        &fake.addr,
        "POST",
        &format!("/api/v10/webhooks/{APP_ID}/{tok}?wait=true"),
        &json!({"content": "only you see this", "flags": 64}),
    );
    assert_eq!(s, 200);
    let _: twilight_model::channel::Message = serde_json::from_value(m).unwrap();
    let (s, _) = http(
        &fake.addr,
        "PATCH",
        &format!("/api/v10/webhooks/{APP_ID}/{tok}/messages/@original"),
        &json!({"content": "done"}),
    );
    assert_eq!(s, 200);
    let r = fake.replies();
    assert!(
        r.iter().all(|a| a.interaction.as_deref() == Some("77")),
        "{r:?}"
    );
    let kinds: Vec<&str> = r.iter().map(|a| a.kind.as_str()).collect();
    assert_eq!(kinds, ["callback", "followup", "original"]);
    assert_eq!(
        (r[0].response_type, r[0].content.as_deref()),
        (Some(6), None)
    );
    assert_eq!(
        (r[1].content.as_deref(), r[1].ephemeral),
        (Some("only you see this"), true)
    );
    assert_eq!(
        (r[2].content.as_deref(), r[2].ephemeral),
        (Some("done"), false)
    );
    // A route under /webhooks the fake doesn't know is a 404 that
    // answers no interaction, and is recorded as asked, token cut out.
    let (s, _) = http(
        &fake.addr,
        "POST",
        &format!("/api/v10/webhooks/{APP_ID}/{tok}/messages/@sideways"),
        &json!({"content": "?"}),
    );
    assert_eq!(s, 404);
    assert_eq!(fake.replies().len(), 3);
    let unknown = fake.seen().into_iter().find(|s| s.outcome == "unknown");
    assert_eq!(
        unknown.map(|s| s.path),
        Some(format!("/webhooks/{APP_ID}/{{token}}/messages/@sideways"))
    );
    assert!(fake.seen().iter().all(|s| !s.path.contains(&tok)));
}

/// theseus-qifw: a message keeps each content it had, in order, and its
/// buttons until an edit takes them away.
#[test]
fn a_message_keeps_every_version_and_its_buttons() {
    let fake = FakeDiscord::start();
    let buttons = json!([{"type": 1, "components": [
        {"type": 2, "style": 3, "label": "Approve", "custom_id": "confirm:approve:act_1"},
        {"type": 2, "style": 4, "label": "Decline", "custom_id": "confirm:decline:act_1"}]}]);
    let (_, m) = http(
        &fake.addr,
        "POST",
        &format!("/api/v10/channels/{LAB}/messages"),
        &json!({"content": "**Approve?**", "components": buttons}),
    );
    let id = m["id"].as_str().unwrap().to_string();
    let msg = &fake.messages(LAB)[0];
    let labels: Vec<&str> = msg.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Approve", "Decline"]);
    assert_eq!(msg.author, BOT_ID.to_string());
    http(
        &fake.addr,
        "PATCH",
        &format!("/api/v10/channels/{LAB}/messages/{id}"),
        &json!({"content": "✅ **Approved**", "components": []}),
    );
    let msg = &fake.messages(LAB)[0];
    assert_eq!(msg.versions, ["**Approve?**", "✅ **Approved**"]);
    assert!(msg.buttons.is_empty() && msg.components == 0);
}

fn read_frame(ws: &mut tungstenite::WebSocket<TcpStream>) -> Value {
    loop {
        if let tungstenite::Message::Text(t) = ws.read().unwrap() {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

/// theseus-6g62: READY, a typed message, and a press go through the
/// gateway in the shapes the binding's model reads. A press names a
/// button the message has; the message rides along, buttons and all.
#[test]
fn say_and_press_send_what_the_binding_reads() {
    use twilight_model::application::interaction::{Interaction, InteractionData};
    let fake = FakeDiscord::start_with_gateway();
    fake.set_guild(guild());
    let gw = fake.gateway().unwrap();
    let stream =
        TcpStream::connect_timeout(&gw.addr.parse().unwrap(), Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let (mut ws, _) = tungstenite::client::client(gw.url(), stream).unwrap();
    assert_eq!(read_frame(&mut ws)["op"], 10);
    ws.send(tungstenite::Message::text(
        json!({"op": 2, "d": {}}).to_string(),
    ))
    .unwrap();
    let ready = read_frame(&mut ws);
    let r: twilight_model::gateway::payload::incoming::Ready =
        serde_json::from_value(ready["d"].clone()).unwrap();
    assert_eq!(r.guilds[0].id.get(), DEFAULT_GUILD);
    assert_eq!(r.user.id.get(), BOT_ID);
    let typed = format!("<@{BOT_ID}> hello");
    fake.say(&Typed {
        user: ANA,
        name: "ana",
        channel: Some(LAB),
        content: &typed,
        file: None,
    })
    .unwrap();
    let m: twilight_model::channel::Message =
        serde_json::from_value(read_frame(&mut ws)["d"].clone()).unwrap();
    assert_eq!(m.guild_id.map(|g| g.get()), Some(DEFAULT_GUILD));
    assert_eq!(m.author.id.get(), ANA);
    assert_eq!(m.mentions[0].id.get(), BOT_ID);
    let buttons = json!([{"type": 1, "components": [
        {"type": 2, "style": 3, "label": "Approve", "custom_id": "confirm:approve:act_1"}]}]);
    let (_, card) = http(
        &fake.addr,
        "POST",
        &format!("/api/v10/channels/{LAB}/messages"),
        &json!({"content": "**Approve?**", "components": buttons}),
    );
    let card = card["id"].as_str().unwrap().to_string();
    let press = |button: &str| {
        fake.press(&Pressed {
            message: &card,
            button,
            user: ANA,
            name: "ana",
        })
    };
    let err = press("Nope").unwrap_err();
    assert!(err.contains("\"Approve\""), "{err}");
    press("Approve").unwrap();
    let i: Interaction = serde_json::from_value(read_frame(&mut ws)["d"].clone()).unwrap();
    let Some(InteractionData::MessageComponent(c)) = &i.data else {
        panic!("{i:?}")
    };
    assert_eq!(c.custom_id, "confirm:approve:act_1");
    assert_eq!(i.author().map(|u| u.id.get()), Some(ANA));
    assert_eq!(i.message.as_ref().unwrap().components.len(), 1);
    assert!(i.token.starts_with(TOKEN_PREFIX));
    fake.say(&Typed {
        user: ANA,
        name: "ana",
        channel: None,
        content: "in the DM",
        file: None,
    })
    .unwrap();
    let dm: twilight_model::channel::Message =
        serde_json::from_value(read_frame(&mut ws)["d"].clone()).unwrap();
    assert!(dm.guild_id.is_none());
    assert_eq!(dm.channel_id.get(), ANA + 1);
    let typed: Vec<String> = fake
        .all_messages()
        .iter()
        .map(|m| m.author.clone())
        .collect();
    assert_eq!(typed.iter().filter(|a| **a == ANA.to_string()).count(), 2);
}

/// The control routes a check in another process uses (`theseus-sim
/// discord say|press|read`, `fake-discord --guild`): each answers, a
/// press of a button the message lacks is refused with why, and none of
/// them is recorded as a request to Discord.
#[test]
fn the_control_routes_drive_and_read_the_fake() {
    let fake = FakeDiscord::start_with_gateway();
    let ctl = |method: &str, route: &str, body: Value| {
        http(&fake.addr, method, &format!("/_fake/{route}"), &body)
    };
    let (s, g) = ctl("GET", "gateway", Value::Null);
    assert_eq!((s, g["connected"].clone()), (200, json!(false)));
    let (s, e) = ctl(
        "POST",
        "say",
        json!({"channel": LAB, "user": ANA, "content": "hi"}),
    );
    assert_eq!(s, 409, "no client yet: {e}");
    let (s, _) = ctl("POST", "guild", serde_json::to_value(guild()).unwrap());
    assert_eq!(s, 200);
    let gw = fake.gateway().unwrap();
    let stream =
        TcpStream::connect_timeout(&gw.addr.parse().unwrap(), Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let (mut ws, _) = tungstenite::client::client(gw.url(), stream).unwrap();
    read_frame(&mut ws);
    ws.send(tungstenite::Message::text(
        json!({"op": 2, "d": {}}).to_string(),
    ))
    .unwrap();
    assert_eq!(
        read_frame(&mut ws)["d"]["guilds"][0]["id"],
        DEFAULT_GUILD.to_string()
    );
    let (s, said) = ctl(
        "POST",
        "say",
        json!({"channel": LAB.to_string(), "user": ANA, "name": "ana", "content": "hi"}),
    );
    assert_eq!(s, 200, "{said}");
    let m = read_frame(&mut ws);
    assert_eq!(
        (m["t"].clone(), m["d"]["content"].clone()),
        (json!("MESSAGE_CREATE"), json!("hi"))
    );
    assert_eq!(m["d"]["id"], said["id"]);
    let (s, e) = ctl(
        "POST",
        "press",
        json!({"message": said["id"], "button": "Approve", "user": ANA, "name": "ana"}),
    );
    assert_eq!(s, 409);
    assert!(e["error"].as_str().unwrap().contains("no button"), "{e}");
    let (_, msgs) = ctl("GET", "messages", Value::Null);
    assert_eq!(msgs[0]["author"], ANA.to_string());
    assert_eq!(ctl("GET", "replies", Value::Null).1, json!([]));
    assert_eq!(ctl("GET", "nothing", Value::Null).0, 404);
    assert!(fake.seen().is_empty(), "{:?}", fake.seen());
}
