//! `slides.write` (the Google design §3.1, row 14): simple operations for the
//! common case, and `requests` passed raw to `batchUpdate` for anything else.
//!
//! - `slides` adds a slide (a `layout`, a `title`, a `body`, `notes`) at the end,
//!   or, with `replace` naming an object, sets that object's text to `body`.
//! - `replace_text` is Slides' replace-all, each `find` to its replacement.
//! - `requests` are Slides' own, appended as given after the rest.
//! - No `id` creates a deck; its own empty first slide goes when `slides` adds
//!   the deck's slides, so the deck holds what was asked for.
//!
//! The simple operations and the raw requests land in one `batchUpdate`, which
//! Slides applies whole or not at all. Speaker notes need the slide made first
//! (its notes shape's id is Slides' to give), so they are a second batch.
//! Slides allows about 60 writes a minute: the client's backoff meets it.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_google::{Repeat, Service};
use theseus_tools::{parse, AsyncRun, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure};

use super::external::{finish, Source};
use super::sheets::{failed, request, segment, with_rows};
use super::{Google, Meta};

pub const WRITE: &str = "slides.write";

pub fn tools(g: &Arc<Google>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Write(g.clone()))]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SlideSpec {
    #[serde(default)]
    layout: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    replace: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    slides: Vec<SlideSpec>,
    #[serde(default)]
    replace_text: BTreeMap<String, String>,
    #[serde(default)]
    requests: Vec<Value>,
}

/// The placeholder types a predefined layout has: its title's and its body's.
fn placeholders(layout: &str) -> Option<(Option<&'static str>, Option<&'static str>)> {
    Some(match layout {
        "BLANK" => (None, None),
        "TITLE" => (Some("CENTERED_TITLE"), Some("SUBTITLE")),
        "TITLE_AND_BODY" | "TITLE_AND_TWO_COLUMNS" | "ONE_COLUMN_TEXT" => {
            (Some("TITLE"), Some("BODY"))
        }
        "TITLE_ONLY" | "SECTION_HEADER" | "MAIN_POINT" => (Some("TITLE"), None),
        "SECTION_TITLE_AND_DESCRIPTION" | "BIG_NUMBER" => (Some("TITLE"), Some("SUBTITLE")),
        "CAPTION_ONLY" => (None, Some("SUBTITLE")),
        _ => return None,
    })
}

/// What a slide's simple operations become: the requests, and the notes to
/// write once the slide exists (the slide's object id and the text).
struct Built {
    requests: Vec<Value>,
    /// How many of the requests are the slide entries'.
    slides_end: usize,
    created: Vec<String>,
    notes: Vec<(String, String)>,
}

impl Args {
    /// The requests the simple operations make, with new slides' object ids
    /// minted from `stamp`. Pure, so the plan and the run say the same words.
    fn build(&self, stamp: u64) -> Result<Built, String> {
        let mut b = Built {
            requests: Vec::new(),
            slides_end: 0,
            created: Vec::new(),
            notes: Vec::new(),
        };
        for (n, s) in self.slides.iter().enumerate() {
            if let Some(object) = &s.replace {
                if s.layout.is_some() || s.title.is_some() || s.notes.is_some() {
                    return Err("a slide entry with `replace` takes only `body`".into());
                }
                let body = s
                    .body
                    .as_deref()
                    .ok_or("`replace` needs the new text in `body`")?;
                b.requests.push(
                    json!({"deleteText": {"objectId": object, "textRange": {"type": "ALL"}}}),
                );
                b.requests
                    .push(json!({"insertText": {"objectId": object, "text": body}}));
                continue;
            }
            let layout = match (&s.layout, &s.title, &s.body) {
                (Some(l), ..) => l.trim().to_ascii_uppercase().replace([' ', '-'], "_"),
                (None, Some(_), Some(_)) => "TITLE_AND_BODY".into(),
                (None, Some(_), None) => "TITLE_ONLY".into(),
                (None, None, Some(_)) => "TITLE_AND_BODY".into(),
                (None, None, None) => "BLANK".into(),
            };
            let (title_type, body_type) = placeholders(&layout)
                .ok_or_else(|| format!("unknown layout `{layout}`: BLANK, TITLE, TITLE_AND_BODY, TITLE_AND_TWO_COLUMNS, TITLE_ONLY, SECTION_HEADER, SECTION_TITLE_AND_DESCRIPTION, ONE_COLUMN_TEXT, MAIN_POINT, BIG_NUMBER, CAPTION_ONLY"))?;
            let slide = format!("ts{stamp:x}_{n}");
            let mut mappings = Vec::new();
            let mut texts = Vec::new();
            for (kind, text, suffix) in [(title_type, &s.title, "t"), (body_type, &s.body, "b")] {
                let Some(text) = text else { continue };
                let kind = kind.ok_or_else(|| {
                    format!(
                        "layout {layout} has no place for `{}`",
                        if suffix == "t" { "title" } else { "body" }
                    )
                })?;
                let shape = format!("{slide}_{suffix}");
                mappings.push(
                    json!({"layoutPlaceholder": {"type": kind, "index": 0}, "objectId": shape}),
                );
                texts.push(json!({"insertText": {"objectId": shape, "text": text}}));
            }
            b.requests.push(json!({"createSlide": {"objectId": slide,
                "slideLayoutReference": {"predefinedLayout": layout}, "placeholderIdMappings": mappings}}));
            b.requests.extend(texts);
            if let Some(notes) = &s.notes {
                b.notes.push((slide.clone(), notes.clone()));
            }
            b.created.push(slide);
        }
        b.slides_end = b.requests.len();
        for (find, with) in &self.replace_text {
            b.requests
                .push(json!({"replaceAllText": {"replaceText": with,
                "containsText": {"text": find, "matchCase": true}}}));
        }
        b.requests.extend(self.requests.iter().cloned());
        Ok(b)
    }

    fn checked(input: &Value) -> Result<(Args, Built), String> {
        let a: Args = parse(input)?;
        if a.requests.iter().any(|r| !r.is_object()) {
            return Err("each of `requests` is a Slides request object".into());
        }
        if a.id.is_some()
            && a.slides.is_empty()
            && a.replace_text.is_empty()
            && a.requests.is_empty()
        {
            return Err("nothing to change: give `slides`, `replace_text` or `requests`".into());
        }
        let built = a.build(0)?;
        Ok((a, built))
    }
}

pub struct Write(Arc<Google>);

impl Tool for Write {
    fn name(&self) -> &str {
        WRITE
    }
    fn description(&self) -> &str {
        "Create or change a Google Slides deck. No `id` creates a deck titled `title`. `slides` \
         adds slides at the end: layout (TITLE_AND_BODY, TITLE_ONLY, SECTION_HEADER, BLANK, ...), \
         title, body, notes; with `replace` (an object id) it sets that object's text to `body`. \
         `replace_text` maps text to its replacement everywhere. `requests` are raw Slides \
         batchUpdate requests for anything else. Drive keeps the revisions."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The deck; none creates one."},
                "title": {"type": "string", "description": "The new deck's title."},
                "slides": {"type": "array", "items": {"type": "object", "properties": {
                    "layout": {"type": "string"}, "title": {"type": "string"},
                    "body": {"type": "string"}, "notes": {"type": "string"},
                    "replace": {"type": "string", "description": "Object id whose text becomes body."}}}},
                "replace_text": {"type": "object", "description": "{find: replacement}."},
                "requests": {"type": "array", "items": {"type": "object"}, "description": "Raw batchUpdate requests."}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        // A batch repeated would insert its text twice.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (a, built) = Args::checked(input)?;
        let what = match &a.id {
            Some(id) => format!("change deck {id}: {} request(s)", built.requests.len()),
            None => format!("create a deck with {} slide(s)", built.created.len()),
        };
        Ok(Plan {
            summary: what,
            class: Some(ToolClass::Write),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let args = Args::checked(input).map(|(a, _)| a);
        let g = self.0.clone();
        Box::pin(async move {
            let a = args.map_err(ToolFailure::new)?;
            let built = a
                .build(theseus_protocol::now_unix_ms())
                .map_err(ToolFailure::new)?;
            let mut run = Run {
                g,
                id: a.id.clone().unwrap_or_default(),
                created: false,
                rows: Vec::new(),
            };
            let mut requests = built.requests.clone();
            let mut text = String::new();
            if a.id.is_none() {
                let first = run.create(a.title.as_deref()).await?;
                // The deck's own empty slide goes after the ones asked for.
                if let (Some(first), false) = (first, built.created.is_empty()) {
                    let delete = json!({"deleteObject": {"objectId": first}});
                    requests.insert(built.slides_end, delete);
                }
                text.push_str("Created ");
            }
            let id = run.id.clone();
            text.push_str(&format!("Presentation {id} ({})\n", edit_url(&id)));
            if !requests.is_empty() {
                let answer = run.batch(&requests, "").await?;
                let changed: u64 = answer["replies"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r["replaceAllText"]["occurrencesChanged"].as_u64())
                    .sum();
                text.push_str(&format!("Applied {} request(s)", requests.len()));
                if !a.replace_text.is_empty() {
                    text.push_str(&format!("; replaced text {changed} time(s)"));
                }
                text.push('\n');
                if !built.created.is_empty() {
                    text.push_str(&format!("Added slides: {}\n", built.created.join(", ")));
                }
            }
            if !built.notes.is_empty() {
                let wrote = run.notes(&built.notes).await?;
                text.push_str(&format!("Wrote notes on {wrote} slide(s)\n"));
            }
            let (meta, row) = with_rows(json!({"id": id, "url": edit_url(&id)}), run.rows);
            Ok(finish(run.g.account(), text, meta, row, Source::None))
        })
    }
}

fn edit_url(id: &str) -> String {
    format!("https://docs.google.com/presentation/d/{id}/edit")
}

/// One call of the tool: the deck's id once it has one, and every call's row.
struct Run {
    g: Arc<Google>,
    id: String,
    /// This call made the deck.
    created: bool,
    rows: Vec<Value>,
}

impl Run {
    /// One call, its row kept; a failure says `why` first and carries every row.
    async fn call(
        &mut self,
        (method, class): (&str, &'static str),
        req: theseus_google::Request,
        why: &str,
    ) -> Result<Value, ToolFailure> {
        let meta = Meta {
            tool: WRITE,
            method: method.into(),
            resource: (!self.id.is_empty()).then(|| self.id.clone()),
            class,
            recipients: None,
        };
        match self.g.call(meta, req).await {
            Ok(done) => {
                self.rows.push(done.row);
                Ok(done.response.json())
            }
            Err(f) => Err(failed(f, std::mem::take(&mut self.rows), why)),
        }
    }

    /// A new deck; the object id of the empty slide it starts with.
    async fn create(&mut self, title: Option<&str>) -> Result<Option<String>, ToolFailure> {
        let body = json!({"title": title.unwrap_or("Untitled presentation")});
        let req = request(
            Service::Slides,
            "POST",
            "/v1/presentations".into(),
            vec![],
            Some(body),
            Repeat::NonRepeatable,
        );
        let made = self
            .call(("slides.presentations.create", "write"), req, "")
            .await?;
        self.id = made["presentationId"].as_str().unwrap_or("").to_string();
        self.created = true;
        Ok(made["slides"][0]["objectId"].as_str().map(String::from))
    }

    /// `requests` as one `batchUpdate`.
    async fn batch(&mut self, requests: &[Value], why: &str) -> Result<Value, ToolFailure> {
        let req = request(
            Service::Slides,
            "POST",
            format!("/v1/presentations/{}:batchUpdate", segment(&self.id)),
            vec![],
            Some(json!({"requests": requests})),
            Repeat::NonRepeatable,
        );
        let why = format!("{}{why}", self.after_create());
        self.call(("slides.presentations.batchUpdate", "write"), req, &why)
            .await
    }

    /// What a failure says when the deck was made by this very call.
    fn after_create(&self) -> String {
        if self.created && !self.id.is_empty() {
            format!("The deck {} was made, but the call then failed: ", self.id)
        } else {
            String::new()
        }
    }

    /// The notes onto their slides' notes shapes, whose ids Slides gives.
    async fn notes(&mut self, notes: &[(String, String)]) -> Result<usize, ToolFailure> {
        let fields =
            "slides(objectId,slideProperties.notesPage.notesProperties.speakerNotesObjectId)";
        let req = request(
            Service::Slides,
            "GET",
            format!("/v1/presentations/{}", segment(&self.id)),
            vec![("fields", fields.into())],
            None,
            Repeat::Safe,
        );
        let deck = self
            .call(
                ("slides.presentations.get", "read"),
                req,
                "the speaker notes could not be placed: ",
            )
            .await?;
        let notes_of = |slide: &str| {
            let slides = deck["slides"].as_array().into_iter().flatten();
            slides
                .into_iter()
                .find(|s| s["objectId"] == slide)
                .and_then(|s| {
                    let shape = &s["slideProperties"]["notesPage"]["notesProperties"];
                    shape["speakerNotesObjectId"].as_str().map(String::from)
                })
        };
        let inserts: Vec<Value> = notes
            .iter()
            .filter_map(|(slide, t)| {
                notes_of(slide).map(|o| json!({"insertText": {"objectId": o, "text": t}}))
            })
            .collect();
        if !inserts.is_empty() {
            self.batch(
                &inserts,
                "the slides were added, but their notes were not: ",
            )
            .await?;
        }
        Ok(inserts.len())
    }
}
