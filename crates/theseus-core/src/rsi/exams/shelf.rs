//! The open half on disk (theseus-pw1q.11): `<state>/self/exams/open/<family>/<id>.json`,
//! one counted item a file. Nothing but a counted item is written here,
//! so a file's existence is the item's review. A sealed item's bytes never
//! come here (`SealedSink`), and the proposer rule holds at every write: a
//! self branch adds, changes and deletes nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::{Family, Item, ItemId, Writer};

/// The exam's open half.
#[derive(Debug, Clone)]
pub struct Shelf {
    root: PathBuf,
}

impl Shelf {
    /// The shelf under a state dir.
    pub fn new(state_dir: &Path) -> Shelf {
        Shelf {
            root: state_dir.join("self").join("exams"),
        }
    }

    fn path(&self, family: Family, id: &ItemId) -> PathBuf {
        self.root
            .join("open")
            .join(family.as_str())
            .join(format!("{id}.json"))
    }

    /// Whether the item is on the shelf.
    pub fn has(&self, family: Family, id: &ItemId) -> bool {
        self.path(family, id).exists()
    }

    /// Add a counted item. Idempotent for the same bytes; an item is never
    /// changed, by anyone, and a self branch adds nothing.
    pub fn put(&self, who: &Writer, item: &Item) -> Result<()> {
        who.check("add or change an exam item")?;
        let path = self.path(item.family, &item.id);
        let bytes = item.bytes()?;
        if let Ok(old) = std::fs::read(&path) {
            if old == bytes {
                return Ok(());
            }
            bail!(
                "exam item {} exists and differs: an item is never changed",
                item.id
            );
        }
        let dir = path.parent().context("an item has a directory")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let tmp = dir.join(format!(".{}.new", item.id));
        std::fs::write(&tmp, &bytes).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("placing {}", path.display()))?;
        Ok(())
    }

    /// Delete an item: nobody may. Deleting or loosening an exam item is the
    /// owner's (the keel); the method exists so the refusal is one place.
    pub fn delete(&self, who: &Writer, _family: Family, id: &ItemId) -> Result<()> {
        who.check("delete an exam item")?;
        bail!("exam item {id} is not deleted by the daemon: deleting an exam item is the owner's")
    }

    /// Every item on the shelf, by family and id.
    pub fn items(&self) -> Vec<Item> {
        let mut out = Vec::new();
        for f in Family::ALL {
            let Ok(dir) = std::fs::read_dir(self.root.join("open").join(f.as_str())) else {
                continue;
            };
            let mut files: Vec<_> = dir.flatten().map(|e| e.path()).collect();
            files.sort();
            for p in files {
                if p.extension().is_some_and(|e| e == "json") {
                    if let Some(item) = std::fs::read(&p)
                        .ok()
                        .and_then(|b| serde_json::from_slice::<Item>(&b).ok())
                    {
                        out.push(item);
                    }
                }
            }
        }
        out
    }

    /// The count of open items by family.
    pub fn counts(&self) -> BTreeMap<Family, u64> {
        let mut m = BTreeMap::new();
        for i in self.items() {
            *m.entry(i.family).or_default() += 1;
        }
        m
    }
}
