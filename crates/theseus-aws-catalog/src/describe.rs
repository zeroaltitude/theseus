//! `aws.describe`, the model's lens on the catalog (AWS design §3.1): with no
//! argument the services, with a service its operations by class, with an
//! operation its input as a compact JSON schema. Local, and microseconds.

use serde_json::{json, Map, Value};

use crate::classify::{Class, SecretBearing};
use crate::model::{Kind, OperationRef, Service, ShapeId, ShapeRef};
use crate::Catalog;

/// Enum values shown before the rest are counted.
const ENUM_SHOWN: usize = 50;
/// Structure depth shown in a schema before a nested shape is named only.
const DEPTH: usize = 8;

/// Every service, by name.
pub fn describe_services(c: &Catalog) -> Value {
    json!({
        "snapshot": c.snapshot(),
        "count": c.services().len(),
        "services": c.services().iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
    })
}

/// A service's operations, grouped by class, with the flagged ones listed.
pub fn describe_service(svc: &Service) -> Value {
    let mut by_class: [Vec<&str>; 3] = Default::default();
    let (mut cost, mut secret, mut iac, mut deprecated) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for op in svc.operations() {
        let c = op.classify();
        let slot = match c.class {
            Class::Read => 0,
            Class::Write => 1,
            Class::Run => 2,
        };
        by_class[slot].push(op.name());
        if c.cost_bearing {
            cost.push(op.name());
        }
        if c.secret != SecretBearing::No {
            secret.push(op.name());
        }
        if c.iac_only {
            iac.push(op.name());
        }
        if op.is_deprecated() {
            deprecated.push(op.name());
        }
    }
    let [read, write, run] = by_class;
    json!({
        "service": svc.name(),
        "id": svc.service_id(),
        "name": svc.full_name(),
        "protocol": svc.protocol().as_str(),
        "api_version": svc.api_version(),
        "operations": { "read": read, "write": write, "run": run },
        "cost_bearing": cost,
        "secret_bearing": secret,
        "iac_only": iac,
        "deprecated": deprecated,
    })
}

/// One operation: its class and flags, its HTTP binding, its paginator, and
/// its input and output as compact schemas.
pub fn describe_operation(op: OperationRef<'_>) -> Value {
    let c = op.classify();
    let mut v = json!({
        "service": op.service().name(),
        "operation": op.name(),
        "class": c.class.as_str(),
        "label": c.label(),
        "retry": c.retry.as_str(),
        "http": format!("{} {}", op.method().as_str(), op.request_uri()),
    });
    let o = v.as_object_mut().expect("an object");
    if let Some(t) = &c.idempotency_token {
        o.insert("idempotency_token".into(), json!(t));
    }
    if let SecretBearing::WhenInputTrue(m) = c.secret {
        o.insert("secret_when".into(), json!(m));
    }
    if c.inert {
        o.insert("inert".into(), json!(true));
    }
    if let Some(n) = c.note {
        o.insert("note".into(), json!(n));
    }
    if op.is_deprecated() {
        o.insert("deprecated".into(), json!(true));
    }
    if let Some(p) = op.paginator() {
        o.insert(
            "paginated".into(),
            json!({ "input_tokens": p.input_tokens(), "result_keys": p.result_keys(), "limit_key": p.limit_key() }),
        );
    }
    if op.has_event_stream() {
        o.insert("event_stream".into(), json!(true));
    }
    o.insert(
        "input".into(),
        op.input().map_or(json!({"type": "object"}), input_schema),
    );
    if let Some(out) = op.output() {
        o.insert("output".into(), schema(out, 2, &mut Vec::new()));
    }
    v
}

/// A shape as a compact JSON schema.
pub fn input_schema(shape: ShapeRef<'_>) -> Value {
    schema(shape, DEPTH, &mut Vec::new())
}

fn schema(shape: ShapeRef<'_>, depth: usize, seen: &mut Vec<ShapeId>) -> Value {
    if shape.is_document() {
        return json!({});
    }
    match shape.kind() {
        Kind::Structure => {
            if depth == 0 || seen.contains(&shape.id()) {
                return json!({"type": "object", "shape": shape.name()});
            }
            seen.push(shape.id());
            let mut props = Map::new();
            let mut required = Vec::new();
            for m in shape.members() {
                let mut s = schema(m.shape(), depth - 1, seen);
                if m.is_idempotency_token() {
                    if let Some(o) = s.as_object_mut() {
                        o.insert("idempotency_token".into(), json!(true));
                    }
                }
                props.insert(m.name().to_owned(), s);
                if m.is_required() {
                    required.push(m.name());
                }
            }
            seen.pop();
            let mut v = json!({"type": "object", "properties": props});
            if !required.is_empty() {
                v["required"] = json!(required);
            }
            if shape.is_union() {
                v["one_of_members"] = json!(true);
            }
            v
        }
        Kind::List => match shape.list_member() {
            Some(m) => json!({"type": "array", "items": schema(m.shape(), depth, seen)}),
            None => json!({"type": "array"}),
        },
        Kind::Map => match shape.map_value() {
            Some(m) => {
                json!({"type": "object", "additionalProperties": schema(m.shape(), depth, seen)})
            }
            None => json!({"type": "object"}),
        },
        Kind::String => {
            if shape.enum_count() == 0 {
                return json!({"type": "string"});
            }
            let shown: Vec<&str> = shape.enum_values().take(ENUM_SHOWN).collect();
            let mut v = json!({"type": "string", "enum": shown});
            if shape.enum_count() > ENUM_SHOWN {
                v["enum_more"] = json!(shape.enum_count() - ENUM_SHOWN);
            }
            v
        }
        Kind::Boolean => json!({"type": "boolean"}),
        Kind::Integer | Kind::Long => json!({"type": "integer"}),
        Kind::Float | Kind::Double => json!({"type": "number"}),
        Kind::Timestamp => json!({"type": "string", "format": "date-time"}),
        Kind::Blob if shape.is_streaming() => json!({"type": "string", "streaming": true}),
        Kind::Blob => json!({"type": "string", "contentEncoding": "base64"}),
    }
}
