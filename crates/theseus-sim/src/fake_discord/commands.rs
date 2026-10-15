//! Drivers for the task commands (theseus-a5uds): a top-level slash command, an
//! autocomplete, a message command, and a modal's submit, each sent through the
//! gateway as Discord sends it. What the binding answered is `replies()`:
//! an autocomplete's `choices`, a modal it opened (`modal`), its messages.

use serde_json::{json, Value};

use super::{FakeDiscord, APP_ID, DEFAULT_GUILD, TOKEN_PREFIX};
use crate::fake_gateway::{self, Interaction, Where};

/// Who sends an interaction, and where: a guild `channel`, or with None the
/// user's DM with the bot.
#[derive(Clone, Copy)]
pub struct Sender<'a> {
    pub user: u64,
    pub name: &'a str,
    pub channel: Option<u64>,
}

/// A file a modal carries: the attachment the user picked.
pub struct Upload<'a> {
    pub field: &'a str,
    pub name: &'a str,
    pub bytes: &'a [u8],
    pub content_type: &'a str,
}

impl FakeDiscord {
    /// The interaction's place, its id and token.
    fn interaction_at(&self, from: &Sender<'_>) -> (Where, u64) {
        let mut st = self.state.lock().unwrap();
        let at = match from.channel {
            Some(c) => Where {
                channel: c,
                guild: Some(st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id)),
            },
            None => {
                let c = from.user + 1;
                st.dms.insert(c, from.user);
                Where {
                    channel: c,
                    guild: None,
                }
            }
        };
        st.next_id += 1;
        let iid = st.next_id;
        st.pressed.insert(iid.to_string(), at.channel);
        (at, iid)
    }

    fn send_interaction(
        &self,
        from: &Sender<'_>,
        make: impl FnOnce(&Interaction<'_>) -> Value,
    ) -> Result<String, String> {
        let gw = self.gateway.get().ok_or("the fake serves no gateway")?;
        let (at, iid) = self.interaction_at(from);
        let token = format!("{TOKEN_PREFIX}{iid}");
        let i = Interaction {
            id: iid,
            app: APP_ID,
            token: &token,
            at,
            user: (from.user, from.name),
        };
        if !gw.dispatch("INTERACTION_CREATE", make(&i)) {
            return Err("no client is connected to the gateway".into());
        }
        Ok(iid.to_string())
    }

    /// `/<command> key:value…`, a command with no subcommand. The
    /// interaction's id.
    pub fn slash(
        &self,
        from: &Sender<'_>,
        command: &str,
        options: &[(&str, &str)],
    ) -> Result<String, String> {
        let options: Vec<Value> = options
            .iter()
            .map(|(k, v)| json!({"name": k, "type": 3, "value": v}))
            .collect();
        let data =
            json!({"id": "1000000000000000004", "name": command, "type": 1, "options": options});
        self.send_interaction(from, |i| fake_gateway::slash_command(i, None, data))
    }

    /// The autocomplete of `command`'s option `focused`, after `typed`.
    pub fn autocomplete(
        &self,
        from: &Sender<'_>,
        command: &str,
        focused: &str,
        typed: &str,
    ) -> Result<String, String> {
        let data = json!({"id": "1000000000000000004", "name": command, "type": 1,
            "options": [{"name": focused, "type": 3, "value": typed, "focused": true}]});
        self.send_interaction(from, |i| {
            let mut d = fake_gateway::slash_command(i, None, data);
            d["type"] = json!(4);
            d
        })
    }

    /// A pick in the select menu `field` of the message the binding answered
    /// `interaction` with (an ephemeral view), by `user`.
    pub fn pick_in_reply(
        &self,
        interaction: &str,
        field: &str,
        values: &[&str],
        user: u64,
        name: &str,
    ) -> Result<String, String> {
        let m = self
            .reply_message(interaction)
            .ok_or_else(|| format!("no message answered interaction {interaction}"))?;
        self.state.lock().unwrap().messages.push(m.clone());
        let r = self.press_values(
            &super::Pressed {
                message: &m.id,
                button: field,
                user,
                name,
            },
            values,
        );
        self.state.lock().unwrap().messages.retain(|x| x.id != m.id);
        r
    }

    /// A message command (right-click › Apps) on a message with `content`.
    pub fn message_command(
        &self,
        from: &Sender<'_>,
        command: &str,
        content: &str,
    ) -> Result<String, String> {
        let target = "1000000000000000777";
        let data = json!({"id": "1000000000000000005", "name": command, "type": 3,
            "target_id": target,
            "resolved": {"messages": {target: {"id": target, "channel_id": "1", "content": content,
                "author": fake_gateway::user_json(from.user, from.name, false),
                "timestamp": "2026-09-30T00:00:00.000000+00:00", "edited_timestamp": null,
                "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
                "attachments": [], "embeds": [], "pinned": false, "type": 0}}}});
        self.send_interaction(from, |i| fake_gateway::slash_command(i, None, data))
    }

    /// A modal submitted: its `custom_id`, its text inputs, its select menus
    /// (`field`, values), and a file. `message` is the message it was opened
    /// from, when it was opened from a button.
    pub fn submit_modal(
        &self,
        from: &Sender<'_>,
        custom_id: &str,
        texts: &[(&str, &str)],
        selects: &[(&str, &[&str])],
        upload: Option<&Upload<'_>>,
    ) -> Result<String, String> {
        let mut labels: Vec<Value> = Vec::new();
        let mut n = 0;
        let mut next = || {
            n += 2;
            (n - 1, n)
        };
        for (field, value) in texts {
            let (a, b) = next();
            labels.push(json!({"type": 18, "id": a, "component":
                {"type": 4, "id": b, "custom_id": field, "value": value}}));
        }
        for (field, values) in selects {
            let (a, b) = next();
            labels.push(json!({"type": 18, "id": a, "component":
                {"type": 3, "id": b, "custom_id": field, "values": values}}));
        }
        let mut data = json!({"custom_id": custom_id, "components": labels});
        if let Some(u) = upload {
            let id = {
                let mut st = self.state.lock().unwrap();
                st.next_id += 1;
                st.next_id
            };
            let path = format!("attachments/modal-{id}/{}", u.name);
            let url = format!("http://{}/{path}", self.addr);
            self.state
                .lock()
                .unwrap()
                .files
                .insert(path, (u.content_type.to_string(), u.bytes.to_vec()));
            let att = id.to_string();
            data["components"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type": 18, "id": 98, "component":
                    {"type": 19, "id": 99, "custom_id": u.field, "values": [att]}}));
            data["resolved"] = json!({"attachments": {att.clone(): {"id": att, "filename": u.name,
                "size": u.bytes.len(), "url": url, "proxy_url": url, "content_type": u.content_type}}});
        }
        self.send_interaction(from, |i| {
            let mut d = fake_gateway::component_press(i, Value::Null, "");
            d["type"] = json!(5);
            if let Some(o) = d.as_object_mut() {
                o.remove("message");
            }
            d["data"] = data;
            d
        })
    }
}

impl FakeDiscord {
    /// `/_fake/interact`: the drivers above, for a live check across
    /// processes. The body names an `op` (`slash`, `autocomplete`,
    /// `message_command`, `modal`, `pick`, `press_reply`), who sends it
    /// (`user`, `name`, `channel`, none for the DM) and the op's fields. The
    /// answer is the interaction's id; `GET /_fake/replies` shows what the
    /// binding answered.
    pub(super) fn control_interact(&self, body: &Value) -> (u16, Value) {
        let s = |k: &str| body[k].as_str().unwrap_or("");
        let from = Sender {
            user: body["user"].as_u64().unwrap_or(0),
            name: s("name"),
            channel: body["channel"].as_u64(),
        };
        let pairs = |k: &str| -> Vec<(String, String)> {
            body[k]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(a, b)| (a.clone(), b.as_str().unwrap_or("").to_string()))
                .collect()
        };
        let r = match s("op") {
            "slash" => {
                let o = pairs("options");
                let o: Vec<(&str, &str)> =
                    o.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
                self.slash(&from, s("command"), &o)
            }
            "autocomplete" => self.autocomplete(&from, s("command"), s("focused"), s("typed")),
            "message_command" => self.message_command(&from, s("command"), s("content")),
            "modal" => {
                let t = pairs("texts");
                let t: Vec<(&str, &str)> =
                    t.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
                let sel: Vec<(String, Vec<String>)> = body["selects"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(a, b)| {
                        (
                            a.clone(),
                            b.as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect(),
                        )
                    })
                    .collect();
                let sel_refs: Vec<(&str, Vec<&str>)> = sel
                    .iter()
                    .map(|(a, b)| (a.as_str(), b.iter().map(String::as_str).collect()))
                    .collect();
                let sel_slices: Vec<(&str, &[&str])> =
                    sel_refs.iter().map(|(a, b)| (*a, b.as_slice())).collect();
                let bytes = body["file_text"].as_str().map(|t| t.as_bytes().to_vec());
                let up = bytes.as_ref().map(|b| Upload {
                    field: "file",
                    name: body["file_name"].as_str().unwrap_or("file.txt"),
                    bytes: b,
                    content_type: "text/plain",
                });
                self.submit_modal(&from, s("custom_id"), &t, &sel_slices, up.as_ref())
            }
            "pick" => {
                let v: Vec<&str> = body["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                self.pick_in_reply(s("interaction"), s("field"), &v, from.user, from.name)
            }
            "press_reply" => self.press_reply(s("interaction"), s("button"), from.user, from.name),
            other => Err(format!("no such op: {other}")),
        };
        match r {
            Ok(id) => (200, json!({"id": id})),
            Err(e) => (409, json!({"error": e})),
        }
    }
}
