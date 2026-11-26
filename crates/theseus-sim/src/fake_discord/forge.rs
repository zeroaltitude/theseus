//! A forged press for the stand-in (issue 56): an interaction that names a
//! button the message does not carry, as a crafted client could send one,
//! on a real message, and optionally from another channel than the
//! message's. The message itself is left exactly as it was, buttons and all,
//! so a test can see that nothing the forgery asked for was done to it.

use super::{fake_gateway, message_json, FakeDiscord, Where, APP_ID, DEFAULT_GUILD, TOKEN_PREFIX};

/// A forged press: `custom_id` on `message`, by `user`, said to come from
/// `channel` (the message's own when None).
pub struct Forged<'a> {
    pub message: &'a str,
    pub custom_id: &'a str,
    pub user: (u64, &'a str),
    pub channel: Option<u64>,
}

impl FakeDiscord {
    /// Send a forged press through the gateway: the interaction carries the
    /// message as it is, and the forged id as the pressed one. The
    /// interaction's id, or why it could not be sent.
    pub fn press_forged(&self, f: &Forged<'_>) -> Result<String, String> {
        let gw = self.gateway.get().ok_or("the fake serves no gateway")?;
        let (msg, at, iid) = {
            let mut st = self.state.lock().unwrap();
            let msg = st
                .messages
                .iter()
                .find(|m| m.id == f.message)
                .cloned()
                .ok_or_else(|| format!("no message {}", f.message))?;
            let channel = f
                .channel
                .unwrap_or_else(|| msg.channel.parse().unwrap_or(0));
            let guild = if st.dms.contains_key(&channel) {
                None
            } else {
                Some(st.guild.as_ref().map_or(DEFAULT_GUILD, |g| g.id))
            };
            st.next_id += 1;
            let iid = st.next_id;
            st.pressed.insert(iid.to_string(), channel);
            (msg, Where { channel, guild }, iid)
        };
        let token = format!("{TOKEN_PREFIX}{iid}");
        let i = fake_gateway::Interaction {
            id: iid,
            app: APP_ID,
            token: &token,
            at,
            user: f.user,
        };
        let mut message = message_json(&msg);
        message["components"] = msg.components_json.clone();
        let d = fake_gateway::component_press_with(&i, message, f.custom_id, 2, &[]);
        if !gw.dispatch("INTERACTION_CREATE", d) {
            return Err("no client is connected to the gateway".into());
        }
        Ok(iid.to_string())
    }
}
