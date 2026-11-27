//! A one-run grant (issue 64): `vault.use`'s run form gives one variable to one run of one
//! program, in the turn that asked for it, and nothing stays.
//!
//! - The value is held on the board under its handle (`vault-use:<call>`) from the moment it is
//!   read, so the scrubber and every exit mask it while the daemon runs.
//! - The grant names the program's file, canonical, as the run form resolved it. A run takes it
//!   only when `argv[0]` resolves to that same file, by its own argv, with no variable the call
//!   sets (`launches`): never a shell, an interpreter or a wrapper that runs it, never a call that
//!   sets `PATH`. A run that does not take it leaves it for the next.
//! - It ends at the first run that takes it, and with its turn. A turn here is what one message
//!   began: the owner's, and the continuations his answers to cards begin, which have no message
//!   of their own. So a run his approval let go still takes it, and once a later message (his, a
//!   wake's, a task's report) is in the session, the next call of the session ends it
//!   ([`Broker::once_turn`]); a run of another turn gets nothing.
//! - A run in the background is not given it: its row says why.
//! - What a run given a value printed is scrubbed of it before its result is kept
//!   ([`Broker::scrub_once`]), so the stored result holds the scrubber's marker after a restart.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

use super::{launcher, launches, resolve, Broker, ForJob, Grant};
use crate::policy::Posture;
use crate::secrets::Secret;

/// The board's name of a run form's value: its call's id after this.
pub const HANDLE: &str = "vault-use:";

/// One run's grant, until a run takes it or its turn ends.
#[derive(Debug, Clone)]
pub struct OnceGrant {
    /// The session, and the id of the message that began the turn it was given in.
    pub session: String,
    pub turn: String,
    /// The program's file, canonical.
    pub file: PathBuf,
    pub variable: String,
    /// The value's name on the board.
    pub handle: String,
    /// The vault, the item and the field, as the vault listed them.
    pub vault: [String; 3],
}

impl OnceGrant {
    fn program(&self) -> String {
        self.file
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_default()
            .to_string()
    }

    fn grant(&self) -> Grant {
        Grant {
            to: self.program(),
            variable: Some(self.variable.clone()),
            secret: self.handle.clone(),
        }
    }
}

/// A job's facts a one-run grant is matched against, as `for_job_of` takes them: its argv, the
/// variables the call sets, where it runs and on what PATH, the posture it ran at, whether it
/// runs in the background, and its correlation id.
#[derive(Clone, Copy)]
pub struct OnceRun<'a> {
    pub argv: &'a [String],
    pub env: &'a [&'a str],
    pub cwd: &'a Path,
    pub job_path: Option<&'a str>,
    pub ran_at: Posture,
    pub background: bool,
    pub correlation_id: &'a str,
}

/// The broker's one-run grants: the live ones, what each job was given, and each handle's names.
#[derive(Debug, Default)]
pub(super) struct Table {
    live: Mutex<Vec<OnceGrant>>,
    /// Each job given a value, by its correlation id: the variable and its handle.
    given: Mutex<BTreeMap<String, Vec<(String, String)>>>,
    /// Each handle's vault, item and field.
    names: Mutex<BTreeMap<String, [String; 3]>>,
}

impl Broker {
    /// Hold `value` on the board under the grant's handle, and give the grant to the next run of
    /// its program in its turn. A later turn of the session ends what an earlier one left.
    pub fn grant_once(&self, g: OnceGrant, value: Secret) {
        self.board.hold(&g.handle, value);
        self.once
            .names
            .lock()
            .unwrap()
            .insert(g.handle.clone(), g.vault.clone());
        let mut live = self.once.live.lock().unwrap();
        live.retain(|o| {
            o.session != g.session
                || (o.turn == g.turn && !(o.file == g.file && o.variable == g.variable))
        });
        live.push(g);
    }

    /// Hold `value` on the board under `handle`, as a run form's is: from now every exit masks it
    /// (`http.request`, issue 65).
    pub fn hold(&self, handle: &str, value: Secret) {
        self.board.hold(handle, value);
    }

    /// The file a run form's `program` names: a bare name on the daemon's PATH, as a configured
    /// grant's is; a path with a `/` relative to `cwd`. Canonical, and an executable file.
    pub fn resolve_once(&self, program: &str, cwd: &Path) -> Option<PathBuf> {
        resolve(program, self.path.as_deref(), cwd, true)
    }

    /// Whether `session` has a live one-run grant.
    pub fn has_once(&self, session: &str) -> bool {
        self.once
            .live
            .lock()
            .unwrap()
            .iter()
            .any(|o| o.session == session)
    }

    /// End every one-run grant of `session`.
    pub fn once_end(&self, session: &str) {
        self.once
            .live
            .lock()
            .unwrap()
            .retain(|o| o.session != session);
    }

    /// Keep only the one-run grants of `session` whose program is one of `programs` (by file
    /// name): the runs that wait on the owner's answer as its turn ends.
    pub fn once_keep_only(&self, session: &str, programs: &[String]) {
        self.once
            .live
            .lock()
            .unwrap()
            .retain(|o| o.session != session || programs.contains(&o.program()));
    }

    /// A turn of `session`, begun by the message `turn`, calls: the grants another turn left end.
    pub fn once_turn(&self, session: &str, turn: &str) {
        self.once
            .live
            .lock()
            .unwrap()
            .retain(|o| o.session != session || o.turn == turn);
    }

    /// The live grants of the session (and the turn, when given) that a run of `argv` takes, or
    /// why one is not taken (None when the run names none of them).
    fn once_taken(
        &self,
        (session, turn): (&str, Option<&str>),
        argv: &[String],
        env: &[&str],
        cwd: &Path,
        job_path: Option<&str>,
    ) -> Result<Vec<OnceGrant>, Option<String>> {
        let live: Vec<OnceGrant> = self
            .once
            .live
            .lock()
            .unwrap()
            .iter()
            .filter(|o| o.session == session && turn.is_none_or(|t| o.turn == t))
            .cloned()
            .collect();
        let first = argv.first().ok_or(None)?;
        let ran = resolve(first, job_path, cwd, false);
        let mine: Vec<OnceGrant> = live
            .iter()
            .filter(|o| ran.as_ref() == Some(&o.file))
            .cloned()
            .collect();
        let Some(one) = mine.first() else {
            // Its program named in another's arguments: a shell's script, an interpreter's, or
            // a wrapper running it.
            let words: Vec<&str> = argv[1..]
                .iter()
                .flat_map(|a| a.split(|c: char| c.is_whitespace() || c == '='))
                .collect();
            return Err(live
                .iter()
                .find(|o| {
                    words.iter().any(|w| {
                        Path::new(w).file_name().and_then(|f| f.to_str())
                            == Some(o.program().as_str())
                    })
                })
                .map(|o| {
                    format!(
                        "{} got no {}: it is run by `{first}`, not by its own argv, and a program \
                         that runs others would hand the variable to each of them",
                        o.program(),
                        o.variable
                    )
                }));
        };
        let program = one.program();
        if let Some(what) = launcher(&program) {
            return Err(Some(format!(
                "{program} got no {}: it is {what}, which runs whatever the call names",
                one.variable
            )));
        }
        if let Some(why) = launches(&program, &argv[1..], env) {
            return Err(Some(format!("{program} got no {}: {why}", one.variable)));
        }
        Ok(mine)
    }

    /// What a run of `argv` in `session` would be given by a one-run grant, for the gate: the
    /// session's live grants are its current turn's ([`Broker::once_turn`]).
    pub fn once_at_gate(
        &self,
        session: &str,
        argv: Option<&[String]>,
        env: &[&str],
        cwd: &Path,
        job_path: Option<&str>,
    ) -> Vec<Grant> {
        argv.and_then(|a| self.once_taken((session, None), a, env, cwd, job_path).ok())
            .map(|m| m.iter().map(OnceGrant::grant).collect())
            .unwrap_or_default()
    }

    /// Give the job `run` the one-run grants of `turn` its run takes, into `out`; each is
    /// taken, given or not. A run in the background, or at a looser posture than the grant's,
    /// is not given it, and the row says why.
    pub fn once_for_job(&self, turn: (&str, &str), run: &OnceRun<'_>, out: &mut ForJob) {
        let OnceRun {
            argv,
            env,
            cwd,
            job_path,
            ran_at,
            background,
            correlation_id,
        } = *run;
        let taken = match self.once_taken((turn.0, Some(turn.1)), argv, env, cwd, job_path) {
            Ok(t) => t,
            Err(note) => {
                if out.indirect.is_none() {
                    out.indirect = note;
                }
                return;
            }
        };
        self.once.live.lock().unwrap().retain(|o| {
            !taken
                .iter()
                .any(|t| t.handle == o.handle && t.variable == o.variable)
        });
        for o in taken {
            let grant = o.grant();
            let (posture, _) = self.posture_of(&o.handle);
            let withheld = if background {
                Some(
                    "it runs in the background, past the turn the key was given for; run it \
                     without `background` to give it the key"
                        .to_string(),
                )
            } else if posture > ran_at {
                Some(format!(
                    "its posture is {}, and this call ran at {}",
                    posture.as_str(),
                    ran_at.as_str()
                ))
            } else {
                match self.board.get(&o.handle) {
                    Some(v) => {
                        out.env.push((o.variable.clone(), v));
                        None
                    }
                    None => Some("its value is no longer held".to_string()),
                }
            };
            match withheld {
                Some(why) => out.withheld.push((grant, why)),
                None => {
                    self.once
                        .given
                        .lock()
                        .unwrap()
                        .entry(correlation_id.to_string())
                        .or_default()
                        .push((o.variable.clone(), o.handle.clone()));
                    self.count(&grant);
                    out.git_pinned |= matches!(grant.to.as_str(), "git" | "gh");
                    out.granted.push(grant);
                }
            }
        }
    }

    /// The row's additive keys for a one-run grant: that it was one run's, and the vault, item
    /// and field it was read from. None for any other grant.
    pub fn once_row(&self, g: &Grant) -> Option<Value> {
        let names = self.once.names.lock().unwrap();
        let [vault, item, field] = names.get(&g.secret)?;
        Some(json!({"once": true, "vault": {"vault": vault, "item": item, "field": field}}))
    }

    /// `text`, a job's output, with each value a one-run grant gave the job `correlation_id`
    /// replaced by the scrubber's marker (named by its variable). Any other job's as it is.
    pub fn scrub_once(&self, correlation_id: &str, text: String) -> String {
        let given = self
            .once
            .given
            .lock()
            .unwrap()
            .get(correlation_id)
            .cloned()
            .unwrap_or_default();
        let values: Vec<(String, String)> = given
            .iter()
            .filter_map(|(var, handle)| {
                Some((self.board.get(handle)?.expose().to_string(), var.clone()))
            })
            .collect();
        if values.is_empty() {
            return text;
        }
        crate::scrub::Scrubber::with_values(values).scrub(&text).0
    }
}
