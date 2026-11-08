//! A credential keyed on the **shape of the value**, not on the member's
//! name (theseus-3i6d1): the presigned URL.
//!
//! **Why no name can catch it.** Every other rule in this directory asks
//! what a member is *called*: [`super::secret::NAMED`], `ENDS`, `HELD`, and
//! the output-shape rule's reading of AWS's `sensitive` marking together
//! with the words of the member and its shape. A presigned URL defeats all
//! of them twice over. It carries its own authorisation — whoever holds the
//! string can perform the operation it was signed for, with no credential of
//! their own — and it arrives in members called `Url`, `url`, `Location`,
//! `DownloadUrl`, `Endpoint`, `S3SignedUrl`, `uri`: names an ordinary,
//! unsigned URL wears too. A name list would therefore have to be maintained
//! one operation at a time, and a new operation that returns one would leak
//! from the day AWS added it, which is exactly how the classes this module
//! was written for got in. `connectparticipant`'s `Websocket.Url` is the
//! proof the list approach does not scale: it took a hand-written `HELD` row
//! of its own, and nothing about that row helps the next one.
//!
//! **What is keyed on.** Not the name, not AWS's marking, not the member's
//! type: the characters of the value itself. A string is a presigned URL
//! when it is an absolute `http`, `https`, `ws` or `wss` URL **and its query
//! carries signature material**: a non-empty signature parameter
//! ([`SIGNATURE`]) together with a parameter naming the key the signature was
//! made with ([`KEYED_BY`]). That covers
//! SigV4 presigning (`X-Amz-Signature` with `X-Amz-Credential`), the older
//! query signing (`Signature` with `AWSAccessKeyId`) and CloudFront's signed
//! URLs (`Signature` with `Key-Pair-Id`, canned or with a `Policy`).
//!
//! **What it does not reach.** The whole value must be the URL. A presigned
//! URL inside free text — a log line, a device configuration, a shell
//! command AWS wrote out for a customer — is not found, for the same reason
//! EC2's VPN sample configuration needed a `MARKED` row of its own: picking a
//! URL out of prose needs a rule about where it ends, and a wrong guess
//! mangles the text. Nor is a presigned URL in a member AWS returns
//! base64-encoded, or one an operation splits into a host and a query.
//!
//! **Why those parameters and not entropy or length.** A signature parameter
//! is what *makes* a URL presigned: a URL that carries one is one, whatever
//! it is called and whoever returned it, and a URL that carries none cannot
//! authorise anything on its own. So the false-positive cost on an ordinary
//! URL in an ordinary field — a console link, a documentation link, a
//! repository URI, an endpoint with `?region=us-west-2` on it — is nil
//! rather than small: none of them carries `X-Amz-Signature`, and the rule
//! never looks at how long the value is or how random it looks. An entropy
//! or length rule would have to guess, and would hold a base64 blob, a
//! request id or a certificate body that is no credential at all.
//!
//! **Where it runs, and why there.** As a last pass over the body, after
//! [`super::secret::hold`] and for **every** call, not only a secret-bearing
//! one (`tools.rs`). Three reasons, in order. It must run for calls the
//! `SECRET` table does not list, because that is where the leak is: the
//! operation that returns a presigned URL is an ordinary read. It must run
//! for secret-bearing calls too, because a presigned URL beside a held token
//! is the partial hold the output-shape rule already fails operations for.
//! And running it *after* the name rule, over what that rule left behind,
//! means a value the name rule already held is an object by the time this
//! pass sees it and is never held twice; the two rules compose instead of
//! racing. A false positive is recoverable in the same way every hold here
//! is: the value is on the secrets board under its handle, the result says
//! so, and nothing is lost — it is withheld from the model, not from the
//! machine.

use serde_json::{json, Value};

use super::secret::{handle, Held};
use crate::secrets::{Secret, SecretBoard};

/// Query parameters whose value is a signature. A presigned URL has one
/// with a non-empty value; nothing else does.
const SIGNATURE: &[&str] = &["x-amz-signature", "signature"];

/// Query parameters that name the key a signature was made with. One of
/// these beside a [`SIGNATURE`] is what tells a signed URL from a field that
/// merely happens to be called `signature`, and every one of them is
/// AWS's own spelling, so the pair cannot be met by accident.
///
/// A CloudFront custom policy's `Policy` parameter is deliberately not here:
/// a URL that carries one carries `Key-Pair-Id` as well, so the row would
/// add nothing but a false-positive surface on an ordinary `?policy=` field.
const KEYED_BY: &[&str] = &[
    "x-amz-credential",
    "x-amz-algorithm",
    "awsaccesskeyid",
    "key-pair-id",
];

/// The schemes a presigned URL arrives under.
const SCHEMES: &[&str] = &["https://", "http://", "wss://", "ws://"];

/// `s` is an absolute URL whose query carries signature material.
///
/// Reads the value and nothing else: no member name, no shape, no catalog.
pub fn is_presigned(s: &str) -> bool {
    let Some(rest) = SCHEMES.iter().find_map(|p| strip_ci(s, p)) else {
        return false;
    };
    // The host must be there, and the query must be after it.
    let Some((host_and_path, after)) = rest.split_once('?') else {
        return false;
    };
    if host_and_path.is_empty() {
        return false;
    }
    // A fragment ends the query.
    let query = after.split('#').next().unwrap_or(after);
    let (mut signed, mut keyed) = (false, false);
    for part in query.split('&') {
        let part = part.strip_prefix("amp;").unwrap_or(part);
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        let name = name.trim().to_ascii_lowercase();
        // An empty signature signs nothing: a template, not a credential.
        if !value.is_empty() && SIGNATURE.contains(&name.as_str()) {
            signed = true;
        }
        if !value.is_empty() && KEYED_BY.contains(&name.as_str()) {
            keyed = true;
        }
    }
    signed && keyed
}

/// `s` without `prefix`, matched in any case.
fn strip_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    (s.len() > prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()))
        .then(|| &s[prefix.len()..])
}

/// What the model is told in a presigned URL's place: where it points, with
/// the query — which is the whole of its authorisation — withheld.
///
/// The origin and the path are kept because they are not the credential: a
/// signature covers them, so neither grants anything on its own, and without
/// them a handle says nothing about what was held.
fn where_it_points(s: &str) -> String {
    let whole = s.chars().count();
    let shown: String = s.split('?').next().unwrap_or(s).chars().take(160).collect();
    format!("a presigned URL, {shown}?<signature withheld>, {whole} characters")
}

/// Hold every presigned URL in `body` on `board`, masked in place: the
/// handles held, in the order they were found.
pub fn hold(board: &SecretBoard, label: &str, body: &mut Value) -> Vec<Held> {
    let mut held = Vec::new();
    walk(body, "", &mut |path, s| {
        let h = handle(label, path);
        board.hold(&h, Secret::new(s.to_string()));
        held.push(Held {
            handle: h.clone(),
            path: path.to_string(),
        });
        h
    });
    held
}

fn walk(v: &mut Value, path: &str, keep: &mut dyn FnMut(&str, &str) -> String) {
    match v {
        Value::String(s) => {
            if is_presigned(s) {
                let shape = where_it_points(s);
                let h = keep(path, s);
                *v = masked_url(&h, &shape);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter_mut().enumerate() {
                walk(x, &format!("{path}[{i}]"), keep);
            }
        }
        Value::Object(o) => {
            for (k, x) in o.iter_mut() {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                walk(x, &p, keep);
            }
        }
        _ => {}
    }
}

/// The mask: the same two keys [`super::secret`] writes for a held value,
/// so every surface that already reads a held secret reads this one.
fn masked_url(handle: &str, shape: &str) -> Value {
    json!({"secret": handle, "shape": format!("<masked: {shape}>")})
}
