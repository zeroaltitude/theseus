//! The store's version rule, held by a test (P5b; Review 2's R8). A change to
//! what a kind's records hold bumps that kind's number in `kinds::SCHEMAS`,
//! with a reader for the layout it replaces. Review held it; this test holds
//! it now.
//!
//! For each kind, a record of its type is filled through the type's own
//! `Deserialize` (`Fill`): every field a struct declares, every `Option`
//! some, every sequence and map one item, numbers non-zero, booleans true,
//! and, over as many passes as it takes, every variant of every enum. So
//! nothing a serializer skips when empty is skipped. Serialized as the store
//! writes it, its shape (each field's path and leaf type, and each enum's
//! variants) is compared with `tests/golden/record_schemas.txt`, under the
//! kind's schema number:
//!
//! - a changed shape under the same number fails: bump the number, with the
//!   reader for the old layout and a test that reads it;
//! - a number with no recorded shape fails until `THESEUS_GOLDEN=write`
//!   records it. That write adds sections and never changes one: a shape is
//!   recorded once per number. `THESEUS_GOLDEN=rewrite` replaces every
//!   section, for a change to this test itself.
//!
//! The internally tagged enums of a node's body (`Body`, `AttachmentContent`)
//! say their shape only with data, so each takes a sample per variant, every
//! field filled, and serde's own list of their variants holds the samples
//! complete. A meta record's layout is its key's (the tightenings carry their
//! own `schema`; the rest are strings), and edges are no longer written, so
//! neither has a section.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::de::value::{Error, StrDeserializer};
use serde::de::{
    DeserializeOwned, DeserializeSeed, Deserializer, EnumAccess, Error as _, Expected, MapAccess,
    SeqAccess, VariantAccess, Visitor,
};
use serde::Serialize;
use serde_json::{json, Value};
use theseus_store::kinds;

use theseus_kernel::types::{RetryClass, Wake};

use crate::node::{Attachment, AttachmentContent, Body, ResultStatus};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/record_schemas.txt"
);

/// What a `serde_json::Value` field is filled with: the shape reads it as
/// `any`, since its content is free.
const ANY: &str = "\u{1}any";
/// A map's key: the shape reads it as `{}`, a map, not a field.
const KEY: &str = "\u{1}key";
/// How deep `Fill` goes before an `Option` is none and a sequence empty: a
/// type that holds itself ends there.
const DEEPEST: usize = 12;

/// An internally tagged enum: the key its tag is written under, its
/// variants, and a sample of the one named, built with the filler for what
/// it holds (only the one taken is built, so what it holds is shaped).
struct Tagged {
    tag: &'static str,
    variants: &'static [&'static str],
    sample: fn(&Filler, &str) -> Value,
}

/// What the passes share: which variant each enum takes next, and what each
/// pass met.
#[derive(Default)]
struct State {
    /// Each enum met, by name, with its variants.
    enums: BTreeMap<&'static str, Vec<String>>,
    /// The variants each enum has taken so far.
    taken: BTreeMap<&'static str, BTreeSet<usize>>,
    /// Whether this pass took a variant no pass took before.
    fresh: bool,
}

struct Filler {
    st: RefCell<State>,
    tagged: BTreeMap<&'static str, Tagged>,
}

impl Filler {
    fn new() -> Self {
        let mut tagged: BTreeMap<&'static str, Tagged> = BTreeMap::new();
        tagged.insert(
            "Body",
            Tagged {
                tag: "kind",
                variants: &[
                    "user_message",
                    "assistant_message",
                    "tool_call",
                    "tool_result",
                ],
                sample: body,
            },
        );
        tagged.insert(
            "AttachmentContent",
            Tagged {
                tag: "kind",
                variants: &["text", "image", "not_read"],
                sample: attachment_content,
            },
        );
        tagged.insert(
            "Wake",
            Tagged {
                tag: "on",
                variants: &[
                    "due_at",
                    "actions",
                    "execution",
                    "confirm",
                    "input",
                    "budget",
                ],
                sample: wake,
            },
        );
        tagged.insert(
            "RetryClass",
            Tagged {
                tag: "class",
                variants: &["safe_to_repeat", "non_repeatable", "idempotent_with_key"],
                sample: retry_class,
            },
        );
        Self {
            st: RefCell::default(),
            tagged,
        }
    }

    /// A `T` as this pass fills it.
    fn fill<T: DeserializeOwned>(&self) -> T {
        T::deserialize(Fill {
            filler: self,
            depth: 0,
            key: false,
        })
        .unwrap_or_else(|e| panic!("filling {}: {e}", std::any::type_name::<T>()))
    }

    /// The variant an enum of `n` takes: the first no pass has taken, while
    /// any is left.
    fn pick(&self, name: &'static str, variants: Vec<String>) -> usize {
        let mut st = self.st.borrow_mut();
        let n = variants.len();
        st.enums.insert(name, variants);
        let taken = st.taken.entry(name).or_default();
        match (0..n).find(|i| !taken.contains(i)) {
            Some(i) => {
                taken.insert(i);
                st.fresh = true;
                i
            }
            None => 0,
        }
    }

    /// Every shape a `T` takes, pass after pass, until a pass takes no new
    /// variant.
    fn shape<T: DeserializeOwned + Serialize>(&self, out: &mut BTreeSet<String>) {
        loop {
            self.st.borrow_mut().fresh = false;
            let v = serde_json::to_value(self.fill::<T>()).unwrap();
            self.paths(&v, "", out);
            if !self.st.borrow().fresh {
                return;
            }
        }
    }

    /// Each leaf's path and type. An internally tagged variant names itself
    /// in the path (`body{kind=tool_call}.input`).
    fn paths(&self, v: &Value, path: &str, out: &mut BTreeSet<String>) {
        match v {
            Value::Object(m) => {
                let tag = self.tagged.values().find_map(|t| match m.get(t.tag) {
                    Some(Value::String(s)) if self.is_variant(s) => Some((t.tag, s)),
                    _ => None,
                });
                let here = match tag {
                    Some((key, s)) => format!("{path}{{{key}={s}}}"),
                    None => path.to_string(),
                };
                for (k, x) in m {
                    if tag.is_some_and(|(key, _)| key == k) {
                        continue;
                    }
                    let seg = if k == KEY || k == "1" { "{}" } else { k };
                    self.paths(x, &format!("{here}.{seg}"), out);
                }
                if m.is_empty() {
                    out.insert(format!("{here}: {{}}"));
                }
            }
            Value::Array(a) => {
                for x in a {
                    self.paths(x, &format!("{path}[]"), out);
                }
                if a.is_empty() {
                    out.insert(format!("{path}: []"));
                }
            }
            leaf => {
                let t = match leaf {
                    Value::String(s) if s == ANY => "any",
                    Value::String(_) => "string",
                    Value::Number(n) if n.is_f64() => "float",
                    Value::Number(_) => "int",
                    Value::Bool(_) => "bool",
                    _ => "null",
                };
                out.insert(format!("{path}: {t}"));
            }
        }
    }

    /// Whether `s` is a variant of an internally tagged enum met so far.
    fn is_variant(&self, s: &str) -> bool {
        let st = self.st.borrow();
        self.tagged
            .keys()
            .any(|name| st.enums.get(name).is_some_and(|v| v.iter().any(|x| x == s)))
    }
}

/// A deserializer that fills whatever type asks it.
#[derive(Clone, Copy)]
struct Fill<'a> {
    filler: &'a Filler,
    depth: usize,
    /// A map's key, which is `KEY` where a string is asked for.
    key: bool,
}

impl<'a> Fill<'a> {
    fn deeper(self) -> Self {
        Self {
            depth: self.depth + 1,
            key: false,
            ..self
        }
    }
    fn more(self) -> usize {
        usize::from(self.depth < DEEPEST)
    }
}

macro_rules! fill_with {
    ($($method:ident => $visit:ident($($value:expr)?);)*) => {
        $(fn $method<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
            v.$visit($($value)?)
        })*
    };
}

impl<'de, 'a> Deserializer<'de> for Fill<'a> {
    type Error = Error;

    /// What a type does not say the shape of: `serde_json::Value`, filled
    /// with `ANY`, and an internally tagged enum, filled from its samples.
    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        let expecting = format!("{}", &v as &dyn Expected);
        if let Some(name) = expecting.strip_prefix("internally tagged enum ") {
            let Some((name, t)) = self.filler.tagged.get_key_value(name) else {
                panic!("{expecting}: give it samples in tests_schemas (Filler::new)");
            };
            let names = t.variants.iter().map(|s| s.to_string()).collect();
            let i = self.filler.pick(name, names);
            let sample = (t.sample)(self.filler, t.variants[i]);
            return sample.deserialize_any(v).map_err(Error::custom);
        }
        if expecting != "any valid JSON value" {
            panic!("tests_schemas can't fill a type that expects {expecting}");
        }
        v.visit_str(ANY)
    }

    fill_with! {
        deserialize_bool => visit_bool(true);
        deserialize_i8 => visit_i64(1);
        deserialize_i16 => visit_i64(1);
        deserialize_i32 => visit_i64(1);
        deserialize_i64 => visit_i64(1);
        deserialize_i128 => visit_i128(1);
        deserialize_u8 => visit_u64(1);
        deserialize_u16 => visit_u64(1);
        deserialize_u32 => visit_u64(1);
        deserialize_u64 => visit_u64(1);
        deserialize_u128 => visit_u128(1);
        deserialize_f32 => visit_f64(1.5);
        deserialize_f64 => visit_f64(1.5);
        deserialize_char => visit_char('x');
        deserialize_bytes => visit_bytes(b"x");
        deserialize_byte_buf => visit_bytes(b"x");
        deserialize_unit => visit_unit();
        deserialize_ignored_any => visit_unit();
    }

    fn deserialize_str<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        v.visit_str(if self.key { KEY } else { "x" })
    }
    fn deserialize_string<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        self.deserialize_str(v)
    }
    fn deserialize_identifier<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        self.deserialize_str(v)
    }
    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        if self.more() == 1 {
            v.visit_some(self.deeper())
        } else {
            v.visit_none()
        }
    }
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        v: V,
    ) -> Result<V::Value, Error> {
        v.visit_unit()
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        v: V,
    ) -> Result<V::Value, Error> {
        v.visit_newtype_struct(self)
    }
    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        v.visit_seq(Items {
            fill: self.deeper(),
            left: self.more(),
        })
    }
    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, v: V) -> Result<V::Value, Error> {
        v.visit_seq(Items {
            fill: self.deeper(),
            left: len,
        })
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        len: usize,
        v: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, v)
    }
    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Error> {
        v.visit_map(Entries {
            fill: self.deeper(),
            left: self.more(),
        })
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Error> {
        v.visit_map(Fields {
            fill: self.deeper(),
            fields,
            at: 0,
        })
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Error> {
        let i = self
            .filler
            .pick(name, variants.iter().map(|s| s.to_string()).collect());
        v.visit_enum(Variant {
            fill: self.deeper(),
            name: variants[i],
        })
    }
}

/// A sequence of `left` items.
struct Items<'a> {
    fill: Fill<'a>,
    left: usize,
}

impl<'de> SeqAccess<'de> for Items<'_> {
    type Error = Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(self.fill).map(Some)
    }
}

/// A map of `left` entries.
struct Entries<'a> {
    fill: Fill<'a>,
    left: usize,
}

impl<'de> MapAccess<'de> for Entries<'_> {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(Fill {
            key: true,
            ..self.fill
        })
        .map(Some)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(self.fill)
    }
}

/// A struct's declared fields, every one.
struct Fields<'a> {
    fill: Fill<'a>,
    fields: &'static [&'static str],
    at: usize,
}

impl<'de> MapAccess<'de> for Fields<'_> {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some(name) = self.fields.get(self.at) else {
            return Ok(None);
        };
        self.at += 1;
        seed.deserialize(StrDeserializer::<Error>::new(name))
            .map(Some)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(self.fill)
    }
}

/// The variant an enum takes this time.
struct Variant<'a> {
    fill: Fill<'a>,
    name: &'static str,
}

impl<'de, 'a> EnumAccess<'de> for Variant<'a> {
    type Error = Error;
    type Variant = Self;
    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
        Ok((
            seed.deserialize(StrDeserializer::<Error>::new(self.name))?,
            self,
        ))
    }
}

impl<'de> VariantAccess<'de> for Variant<'_> {
    type Error = Error;
    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(self.fill)
    }
    fn tuple_variant<V: Visitor<'de>>(self, len: usize, v: V) -> Result<V::Value, Error> {
        v.visit_seq(Items {
            fill: self.fill,
            left: len,
        })
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Error> {
        v.visit_map(Fields {
            fill: self.fill,
            fields,
            at: 0,
        })
    }
}

/// A node's body of the variant named, each field filled (a struct literal
/// names every one, so a new field stops this from compiling until it is
/// filled here too).
fn body(f: &Filler, variant: &str) -> Value {
    let b = match variant {
        "user_message" => Body::UserMessage {
            text: "x".into(),
            attachments: vec![f.fill::<Attachment>()],
        },
        "assistant_message" => Body::AssistantMessage {
            blocks: vec![json!(ANY)],
            model: "x".into(),
            provider: "x".into(),
            stop_reason: Some("x".into()),
            usage: f.fill(),
            cost_usd: Some(1.5),
            catalog_version: Some("x".into()),
            request_id: Some("x".into()),
            correlation_id: Some("x".into()),
            compilation_id: Some("x".into()),
            request_digest: Some("x".into()),
        },
        "tool_call" => Body::ToolCall {
            tool_use_id: "x".into(),
            tool: "x".into(),
            wire_name: "x".into(),
            input: json!(ANY),
            assistant_node: "x".into(),
            correlation_id: Some("x".into()),
            gate: Some(Box::new(f.fill())),
        },
        "tool_result" => Body::ToolResult {
            tool_use_id: "x".into(),
            tool: "x".into(),
            status: f.fill::<ResultStatus>(),
            is_error: true,
            content: "x".into(),
            correlation_id: Some("x".into()),
            bytes_total: 1,
            truncated: true,
            full_ref: Some("x".into()),
            duration_ms: Some(1),
            late: true,
            meta: json!(ANY),
            image: Some(f.fill()),
            external: Some(f.fill()),
        },
        other => panic!("no sample of Body::{other}"),
    };
    serde_json::to_value(b).unwrap()
}

/// An attachment's content of the variant named.
fn attachment_content(_: &Filler, variant: &str) -> Value {
    let c = match variant {
        "text" => AttachmentContent::Text {
            text: "x".into(),
            cut: true,
        },
        "image" => AttachmentContent::Image {
            digest: "x".into(),
            width: 1,
            height: 1,
        },
        "not_read" => AttachmentContent::NotRead { reason: "x".into() },
        other => panic!("no sample of AttachmentContent::{other}"),
    };
    serde_json::to_value(c).unwrap()
}

/// What wakes an execution, of the variant named.
fn wake(_: &Filler, variant: &str) -> Value {
    let w = match variant {
        "due_at" => Wake::DueAt { at_ms: 1 },
        "actions" => Wake::Actions {
            correlation_ids: vec!["x".into()],
        },
        "execution" => Wake::Execution {
            execution_id: "x".into(),
        },
        "confirm" => Wake::Confirm {
            confirm_id: "x".into(),
        },
        "input" => Wake::Input,
        "budget" => Wake::Budget {
            correlation_id: "x".into(),
        },
        other => panic!("no sample of Wake::{other}"),
    };
    serde_json::to_value(w).unwrap()
}

/// An action's retry class, of the variant named.
fn retry_class(_: &Filler, variant: &str) -> Value {
    let c = match variant {
        "safe_to_repeat" => RetryClass::SafeToRepeat,
        "non_repeatable" => RetryClass::NonRepeatable,
        "idempotent_with_key" => RetryClass::IdempotentWithKey { key: "x".into() },
        other => panic!("no sample of RetryClass::{other}"),
    };
    serde_json::to_value(c).unwrap()
}

/// An internally tagged enum's variants, as serde lists them when it meets
/// one it doesn't know.
fn variants_of<T: DeserializeOwned>(tag: &str) -> BTreeSet<String> {
    let mut probe = serde_json::Map::new();
    probe.insert(tag.to_string(), json!("\u{1}"));
    let err = serde_json::from_value::<T>(Value::Object(probe))
        .err()
        .expect("an unknown variant is refused")
        .to_string();
    let listed = err.split("expected").nth(1).unwrap_or_default();
    listed
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// Each kind's record types, and the shape they take: their paths, then
/// every enum they hold.
fn shapes() -> BTreeMap<u16, Vec<String>> {
    let mut out = BTreeMap::new();
    let mut kind = |k: u16, add: &dyn Fn(&Filler, &mut BTreeSet<String>)| {
        let f = Filler::new();
        let mut paths = BTreeSet::new();
        add(&f, &mut paths);
        let mut lines: Vec<String> = paths.into_iter().collect();
        for (name, variants) in &f.st.borrow().enums {
            lines.push(format!("enum {name}: {}", variants.join("|")));
        }
        out.insert(k, lines);
    };
    kind(kinds::SESSION, &|f, p| {
        f.shape::<crate::session::SessionRecord>(p)
    });
    // The kernel's rows and the core's are one layout.
    kind(kinds::LEDGER, &|f, p| {
        f.shape::<theseus_kernel::types::LedgerRow>(p);
        f.shape::<crate::ledger::LedgerRow>(p);
    });
    kind(kinds::EXECUTION, &|f, p| {
        f.shape::<theseus_kernel::types::Execution>(p)
    });
    kind(kinds::ACTION, &|f, p| {
        f.shape::<theseus_kernel::types::Action>(p)
    });
    kind(kinds::COMPLETION, &|f, p| {
        f.shape::<theseus_kernel::types::Completion>(p)
    });
    kind(kinds::NODE, &|f, p| f.shape::<crate::node::Node>(p));
    kind(kinds::COMPILATION, &|f, p| {
        f.shape::<crate::compiler::Compilation>(p)
    });
    // The outbox's posts are actions of their own kind.
    kind(kinds::OUTBOX, &|f, p| {
        f.shape::<theseus_kernel::types::Action>(p)
    });
    out
}

/// The kinds with no section, and why.
const UNSHAPED: [(u16, &str); 2] = [
    (kinds::META, "a meta record's layout is its key's"),
    (kinds::EDGE, "no longer written"),
];

/// The golden's sections: `name @ schema` to its lines, in file order.
fn sections(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut at: Option<String> = None;
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("== ") {
            at = Some(head.to_string());
            out.entry(head.to_string()).or_default();
        } else if let (Some(h), false) = (&at, line.is_empty() || line.starts_with('#')) {
            out.get_mut(h).unwrap().push(line.to_string());
        }
    }
    out
}

fn render(sections: &BTreeMap<String, Vec<String>>) -> String {
    let mut s = String::from(
        "# The shape of each record kind, by schema number (P5b; Review 2's R8), as\n\
         # crates/theseus-core/src/tests_schemas.rs fills and writes it. A shape is\n\
         # recorded once per number: a changed shape takes a new number.\n",
    );
    for (head, lines) in sections {
        let _ = write!(s, "\n== {head}\n");
        for l in lines {
            let _ = writeln!(s, "{l}");
        }
    }
    s
}

#[test]
fn each_kinds_record_shape_is_recorded_under_its_schema_number() {
    // Every kind is shaped here, or says why not.
    let shapes = shapes();
    for (k, _) in kinds::SCHEMAS {
        assert!(
            shapes.contains_key(&k) || UNSHAPED.iter().any(|(u, _)| *u == k),
            "kind {} ({k}) has no shape in tests_schemas: add its record type",
            kinds::name(k)
        );
    }
    // The tagged enums' samples cover every variant serde knows, and each
    // sample is the variant it says.
    let f = Filler::new();
    let checked = [
        ("Body", variants_of::<Body>("kind")),
        (
            "AttachmentContent",
            variants_of::<AttachmentContent>("kind"),
        ),
        ("Wake", variants_of::<Wake>("on")),
        ("RetryClass", variants_of::<RetryClass>("class")),
    ];
    assert_eq!(
        checked.iter().map(|(n, _)| *n).collect::<BTreeSet<_>>(),
        f.tagged.keys().copied().collect::<BTreeSet<_>>(),
        "every tagged enum with samples is checked against serde's variants"
    );
    for (name, known) in checked {
        let t = &f.tagged[name];
        let listed: BTreeSet<String> = t.variants.iter().map(|s| s.to_string()).collect();
        assert_eq!(listed, known, "{name}: a sample for each variant");
        for v in t.variants {
            assert_eq!((t.sample)(&f, v)[t.tag], *v, "{name}::{v}'s sample");
        }
    }

    let want_text = std::fs::read_to_string(GOLDEN).unwrap_or_default();
    let mut golden = sections(&want_text);
    let mode = std::env::var("THESEUS_GOLDEN").unwrap_or_default();
    let mut problems = Vec::new();
    for (k, got) in &shapes {
        let head = format!("{} @ {}", kinds::name(*k), kinds::schema(*k));
        match golden.get(&head) {
            Some(want) if want == got => {}
            Some(want) if mode != "rewrite" => {
                let added: Vec<&String> = got.iter().filter(|l| !want.contains(l)).collect();
                let gone: Vec<&String> = want.iter().filter(|l| !got.contains(l)).collect();
                problems.push(format!(
                    "the shape of {head} changed, and kinds::SCHEMAS still says {}: bump it, with \
                     the reader for the old layout and a test that reads it (P5b), then \
                     THESEUS_GOLDEN=write records the new shape.\n  new: {added:?}\n  gone: {gone:?}",
                    kinds::schema(*k)
                ));
            }
            _ if mode == "write" || mode == "rewrite" => {
                golden.insert(head, got.clone());
            }
            _ => problems.push(format!(
                "{head} has no recorded shape: THESEUS_GOLDEN=write records it"
            )),
        }
    }
    if mode == "write" || mode == "rewrite" {
        std::fs::write(GOLDEN, render(&golden)).unwrap();
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
