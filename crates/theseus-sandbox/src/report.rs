//! What the init tells the wrapper, one JSON line at a time on its report
//! pipe: `started` once the command has been exec'd (or `failed`, naming the
//! stage that could not be done), then `exited`.

use std::io::Write;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(crate) enum Report {
    Started(Started),
    Failed { stage: String, error: String },
    Exited(Exited),
}

impl Report {
    pub(crate) fn write_to(&self, w: &mut impl Write) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        line.push(b'\n');
        w.write_all(&line)
    }
}

/// The job is running: its command has been exec'd.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Started {
    /// The command's pid inside the job's pid namespace (the init is 1).
    pub pid: i32,
    /// The init's own part of the start, from its exec to the command's,
    /// in microseconds.
    pub setup_us: u64,
    /// A fresh sysfs is at `/sys`. False when the kernel refused the mount
    /// (`mount_too_revealing`): the view then has no `/sys`.
    pub sys: bool,
    /// What the bounded inode walk covered, and what it did not
    /// (quft-hidden-bind). `#[serde(default)]`, so a wrapper reading an
    /// older init's line gets the empty value rather than a parse failure.
    #[serde(default)]
    pub residue: crate::residue::Residue,
    /// `lo` is up in the job's network namespace.
    pub lo: bool,
    /// The limits the command got lower than its spec asked, because this
    /// process may not raise its own hard limit above what it has (a job
    /// started from inside another sandbox). Empty when every limit was set
    /// as asked. `#[serde(default)]`, as `residue` is.
    #[serde(default)]
    pub lowered: Vec<Lowered>,
}

/// One limit set lower than asked: the `Limits` field it is, what the spec
/// asked, and what the command got, both in that field's own unit
/// (`output_mb` in MiB, rounded down; `pids` a count).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lowered {
    pub limit: String,
    pub asked: u64,
    pub got: u64,
}

/// What the job wrote to scratch, discarded with it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scratch {
    /// Files written, and their bytes.
    pub files: u64,
    pub bytes: u64,
    /// What those files occupy on the scratch tmpfs (`st_blocks`, in bytes),
    /// which is what the scratch limit counts: a sparse file has a size and
    /// occupies nothing, and a small file occupies a page. Zero from a wrapper
    /// that predates it.
    #[serde(default)]
    pub allocated: u64,
    /// Paths of the tree the job removed (whiteouts).
    pub removed: u64,
    /// The first 20 paths written, as the job saw them.
    pub paths: Vec<String>,
}

impl Scratch {
    /// "wrote 3 files, 41 KB, to scratch: target/…; discarded" (design §2.2).
    pub fn summary(&self) -> String {
        if self.files == 0 && self.removed == 0 {
            return "wrote nothing to scratch".into();
        }
        let kb = self.bytes.div_ceil(1024);
        let mut s = format!(
            "wrote {} file{}, {kb} KB, to scratch",
            self.files,
            if self.files == 1 { "" } else { "s" }
        );
        if let Some(first) = self.paths.first() {
            s.push_str(&format!(": {first}"));
            if self.files > 1 {
                s.push('…');
            }
        }
        if self.removed > 0 {
            s.push_str(&format!("; removed {}", self.removed));
        }
        s.push_str("; discarded");
        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Exited {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub scratch: Scratch,
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    /// The command's exit code, when it exited.
    pub code: Option<i32>,
    /// The signal that ended the command, when one did.
    pub signal: Option<i32>,
    /// The init itself died of this signal (a cancel's SIGKILL): the kernel
    /// then killed the whole pid namespace, and the command's own status was
    /// never reported.
    pub init_signal: Option<i32>,
    /// The command was killed by `SIGXFSZ`: it wrote past `output_mb`, and
    /// its output was cut there.
    pub output_capped: bool,
    /// What it wrote to scratch, when the init lived to report it.
    pub scratch: Option<Scratch>,
}

impl Exit {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}
