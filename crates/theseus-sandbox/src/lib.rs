//! L1, the native sandbox (M4 step 17a, and 18b's egress proxy; design
//! §2.2, §2.4, §7).
//!
//! One binary in three roles:
//!
//! ```text
//! wrapper   host namespaces: `spawn` clones the init, writes user namespace
//! │         1's maps (0 is the operator's uid), and releases it
//! └─ init   `init_main` (`theseusd job-sandbox`): pid 1 of the job, root in
//!    │      user namespace 1; builds the view, then drops its capabilities
//!    │      and takes the seccomp filter
//!    └─ the command: user namespace 2 maps the operator's uid back, so it
//!                    runs as the same uid as at L0, with no capabilities,
//!                    no_new_privs, and the filter
//! ```
//!
//! The job gets user, pid, mount, network, uts, ipc, and cgroup namespaces;
//! a tmpfs root with read-only binds of the system and its `ro_paths`, and
//! overlays over the workspace whose writes are scratch; a fresh `/proc` and
//! `/sys`, masked as Docker masks them, and four device nodes; and an init
//! whose exit kills the whole tree. Nothing here needs privilege: every
//! piece works for an unprivileged user on a kernel with user namespaces.

pub mod cgroup;
pub mod egress;
mod init;
mod report;
pub mod seccomp;
mod spawn;
mod spec;
mod sys;
mod view;

pub use init::init_main;
pub use report::{Exit, Scratch, Started};
pub use spawn::{spawn, SandboxChild, SpawnError, Stdio};
pub use spec::{Init, Limits, Spec, HOSTNAME, INIT_ROLE};
pub use view::DEVICES;
