//! `/project trust`, `/project untrust` and `/project trusted`
//! (theseus-me9fc): the owner grants a repository's own instruction files
//! from chat, without opening a shell. A child of `runtime`, apart from it
//! for the shape budget's file ceiling, and answered from `orders.rs`'s one
//! interaction chain so `runtime.rs` grows by its `mod` line alone.
//!
//! The three are `project.trust`, `project.untrust` and `project.trusted`
//! as the presser: the presser's `author` and their `DiscordOrigin` ride the
//! call, and **the core judges it** (`Act::Trust`, which asks
//! `places::owner_in_person`). So the chat route is as narrow as the
//! terminal one and no narrower: a grant counts from an owner's **DM**
//! alone, never from a guild channel, not even one the bindings file binds
//! private, and the row records the route it came by in `via`
//! (`discord:dm`). Nothing here decides who may grant; this module only
//! carries the words and shows what the core said.
//!
//! Two things it does decide, and both narrow:
//!
//! - **The list is a private place's.** `project.trusted` is a read with no
//!   place rule of its own, and it names the roots of the owner's own
//!   repositories, so this route answers it only where the place rule says
//!   private (`Shared::is_private`), as `self digest` answers the self log.
//!   Elsewhere it says so and asks nothing of the core.
//! - **A leading `~` is the daemon's home.** The repository is on the
//!   daemon's machine and a chat surface has no working directory, so
//!   `theseus_core::config::expand_home` resolves `~/projects/…` here; the
//!   core still refuses anything that is not absolute after that, as it
//!   refuses the CLI's relative path.
//!
//! Every answer is ephemeral, so the command adds no noise to the place.

use std::sync::Arc;

use theseus_protocol::method;
use theseus_protocol::projects::{
    ProjectGrant, ProjectTrustParams, ProjectTrustedParams, ProjectTrustedResult,
};
use theseus_protocol::{DiscordOrigin, LedgerKind};
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::application_command::{
    CommandDataOption, CommandOptionValue,
};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_util::builder::command::{CommandBuilder, StringBuilder, SubCommandBuilder};

use super::Shared;

/// What the list may hold, with room for the lines after it.
const MAX_TEXT: usize = 1800;

/// What a place that is not private is told instead of the list.
const PRIVATE: &str = "The trusted repositories are private.";

/// The one word for a repository whose path says nothing absolute.
const NO_PATH: &str = "⚠️ Name the repository: `/project trust path:~/projects/tide-gauges`.";

/// `/project trust path:`, `/project untrust path:` and `/project trusted`.
pub(super) fn command() -> Command {
    let path = |what: &str| StringBuilder::new("path", what).required(true);
    CommandBuilder::new(
        "project",
        "A repository's own instruction files: trust them, take the trust back, or list them",
        CommandType::ChatInput,
    )
    .option(
        SubCommandBuilder::new(
            "trust",
            "Let a repository's AGENTS.md and CLAUDE.md join the system block (yours alone)",
        )
        .option(path("Any path inside the repository, absolute or ~/")),
    )
    .option(
        SubCommandBuilder::new(
            "untrust",
            "Take a repository's trust back; its files are named, not read",
        )
        .option(path("Any path inside the repository, absolute or ~/")),
    )
    .option(SubCommandBuilder::new(
        "trusted",
        "The repositories you have trusted, with who granted each and by what route",
    ))
    .build()
}

/// A subcommand and its string options, as `/thread`'s `parsed` reads them.
fn parsed(options: &[CommandDataOption]) -> Option<(&str, Vec<(&str, String)>)> {
    let sub = options.first()?;
    let CommandOptionValue::SubCommand(inner) = &sub.value else {
        return None;
    };
    let args = inner
        .iter()
        .filter_map(|o| match &o.value {
            CommandOptionValue::String(s) => Some((o.name.as_str(), s.clone())),
            _ => None,
        })
        .collect();
    Some((sub.name.as_str(), args))
}

/// The place a command was typed in, as the bindings file names it.
fn typed_in(d: &DiscordOrigin) -> String {
    match d.guild_id {
        Some(_) => format!("discord:channel:{}", d.channel_id),
        None => format!("discord:dm:{}", d.user_id),
    }
}

/// The `path` option with a leading `~` resolved against the daemon's home;
/// none when it was not given or is empty after that.
fn path_of(args: &[(&str, String)]) -> Option<String> {
    let raw = args.iter().find(|(n, _)| *n == "path")?.1.trim();
    (!raw.is_empty()).then(|| {
        theseus_core::config::expand_home(raw)
            .to_string_lossy()
            .into_owned()
    })
}

/// What a grant or a revoke says: the repository and what changed, or why
/// the core refused it, in the core's own words.
fn said(granted: bool, r: Result<ProjectGrant, crate::rpc_client::CallError>) -> String {
    match r {
        Ok(g) if granted => format!(
            "🔓 Trusted `{}` ({}): its AGENTS.md and CLAUDE.md join the system block from the \
             next recompile.",
            g.root, g.id
        ),
        Ok(g) => format!(
            "🔒 `{}` ({}) is no longer trusted: its instruction files are named, not read.",
            g.root, g.id
        ),
        Err(e) if e.code == theseus_protocol::error_code::REFUSED => format!(
            "🔐 That did not count: {}. Nothing was granted or taken back.",
            e.data
                .get("why")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&e.message)
        ),
        Err(e) => format!("⚠️ Nothing changed: {}.", e.message),
    }
}

/// The trusted repositories, one line each, within [`MAX_TEXT`].
fn list_text(r: &ProjectTrustedResult) -> String {
    if r.trusted.is_empty() {
        return "No repository is trusted. `/project trust path:` gives one its instruction files."
            .into();
    }
    let mut out = String::from("**Trusted repositories**\n");
    let mut left = 0;
    for g in &r.trusted {
        let line = format!("`{}` — {} · by {} through {}\n", g.root, g.id, g.by, g.via);
        if out.len() + line.len() > MAX_TEXT {
            left += 1;
            continue;
        }
        out.push_str(&line);
    }
    if left > 0 {
        out.push_str(&format!("-# and {left} more · `theseus project trusted`\n"));
    }
    out
}

impl Shared {
    /// An interaction of `/project`: true when it was one, and answered.
    /// One call from `revokable_interaction`'s chain.
    pub(super) async fn project_trust_interaction(
        self: &Arc<Self>,
        i: &Interaction,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        let Some(InteractionData::ApplicationCommand(c)) = &i.data else {
            return false;
        };
        if c.name != "project" {
            return false;
        }
        self.core.binding_ledger(
            LedgerKind::DiscordCommand,
            None,
            serde_json::json!({"command": c.name, "by": who}),
        );
        let said = match parsed(&c.options) {
            Some((sub @ ("trust" | "untrust"), args)) => match path_of(&args) {
                None => NO_PATH.to_string(),
                Some(path) => {
                    self.project_trust_said(sub == "trust", path, who, discord)
                        .await
                }
            },
            Some(("trusted", _)) => self.project_trusted_said(discord).await,
            _ => "⚠️ `/project trust path:`, `untrust path:` or `trusted`.".to_string(),
        };
        let resp = InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(InteractionResponseData {
                content: Some(said),
                flags: Some(MessageFlags::EPHEMERAL),
                allowed_mentions: Some(AllowedMentions::default()),
                ..Default::default()
            }),
        };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("project response", None, e);
        }
        true
    }

    /// `/project trust` or `untrust`: the call as the presser, which the core
    /// judges (`Act::Trust`), and what it said.
    async fn project_trust_said(
        &self,
        granted: bool,
        path: String,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> String {
        let name = if granted {
            method::PROJECT_TRUST
        } else {
            method::PROJECT_UNTRUST
        };
        let p = ProjectTrustParams {
            path,
            author: Some(who.to_string()),
            discord,
        };
        said(granted, self.rpc.call::<_, ProjectGrant>(name, p).await)
    }

    /// `/project trusted`: the list, in a private place alone.
    async fn project_trusted_said(&self, discord: Option<DiscordOrigin>) -> String {
        let private = discord
            .as_ref()
            .map(typed_in)
            .is_some_and(|t| self.is_private(&t));
        if !private {
            return PRIVATE.into();
        }
        match self
            .rpc
            .call::<_, ProjectTrustedResult>(method::PROJECT_TRUSTED, ProjectTrustedParams {})
            .await
        {
            Ok(list) => list_text(&list),
            Err(e) => format!("⚠️ Could not read the trusted repositories: {e}."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The command's shape: three subcommands, one effect each, and `path`
    /// required on the two that write and absent from the read.
    #[test]
    fn the_project_command_has_one_effect_per_subcommand() {
        let c = command();
        assert_eq!(c.name, "project");
        let subs: Vec<&str> = c.options.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(subs, ["trust", "untrust", "trusted"]);
        for name in ["trust", "untrust"] {
            let o = c.options.iter().find(|o| o.name == name).unwrap();
            let opts = o.options.as_ref().expect("the path option");
            assert_eq!(opts.len(), 1);
            assert_eq!(opts[0].name, "path");
            assert_eq!(opts[0].required, Some(true));
        }
        let read = c.options.iter().find(|o| o.name == "trusted").unwrap();
        assert!(
            read.options.as_ref().is_none_or(Vec::is_empty),
            "a read takes nothing"
        );
    }

    /// `~/` is the daemon's home, an absolute path is itself, and a path that
    /// is nothing but space is no path at all (so the core is never asked).
    #[test]
    fn a_leading_tilde_is_the_daemons_home() {
        let args = |v: &str| vec![("path", v.to_string())];
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            assert_eq!(
                path_of(&args("~/projects/tide-gauges")),
                Some(format!("{home}/projects/tide-gauges"))
            );
        }
        assert_eq!(
            path_of(&args("/srv/tide-gauges")),
            Some("/srv/tide-gauges".into())
        );
        assert_eq!(path_of(&args("   ")), None);
        assert_eq!(path_of(&[]), None);
    }

    /// What a refused grant says: the core's own `why`, and that nothing
    /// changed. The chat route states the refusal; it never restates the rule.
    #[test]
    fn a_refused_grant_says_the_cores_own_reason() {
        let refusal = crate::rpc_client::CallError {
            code: theseus_protocol::error_code::REFUSED,
            message: "the answer from agent:no terminal does not count".into(),
            data: serde_json::json!({"why": "it came from an agent's process (no terminal)"}),
        };
        let out = said(true, Err(refusal));
        assert!(
            out.contains("it came from an agent's process (no terminal)"),
            "{out}"
        );
        assert!(out.contains("Nothing was granted or taken back"), "{out}");
        let ok = said(
            true,
            Ok(ProjectGrant {
                id: "r:1a2b3c4d".into(),
                root: "/srv/tide-gauges".into(),
                by: "discord:4210 (discord:zeroaltitude)".into(),
                via: "discord:dm".into(),
                at_ms: 0,
            }),
        );
        assert!(ok.contains("Trusted `/srv/tide-gauges`"), "{ok}");
        assert!(ok.contains("join the system block"), "{ok}");
        let gone = said(
            false,
            Ok(ProjectGrant {
                id: "r:1a2b3c4d".into(),
                root: "/srv/tide-gauges".into(),
                by: "the CLI".into(),
                via: "cli".into(),
                at_ms: 0,
            }),
        );
        assert!(gone.contains("no longer trusted"), "{gone}");
    }

    /// The list: a line per repository with its route, an empty list that
    /// says how to grant one, and a long list cut with the command for the
    /// rest (the positive count, so an empty answer cannot pass for a cut).
    #[test]
    fn the_list_names_each_repositorys_route_and_says_what_it_cut() {
        let grant = |n: usize| ProjectGrant {
            id: format!("r:{n:08x}"),
            root: format!("/srv/gauge-{n}"),
            by: "the CLI".into(),
            via: "cli".into(),
            at_ms: 0,
        };
        let empty = list_text(&ProjectTrustedResult { trusted: vec![] });
        assert!(empty.contains("No repository is trusted"), "{empty}");
        let one = list_text(&ProjectTrustedResult {
            trusted: vec![ProjectGrant {
                via: "discord:dm".into(),
                ..grant(1)
            }],
        });
        assert!(one.contains("/srv/gauge-1"), "{one}");
        assert!(one.contains("through discord:dm"), "{one}");
        let many = list_text(&ProjectTrustedResult {
            trusted: (0..200).map(grant).collect(),
        });
        assert!(many.len() <= MAX_TEXT + 80, "{} bytes", many.len());
        assert!(many.contains("/srv/gauge-0"), "it keeps the first: {many}");
        assert!(many.contains("more · `theseus project trusted`"), "{many}");
    }
}
