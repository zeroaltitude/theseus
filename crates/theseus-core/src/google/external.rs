//! What a Google result is, to the marker and to the data rule (the Google
//! design §4.1, §4.2). **Every** result is private. A result is also outside
//! text (`theseus_tools::External`) unless it is the owner's own: anyone can
//! email the owner, share a document, or invite them to an event, so a read
//! returns a stable marker (`gmail:thread/<id>`, `drive:<id>`, `calendar:<id>`)
//! and the content pack reads it from there. The owner-authored rule (§4.2) is one
//! metadata check per result: every `From` is the account, the event's
//! organizer is the account, or the file is the account's own and not shared.
//! A row never builds a Google `ToolOutput` any way but [`finish`].

use serde_json::{json, Value};
use theseus_tools::{External, ToolOutput};

use super::private;

/// What a result's text came from, as far as who wrote it.
pub enum Source<'a> {
    /// A mail thread or message: its thread id and each message's `From`.
    Mail { thread: &'a str, from: &'a [String] },
    /// A Drive file: its id, whether the account owns it, and whether it is shared.
    File {
        id: &'a str,
        owned_by_account: bool,
        shared: bool,
    },
    /// A calendar event: its id, and whether the account organizes it.
    Event {
        id: &'a str,
        organizer_is_account: bool,
    },
    /// Text no one metadata check clears (`google.call`'s, a listing of rows
    /// from several senders): outside text, marked by this label.
    Outside { label: &'a str },
    /// Not text anyone else wrote (a draft the owner made, a label change).
    None,
}

/// The address in a `From` header: `Name <a@b>` and `a@b` alike.
fn address(from: &str) -> String {
    let inner = match (from.rfind('<'), from.rfind('>')) {
        (Some(a), Some(b)) if a < b => &from[a + 1..b],
        _ => from,
    };
    inner.trim().to_ascii_lowercase()
}

/// Every sender is the account.
pub fn owner_authored(account: &str, from: &[String]) -> bool {
    !from.is_empty()
        && from
            .iter()
            .all(|f| address(f) == account.to_ascii_lowercase())
}

impl Source<'_> {
    /// The marker of outside text, or None when the result is the owner's own.
    pub fn marker(&self, account: &str) -> Option<External> {
        let url = match self {
            Source::Mail { thread, from } => {
                (!owner_authored(account, from)).then(|| format!("gmail:thread/{thread}"))?
            }
            Source::File {
                id,
                owned_by_account,
                shared,
            } => (!(*owned_by_account && !*shared)).then(|| format!("drive:{id}"))?,
            Source::Event {
                id,
                organizer_is_account,
            } => (!organizer_is_account).then(|| format!("calendar:{id}"))?,
            Source::Outside { label } => (*label).to_string(),
            Source::None => return None,
        };
        Some(External { url })
    }
}

/// A Google tool's result: its text, its `meta` (the tool's own object, with
/// the `private` mark and the call's `google.called` row added), and the
/// External marker the source calls for. The one way a Google tool makes a
/// `ToolOutput`.
pub fn finish(
    account: &str,
    text: String,
    meta: Value,
    row: Value,
    source: Source<'_>,
) -> (ToolOutput, Option<External>) {
    let mut meta = match meta {
        Value::Object(m) => Value::Object(m),
        _ => json!({}),
    };
    meta[private::KEY] = true.into();
    meta["google_called"] = row;
    (ToolOutput { text, meta }, source.marker(account))
}

/// What `finish` returns, for a tool's `run_async` to hand back.
pub type Finished = (ToolOutput, Option<External>);

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "owner@example.test";

    fn url(s: Source<'_>) -> Option<String> {
        s.marker(ME).map(|e| e.url)
    }

    #[test]
    fn owner_authored_unshared_content_is_not_external() {
        let own = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        // A thread where every message is from the account, in either header form.
        assert_eq!(
            url(Source::Mail {
                thread: "t1",
                from: &own(&[ME, "Owner <Owner@Example.Test>"])
            }),
            None
        );
        // One stranger among them, or none at all to judge by, and it is outside text.
        assert_eq!(
            url(Source::Mail {
                thread: "t1",
                from: &own(&[ME, "A Stranger <stranger@example.test>"])
            })
            .as_deref(),
            Some("gmail:thread/t1")
        );
        assert_eq!(
            url(Source::Mail {
                thread: "t2",
                from: &[]
            })
            .as_deref(),
            Some("gmail:thread/t2")
        );
        // A lookalike address is not the account.
        assert!(url(Source::Mail {
            thread: "t3",
            from: &own(&["owner@example.test.evil.test"])
        })
        .is_some());
        // An event the account organizes, and a file it owns that is not shared, are its own.
        assert_eq!(
            url(Source::Event {
                id: "e1",
                organizer_is_account: true
            }),
            None
        );
        assert_eq!(
            url(Source::Event {
                id: "e2",
                organizer_is_account: false
            })
            .as_deref(),
            Some("calendar:e2")
        );
        assert_eq!(
            url(Source::File {
                id: "f1",
                owned_by_account: true,
                shared: false
            }),
            None
        );
        assert_eq!(
            url(Source::File {
                id: "f2",
                owned_by_account: true,
                shared: true
            })
            .as_deref(),
            Some("drive:f2")
        );
        assert_eq!(
            url(Source::File {
                id: "f3",
                owned_by_account: false,
                shared: false
            })
            .as_deref(),
            Some("drive:f3")
        );
        // `google.call` is always outside text; a draft the owner made is not.
        assert_eq!(
            url(Source::Outside {
                label: "google:drive.files.list"
            })
            .as_deref(),
            Some("google:drive.files.list")
        );
        assert_eq!(url(Source::None), None);
    }

    #[test]
    fn every_result_is_private_whoever_wrote_it() {
        for source in [
            Source::Mail {
                thread: "t",
                from: &[ME.to_string()],
            },
            Source::File {
                id: "f",
                owned_by_account: true,
                shared: false,
            },
            Source::Event {
                id: "e",
                organizer_is_account: true,
            },
            Source::None,
        ] {
            let (out, _) = finish(
                ME,
                "text".into(),
                json!({"id": "x"}),
                json!({"method": "m"}),
                source,
            );
            assert!(private::marked(&out.meta), "{:?}", out.meta);
            assert_eq!(out.meta["id"], "x", "the tool's own meta stays");
            assert_eq!(out.meta["google_called"]["method"], "m");
        }
    }
}
