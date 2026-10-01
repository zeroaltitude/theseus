//! An HTTP request, as the protocols build it and the signer signs it.

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

/// RFC 3986's unreserved characters stay; everything else is encoded. This is
/// AWS's "URI encode", for query strings, form bodies, and URI labels.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// The same, keeping `/`: a greedy URI label (`{Key+}`).
const PATH: &AsciiSet = &UNRESERVED.remove(b'/');

pub(crate) fn encode(s: &str) -> String {
    utf8_percent_encode(s, UNRESERVED).to_string()
}

pub(crate) fn encode_path(s: &str) -> String {
    utf8_percent_encode(s, PATH).to_string()
}

/// `k=v&k2=v2`, every key and value encoded; a key without a value is bare.
pub(crate) fn encode_query(pairs: &[(String, Option<String>)]) -> String {
    let mut out = String::new();
    for (k, v) in pairs {
        if !out.is_empty() {
            out.push('&');
        }
        out.push_str(&encode(k));
        if let Some(v) = v {
            out.push('=');
            out.push_str(&encode(v));
        }
    }
    out
}

/// One request: where it goes, its headers, and its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    /// `https://host[:port]`, with no path.
    pub origin: String,
    /// The path, encoded, from `/`.
    pub path: String,
    /// The query's parameters, not yet encoded, in order. `None` is a key
    /// with no value (`?tagging`).
    pub query: Vec<(String, Option<String>)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// The whole URL, encoded.
    pub fn url(&self) -> String {
        let mut url = format!("{}{}", self.origin, self.path);
        if !self.query.is_empty() {
            url.push('?');
            url.push_str(&encode_query(&self.query));
        }
        url
    }

    /// The host, with its port when it has one.
    pub fn host(&self) -> &str {
        self.origin
            .split_once("://")
            .map_or(self.origin.as_str(), |(_, h)| h)
    }

    /// A header's value; names are case-insensitive.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Sets a header, replacing any of the same name.
    pub(crate) fn set_header(&mut self, name: &str, value: impl Into<String>) {
        self.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        self.headers.push((name.to_owned(), value.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_is_awss() {
        assert_eq!(encode("a b/c~d_e.f-g"), "a%20b%2Fc~d_e.f-g");
        assert_eq!(
            encode_path("photos/2026 trip/é.jpg"),
            "photos/2026%20trip/%C3%A9.jpg"
        );
        assert_eq!(
            encode_query(&[
                ("tagging".into(), None),
                ("prefix".into(), Some("a b".into())),
                ("empty".into(), Some(String::new())),
            ]),
            "tagging&prefix=a%20b&empty="
        );
    }

    #[test]
    fn a_request_reads_back() {
        let mut r = HttpRequest {
            method: "GET",
            origin: "https://s3.us-west-2.amazonaws.com".into(),
            path: "/example-bucket".into(),
            query: vec![("list-type".into(), Some("2".into()))],
            headers: vec![("Content-Type".into(), "text/plain".into())],
            body: Vec::new(),
        };
        assert_eq!(
            r.url(),
            "https://s3.us-west-2.amazonaws.com/example-bucket?list-type=2"
        );
        assert_eq!(r.host(), "s3.us-west-2.amazonaws.com");
        assert_eq!(r.header("content-type"), Some("text/plain"));
        r.set_header("content-type", "application/json");
        assert_eq!(r.headers.len(), 1);
        assert_eq!(r.header("Content-Type"), Some("application/json"));
    }
}
