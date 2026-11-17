//! `config.show` over the wire (issue 23): the whole text, before and after,
//! of a change waiting for the owner's yes, for `theseus config show` and the
//! cockpit's card. A read, and the owner's alone, judged as `config.history`
//! is (`Act::Settings`: the owner, from a private place; never an agent's
//! process, never a job, never an MCP client): a prompt file's text is
//! what goes into every prompt.

use serde_json::Value;
use theseus_protocol::{ConfigShowParams, ConfigShowResult};

use super::confirms::Act;
use super::projects::failure;
use super::server::{route, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};

impl Core {
    pub(super) fn rpc_config_show(
        &self,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        route(params, |p: ConfigShowParams| {
            let who = conn.answerer(p.author.clone(), p.discord.clone());
            self.config_show(&p, who).map_err(failure)
        })
    }

    /// The whole text as words, after the place rule has judged `who`.
    pub fn config_show(
        &self,
        p: &ConfigShowParams,
        who: impl Into<Answerer>,
    ) -> anyhow::Result<ConfigShowResult> {
        let what = "The whole text of a change waiting for your yes";
        let who = who.into();
        if who.agent.is_some() {
            let r = Refusal {
                who: who.who(),
                via: who.via(),
                why: "an agent's process is not the owner: the text of a change to Theseus's own \
                      files is the owner's alone, from their own terminal, the web UI or a \
                      private place"
                    .into(),
            };
            self.refused(Act::Settings { what }, &who, &r)?;
            return Err(r.into());
        }
        self.judge_act(&who, Act::Settings { what })?;
        let (question, words) = crate::config_change::show::words(self, p.question.as_deref())
            .map_err(anyhow::Error::msg)?;
        Ok(ConfigShowResult { question, words })
    }
}
