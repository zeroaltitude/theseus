//! The vault's names, read by the daemon (issue 26): who the token is, which
//! vaults it sees, and the title of each item in one. Never a value, a field,
//! a note or a document.
//!
//! **What keeps a value out.** Three things, each enough to be tested alone:
//!
//! - The command line is built from [`Verb`], a closed set of list verbs. No
//!   string a model wrote is ever part of it: a vault is named to `op` by the
//!   id `op` itself listed ([`VaultId`], letters and digits only), and a title
//!   filter is applied here, to what came back.
//! - What comes back is parsed and rebuilt from the named keys only
//!   ([`Account`], [`Vault`], [`ItemName`], [`FieldName`]), so a key `op` adds
//!   tomorrow, or a field it already lists (an item's username, its web
//!   addresses, its tags), does not ride through. An item's fields are the one
//!   answer that carries values (`op` lists them beside each label): the item's
//!   text is held in zeroizing memory, only each field's label, section and a
//!   kind from a closed list are kept, and the value key is not read.
//! - A failure is a [`Failed`] class, never `op`'s own stderr.
//!
//! The token is the daemon's: `OpReader::op` hands it to the child, and the
//! child's answer is read here and nowhere a turn's process can reach.

use std::process::Stdio;

use serde_json::Value;
use zeroize::Zeroizing;

use super::{OpReader, OP_TIMEOUT};

/// The most output of one list this reads, in bytes: a vault of many
/// thousands of items is asked for by a narrower look, not read whole.
pub const MAX_OUTPUT: usize = 4 * 1024 * 1024;
/// The longest a name is kept, in characters.
const MAX_NAME: usize = 120;

/// A vault's id as `op` lists it: letters and digits, never a flag, a path, a
/// reference or a space. Made only from what `op` itself answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultId(String);

impl VaultId {
    /// The id, or `None` for anything that is not one.
    pub fn parse(s: &str) -> Option<Self> {
        let ok = (8..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric());
        ok.then(|| Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An item's id as `op` lists it: letters and digits, never a flag, a path, a
/// reference or a space. Made only from what `op` itself answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemId(String);

impl ItemId {
    /// The id, or `None` for anything that is not one.
    pub fn parse(s: &str) -> Option<Self> {
        let ok = (8..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric());
        ok.then(|| Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What an account may do in a vault, as the vault's program names it
/// (`op vault list --permission`): the two a keep needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Make an item.
    CreateItems,
    /// Change an item that is there.
    EditItems,
}

impl Permission {
    fn name(self) -> &'static str {
        match self {
            Permission::CreateItems => "create_items",
            Permission::EditItems => "edit_items",
        }
    }
}

/// What may be asked of `op`: four list verbs and one get of an item by the
/// id `op` listed, and nothing else exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verb {
    /// Which account the token is.
    Whoami,
    /// The vaults the token sees.
    Vaults,
    /// The vaults the token sees AND holds a permission in: the program's own
    /// answer to "may this account write there", asked by name and never
    /// guessed from the plain list.
    VaultsWith(Permission),
    /// The items of one vault, or of every vault the token sees.
    Items(Option<VaultId>),
    /// One item, by its id in the vault of that id, for the labels of its
    /// fields. Its answer holds the fields' values, so it is read by
    /// [`OpReader::look_item`] alone, which keeps its text out of any plain
    /// `String`. The vault is named because the program refuses a service
    /// account an `item get` that does not name one.
    Item(VaultId, ItemId),
}

impl Verb {
    /// The arguments, in full. Every one is a literal here but a vault's id
    /// and an item's.
    pub fn argv(&self) -> Vec<String> {
        let lit = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        match self {
            Verb::Whoami => lit(&["whoami", "--format", "json"]),
            Verb::Vaults => lit(&["vault", "list", "--format", "json"]),
            Verb::VaultsWith(p) => lit(&[
                "vault",
                "list",
                "--permission",
                p.name(),
                "--format",
                "json",
            ]),
            Verb::Items(None) => lit(&["item", "list", "--format", "json"]),
            Verb::Items(Some(v)) => {
                let mut a = lit(&["item", "list", "--format", "json", "--vault"]);
                a.push(v.as_str().to_string());
                a
            }
            Verb::Item(vault, id) => {
                let mut a = lit(&["item", "get"]);
                a.push(id.as_str().to_string());
                a.push("--vault".to_string());
                a.push(vault.as_str().to_string());
                a.extend(lit(&["--format", "json"]));
                a
            }
        }
    }
}

/// Why a look gave nothing, by class: what the model is told, in place of
/// `op`'s own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failed {
    /// No token and no vault program: an install that keeps its secrets in the
    /// environment or in files.
    NoVault,
    /// The program could not be started.
    NotStarted,
    /// It did not answer within `OP_TIMEOUT`.
    TimedOut,
    /// It answered that the token is not allowed, or has no session.
    Denied,
    /// It could not reach the vault's service.
    Unreachable,
    /// It failed, for a reason this does not name.
    Failed,
    /// Its answer was larger than this reads.
    TooLarge,
    /// Its answer was not the list it was asked for.
    Unreadable,
}

impl Failed {
    /// The class, in words for the model: it says what to do, and never why.
    pub fn words(self) -> &'static str {
        match self {
            Failed::NoVault => "this Theseus has no vault session (its secrets come from the environment or from files)",
            Failed::NotStarted => "the vault's program could not be started",
            Failed::TimedOut => "the vault did not answer in time",
            Failed::Denied => "the vault refused the daemon's token, or it has no session",
            Failed::Unreachable => "the vault could not be reached",
            Failed::Failed => "the vault's program failed",
            Failed::TooLarge => "the vault's answer was too large to list: name one vault and a part of a title",
            Failed::Unreadable => "the vault's program answered in a form Theseus does not read",
        }
    }
}

/// Who the token is: where, and what kind. Not an identifier of the account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub host: String,
    pub kind: String,
}

/// One vault: its id and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vault {
    pub id: String,
    pub name: String,
}

/// One item as the list shows it: nothing but these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemName {
    pub id: String,
    pub title: String,
    pub category: String,
    pub vault: String,
    /// The id of the vault it is in: an item is asked for by both, since the
    /// vault's program answers a service account's `item get` only when the
    /// vault is named.
    pub vault_id: String,
}

/// One field of an item as a reference needs to name it: its label and id,
/// the section it is in (if any, by label and id) and what kind of field it
/// is. Never its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldName {
    pub id: String,
    pub section: Option<String>,
    pub section_id: Option<String>,
    pub label: String,
    pub kind: &'static str,
}

/// A name as a line may carry it: control characters and line breaks gone,
/// whitespace folded, cut at [`MAX_NAME`] characters.
pub fn clean(s: &str) -> String {
    let one: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let folded = one.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::scrub::safe::head_marked(&folded, MAX_NAME)
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn list(raw: &str) -> Result<Vec<Value>, Failed> {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(a)) => Ok(a),
        _ => Err(Failed::Unreadable),
    }
}

/// `whoami`'s answer: the account's host and the kind of token.
pub fn parse_account(raw: &str) -> Result<Account, Failed> {
    let v: Value = serde_json::from_str(raw).map_err(|_| Failed::Unreadable)?;
    let url = text(&v, "url").ok_or(Failed::Unreadable)?;
    let host = url.split("://").last().unwrap_or(url);
    let host = host.split(['/', '?', '#']).next().unwrap_or("");
    let kind = match text(&v, "user_type") {
        Some("SERVICE_ACCOUNT") => "a service account",
        Some("MEMBER") => "a person's account",
        _ => "an account",
    };
    Ok(Account {
        host: clean(host),
        kind: kind.into(),
    })
}

/// `vault list`'s answer: each vault's id and name.
pub fn parse_vaults(raw: &str) -> Result<Vec<Vault>, Failed> {
    Ok(list(raw)?
        .iter()
        .filter_map(|v| {
            Some(Vault {
                id: text(v, "id")?.to_string(),
                name: clean(text(v, "name")?),
            })
        })
        .collect())
}

/// `item list`'s answer: each item's id, title, category and vault's name.
pub fn parse_items(raw: &str) -> Result<Vec<ItemName>, Failed> {
    Ok(list(raw)?
        .iter()
        .filter_map(|v| {
            Some(ItemName {
                id: clean(text(v, "id")?),
                title: clean(text(v, "title")?),
                category: clean(text(v, "category").unwrap_or("")),
                vault: clean(
                    v.get("vault")
                        .and_then(|x| text(x, "name"))
                        .unwrap_or_default(),
                ),
                vault_id: clean(
                    v.get("vault")
                        .and_then(|x| text(x, "id"))
                        .unwrap_or_default(),
                ),
            })
        })
        .collect())
}

/// The kind of a field, from the closed list: what `op` calls its type, and
/// its purpose where that says more. A type this does not know is `other`, so
/// no string of the program's is passed on as a kind.
fn kind_of(v: &Value) -> &'static str {
    if text(v, "purpose") == Some("NOTES") {
        return "a note";
    }
    match text(v, "type").unwrap_or("") {
        "CONCEALED" => "concealed",
        "STRING" => "text",
        "EMAIL" => "email address",
        "URL" => "web address",
        "OTP" => "one-time code",
        "DATE" | "MONTH_YEAR" => "date",
        "PHONE" => "phone number",
        "MENU" => "choice",
        "ADDRESS" => "address",
        "REFERENCE" => "link to an item",
        "SSHKEY" => "ssh key",
        "CREDIT_CARD_NUMBER" | "CREDIT_CARD_TYPE" => "card",
        _ => "other",
    }
}

/// `item get`'s answer, as the labels of its fields: each one's label (its id
/// when it has none), its section's label and a kind. The `value` key of a
/// field, and everything else the program lists (urls, notes, history), is
/// never read.
pub fn parse_fields(raw: &str) -> Result<Vec<FieldName>, Failed> {
    let v: Value = serde_json::from_str(raw).map_err(|_| Failed::Unreadable)?;
    let fields = v
        .get("fields")
        .and_then(Value::as_array)
        .ok_or(Failed::Unreadable)?;
    Ok(fields
        .iter()
        .filter_map(|f| {
            let label = text(f, "label")
                .filter(|l| !l.trim().is_empty())
                .or_else(|| text(f, "id"))?;
            let section = f.get("section");
            Some(FieldName {
                id: clean(text(f, "id").unwrap_or_default()),
                section: section
                    .and_then(|s| text(s, "label"))
                    .map(clean)
                    .filter(|s| !s.is_empty()),
                section_id: section
                    .and_then(|s| text(s, "id"))
                    .map(clean)
                    .filter(|s| !s.is_empty()),
                label: clean(label),
                kind: kind_of(f),
            })
        })
        .collect())
}

/// The class of a failed run, read from what the program said and never
/// returned.
pub(crate) fn class_of(stderr: &[u8]) -> Failed {
    let s = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| s.contains(w));
    if has(&[
        "unauthorized",
        "not authorized",
        "not signed in",
        "no session",
        "401",
        "403",
        "token",
        "forbidden",
        "sign in",
        "signed in",
    ]) {
        Failed::Denied
    } else if has(&[
        "dial",
        "no such host",
        "network",
        "connection",
        "unreachable",
        "lookup ",
        "tls",
    ]) {
        Failed::Unreachable
    } else {
        Failed::Failed
    }
}

impl OpReader {
    /// Whether this reader has no vault behind it.
    pub fn has_no_vault(&self) -> bool {
        self.absent.is_some()
    }

    /// One verb, run under the daemon's token with the same time limit as
    /// every other `op` call here; its stdout, or the class of its failure.
    async fn run_verb(&self, verb: &Verb) -> Result<Vec<u8>, Failed> {
        if self.absent.is_some() {
            return Err(Failed::NoVault);
        }
        let mut cmd = self.op();
        cmd.args(verb.argv()).stdin(Stdio::null());
        // A list asks for names; an item's get reads the item (its values
        // are dropped here, but the vault counts the read).
        let entries = usize::from(matches!(verb, Verb::Item(..)));
        let child = self
            .start(&mut cmd, entries)
            .map_err(|_| Failed::NotStarted)?;
        let out = match tokio::time::timeout(OP_TIMEOUT, child.wait_with_output()).await {
            Err(_) => return Err(Failed::TimedOut),
            Ok(Err(_)) => return Err(Failed::Failed),
            Ok(Ok(o)) => o,
        };
        if !out.status.success() {
            return Err(class_of(&out.stderr));
        }
        if out.stdout.len() > MAX_OUTPUT {
            return Err(Failed::TooLarge);
        }
        Ok(out.stdout)
    }

    /// One list verb: its output as text, or the class of its failure.
    pub async fn look(&self, verb: &Verb) -> Result<String, Failed> {
        String::from_utf8(self.run_verb(verb).await?).map_err(|_| Failed::Unreadable)
    }

    /// One item's text, for [`parse_fields`] and nothing else: it holds the
    /// item's values, so it is kept in zeroizing memory and dropped as soon as
    /// the labels are read.
    pub async fn look_item(
        &self,
        vault: &VaultId,
        id: &ItemId,
    ) -> Result<Zeroizing<String>, Failed> {
        match String::from_utf8(
            self.run_verb(&Verb::Item(vault.clone(), id.clone()))
                .await?,
        ) {
            Ok(text) => Ok(Zeroizing::new(text)),
            Err(e) => {
                drop(Zeroizing::new(e.into_bytes()));
                Err(Failed::Unreadable)
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
