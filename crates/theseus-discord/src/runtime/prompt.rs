//! `/prompt` (M7 step 36c): run an MCP server's prompt as this place's next
//! turn. A child of `runtime`, apart from it for the shape budget's file
//! ceiling.
//!
//! - **`/prompt name:<server/prompt>`**, with autocomplete from
//!   `mcp.prompt.list`: at most 25 choices, filtered by what was typed.
//! - **A modal** then asks for the prompt's arguments: one text input each,
//!   up to 5 (a required one is marked, and a prompt without arguments runs
//!   at once), or, for a prompt with more, one `args` input of `name=value`
//!   lines. The modal's submit is parsed here (`parse_modal`): a missing
//!   required argument or an unknown one is said, and nothing is sent.
//! - **The run** is `turn.submit { prompt }` through the place's own line of
//!   turns (`Place::run_prompt`), one at a time. The core asks the server and
//!   refuses a shared place's prompt, saying why.

use std::collections::BTreeMap;

use serde_json::json;
use theseus_protocol::mcp::{
    McpPromptInfo, McpPromptListParams, McpPromptListResult, McpPromptRef,
};
use theseus_protocol::{LedgerKind, TurnSubmitParams, TurnSubmitResult};
use tokio::sync::{mpsc, oneshot};
use twilight_model::application::command::{
    Command, CommandOptionChoice, CommandOptionChoiceValue, CommandType,
};
use twilight_model::application::interaction::application_command::{
    CommandData, CommandOptionValue,
};
use twilight_model::application::interaction::modal::{
    ModalInteractionComponent, ModalInteractionData,
};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{Label, TextInput, TextInputStyle};
use twilight_model::channel::message::{Component, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_util::builder::command::{CommandBuilder, StringBuilder};

use super::{Control, Place, PlaceMsg, Shared};

/// The modal's `custom_id` starts with this, then `<server>/<prompt>`.
pub(super) const MODAL_PREFIX: &str = "prompt:";
/// Discord's most autocomplete choices, and a modal's most inputs.
pub(super) const MAX_CHOICES: usize = 25;
pub(super) const MAX_INPUTS: usize = 5;
/// The one input a prompt of more than [`MAX_INPUTS`] arguments gets.
const ARGS_FIELD: &str = "args";

/// `/prompt name:<server/prompt>`.
pub(super) fn command() -> Command {
    CommandBuilder::new(
        "prompt",
        "Run an MCP server's prompt as this conversation's next turn (you, from a private place)",
        CommandType::ChatInput,
    )
    .option(
        StringBuilder::new("name", "The prompt, as server/prompt")
            .required(true)
            .autocomplete(true),
    )
    .build()
}

/// A string of at most `max` characters: cut, with an ellipsis, when longer.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// The autocomplete's answer: the prompts whose name, title, or description
/// holds what was typed (case aside), at most [`MAX_CHOICES`], each as
/// `server/prompt` with its description beside it.
pub(super) fn choices(prompts: &[McpPromptInfo], typed: &str) -> Vec<CommandOptionChoice> {
    let typed = typed.trim().to_lowercase();
    prompts
        .iter()
        .filter(|p| {
            typed.is_empty()
                || [Some(&p.name), p.title.as_ref(), p.description.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|s| s.to_lowercase().contains(&typed))
        })
        .filter(|p| p.name.chars().count() <= 100)
        .take(MAX_CHOICES)
        .map(|p| {
            let what = p.description.as_deref().or(p.title.as_deref());
            let shown = match what {
                Some(w) => format!("{} · {}", p.name, w.lines().next().unwrap_or("")),
                None => p.name.clone(),
            };
            CommandOptionChoice {
                name: clip(&shown, 100),
                name_localizations: None,
                value: CommandOptionChoiceValue::String(p.name.clone()),
            }
        })
        .collect()
}

fn input(
    custom_id: &str,
    style: TextInputStyle,
    required: bool,
    placeholder: Option<String>,
) -> Component {
    #[allow(deprecated)]
    Component::TextInput(TextInput {
        id: None,
        custom_id: custom_id.into(),
        label: None,
        max_length: Some(4000),
        min_length: None,
        placeholder,
        required: Some(required),
        style,
        value: None,
    })
}

fn labelled(label: &str, description: Option<String>, inner: Component) -> Component {
    Component::Label(Label {
        id: None,
        label: clip(label, 45),
        description: description.map(|d| clip(&d, 100)),
        component: Box::new(inner),
    })
}

/// The modal that asks for a prompt's arguments: a text input each, up to
/// [`MAX_INPUTS`], with `*` on a required one; more than that is one `args`
/// input of `name=value` lines.
pub(super) fn modal(p: &McpPromptInfo) -> InteractionResponseData {
    let components: Vec<Component> = if p.arguments.len() <= MAX_INPUTS {
        p.arguments
            .iter()
            .map(|a| {
                let mark = if a.required { " *" } else { "" };
                labelled(
                    &format!("{}{mark}", a.name),
                    a.description.clone(),
                    input(&a.name, TextInputStyle::Paragraph, a.required, None),
                )
            })
            .collect()
    } else {
        let names: Vec<String> = p
            .arguments
            .iter()
            .map(|a| {
                if a.required {
                    format!("{}*", a.name)
                } else {
                    a.name.clone()
                }
            })
            .collect();
        let required = p.arguments.iter().any(|a| a.required);
        vec![labelled(
            "Arguments, one name=value per line",
            Some(format!("{} (* is required)", names.join(", "))),
            input(
                ARGS_FIELD,
                TextInputStyle::Paragraph,
                required,
                Some("name=value".into()),
            ),
        )]
    };
    InteractionResponseData {
        custom_id: Some(format!("{MODAL_PREFIX}{}", p.name)),
        title: Some(clip(&p.name, 45)),
        components: Some(components),
        ..Default::default()
    }
}

/// `name=value` lines: blank lines skipped, a value that may hold `=`, a
/// name given twice a mistake.
fn parse_lines(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Some((k, v)) = line.split_once('=').filter(|(k, _)| !k.trim().is_empty()) else {
            return Err(format!("{line:?} is not name=value"));
        };
        if out
            .insert(k.trim().to_string(), v.trim().to_string())
            .is_some()
        {
            return Err(format!("{} was given twice", k.trim()));
        }
    }
    Ok(out)
}

/// A submitted modal's arguments, checked against the prompt: what was
/// filled in (an empty input is no argument), by name, for a prompt of up to
/// [`MAX_INPUTS`] arguments, or parsed from the `args` input for one with
/// more. A required argument left out, and a name the prompt does not take,
/// are errors, worded for the person who typed them.
pub(super) fn parse_modal(
    p: &McpPromptInfo,
    fields: &[(String, String)],
) -> Result<BTreeMap<String, String>, String> {
    let mut given: BTreeMap<String, String> = if p.arguments.len() <= MAX_INPUTS {
        fields
            .iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    } else {
        let text = fields
            .iter()
            .find(|(k, _)| k == ARGS_FIELD)
            .map_or("", |(_, v)| v.as_str());
        parse_lines(text)?
    };
    let unknown: Vec<&str> = given
        .keys()
        .filter(|k| !p.arguments.iter().any(|a| &a.name == *k))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        let takes: Vec<&str> = p.arguments.iter().map(|a| a.name.as_str()).collect();
        return Err(format!(
            "{} takes no argument {} (it takes: {})",
            p.name,
            unknown.join(", "),
            takes.join(", ")
        ));
    }
    given.retain(|_, v| !v.trim().is_empty());
    let missing: Vec<&str> = p
        .arguments
        .iter()
        .filter(|a| a.required && !given.contains_key(&a.name))
        .map(|a| a.name.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "{} needs {}: {}",
            p.name,
            if missing.len() == 1 {
                "its argument"
            } else {
                "its arguments"
            },
            missing.join(", ")
        ));
    }
    Ok(given)
}

/// Every text input a submitted modal holds, by `custom_id`, whether it
/// came in labels or action rows.
pub(super) fn fields_of(data: &ModalInteractionData) -> Vec<(String, String)> {
    fn walk(c: &ModalInteractionComponent, out: &mut Vec<(String, String)>) {
        match c {
            ModalInteractionComponent::Label(l) => walk(&l.component, out),
            ModalInteractionComponent::ActionRow(r) => {
                r.components.iter().for_each(|c| walk(c, out))
            }
            ModalInteractionComponent::TextInput(t) => {
                out.push((t.custom_id.clone(), t.value.clone()))
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    data.components.iter().for_each(|c| walk(c, &mut out));
    out
}

/// `server/prompt` as the pair it names.
fn split(name: &str) -> Option<(&str, &str)> {
    name.split_once('/')
        .filter(|(s, p)| !s.is_empty() && !p.is_empty())
}

impl Shared {
    /// What the autocomplete typed, from its focused option.
    fn typed(c: &CommandData) -> String {
        c.options
            .iter()
            .find_map(|o| match &o.value {
                CommandOptionValue::Focused(t, _) if o.name == "name" => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    async fn prompt_infos(&self, server: Option<String>) -> Result<Vec<McpPromptInfo>, String> {
        self.rpc
            .call::<_, McpPromptListResult>(
                theseus_protocol::method::MCP_PROMPT_LIST,
                McpPromptListParams { server },
            )
            .await
            .map(|r| r.prompts)
            .map_err(|e| e.message)
    }

    async fn answer(
        &self,
        i: &Interaction,
        kind: InteractionResponseType,
        data: Option<InteractionResponseData>,
    ) {
        let resp = InteractionResponse { kind, data };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("prompt response", None, e);
        }
    }

    async fn say_ephemeral(&self, i: &Interaction, text: String) {
        let data = InteractionResponseData {
            content: Some(text),
            flags: Some(MessageFlags::EPHEMERAL),
            allowed_mentions: Some(twilight_model::channel::message::AllowedMentions::default()),
            ..Default::default()
        };
        self.answer(
            i,
            InteractionResponseType::ChannelMessageWithSource,
            Some(data),
        )
        .await;
    }

    /// An interaction of `/prompt`: true when it was one, and answered.
    pub(super) async fn prompt_interaction(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: &str,
    ) -> bool {
        use twilight_model::application::interaction::{InteractionData, InteractionType};
        match &i.data {
            Some(InteractionData::ApplicationCommand(c)) if c.name == "prompt" => {
                if i.kind == InteractionType::ApplicationCommandAutocomplete {
                    self.prompt_autocomplete(i, c).await;
                } else {
                    self.prompt_command(i, c, tx, who.to_string()).await;
                }
                true
            }
            Some(InteractionData::ModalSubmit(m)) if m.custom_id.starts_with(MODAL_PREFIX) => {
                self.prompt_submit(i, m, tx, who.to_string()).await;
                true
            }
            _ => false,
        }
    }

    /// An autocomplete request: the choices for what was typed.
    async fn prompt_autocomplete(&self, i: &Interaction, c: &CommandData) {
        let list = self.prompt_infos(None).await.unwrap_or_default();
        let data = InteractionResponseData {
            choices: Some(choices(&list, &Self::typed(c))),
            ..Default::default()
        };
        self.answer(
            i,
            InteractionResponseType::ApplicationCommandAutocompleteResult,
            Some(data),
        )
        .await;
    }

    /// `/prompt name:…`: the modal for its arguments, or the run at once when
    /// it takes none.
    async fn prompt_command(
        &self,
        i: &Interaction,
        c: &CommandData,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: String,
    ) {
        let name = c
            .options
            .iter()
            .find_map(|o| match (&*o.name, &o.value) {
                ("name", CommandOptionValue::String(s)) => Some(s.trim().to_string()),
                _ => None,
            })
            .unwrap_or_default();
        let Some((server, _)) = split(&name) else {
            self.say_ephemeral(
                i,
                "Name the prompt as `server/prompt`: pick one from the list as you type.".into(),
            )
            .await;
            return;
        };
        let list = match self.prompt_infos(Some(server.to_string())).await {
            Ok(l) => l,
            Err(e) => {
                self.say_ephemeral(i, format!("⚠️ {e}")).await;
                return;
            }
        };
        let Some(info) = list.iter().find(|p| p.name == name) else {
            self.say_ephemeral(i, format!("MCP server {server} lists no prompt `{name}`."))
                .await;
            return;
        };
        if info.arguments.is_empty() {
            let r = McpPromptRef {
                server: info.server.clone(),
                name: info.prompt.clone(),
                arguments: BTreeMap::new(),
            };
            self.prompt_run(i, tx, who, r).await;
            return;
        }
        self.answer(i, InteractionResponseType::Modal, Some(modal(info)))
            .await;
    }

    /// A submitted modal: its arguments parsed, then the run.
    async fn prompt_submit(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: String,
    ) {
        let name = m.custom_id.trim_start_matches(MODAL_PREFIX);
        let Some((server, prompt)) = split(name) else {
            return;
        };
        let list = match self.prompt_infos(Some(server.to_string())).await {
            Ok(l) => l,
            Err(e) => {
                self.say_ephemeral(i, format!("⚠️ {e}")).await;
                return;
            }
        };
        let Some(info) = list.iter().find(|p| p.name == name) else {
            self.say_ephemeral(i, format!("MCP server {server} no longer lists `{name}`."))
                .await;
            return;
        };
        match parse_modal(info, &fields_of(m)) {
            Ok(arguments) => {
                let r = McpPromptRef {
                    server: server.into(),
                    name: prompt.into(),
                    arguments,
                };
                self.prompt_run(i, tx, who, r).await;
            }
            Err(why) => self.say_ephemeral(i, format!("⚠️ {why}")).await,
        }
    }

    /// Run it in the place, and say so where the command was typed. The
    /// arguments are not echoed: they may be private.
    async fn prompt_run(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: String,
        r: McpPromptRef,
    ) {
        self.answer(
            i,
            InteractionResponseType::DeferredChannelMessageWithSource,
            None,
        )
        .await;
        self.core.binding_ledger(
            LedgerKind::DiscordCommand,
            None,
            json!({"command": "prompt", "by": who, "prompt": format!("{}/{}", r.server, r.name)}),
        );
        let (rtx, rrx) = oneshot::channel();
        let _ = tx.send(PlaceMsg::Control {
            cmd: Control::Prompt(Box::new(r)),
            by: who,
            reply: rtx,
        });
        let text = rrx
            .await
            .unwrap_or_else(|_| "The place did not answer.".into());
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .update_response(&i.token)
            .content(Some(&text))
            .await
        {
            self.board.error("command reply", None, e);
        }
    }
}

impl Place {
    /// The place's line of turns takes the prompt, one at a time: a turn in
    /// flight keeps its place, and the prompt is not queued behind it.
    pub(super) fn run_prompt(&mut self, r: McpPromptRef, by: &str) -> String {
        if self.inflight {
            return "A turn is running here: let it finish, or `/stop` it, then run the prompt again.".into();
        }
        self.inflight = true;
        self.saw_failure = false;
        let name = format!("{}/{}", r.server, r.name);
        let (rpc, tx, sid) = (
            self.shared.rpc.clone(),
            self.tx.clone(),
            self.session_id.clone(),
        );
        let author = Some(by.to_string());
        tokio::spawn(async move {
            let res = rpc
                .call::<_, TurnSubmitResult>(
                    theseus_protocol::method::TURN_SUBMIT,
                    TurnSubmitParams {
                        dir: None,
                        carried: false,
                        prompt: Some(r),
                        session_id: Some(sid),
                        input: String::new(),
                        profile: None,
                        provider: None,
                        model: None,
                        author,
                        attachments: Vec::new(),
                        reply_to: None,
                        opened_from: None,
                    },
                )
                .await
                .map(|_| ());
            let _ = tx.send(PlaceMsg::SubmitDone(res));
        });
        format!("▶️ Running the prompt `{name}`.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::mcp::McpPromptArgument;

    fn prompt(name: &str, args: &[(&str, bool)]) -> McpPromptInfo {
        let (server, p) = name.split_once('/').unwrap();
        McpPromptInfo {
            server: server.into(),
            prompt: p.into(),
            name: name.into(),
            description: Some(format!("Does {p}.")),
            arguments: args
                .iter()
                .map(|(n, r)| McpPromptArgument {
                    name: n.to_string(),
                    description: None,
                    required: *r,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn fields(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn values(c: &[CommandOptionChoice]) -> Vec<String> {
        c.iter()
            .map(|c| match &c.value {
                CommandOptionChoiceValue::String(s) => s.clone(),
                _ => panic!("a string option"),
            })
            .collect()
    }

    #[test]
    fn the_autocomplete_filters_by_what_was_typed_and_answers_at_most_25() {
        let list = vec![
            prompt("fake/greet", &[("name", true)]),
            prompt("fake/brief", &[]),
            prompt("docs/review", &[]),
        ];
        assert_eq!(
            values(&choices(&list, "")),
            ["fake/greet", "fake/brief", "docs/review"]
        );
        assert_eq!(values(&choices(&list, "GRE")), ["fake/greet"], "case aside");
        assert_eq!(values(&choices(&list, "docs")), ["docs/review"]);
        // It matches the description too, and shows it beside the name.
        let c = choices(&list, "does brief");
        assert_eq!(values(&c), ["fake/brief"]);
        assert_eq!(c[0].name, "fake/brief · Does brief.");
        assert!(choices(&list, "zzz").is_empty());
        // Discord's limits: 25 choices, names of 100 characters.
        let many: Vec<McpPromptInfo> = (0..40).map(|n| prompt(&format!("big/p{n}"), &[])).collect();
        assert_eq!(choices(&many, "").len(), 25);
        let mut long = prompt("big/long", &[]);
        long.description = Some("x".repeat(300));
        let c = choices(&[long], "");
        assert!(c[0].name.chars().count() <= 100, "{}", c[0].name.len());
        assert_eq!(values(&c), ["big/long"], "the value stays the bare name");
    }

    #[test]
    fn the_modal_has_an_input_per_argument_up_to_five_then_one_args_input() {
        let five = prompt(
            "s/p",
            &[
                ("a", true),
                ("b", false),
                ("c", false),
                ("d", false),
                ("e", true),
            ],
        );
        let m = modal(&five);
        assert_eq!(m.custom_id.as_deref(), Some("prompt:s/p"));
        let comps = m.components.unwrap();
        assert_eq!(comps.len(), 5);
        let Component::Label(l) = &comps[0] else {
            panic!("a label")
        };
        assert_eq!(l.label, "a *", "a required argument is marked");
        let Component::Label(l) = &comps[1] else {
            panic!("a label")
        };
        assert_eq!(l.label, "b");
        let Component::TextInput(t) = &*l.component else {
            panic!("a text input")
        };
        assert_eq!((t.custom_id.as_str(), t.required), ("b", Some(false)));

        let six = prompt(
            "s/p",
            &[
                ("a", true),
                ("b", false),
                ("c", false),
                ("d", false),
                ("e", false),
                ("f", false),
            ],
        );
        let comps = modal(&six).components.unwrap();
        assert_eq!(comps.len(), 1, "one args input");
        let Component::Label(l) = &comps[0] else {
            panic!("a label")
        };
        assert!(
            l.description.as_ref().unwrap().contains("a*"),
            "{:?}",
            l.description
        );
        let Component::TextInput(t) = &*l.component else {
            panic!("a text input")
        };
        assert_eq!(t.custom_id, "args");
    }

    #[test]
    fn a_modal_of_five_is_parsed_by_name_and_six_by_its_args_lines() {
        let five = prompt(
            "s/p",
            &[
                ("a", true),
                ("b", false),
                ("c", false),
                ("d", false),
                ("e", false),
            ],
        );
        let got = parse_modal(
            &five,
            &fields(&[("a", "x"), ("b", ""), ("c", " "), ("e", "y")]),
        )
        .unwrap();
        assert_eq!(
            got.into_iter().collect::<Vec<_>>(),
            [
                ("a".to_string(), "x".to_string()),
                ("e".to_string(), "y".to_string())
            ],
            "an empty input is no argument"
        );

        let six = prompt(
            "s/p",
            &[
                ("a", true),
                ("b", false),
                ("c", false),
                ("d", false),
                ("e", false),
                ("f", false),
            ],
        );
        let got = parse_modal(&six, &fields(&[("args", "a = 1\n\nf=x=y\n")])).unwrap();
        assert_eq!(got["a"], "1");
        assert_eq!(got["f"], "x=y", "a value may hold =");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn a_missing_required_argument_an_unknown_one_and_a_bad_line_are_said() {
        let five = prompt("s/p", &[("a", true), ("b", true)]);
        let why = parse_modal(&five, &fields(&[("a", "x"), ("b", "  ")])).unwrap_err();
        assert_eq!(why, "s/p needs its argument: b");
        let why = parse_modal(&five, &fields(&[])).unwrap_err();
        assert_eq!(why, "s/p needs its arguments: a, b");
        let why = parse_modal(&five, &fields(&[("a", "1"), ("b", "2"), ("z", "3")])).unwrap_err();
        assert!(
            why.contains("takes no argument z (it takes: a, b)"),
            "{why}"
        );

        let six = prompt(
            "s/p",
            &[
                ("a", true),
                ("b", false),
                ("c", false),
                ("d", false),
                ("e", false),
                ("f", false),
            ],
        );
        let why = parse_modal(&six, &fields(&[("args", "b=2")])).unwrap_err();
        assert_eq!(why, "s/p needs its argument: a");
        let why = parse_modal(&six, &fields(&[("args", "a=1\nnoequals")])).unwrap_err();
        assert!(why.contains("is not name=value"), "{why}");
        let why = parse_modal(&six, &fields(&[("args", "a=1\na=2")])).unwrap_err();
        assert!(why.contains("given twice"), "{why}");
        let why = parse_modal(&six, &fields(&[("args", "a=1\nq=2")])).unwrap_err();
        assert!(why.contains("takes no argument q"), "{why}");
    }

    #[test]
    fn a_submitted_modal_is_read_from_its_labels_and_its_action_rows() {
        use twilight_model::application::interaction::modal::{
            ModalInteractionActionRow, ModalInteractionLabel, ModalInteractionTextInput,
        };
        let text = |id: &str, v: &str| {
            ModalInteractionComponent::TextInput(ModalInteractionTextInput {
                custom_id: id.into(),
                id: 1,
                value: v.into(),
            })
        };
        let data = ModalInteractionData {
            custom_id: "prompt:s/p".into(),
            resolved: None,
            components: vec![
                ModalInteractionComponent::Label(ModalInteractionLabel {
                    id: 1,
                    component: Box::new(text("a", "one")),
                }),
                ModalInteractionComponent::ActionRow(ModalInteractionActionRow {
                    id: 2,
                    components: vec![text("b", "two")],
                }),
            ],
        };
        assert_eq!(fields_of(&data), fields(&[("a", "one"), ("b", "two")]));
    }

    #[test]
    fn the_command_has_one_autocompleted_required_name() {
        let c = command();
        assert_eq!(c.name, "prompt");
        assert_eq!(c.options.len(), 1);
        assert_eq!(c.options[0].name, "name");
        assert_eq!(c.options[0].autocomplete, Some(true));
        assert_eq!(c.options[0].required, Some(true));
    }
}
