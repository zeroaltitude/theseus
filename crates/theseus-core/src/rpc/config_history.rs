//! `config.history` over the wire (issue 22, undo): the last changes the owner
//! approved to Theseus's own settings, as words, for `theseus config history`
//! and any other surface. A read, and the owner's alone: the words carry every
//! setting a change touched and the name of every secret among them, so it is
//! judged as an act (`Act::Settings`, the place rule: the owner, from a
//! private place) and the MCP server does not offer it.

use serde_json::Value;
use theseus_protocol::{ConfigHistoryParams, ConfigHistoryResult};

use super::confirms::Act;
use super::projects::failure;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::config_change::history;

impl Core {
    pub(super) fn rpc_config_history(
        &self,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        route(params, |p: ConfigHistoryParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            self.config_history(&p, who).map_err(failure)
        })
    }

    /// The history as words, after the place rule has judged `who`.
    pub fn config_history(
        &self,
        p: &ConfigHistoryParams,
        who: impl Into<Answerer>,
    ) -> anyhow::Result<ConfigHistoryResult> {
        let what = "The history of changes to Theseus's settings";
        let who = who.into();
        // An agent's process on the CLI or the web socket (Claude Code, Codex,
        // anything `peer_proc` names) is not the owner, and the history holds
        // every setting and the names of the secrets among them. Refused here,
        // as `judge_act` refuses an act, so the refusal is ledgered the same way.
        if who.agent.is_some() {
            let r = Refusal {
                who: who.who(),
                via: who.via(),
                why: "an agent's process is not the owner: the history of changes to Theseus's \
                      settings is the owner's alone, from their own terminal, the web UI or a \
                      private place"
                    .into(),
            };
            self.refused(Act::Settings { what }, &who, &r)?;
            return Err(r.into());
        }
        self.judge_act(&who, Act::Settings { what })?;
        let limit = p
            .limit
            .map_or(history::SHOWN, |n| (n as usize).clamp(1, history::SHOWN));
        let w = &self.tools.config_change;
        let list = history::entries(
            &history::rows(&self.store),
            &w.state,
            w.file.as_deref(),
            limit,
        );
        Ok(ConfigHistoryResult {
            changes: list.len() as u32,
            words: history::words(&list, theseus_protocol::now_unix_ms()),
        })
    }
}
