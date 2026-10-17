//! Permission overwrites and thread members for the stand-in (theseus-bh3t.4):
//! `PUT` and `DELETE /channels/<c>/permissions/<id>` change a guild channel's
//! overwrites, which the viewer check reads back, and `PUT` and `DELETE
//! /channels/<t>/thread-members/<u>` keep a set of members for each thread,
//! which tests read.

use std::net::TcpStream;

use serde_json::{json, Value};

use super::{reply, reply_empty, seen, FakeDiscord, Overwrite};

/// A permission bit string or number, as Discord sends one.
fn bits(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

impl FakeDiscord {
    /// A guild channel's overwrites now.
    pub fn overwrites_of(&self, channel: u64) -> Vec<Overwrite> {
        let st = self.state.lock().unwrap();
        let found = st
            .guild
            .as_ref()
            .and_then(|g| g.channels.iter().find(|c| c.id == channel));
        found.map(|c| c.overwrites.clone()).unwrap_or_default()
    }

    /// The members the bot added to a thread, in id order.
    pub fn thread_members(&self, thread: u64) -> Vec<u64> {
        let st = self.state.lock().unwrap();
        st.thread_members
            .get(&thread)
            .into_iter()
            .flatten()
            .copied()
            .collect()
    }

    /// `PUT` sets and `DELETE` removes `id`'s overwrite in a channel.
    pub(super) fn overwrite_route(
        &self,
        stream: TcpStream,
        method: &str,
        channel: &str,
        id: &str,
        body: &Value,
    ) -> std::io::Result<()> {
        self.record(seen(
            method,
            &format!("/channels/{channel}/permissions/{id}"),
            "overwrite",
        ));
        let (c, id): (u64, u64) = (channel.parse().unwrap_or(0), id.parse().unwrap_or(0));
        let mut st = self.state.lock().unwrap();
        let Some(ch) = st
            .guild
            .as_mut()
            .and_then(|g| g.channels.iter_mut().find(|x| x.id == c))
        else {
            drop(st);
            return reply(
                stream,
                404,
                &json!({"message": "Unknown Channel", "code": 10003}),
            );
        };
        ch.overwrites.retain(|o| o.id != id);
        if method == "PUT" {
            ch.overwrites.push(Overwrite {
                id,
                kind: body["type"].as_u64().unwrap_or(0) as u8,
                allow: bits(&body["allow"]),
                deny: bits(&body["deny"]),
            });
        }
        drop(st);
        reply_empty(stream)
    }

    /// `PUT` adds and `DELETE` removes a member of a thread.
    pub(super) fn member_route(
        &self,
        stream: TcpStream,
        method: &str,
        thread: &str,
        user: &str,
    ) -> std::io::Result<()> {
        self.record(seen(
            method,
            &format!("/channels/{thread}/thread-members/{user}"),
            "member",
        ));
        let (t, u): (u64, u64) = (thread.parse().unwrap_or(0), user.parse().unwrap_or(0));
        let mut st = self.state.lock().unwrap();
        if !st.threads.contains_key(&t) {
            drop(st);
            return reply(
                stream,
                404,
                &json!({"message": "Unknown Channel", "code": 10003}),
            );
        }
        let members = st.thread_members.entry(t).or_default();
        if method == "PUT" {
            members.insert(u);
        } else {
            members.remove(&u);
        }
        drop(st);
        reply_empty(stream)
    }
}
