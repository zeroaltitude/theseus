//! The three item tools (issue 13): add, close and list an item of the
//! project this conversation is attached to. They are the model's front door
//! to the project's own list (`project::items`), as `/project item …` and
//! `theseus project item …` are the owner's.
//!
//! Chat acts like the rest: in no place's kept catalog, offered by
//! [`Board::offers`] only in a turn the owner's own message started and only
//! in a session attached to a project, with the project and its open items
//! named in the description. They go through `project.item.*`'s verbs, judged
//! as the owner who spoke.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::projects::{
    ProjectItemAddParams, ProjectItemCloseParams, ProjectItemListParams,
};
use theseus_protocol::Plan;
use theseus_tools::{parse, Backend, Retry, Tool, ToolClass, ToolCtx};

use super::{Board, ITEM_ADD, ITEM_CLOSE, ITEM_LIST};
use crate::project::items;
use crate::toolrun::TurnCtx;
use crate::Core;

type Done = Result<(String, Value), String>;

/// The project a conversation is attached to, and its open items as
/// `(task id, title)`: what the description names and a close picks from.
#[derive(Debug, Clone)]
pub(crate) struct ProjectOffer {
    pub id: String,
    pub title: String,
    pub open: Vec<(String, String)>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddArgs {
    title: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseArgs {
    item: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    /// `open` (the default) or `all`.
    #[serde(default)]
    which: Option<String>,
}

impl ListArgs {
    fn all(&self) -> Result<bool, String> {
        match self.which.as_deref().map(str::trim) {
            None | Some("") | Some("open") => Ok(false),
            Some("all") => Ok(true),
            Some(other) => Err(format!("`which` is `open` or `all`, not `{other}`")),
        }
    }
}

pub struct ItemAdd;
pub struct ItemClose;
pub struct ItemList;

fn one_string(name: &str, what: &str) -> Value {
    json!({
        "type": "object",
        "properties": {name: {"type": "string", "description": what}},
        "required": [name],
        "additionalProperties": false,
    })
}

impl Tool for ItemAdd {
    fn name(&self) -> &'static str {
        ITEM_ADD
    }
    fn description(&self) -> &'static str {
        "Add an item to the project this conversation is attached to, when the operator asks for \
         one to be added. An item is a title and a state: one line."
    }
    fn input_schema(&self) -> Value {
        one_string("title", "The item: one line of at most 200 characters.")
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Harness
    }
    fn retry(&self) -> Retry {
        // A repeat adds the item again: not safe.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: AddArgs = parse(input)?;
        let title = items::title(&a.title)?;
        Ok(Plan {
            summary: format!("add the item “{title}” to the project"),
            ..Default::default()
        })
    }
}

impl Tool for ItemClose {
    fn name(&self) -> &'static str {
        ITEM_CLOSE
    }
    fn description(&self) -> &'static str {
        "Close an item of the project this conversation is attached to, when the operator says \
         it is done. Offered only while the project has open items."
    }
    fn input_schema(&self) -> Value {
        one_string(
            "item",
            "The item, by its id from the list in this tool's description.",
        )
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Harness
    }
    fn retry(&self) -> Retry {
        // A repeat finds the item closed and says so.
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: CloseArgs = parse(input)?;
        if a.item.trim().is_empty() {
            return Err("name the item to close, by its id".into());
        }
        Ok(Plan {
            summary: format!("close the item {}", a.item.trim()),
            ..Default::default()
        })
    }
}

impl Tool for ItemList {
    fn name(&self) -> &'static str {
        ITEM_LIST
    }
    fn description(&self) -> &'static str {
        "List the items of the project this conversation is attached to: the open ones, or all \
         with `all`."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"which": {"type": "string",
                "description": "`open` (the default), or `all` to include the closed items."}},
            "additionalProperties": false,
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Harness
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: ListArgs = parse(input)?;
        Ok(Plan {
            summary: match a.all()? {
                true => "list all the project's items".into(),
                false => "list the project's open items".into(),
            },
            ..Default::default()
        })
    }
}

/// The add tool's description for this turn: the project it adds to.
pub(crate) fn add_description(p: &ProjectOffer) -> String {
    format!(
        "Add an item to the project «{}» ({}) this conversation is attached to, when the \
         operator asks for one to be added. An item is a title and a state: one line. It is \
         one of the project's steps, which the next-steps list and the progress count read.",
        p.title, p.id
    )
}

/// The close tool's description for this turn: the open items, by id.
pub(crate) fn close_description(p: &ProjectOffer) -> String {
    let list: String = p
        .open
        .iter()
        .map(|(id, title)| format!("\n- `{}`: {title}", crate::task::short(id)))
        .collect();
    format!(
        "Close an item of the project «{}» when the operator says it is done. Name it by its id. \
         The open items:{list}",
        p.title
    )
}

/// The list tool's description for this turn.
pub(crate) fn list_description(p: &ProjectOffer) -> String {
    format!(
        "List the items of the project «{}» this conversation is attached to: the open ones, or \
         all with `which` set to `all`.",
        p.title
    )
}

impl Board {
    /// The project of `session`, when it has one that is not closed, with its
    /// open items.
    pub(crate) fn project_offer(core: &Core, session: &str) -> Option<ProjectOffer> {
        let project = core
            .store
            .get_session::<crate::session::SessionRecord>(session)
            .ok()??
            .project?;
        let tasks = crate::task_graph::all(&core.store).ok()?;
        let p = tasks
            .iter()
            .find(|t| t.id == project && !t.state.is_closed())?;
        let open = items::list(&tasks, &p.id, false)
            .into_iter()
            .map(|t| (t.id.clone(), t.title.clone()))
            .collect();
        Some(ProjectOffer {
            id: p.id.clone(),
            title: p.title.clone(),
            open,
        })
    }

    fn project_of_turn(
        &self,
        tc: &TurnCtx<'_>,
    ) -> Result<(std::sync::Arc<Core>, super::Offered, ProjectOffer), String> {
        let (core, offered) = self.standing(tc)?;
        let p = offered.project.clone().ok_or(
            "This conversation is attached to no project (or its project is closed). Nothing was \
             done.",
        )?;
        Ok((core, offered, p))
    }

    /// Add an item to the conversation's project, as the owner who spoke.
    pub(crate) fn item_add(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, offered, p) = self.project_of_turn(tc)?;
        let a: AddArgs = parse(input)?;
        let r = core
            .project_item_add(
                &ProjectItemAddParams {
                    project: p.id.clone(),
                    title: a.title,
                    ..Default::default()
                },
                offered.said.who.clone(),
            )
            .map_err(|e| format!("nothing was added: {}", e.message))?;
        let item = r.changed.ok_or("the item was not recorded")?;
        Ok((
            theseus_protocol::projects::item_added(&r.title, &item.title),
            json!({"chat_act": {"item_added": item.id}}),
        ))
    }

    /// Close one of the project's open items, as the owner who spoke.
    pub(crate) fn item_close(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, offered, p) = self.project_of_turn(tc)?;
        let a: CloseArgs = parse(input)?;
        let want = a.item.trim().to_lowercase();
        let matches: Vec<&(String, String)> = p
            .open
            .iter()
            .filter(|(id, _)| {
                let id = id.to_lowercase();
                let short = crate::task::short(&id).to_lowercase();
                want.len() >= 3 && (id == want || id.ends_with(&want) || short == want)
            })
            .collect();
        let [(id, _)] = matches.as_slice() else {
            let list: String = p
                .open
                .iter()
                .map(|(id, title)| format!("\n- `{}`: {title}", crate::task::short(id)))
                .collect();
            return Err(format!(
                "`{}` names {} of the open items. Nothing was closed. The open items:{list}",
                a.item.trim(),
                matches.len()
            ));
        };
        let r = core
            .project_item_close(
                &ProjectItemCloseParams {
                    item: id.clone(),
                    ..Default::default()
                },
                offered.said.who.clone(),
            )
            .map_err(|e| format!("nothing was closed: {}", e.message))?;
        let item = r.changed.ok_or("the item was not recorded")?;
        Ok((
            theseus_protocol::projects::item_closed(&item.title),
            json!({"chat_act": {"item_closed": item.id}}),
        ))
    }

    /// The conversation's project's items, as `project.item.list` says them.
    pub(crate) fn item_list(&self, tc: &TurnCtx<'_>, input: &Value) -> Done {
        let (core, _offered, p) = self.project_of_turn(tc)?;
        let a: ListArgs = parse(input)?;
        let r = core
            .project_item_list(&ProjectItemListParams {
                project: p.id.clone(),
                all: a.all()?,
            })
            .map_err(|e| format!("the list could not be read: {}", e.message))?;
        Ok((
            theseus_protocol::projects::item_list(&r.title, &r.items),
            json!({"chat_act": {"items": r.items.len()}}),
        ))
    }
}
