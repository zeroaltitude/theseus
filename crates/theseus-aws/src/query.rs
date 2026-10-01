//! The query protocols' requests: awsQuery's form body (IAM, STS, SNS,
//! CloudFormation) and EC2's variant of it, as botocore serializes them.

use theseus_aws_catalog::{MemberRef, OperationRef, TimestampFormat};

use crate::scalar;
use crate::value::{self, V};

/// The form's parameters: `Action`, `Version`, then the input, in the
/// shape's member order.
pub(crate) fn form(
    op: OperationRef<'_>,
    input: Option<&V<'_>>,
    ec2: bool,
) -> Vec<(String, Option<String>)> {
    let mut out = vec![
        ("Action".to_owned(), op.name().to_owned()),
        ("Version".to_owned(), op.service().api_version().to_owned()),
    ];
    if let Some(V::Struct(ms)) = input {
        for (m, v) in ms {
            ser(&mut out, name(*m, ec2), *m, v, ec2);
        }
    }
    out.into_iter().map(|(k, v)| (k, Some(v))).collect()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A member's name in the form. EC2 prefers its `queryName`, then its
/// `locationName` capitalized; the query protocol, its `locationName`.
fn name(m: MemberRef<'_>, ec2: bool) -> String {
    sub_name(m, m.name(), ec2)
}

fn sub_name(m: MemberRef<'_>, default: &str, ec2: bool) -> String {
    if ec2 {
        if let Some(q) = m.query_name() {
            return q.to_owned();
        }
        return value::location_name(m).map_or_else(|| default.to_owned(), capitalize);
    }
    value::location_name(m).unwrap_or(default).to_owned()
}

fn ser(out: &mut Vec<(String, String)>, prefix: String, m: MemberRef<'_>, v: &V<'_>, ec2: bool) {
    match v {
        V::Struct(ms) => {
            for (cm, cv) in ms {
                let p = format!("{prefix}.{}", name(*cm, ec2));
                ser(out, p, *cm, cv, ec2);
            }
        }
        V::List(items) => {
            let Some(lm) = m.shape().list_member() else {
                return;
            };
            if ec2 {
                // EC2's lists are always flattened, and an empty one is left out.
                for (i, item) in items.iter().enumerate() {
                    ser(out, format!("{prefix}.{}", i + 1), lm, item, ec2);
                }
                return;
            }
            if items.is_empty() {
                // The query protocol sends an empty list as an empty value.
                out.push((prefix, String::new()));
                return;
            }
            let list_prefix = if m.is_flattened() {
                // A flattened list's member name, if it has one, replaces the
                // last part of the prefix.
                match value::location_name(lm) {
                    Some(n) => match prefix.rsplit_once('.') {
                        Some((head, _)) => format!("{head}.{n}"),
                        None => n.to_owned(),
                    },
                    None => prefix,
                }
            } else {
                format!("{prefix}.{}", value::location_name(lm).unwrap_or("member"))
            };
            for (i, item) in items.iter().enumerate() {
                ser(out, format!("{list_prefix}.{}", i + 1), lm, item, ec2);
            }
        }
        V::Map(entries) => {
            let (Some(km), Some(vm)) = (m.shape().map_key(), m.shape().map_value()) else {
                return;
            };
            let full = if m.is_flattened() {
                prefix
            } else {
                format!("{prefix}.entry")
            };
            let kn = sub_name(km, "key", ec2);
            let vn = sub_name(vm, "value", ec2);
            for (i, (k, item)) in entries.iter().enumerate() {
                out.push((format!("{full}.{}.{kn}", i + 1), k.clone()));
                ser(out, format!("{full}.{}.{vn}", i + 1), vm, item, ec2);
            }
        }
        scalar => out.push((
            prefix,
            scalar::text(m.shape(), scalar, TimestampFormat::Iso8601),
        )),
    }
}
