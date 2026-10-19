//! The client's id and secret, from the one secret that holds both (Google
//! design §2.1, as the owner decided): two labelled lines in either order, or
//! the JSON the console downloads. An error names the secret and the missing
//! line, never the value.

use std::fmt;

use serde_json::Value;
use zeroize::Zeroizing;

/// An OAuth client. The secret is zeroized on drop and never shown.
#[derive(Clone)]
pub struct ClientSecret {
    pub id: String,
    secret: Zeroizing<String>,
}

impl ClientSecret {
    pub fn new(id: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            secret: Zeroizing::new(secret.into()),
        }
    }

    /// For the token endpoint's form, and nothing else.
    pub fn expose_secret(&self) -> &str {
        &self.secret
    }
}

impl fmt::Debug for ClientSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientSecret")
            .field("id", &"(hidden)")
            .field("secret", &"(hidden)")
            .finish()
    }
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner.trim();
        }
    }
    s
}

/// `client_id: x` or `client_secret = "y"`; other lines are ignored.
fn labelled(text: &str, label: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let at = line.find([':', '='])?;
        let key = line[..at]
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-'], "_");
        let value = unquote(&line[at + 1..]);
        (key == label && !value.is_empty()).then(|| value.to_string())
    })
}

/// The console's `{"installed": {...}}` (or `"web"`), or the bare object.
fn from_json(text: &str, label: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let obj = v.get("installed").or_else(|| v.get("web")).unwrap_or(&v);
    let s = obj.get(label)?.as_str()?.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// The client in `value`, the text of the `[secrets]` entry `name`.
pub fn parse_client(name: &str, value: &str) -> Result<ClientSecret, String> {
    let get = |label: &str| {
        if value.trim_start().starts_with('{') {
            from_json(value, label)
        } else {
            labelled(value, label)
        }
    };
    let id = get("client_id")
        .ok_or_else(|| format!("the client secret `{name}` holds no client_id line"))?;
    let secret = get("client_secret")
        .ok_or_else(|| format!("the client secret `{name}` holds no client_secret line"))?;
    Ok(ClientSecret::new(id, secret))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_secret_parses_from_two_labelled_lines_in_either_order() {
        for text in [
            "client_id: id-1.apps.example\nclient_secret: s3cret-1",
            "client_secret = \"s3cret-1\"\nnotes: ignored\nclient_id = 'id-1.apps.example'  \n",
            "  Client_Secret :s3cret-1\nclient id: id-1.apps.example",
        ] {
            let c = parse_client("google_client", text).unwrap();
            assert_eq!(c.id, "id-1.apps.example", "{text}");
            assert_eq!(c.expose_secret(), "s3cret-1", "{text}");
        }
        let json = r#"{"installed": {"client_id": "id-1.apps.example", "client_secret": "s3cret-1", "auth_uri": "x"}}"#;
        let c = parse_client("google_client", json).unwrap();
        assert_eq!(
            (c.id.as_str(), c.expose_secret()),
            ("id-1.apps.example", "s3cret-1")
        );
        // The error names the secret and the line, never the value.
        let e = parse_client("google_client", "client_secret: only-this-one").unwrap_err();
        assert_eq!(
            e,
            "the client secret `google_client` holds no client_id line"
        );
        assert!(!e.contains("only-this-one"));
        let e = parse_client("google_client", "client_id: only-an-id").unwrap_err();
        assert_eq!(
            e,
            "the client secret `google_client` holds no client_secret line"
        );
        assert!(!format!("{:?}", c).contains("s3cret"));
    }
}
