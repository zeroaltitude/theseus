//! The tool definitions a request spec carries (theseus-nsp9l.2): shared, never changed once made, and what is
//! worked out from them kept with them. A spec is cloned at every compile, and a catalog's definitions run to
//! 100 KB of JSON: as a `Vec<Value>` each clone, each digest and each length copied or walked all of it, a dozen
//! times before a turn's request left. Here a clone is a pointer, a digest or a length is worked out once, and the
//! tools a deferral loads are resolved once. A different set of tools is a new `Tools`, so nothing kept can go stale.

use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::tools_defer::Deferral;

/// How many times a set's definitions have been walked, to write their JSON (a test's count).
static WALKS: AtomicU64 = AtomicU64::new(0);

/// The walks so far, by every set in this process.
pub fn walks() -> u64 {
    WALKS.load(Ordering::Relaxed)
}

/// The deferrals whose resolved tools one set keeps: a turn's append and its recompile, a task's and a thread's.
const RESOLVED_KEPT: usize = 4;

#[derive(Default)]
struct Inner {
    list: Vec<Value>,
    json_len: OnceLock<u64>,
    digest: OnceLock<String>,
    /// The hash after the list's JSON, which `Tools::hashed` carries on.
    hash: OnceLock<Sha256>,
    resolved: Mutex<Vec<(Deferral, Tools)>>,
    /// The sets made from this one by a fixed rule, by the rule's name.
    reduced: Mutex<Vec<(&'static str, Tools)>>,
}

/// Wire tool definitions, sorted by name.
#[derive(Clone, Default)]
pub struct Tools(Arc<Inner>);

impl From<Vec<Value>> for Tools {
    fn from(list: Vec<Value>) -> Self {
        Self(Arc::new(Inner {
            list,
            ..Inner::default()
        }))
    }
}

impl Deref for Tools {
    type Target = [Value];
    fn deref(&self) -> &[Value] {
        &self.0.list
    }
}

impl std::fmt::Debug for Tools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.0.list.iter()).finish()
    }
}

/// A writer that counts what it is given.
struct Counting(u64);

impl std::io::Write for Counting {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A writer that hashes what it is given.
struct Hashing(Sha256);

impl std::io::Write for Hashing {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Tools {
    /// The bytes of the definitions' JSON array.
    pub fn json_len(&self) -> u64 {
        *self.0.json_len.get_or_init(|| {
            WALKS.fetch_add(1, Ordering::Relaxed);
            let mut n = Counting(0);
            let _ = serde_json::to_writer(&mut n, &self.0.list);
            n.0
        })
    }

    /// The hash state after the definitions' JSON array: what a digest of "the tools, then more" carries on from.
    fn hashed(&self) -> Sha256 {
        self.0
            .hash
            .get_or_init(|| {
                WALKS.fetch_add(1, Ordering::Relaxed);
                let mut h = Hashing(Sha256::new());
                let _ = serde_json::to_writer(&mut h, &self.0.list);
                h.0
            })
            .clone()
    }

    /// The first 16 characters of the sha256 of the definitions' JSON array.
    pub fn digest(&self) -> &str {
        self.0
            .digest
            .get_or_init(|| hex::encode(self.hashed().finalize())[..16].to_string())
    }

    /// The first 16 characters of the sha256 of the JSON array, a NUL and `rest`.
    pub fn digest_with(&self, rest: &[&str]) -> String {
        let mut h = self.hashed();
        for r in rest {
            h.update(b"\0");
            h.update(r.as_bytes());
        }
        hex::encode(h.finalize())[..16].to_string()
    }

    /// The tools `d` loads, made by `make` the first time this set is asked for them.
    pub fn resolved(&self, d: &Deferral, make: impl FnOnce(&[Value]) -> Vec<Value>) -> Tools {
        let mut kept = self
            .0
            .resolved
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((_, t)) = kept.iter().find(|(k, _)| k == d) {
            return t.clone();
        }
        let t = Tools::from(make(&self.0.list));
        if kept.len() >= RESOLVED_KEPT {
            kept.remove(0);
        }
        kept.push((d.clone(), t.clone()));
        t
    }
}

impl<'a> IntoIterator for &'a Tools {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.list.iter()
    }
}

impl Tools {
    /// The set `rule` makes of this one, made the first time and kept under the rule's name: a rule of one name must
    /// always be the same function of the list.
    pub fn reduced(&self, rule: &'static str, make: impl FnOnce(&[Value]) -> Vec<Value>) -> Tools {
        let mut kept = self
            .0
            .reduced
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((_, t)) = kept.iter().find(|(r, _)| *r == rule) {
            return t.clone();
        }
        let t = Tools::from(make(&self.0.list));
        kept.push((rule, t.clone()));
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn defs() -> Vec<Value> {
        vec![
            json!({"name": "a_tool", "description": "does a", "input_schema": {"type": "object"}}),
            json!({"name": "b_tool", "description": "does b", "input_schema": {"type": "object"}}),
        ]
    }

    #[test]
    fn what_is_kept_equals_what_a_fresh_walk_gives() {
        let tools = Tools::from(defs());
        let whole = Value::Array(defs());
        assert_eq!(
            tools.json_len(),
            serde_json::to_string(&whole).unwrap().len() as u64
        );
        assert_eq!(tools.digest(), &theseus_kernel::digest_json(&whole)[..16]);
        let joined = ["{{tools}}", "system", "persona"].join("\u{0}");
        let by_hand = crate::compiler::sha(
            &joined.replace("{{tools}}", &serde_json::to_string(&whole).unwrap()),
        );
        assert_eq!(tools.digest_with(&["system", "persona"]), by_hand);
    }

    #[test]
    fn a_clone_shares_its_definitions_and_a_new_set_starts_fresh() {
        let tools = Tools::from(defs());
        let same = tools.clone();
        assert!(std::ptr::eq(tools.as_ptr(), same.as_ptr()));
        let mut more = defs();
        more.push(json!({"name": "c_tool"}));
        let other = Tools::from(more);
        assert_ne!(other.digest(), tools.digest());
        assert_ne!(other.json_len(), tools.json_len());
    }

    #[test]
    fn a_deferral_is_resolved_once_and_the_oldest_is_dropped_past_the_limit() {
        use crate::tools_defer::Mechanism;
        let tools = Tools::from(defs());
        let d = |name: &str| Deferral {
            mechanism: Mechanism::Search,
            loaded: [name.to_string()].into(),
            resolved: false,
            added: Default::default(),
            raised: None,
            bare: false,
        };
        let mut made = 0;
        let first = tools.resolved(&d("x"), |l| {
            made += 1;
            l.to_vec()
        });
        let again = tools.resolved(&d("x"), |_| unreachable!("kept"));
        assert!(std::ptr::eq(first.as_ptr(), again.as_ptr()));
        assert_eq!(made, 1);
        for n in 0..RESOLVED_KEPT {
            tools.resolved(&d(&format!("f{n}")), |l| l.to_vec());
        }
        let mut remade = 0;
        tools.resolved(&d("x"), |l| {
            remade += 1;
            l.to_vec()
        });
        assert_eq!(remade, 1, "the oldest was dropped");
    }
}
